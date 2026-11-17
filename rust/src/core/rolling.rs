//! Rolling v1 protocol core (`scripts/rolling/core/src/{state,profile,transactions,accepted}.ts`).
//! Pure computation, no I/O. Pinned to TN10 candidate profile `b56bcd9cb654610b238a32bae25c41128ebd2837cc7a91b3ea835f8a102fc4e5`.
use super::accepted::{script_number, script_pushes};
use super::bytes::{cat, check, from_le, hex, integer, le, same, unhex, unhex_len};
use super::covenant::{AuthorizedOutput, Outpoint, Spk, covenant_id, p2pk, p2sh};
use super::hashes::{blake2b256, domain_hash};
use super::state::{script_for, template_hash};
use super::transaction::{Binding, Output, Transaction, outpoint_key, validate_transaction};
use anyhow::{Result, anyhow, bail};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashSet};

pub const TN10_PROFILE_ID: &str =
    "b56bcd9cb654610b238a32bae25c41128ebd2837cc7a91b3ea835f8a102fc4e5";
pub const COMPILER_COMMIT: &str = "a17db2c7c95cf9f12f8e1264b21f32a8552af8df";
pub const GENESIS_PREFIX: &[u8] = b"KASWIN_ROLLING_GENESIS_V1";

pub const MAX_LEDGER_BYTES: usize = 18453;
pub const MAX_OUTPUTS: usize = 36;
pub const TAIL_POOL_MIN: i128 = 300_000_000;
pub const DEPOSIT: i128 = 400_000_000;
pub const PRICE_MIN: i128 = 100_000_000;
pub const REWARD: i128 = 100_000_000;
pub const REFUND_FEE: i128 = 2_000_000;
pub const VALUE_LIMIT: i128 = 9_000_000_000_000_000;
pub const PAY_FEE_CAP: i128 = 100_000_000;
pub const FEE_CAP: i128 = 50_000_000;
pub const DAA_LIMIT: i128 = 500_000_000_000;
pub const DRAW_DELAY: i128 = 100;
pub const TIMEOUT_DELAY: i128 = 432_000;
pub const MAX_TICKETS: i64 = 10_000_000;
pub const ZERO: &str = "0000000000000000000000000000000000000000000000000000000000000000";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Module {
    Sales,
    Sealed,
    Win,
    Refund,
    Frozen,
}

pub const MODULES: [Module; 5] = [
    Module::Sales,
    Module::Sealed,
    Module::Win,
    Module::Refund,
    Module::Frozen,
];

impl Module {
    pub fn as_str(self) -> &'static str {
        match self {
            Module::Sales => "sales",
            Module::Sealed => "sealed",
            Module::Win => "win",
            Module::Refund => "refund",
            Module::Frozen => "frozen",
        }
    }
    pub fn contract_name(self) -> &'static str {
        match self {
            Module::Sales => "KaswinRollingSales",
            Module::Sealed => "KaswinRollingSealed",
            Module::Win => "KaswinRollingWin",
            Module::Refund => "KaswinRollingRefund",
            Module::Frozen => "KaswinRollingFrozen",
        }
    }
    pub fn header_len(self) -> usize {
        match self {
            Module::Sales => 59,
            Module::Sealed => 59,
            Module::Win => 93,
            Module::Refund => 63,
            Module::Frozen => 1,
        }
    }
    pub fn is_manager(self) -> bool {
        !matches!(self, Module::Frozen)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Index {
    pub hash: String,
    pub end: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TicketRecord {
    pub end: i64,
    pub key: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct State {
    pub module: Module,
    pub owner_key: String,
    pub ticket_price: i128,
    pub ticket_cap: i64,
    pub min_tickets: i64,
    pub close_eligible_daa: i128,
    pub indices: Vec<Index>,
    pub records: Vec<TicketRecord>,
    pub winner_key: Option<String>,
    pub p: Option<usize>,
    pub r: Option<usize>,
    pub c: Option<usize>,
}

impl State {
    pub fn frozen(records: Vec<TicketRecord>) -> Result<Self> {
        let s = Self {
            module: Module::Frozen,
            owner_key: String::new(),
            ticket_price: 0,
            ticket_cap: 0,
            min_tickets: 0,
            close_eligible_daa: 0,
            indices: vec![],
            records,
            winner_key: None,
            p: None,
            r: None,
            c: None,
        };
        validate(&s)?;
        Ok(s)
    }
    pub fn phase_raw(&self, module: Module) -> Result<Self> {
        check(
            self.module.is_manager() && module.is_manager(),
            "MANAGER_REQUIRED",
        )?;
        let mut next = self.clone();
        next.module = module;
        if module != Module::Win {
            next.winner_key = None;
            next.p = None;
        }
        if module != Module::Refund {
            next.r = None;
            next.c = None;
        } else {
            if next.r.is_none() {
                next.r = Some(0);
            }
            if next.c.is_none() {
                next.c = Some(0);
            }
        }
        Ok(next)
    }
    pub fn phase(&self, module: Module) -> Result<Self> {
        let next = self.phase_raw(module)?;
        validate(&next)?;
        Ok(next)
    }
    pub fn to_json(&self) -> Value {
        if self.module == Module::Frozen {
            return json!({
                "module": "frozen",
                "records": self.records.iter().map(|r| json!({"end": r.end, "key": r.key})).collect::<Vec<_>>(),
            });
        }
        let mut m = serde_json::Map::new();
        m.insert("module".into(), json!(self.module.as_str()));
        m.insert("ownerKey".into(), json!(self.owner_key));
        m.insert("ticketPrice".into(), json!(self.ticket_price.to_string()));
        m.insert("ticketCap".into(), json!(self.ticket_cap));
        m.insert("minTickets".into(), json!(self.min_tickets));
        m.insert(
            "closeEligibleDaa".into(),
            json!(self.close_eligible_daa.to_string()),
        );
        m.insert(
            "indices".into(),
            json!(
                self.indices
                    .iter()
                    .map(|i| json!({"hash": i.hash, "end": i.end}))
                    .collect::<Vec<_>>()
            ),
        );
        m.insert(
            "records".into(),
            json!(
                self.records
                    .iter()
                    .map(|r| json!({"end": r.end, "key": r.key}))
                    .collect::<Vec<_>>()
            ),
        );
        if self.module == Module::Win {
            m.insert(
                "winnerKey".into(),
                json!(self.winner_key.clone().unwrap_or_default()),
            );
            m.insert("p".into(), json!(self.p.unwrap_or(0)));
        }
        if self.module == Module::Refund {
            m.insert("r".into(), json!(self.r.unwrap_or(0)));
            m.insert("c".into(), json!(self.c.unwrap_or(0)));
        }
        Value::Object(m)
    }
}

pub fn record_total(records: &[TicketRecord]) -> i64 {
    records.last().map(|r| r.end).unwrap_or(0)
}
pub fn base_tickets(s: &State) -> i64 {
    s.indices.last().map(|i| i.end).unwrap_or(0)
}
pub fn total_tickets(s: &State) -> i64 {
    base_tickets(s) + record_total(&s.records)
}
pub fn paid_tickets(records: &[TicketRecord], c: usize) -> Result<i64> {
    check(c <= records.len(), "PAID_CURSOR")?;
    Ok(if c == 0 { 0 } else { records[c - 1].end })
}
pub fn merge_tail(s: &State) -> bool {
    !s.indices.is_empty() && (record_total(&s.records) as i128) * s.ticket_price < TAIL_POOL_MIN
}

pub fn validate_records(records: &[TicketRecord], max_count: usize) -> Result<()> {
    check(records.len() <= max_count, "RECORDS")?;
    let mut end = 0i64;
    for r in records {
        integer(r.end as i128, (end + 1) as i128, MAX_TICKETS as i128)?;
        unhex_len(&r.key, 32)?;
        end = r.end;
    }
    Ok(())
}

pub fn validate(s: &State) -> Result<()> {
    validate_records(
        &s.records,
        if s.module == Module::Frozen { 256 } else { 255 },
    )?;
    if s.module == Module::Frozen {
        check(s.records.len() == 256, "FROZEN_RECORDS")?;
        return Ok(());
    }
    unhex_len(&s.owner_key, 32)?;
    integer(s.ticket_cap as i128, 3, MAX_TICKETS as i128)?;
    integer(s.min_tickets as i128, 3, s.ticket_cap as i128)?;
    let max_price = (VALUE_LIMIT - DEPOSIT) / (s.ticket_cap as i128);
    check(
        s.ticket_price >= PRICE_MIN && s.ticket_price <= max_price,
        "PRICE",
    )?;
    check(
        s.close_eligible_daa > 0 && s.close_eligible_daa < DAA_LIMIT - TIMEOUT_DELAY,
        "CLOSE_DAA",
    )?;
    check(s.indices.len() <= 256, "INDICES")?;
    let mut end = 0i64;
    for item in &s.indices {
        unhex_len(&item.hash, 32)?;
        integer(item.end as i128, (end + 256) as i128, s.ticket_cap as i128)?;
        end = item.end;
    }
    check(s.indices.len() < 256 || s.records.is_empty(), "CAPACITY_MANAGER")?;
    check(total_tickets(s) <= s.ticket_cap, "TICKET_CAP")?;
    if matches!(s.module, Module::Sealed | Module::Win) {
        check(total_tickets(s) >= s.min_tickets, "BELOW_MINIMUM")?;
    }
    let mt = if merge_tail(s) { 1 } else { 0 };
    if s.module == Module::Win {
        let wk = s.winner_key.as_deref().unwrap_or("");
        unhex_len(wk, 32)?;
        let p = s.p.ok_or_else(|| anyhow!("WIN_P"))?;
        check(p <= s.indices.len().saturating_sub(mt), "WIN_P_RANGE")?;
    }
    if s.module == Module::Refund {
        check(total_tickets(s) > 0, "EMPTY_REFUND_STATE")?;
        let r = s.r.ok_or_else(|| anyhow!("REFUND_R"))?;
        let c = s.c.ok_or_else(|| anyhow!("REFUND_C"))?;
        check(r <= s.indices.len().saturating_sub(mt), "REFUND_R_RANGE")?;
        check(c <= 255 && c % 32 == 0, "REFUND_CURSOR")?;
        if r < s.indices.len() {
            check(c <= 224, "OLD_CURSOR")?;
        } else {
            check(!s.records.is_empty() && c < s.records.len(), "LOCAL_CURSOR")?;
        }
    }
    Ok(())
}

pub fn encode(s: &State) -> Result<Vec<u8>> {
    validate(s)?;
    let mut rs = Vec::with_capacity(s.records.len() * 36);
    for r in &s.records {
        rs.extend(le(r.end as i128, 4)?);
        rs.extend(unhex_len(&r.key, 32)?);
    }
    if s.module == Module::Frozen {
        return Ok(cat(&[&[0u8], &rs]));
    }
    let mut out = Vec::new();
    out.push(1u8);
    out.extend(unhex_len(&s.owner_key, 32)?);
    out.extend(le(s.ticket_price, 8)?);
    out.extend(le(s.ticket_cap as i128, 4)?);
    out.extend(le(s.min_tickets as i128, 4)?);
    out.extend(le(s.close_eligible_daa, 8)?);
    out.extend(le(s.indices.len() as i128, 2)?);
    if s.module == Module::Win {
        out.extend(unhex_len(s.winner_key.as_deref().unwrap_or(""), 32)?);
        out.extend(le(s.p.unwrap_or(0) as i128, 2)?);
    } else if s.module == Module::Refund {
        out.extend(le(s.r.unwrap_or(0) as i128, 2)?);
        out.extend(le(s.c.unwrap_or(0) as i128, 2)?);
    }
    for idx in &s.indices {
        out.extend(unhex_len(&idx.hash, 32)?);
        out.extend(le(idx.end as i128, 4)?);
    }
    out.extend(rs);
    Ok(out)
}

pub fn decode(module: Module, raw: &[u8]) -> Result<State> {
    let h = module.header_len();
    check(raw.len() >= h && raw.len() <= MAX_LEDGER_BYTES, "LEDGER_LENGTH")?;
    let n = if module == Module::Frozen {
        0usize
    } else {
        from_le(&raw[57..59]) as usize
    };
    check(n <= 256, "INDICES")?;
    check(
        raw[0] == if module == Module::Frozen { 0 } else { 1 },
        "ROLE",
    )?;
    check(
        raw.len() >= h + 36 * n && (raw.len() - h - 36 * n) % 36 == 0,
        "LEDGER_PARTITION",
    )?;
    let mut records = Vec::new();
    let mut at = h + 36 * n;
    while at < raw.len() {
        records.push(TicketRecord {
            end: from_le(&raw[at..at + 4]) as i64,
            key: hex(&raw[at + 4..at + 36]),
        });
        at += 36;
    }
    if module == Module::Frozen {
        return State::frozen(records);
    }
    let mut indices = Vec::with_capacity(n);
    for idx in 0..n {
        let p = h + idx * 36;
        indices.push(Index {
            hash: hex(&raw[p..p + 32]),
            end: from_le(&raw[p + 32..p + 36]) as i64,
        });
    }
    let s = State {
        module,
        owner_key: hex(&raw[1..33]),
        ticket_price: from_le(&raw[33..41]) as i128,
        ticket_cap: from_le(&raw[41..45]) as i64,
        min_tickets: from_le(&raw[45..49]) as i64,
        close_eligible_daa: from_le(&raw[49..57]) as i128,
        indices,
        records,
        winner_key: if module == Module::Win {
            Some(hex(&raw[59..91]))
        } else {
            None
        },
        p: if module == Module::Win {
            Some(from_le(&raw[91..93]) as usize)
        } else {
            None
        },
        r: if module == Module::Refund {
            Some(from_le(&raw[59..61]) as usize)
        } else {
            None
        },
        c: if module == Module::Refund {
            Some(from_le(&raw[61..63]) as usize)
        } else {
            None
        },
    };
    validate(&s)?;
    Ok(s)
}

pub fn manager_value(s: &State) -> Result<i128> {
    validate(s)?;
    check(s.module.is_manager(), "MANAGER_REQUIRED")?;
    let paid = if s.module == Module::Refund && s.r == Some(s.indices.len()) {
        paid_tickets(&s.records, s.c.unwrap_or(0))?
    } else {
        0
    };
    Ok(DEPOSIT + ((record_total(&s.records) - paid) as i128) * s.ticket_price)
}

pub fn shard_tickets(s: &State, j: usize) -> Result<i64> {
    check(j < s.indices.len(), "SHARD_INDEX")?;
    let prev = if j == 0 { 0 } else { s.indices[j - 1].end };
    Ok(s.indices[j].end - prev)
}

pub fn frozen_value(s: &State, j: usize, records: &[TicketRecord]) -> Result<i128> {
    check(record_total(records) == shard_tickets(s, j)?, "SHARD_TICKETS")?;
    let paid = if s.module == Module::Refund && s.r == Some(j) {
        paid_tickets(records, s.c.unwrap_or(0))?
    } else {
        0
    };
    Ok(((record_total(records) - paid) as i128) * s.ticket_price)
}

// ---------- Profile & Artifacts ----------

macro_rules! rolling_art {
    ($p:literal) => {
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../artifacts/rolling-v1/",
            $p
        ))
    };
}

struct PinnedArtifact {
    module: Module,
    artifact: &'static [u8],
    artifact_sha256: &'static str,
    source: &'static [u8],
    source_sha256: &'static str,
}

const PINNED_ARTIFACTS: [PinnedArtifact; 5] = [
    PinnedArtifact {
        module: Module::Sales,
        artifact: rolling_art!("sales.json"),
        artifact_sha256: "fe9469795cf15b3a52550a7b86237ce9ac5de915bc5c34d7431664bb66295fe8",
        source: rolling_art!("sales.sil"),
        source_sha256: "587d3b754b7beacbd8f5a8e6e63cdb533c1d6325e2553c81523c1a6fd8019011",
    },
    PinnedArtifact {
        module: Module::Sealed,
        artifact: rolling_art!("sealed.json"),
        artifact_sha256: "4da7ece5d5248a5ef59130e176b7f05fd3561d0bc5b0631c1be612231dac8ab8",
        source: rolling_art!("sealed.sil"),
        source_sha256: "62ae51daf8660158e56f8f284daee2637a851529f8f1a2ad7e660d177c7192e3",
    },
    PinnedArtifact {
        module: Module::Win,
        artifact: rolling_art!("win.json"),
        artifact_sha256: "d4ce23f8f0b3b764e5c4f0d3d8ea4e2a93739b0c30dff05c41c7b97ee86126d9",
        source: rolling_art!("win.sil"),
        source_sha256: "5204ca57fcfc204857c552a7f1603d0cc1c7d4a9f2ff4867908df336af5931b6",
    },
    PinnedArtifact {
        module: Module::Refund,
        artifact: rolling_art!("refund.json"),
        artifact_sha256: "f692e7afd1f69a0f25c1e75886f0417833e1a65f90939a8288933b45ed1c1e37",
        source: rolling_art!("refund.sil"),
        source_sha256: "212d295a2ca4e802f6415641ea526eb51ff84c171f5c72fd333a7cd54593c363",
    },
    PinnedArtifact {
        module: Module::Frozen,
        artifact: rolling_art!("frozen.json"),
        artifact_sha256: "becb53dc804549b915361c3bdcbd710cc32008c2363536416786586ed414e451",
        source: rolling_art!("frozen.sil"),
        source_sha256: "4a2642aebdacfa2e1efed0714535f44bfebcf8d490754b30c8f3d6933995ccce",
    },
];

pub fn abi_spec(m: Module) -> &'static [(&'static str, &'static str, Option<usize>)] {
    match m {
        Module::Sales => &[
            ("action", "int", None),
            ("originTxId", "fixed_bytes", Some(32)),
            ("originIndex", "int", None),
            ("ownTailBytes", "int", None),
            ("nextTail", "bytes", None),
            ("data", "bytes", None),
        ],
        Module::Sealed => &[
            ("action", "int", None),
            ("ownTailBytes", "int", None),
            ("nextTail", "bytes", None),
            ("data", "bytes", None),
        ],
        Module::Win => &[
            ("action", "int", None),
            ("ownTailBytes", "int", None),
            ("targetSizes", "bytes", None),
        ],
        Module::Refund => &[
            ("action", "int", None),
            ("ownTailBytes", "int", None),
            ("targetSizes", "bytes", None),
        ],
        Module::Frozen => &[
            ("managerRedeemBytes", "int", None),
            ("managerTailBytes", "int", None),
        ],
    }
}

#[derive(Clone, Debug)]
pub struct Frame {
    pub tail: Vec<u8>,
    pub template_hash: String,
    pub dispatch_tag: String,
    pub source_sha256: String,
    pub artifact_sha256: String,
}

#[derive(Clone, Debug)]
pub struct Profile {
    pub id: String,
    pub frames: BTreeMap<&'static str, Frame>,
}

impl Profile {
    pub fn frame(&self, m: Module) -> Result<&Frame> {
        self.frames
            .get(m.as_str())
            .ok_or_else(|| anyhow!("PROFILE_FRAME {}", m.as_str()))
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

fn parse_artifact_bytes(v: &Value) -> Result<Vec<u8>> {
    if let Some(s) = v.as_str() {
        return unhex(s.strip_prefix("0x").unwrap_or(s));
    }
    let arr = v
        .as_array()
        .ok_or_else(|| anyhow!("ARTIFACT_BYTES"))?;
    let mut out = Vec::with_capacity(arr.len());
    for item in arr {
        let n = item.as_u64().filter(|n| *n <= 255).ok_or_else(|| anyhow!("ARTIFACT_BYTES"))?;
        out.push(n as u8);
    }
    Ok(out)
}

pub fn load_pinned_profile() -> Result<Profile> {
    let mut frames = BTreeMap::new();
    let mut preimage = b"KASWIN_ROLLING_PROFILE_V1".to_vec();
    let mut seen_th = HashSet::new();

    for pin in &PINNED_ARTIFACTS {
        check(
            sha256_hex(pin.artifact) == pin.artifact_sha256,
            "ARTIFACT_DIGEST",
        )?;
        check(sha256_hex(pin.source) == pin.source_sha256, "SOURCE_DIGEST")?;
        let doc: Value = serde_json::from_slice(pin.artifact)?;
        check(
            doc.get("schema_version").and_then(|v| v.as_i64()) == Some(1)
                && doc.get("compiler_version").and_then(|v| v.as_str()) == Some("0.1.0"),
            "ARTIFACT_VERSION",
        )?;
        let c = doc
            .get("contracts")
            .and_then(|x| x.get(pin.module.contract_name()))
            .ok_or_else(|| anyhow!("ARTIFACT_CONTRACT"))?;
        let fields = c
            .get("runtime_state")
            .and_then(|x| x.get("fields"))
            .and_then(|x| x.as_array())
            .ok_or_else(|| anyhow!("ARTIFACT_STATE"))?;
        check(
            fields.len() == 1 && fields[0].get("name").and_then(|x| x.as_str()) == Some("ledger"),
            "ARTIFACT_STATE",
        )?;
        let compiled = c.get("compiled").ok_or_else(|| anyhow!("ARTIFACT_FRAME"))?;
        let raw = parse_artifact_bytes(compiled.get("bytecode").unwrap_or(&Value::Null))?;
        let span = compiled
            .get("state_span")
            .ok_or_else(|| anyhow!("ARTIFACT_FRAME"))?;
        let offset = span.get("offset").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
        let len = span.get("len").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
        check(
            offset == 1 && len > 0 && len + 1 < raw.len() && raw[0] == 0x6b,
            "ARTIFACT_FRAME",
        )?;
        let tail = raw[1 + len..].to_vec();
        let th = template_hash(&tail)?;
        let expected_th = hex(&parse_artifact_bytes(
            compiled.get("template_hash").unwrap_or(&Value::Null),
        )?);
        check(th == expected_th, "ARTIFACT_TEMPLATE_HASH")?;
        check(seen_th.insert(th.clone()), "TEMPLATE_COLLISION")?;

        let entry = c
            .get("entries")
            .and_then(|x| x.get("spend"))
            .ok_or_else(|| anyhow!("ARTIFACT_ABI"))?;
        let params = entry
            .get("params")
            .and_then(|x| x.as_array())
            .ok_or_else(|| anyhow!("ARTIFACT_ABI"))?;
        let spec = abi_spec(pin.module);
        check(params.len() == spec.len(), "ARTIFACT_ABI")?;
        for (p, (name, kind, width)) in params.iter().zip(spec.iter()) {
            let pname = p.get("name").and_then(|v| v.as_str());
            let pkind = p.get("type").and_then(|t| t.get("kind")).and_then(|v| v.as_str());
            let plen = p.get("type").and_then(|t| t.get("len")).and_then(|v| v.as_u64());
            check(
                pname == Some(*name)
                    && pkind == Some(*kind)
                    && (*kind != "fixed_bytes" || plen == width.map(|w| w as u64)),
                "ARTIFACT_ABI",
            )?;
        }
        let dispatch_tag = entry
            .get("dispatch_tag")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("ARTIFACT_ABI"))?
            .to_string();
        let tag_bytes = unhex_len(&dispatch_tag, 4)?;
        preimage.extend(unhex_len(pin.source_sha256, 32)?);
        preimage.extend(unhex_len(&th, 32)?);
        preimage.extend(tag_bytes);

        frames.insert(
            pin.module.as_str(),
            Frame {
                tail,
                template_hash: th,
                dispatch_tag,
                source_sha256: pin.source_sha256.to_string(),
                artifact_sha256: pin.artifact_sha256.to_string(),
            },
        );
    }

    let id = hex(&blake2b256(&preimage));
    check(id == TN10_PROFILE_ID, "DEPLOYMENT_PROFILE_MISMATCH")?;
    Ok(Profile { id, frames })
}

pub fn redeem(s: &State, p: &Profile) -> Result<Vec<u8>> {
    let encoded = encode(s)?;
    let frame = p.frame(s.module)?;
    script_for(&encoded, &frame.tail)
}

pub fn script_hash(s: &State, p: &Profile) -> Result<String> {
    Ok(hex(&blake2b256(&redeem(s, p)?)))
}

pub fn identify(raw: &[u8], p: &Profile) -> Result<State> {
    check(raw.len() <= 250_000 && !raw.is_empty() && raw[0] == 0x6b, "REDEEM_FRAME")?;
    let mut matched = None;
    for m in MODULES {
        let tail = &p.frame(m)?.tail;
        if raw.len() > tail.len() && same(&raw[raw.len() - tail.len()..], tail) {
            matched = Some((m, tail.len()));
            break;
        }
    }
    let (module, tail_len) = matched.ok_or_else(|| anyhow!("UNSUPPORTED_TEMPLATE"))?;
    let (len, at) = if raw[1] >= 1 && raw[1] <= 75 {
        (raw[1] as usize, 2usize)
    } else if raw[1] == 0x4c {
        check(raw.len() > 3, "REDEEM_FRAME")?;
        (raw[2] as usize, 3usize)
    } else if raw[1] == 0x4d {
        check(raw.len() > 4, "REDEEM_FRAME")?;
        (from_le(&raw[2..4]) as usize, 4usize)
    } else {
        bail!("REDEEM_FRAME");
    };
    check(at + len + tail_len == raw.len(), "REDEEM_FRAME")?;
    let ledger = &raw[at..at + len];
    let s = decode(module, ledger)?;
    check(same(&redeem(&s, p)?, raw), "REDEEM_CANONICAL")?;
    Ok(s)
}

pub fn root_id(origin: &Outpoint, s: &State, p: &Profile) -> Result<String> {
    let init = State {
        module: Module::Sales,
        owner_key: s.owner_key.clone(),
        ticket_price: s.ticket_price,
        ticket_cap: s.ticket_cap,
        min_tickets: s.min_tickets,
        close_eligible_daa: s.close_eligible_daa,
        indices: vec![],
        records: vec![],
        winner_key: None,
        p: None,
        r: None,
        c: None,
    };
    let sh = script_hash(&init, p)?;
    let spk = p2sh(&sh)?;
    covenant_id(
        origin,
        &[AuthorizedOutput {
            index: 0,
            value: DEPOSIT,
            spk: &spk,
        }],
    )
}

#[derive(Clone, Debug)]
pub struct ManagerContext {
    pub cid: String,
    pub origin: Outpoint,
    pub outpoint: Outpoint,
    pub value: i128,
    pub spk: Spk,
    pub redeem_script: Vec<u8>,
    pub utxo_daa: i128,
    pub current_daa: i128,
}

#[derive(Clone, Debug)]
pub struct TargetContext {
    pub cid: String,
    pub outpoint: Outpoint,
    pub value: i128,
    pub spk: Spk,
    pub redeem_script: Vec<u8>,
}

#[derive(Clone, Debug)]
pub struct FundingContext {
    pub outpoint: Outpoint,
    pub value: i128,
    pub cid: Option<String>,
}

pub fn verify_manager(x: &ManagerContext, p: &Profile) -> Result<State> {
    let s = identify(&x.redeem_script, p)?;
    check(s.module.is_manager(), "MANAGER_MODULE")?;
    check(x.value == manager_value(&s)?, "MANAGER_VALUE")?;
    let expected_spk = p2sh(&hex(&blake2b256(&x.redeem_script)))?;
    check(x.spk == expected_spk, "MANAGER_SPK")?;
    check(x.cid == root_id(&x.origin, &s, p)?, "MANAGER_CID")?;
    Ok(s)
}

// ---------- Transitions & Accepted Interpretation ----------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Buy = 1,
    Close = 2,
    Draw = 4,
    Timeout = 9,
    PayOld = 20,
    PaySelf = 21,
    RefundOld = 30,
    RefundSelf = 31,
}

impl Action {
    pub fn from_id(id: i128) -> Result<Self> {
        match id {
            1 => Ok(Action::Buy),
            2 => Ok(Action::Close),
            4 => Ok(Action::Draw),
            9 => Ok(Action::Timeout),
            20 => Ok(Action::PayOld),
            21 => Ok(Action::PaySelf),
            30 => Ok(Action::RefundOld),
            31 => Ok(Action::RefundSelf),
            _ => Err(anyhow!("UNSUPPORTED_ACTION")),
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Action::Buy => "BUY",
            Action::Close => "CLOSE",
            Action::Draw => "DRAW",
            Action::Timeout => "TIMEOUT",
            Action::PayOld => "PAY_OLD",
            Action::PaySelf => "PAY_SELF",
            Action::RefundOld => "REFUND_OLD",
            Action::RefundSelf => "REFUND_SELF",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Accessor {
    pub block_hash: String,
    pub sequence_commitment: String,
}

#[derive(Clone, Debug)]
pub enum SpendArg {
    Int(i128),
    Bytes(Vec<u8>),
}

#[derive(Clone, Debug)]
pub struct SpendWitness {
    pub state: State,
    pub args: BTreeMap<&'static str, SpendArg>,
    pub raw: Vec<u8>,
}

pub fn decode_spend(signature_script: &str, p: &Profile) -> Result<SpendWitness> {
    let pushes = script_pushes(&unhex(signature_script)?)?;
    let raw = pushes.last().ok_or_else(|| anyhow!("EMPTY_WITNESS"))?.clone();
    let state = identify(&raw, p)?;
    let frame = p.frame(state.module)?;
    let spec = abi_spec(state.module);
    check(pushes.len() == spec.len() + 2, "WITNESS_ABI")?;
    check(hex(&pushes[pushes.len() - 2]) == frame.dispatch_tag, "WITNESS_ABI")?;
    let mut args = BTreeMap::new();
    for (i, (name, kind, width)) in spec.iter().enumerate() {
        let b = &pushes[i];
        if *kind == "int" {
            let n = script_number(b)?;
            check(n >= 0, "NEGATIVE_ARGUMENT")?;
            args.insert(*name, SpendArg::Int(n));
        } else {
            if let Some(w) = width {
                check(b.len() == *w, "ARGUMENT_LENGTH")?;
            }
            args.insert(*name, SpendArg::Bytes(b.clone()));
        }
    }
    Ok(SpendWitness { state, args, raw })
}

fn payment_key(spk: &Spk) -> Result<String> {
    check(
        spk.version == 0
            && spk.script.len() == 68
            && spk.script.starts_with("20")
            && spk.script.ends_with("ac")
            && unhex_len(&spk.script[2..66], 32).is_ok(),
        "P2PK_PAYMENT",
    )?;
    Ok(spk.script[2..66].to_string())
}

fn opening_int(proof: &[u8], offset: usize) -> Result<i128> {
    let n = from_le(&proof[offset..offset + 8]) as i128;
    check(n < (1i128 << 63), "PROOF_INTEGER")?;
    Ok(n)
}

fn open_seq(proof: &[u8], base: usize, parent: &[u8]) -> Result<[u8; 32]> {
    let ctx = domain_hash("SeqCommitMergesetContext", &proof[base + 64..base + 88])?;
    let r = domain_hash(
        "SeqCommitmentMerkleBranchHash",
        &cat(&[&ctx, &proof[base + 32..base + 64]]),
    )?;
    let m = domain_hash(
        "SeqCommitmentMerkleBranchHash",
        &cat(&[&proof[base..base + 32], &r]),
    )?;
    domain_hash("SeqCommitmentMerkleBranchHash", &cat(&[parent, &m]))
}

pub fn draw_sample(
    cid: &str,
    outpoint: &Outpoint,
    utxo_daa: i128,
    current_daa: i128,
    s: &State,
    proof: &[u8],
    accessor: &Accessor,
) -> Result<i64> {
    check(proof.len() == 240, "DRAW_PROOF")?;
    check(
        utxo_daa >= 0 && utxo_daa < DAA_LIMIT - TIMEOUT_DELAY && current_daa >= utxo_daa + DRAW_DELAY,
        "DRAW_MATURITY",
    )?;
    let boundary = utxo_daa + DRAW_DELAY;
    let td = opening_int(proof, 104)?;
    check(
        opening_int(proof, 224)? < boundary && td >= boundary && td < DAA_LIMIT,
        "FIRST_CROSSING",
    )?;
    check(opening_int(proof, 112)? > opening_int(proof, 232)?, "BLUE_ORDER")?;
    opening_int(proof, 96)?;
    opening_int(proof, 216)?;
    let seq1 = open_seq(proof, 152, &proof[120..152])?;
    let seq = open_seq(proof, 32, &seq1)?;
    let target = hex(&proof[0..32]);
    check(
        accessor.block_hash == target && accessor.sequence_commitment == hex(&seq),
        "CHAIN_ACCESSOR",
    )?;
    let seed = blake2b256(&cat(&[
        b"KASWIN_ROLLING_DRAW_V1",
        &unhex_len(cid, 32)?,
        &unhex_len(&outpoint.transaction_id, 32)?,
        &le(outpoint.index, 4)?,
        &le(boundary, 8)?,
        &proof[0..32],
        &seq,
    ]));
    let digest = blake2b256(&cat(&[b"KASWIN_V2_SAMPLE", &seed, &[0u8; 8]]));
    let sample = from_le(&digest[0..7]) as i128;
    let t = total_tickets(s) as i128;
    let space = 1i128 << 56;
    check(sample < space - (space % t), "SAMPLE_REJECTED")?;
    Ok((sample % t) as i64)
}

#[derive(Clone, Debug)]
pub struct TransitionResult {
    pub next: Option<State>,
    pub terminal: Option<String>,
    pub outputs: Vec<Output>,
    pub frozen_outputs: Vec<(String, Vec<u8>)>,
    pub data: Vec<u8>,
    pub next_tail: Vec<u8>,
}

fn check_frozen_target(
    x: &ManagerContext,
    p: &Profile,
    s: &State,
    f: &TargetContext,
    j: usize,
) -> Result<State> {
    check(f.cid == x.cid, "TARGET_CID")?;
    check(j < s.indices.len(), "TARGET_INDEX")?;
    check(
        hex(&blake2b256(&f.redeem_script)) == s.indices[j].hash,
        "TARGET_HASH",
    )?;
    let q = identify(&f.redeem_script, p)?;
    check(q.module == Module::Frozen, "TARGET_ROLE")?;
    check(f.spk == p2sh(&s.indices[j].hash)?, "TARGET_SPK")?;
    check(f.value == frozen_value(s, j, &q.records)?, "TARGET_VALUE")?;
    Ok(q)
}

pub struct AcceptedInterpretation {
    pub action: Action,
    pub fee: i128,
    pub next: Option<State>,
    pub terminal: Option<String>,
    pub origin: Outpoint,
    pub cid: String,
    pub frozen_outputs: Vec<(String, Vec<u8>)>,
}

pub fn interpret_accepted(
    manager: &ManagerContext,
    targets: &[TargetContext],
    funds: &[FundingContext],
    tx: &Transaction,
    accessor: Option<&Accessor>,
    p: &Profile,
) -> Result<AcceptedInterpretation> {
    validate_transaction(tx)?;
    check(
        tx.version == 1 && !tx.inputs.is_empty() && tx.inputs.len() <= 17,
        "ROLLING_TRANSACTION",
    )?;
    let s = verify_manager(manager, p)?;
    let w = decode_spend(&tx.inputs[0].signature_script, p)?;
    check(same(&w.raw, &manager.redeem_script), "SPENT_MANAGER")?;

    let get_int = |name: &str| -> Result<i128> {
        match w.args.get(name) {
            Some(SpendArg::Int(v)) => Ok(*v),
            _ => Err(anyhow!("MISSING_ARG {name}")),
        }
    };
    let get_bytes = |name: &str| -> Result<&[u8]> {
        match w.args.get(name) {
            Some(SpendArg::Bytes(v)) => Ok(v.as_slice()),
            _ => Err(anyhow!("MISSING_ARG {name}")),
        }
    };

    let action = Action::from_id(get_int("action")?)?;
    check(
        get_int("ownTailBytes")? == p.frame(s.module)?.tail.len() as i128,
        "TAIL_LENGTH",
    )?;

    let mut total_in = manager.value;
    let mut seen_outpoints = HashSet::new();
    let mut ctx_outpoints = Vec::with_capacity(1 + targets.len() + funds.len());
    ctx_outpoints.push(&manager.outpoint);
    for t in targets {
        total_in += t.value;
        ctx_outpoints.push(&t.outpoint);
    }
    for f in funds {
        total_in += f.value;
        ctx_outpoints.push(&f.outpoint);
        check(
            f.cid.as_deref().map(|c| c == ZERO).unwrap_or(true),
            "FUNDING_CID",
        )?;
    }
    check(ctx_outpoints.len() == tx.inputs.len(), "INPUT_CONTEXT")?;
    for (i, op) in ctx_outpoints.iter().enumerate() {
        let k = outpoint_key(op)?;
        check(seen_outpoints.insert(k.clone()), "DUPLICATE_INPUT")?;
        check(k == outpoint_key(&tx.inputs[i].previous_outpoint)?, "INPUT_OUTPOINT")?;
    }
    if s.module == Module::Sales {
        check(
            hex(get_bytes("originTxId")?) == manager.origin.transaction_id
                && get_int("originIndex")? == manager.origin.index,
            "ORIGIN",
        )?;
    }

    let total_out: i128 = tx.outputs.iter().map(|o| o.value).sum();
    let fee = total_in - total_out;
    let paying = matches!(action, Action::PayOld | Action::PaySelf);
    check(
        fee > 0 && fee <= if paying { PAY_FEE_CAP } else { FEE_CAP },
        "FEE",
    )?;
    let external: i128 = funds.iter().map(|f| f.value).sum();
    let last_out = tx.outputs.last().ok_or_else(|| anyhow!("OUTPUTS"))?;
    let actor_key = if last_out.covenant.is_some() {
        ZERO.to_string()
    } else {
        payment_key(&last_out.script_public_key)?
    };

    let n = s.indices.len();
    let local = record_total(&s.records);
    let total_t = total_tickets(&s);
    let mut next: Option<State> = None;
    let mut terminal: Option<String> = None;
    let mut expected_outputs: Vec<Output> = Vec::new();
    let mut frozen_outputs: Vec<(String, Vec<u8>)> = Vec::new();
    let mut data: Vec<u8> = Vec::new();
    let mut next_tail: Vec<u8> = Vec::new();
    let mut external_cost: Option<i128> = None;

    let mut push_pay = |value: i128, key: &str, expected: &mut Vec<Output>| -> Result<()> {
        check(value > 0 && value <= VALUE_LIMIT, "PAYMENT")?;
        expected.push(Output {
            value,
            script_public_key: p2pk(key)?,
            covenant: None,
        });
        Ok(())
    };
    let push_state = |st: &State,
                      value: i128,
                      expected: &mut Vec<Output>,
                      frozen_out: &mut Vec<(String, Vec<u8>)>|
     -> Result<()> {
        let raw = redeem(st, p)?;
        let sh = hex(&blake2b256(&raw));
        if st.module == Module::Frozen {
            frozen_out.push((sh.clone(), raw));
        }
        expected.push(Output {
            value,
            script_public_key: p2sh(&sh)?,
            covenant: Some(Binding {
                covenant_id: manager.cid.clone(),
                authorizing_input: 0,
            }),
        });
        Ok(())
    };

    match action {
        Action::Buy => {
            check(s.module == Module::Sales && n < 256, "NOT_SELLING")?;
            check(targets.is_empty(), "UNEXPECTED_TARGETS")?;
            let d = get_bytes("data")?;
            check(d.len() == 36, "BUY_DATA")?;
            let q = from_le(&d[0..4]) as i64;
            integer(q as i128, 1, (s.ticket_cap - total_t) as i128)?;
            let buyer = hex(&d[4..36]);
            data = d.to_vec();
            let mut records = s.records.clone();
            records.push(TicketRecord {
                end: local + q,
                key: buyer,
            });
            external_cost = Some((q as i128) * s.ticket_price + fee);
            if records.len() == 256 {
                let f = State::frozen(records)?;
                let fhash = script_hash(&f, p)?;
                let mut indices = s.indices.clone();
                indices.push(Index {
                    hash: fhash,
                    end: total_t + q,
                });
                let mut ns = s.phase(Module::Sales)?;
                ns.indices = indices;
                ns.records = vec![];
                validate(&ns)?;
                push_state(
                    &ns,
                    manager_value(&ns)?,
                    &mut expected_outputs,
                    &mut frozen_outputs,
                )?;
                push_state(
                    &f,
                    ((local + q) as i128) * s.ticket_price,
                    &mut expected_outputs,
                    &mut frozen_outputs,
                )?;
                next = Some(ns);
                next_tail = p.frame(Module::Frozen)?.tail.clone();
            } else {
                let mut ns = s.phase(Module::Sales)?;
                ns.records = records;
                validate(&ns)?;
                push_state(
                    &ns,
                    manager_value(&ns)?,
                    &mut expected_outputs,
                    &mut frozen_outputs,
                )?;
                next = Some(ns);
            }
        }
        Action::Close => {
            check(s.module == Module::Sales, "NOT_SALES")?;
            check(targets.is_empty(), "UNEXPECTED_TARGETS")?;
            if total_t < s.ticket_cap && n < 256 {
                check(manager.current_daa >= s.close_eligible_daa, "CLOSE_EARLY")?;
            }
            external_cost = Some(fee);
            if total_t == 0 {
                push_pay(DEPOSIT, &s.owner_key, &mut expected_outputs)?;
                terminal = Some("EMPTY".into());
            } else {
                let m = if total_t >= s.min_tickets {
                    Module::Sealed
                } else {
                    Module::Refund
                };
                let mut ns = s.phase(m)?;
                if m == Module::Refund {
                    ns.r = Some(0);
                    ns.c = Some(0);
                }
                validate(&ns)?;
                push_state(
                    &ns,
                    manager_value(&ns)?,
                    &mut expected_outputs,
                    &mut frozen_outputs,
                )?;
                next = Some(ns);
                next_tail = p.frame(m)?.tail.clone();
            }
        }
        Action::Draw => {
            check(s.module == Module::Sealed, "NOT_SEALED")?;
            check(targets.is_empty(), "UNEXPECTED_TARGETS")?;
            let d = get_bytes("data")?;
            check(d.len() >= 248, "DRAW_DATA")?;
            let proof = &d[0..240];
            let j = from_le(&d[240..242]) as usize;
            let i = from_le(&d[242..244]) as usize;
            let nlen = from_le(&d[244..248]) as usize;
            check(d.len() == 248 + nlen, "DRAW_PREIMAGE_LENGTH")?;
            let winning_redeem = &d[248..];
            let acc = accessor.ok_or_else(|| anyhow!("DRAW_ACCESSOR_REQUIRED"))?;
            let ticket = draw_sample(
                &manager.cid,
                &manager.outpoint,
                manager.utxo_daa,
                manager.current_daa,
                &s,
                proof,
                acc,
            )?;
            check(j <= n, "SHARD_HINT")?;
            let start = if j == 0 { 0 } else { s.indices[j - 1].end };
            let end = if j < n { s.indices[j].end } else { total_t };
            check(start <= ticket && ticket < end, "SHARD_HINT")?;
            let frozen_state;
            let records: &[TicketRecord] = if j < n {
                check(
                    hex(&blake2b256(winning_redeem)) == s.indices[j].hash,
                    "WINNING_HASH",
                )?;
                frozen_state = identify(winning_redeem, p)?;
                check(frozen_state.module == Module::Frozen, "WINNING_ROLE")?;
                check(
                    record_total(&frozen_state.records) == end - start,
                    "WINNING_TICKETS",
                )?;
                &frozen_state.records
            } else {
                check(winning_redeem.is_empty(), "UNEXPECTED_WINNING_PREIMAGE")?;
                &s.records
            };
            let local_ticket = ticket - start;
            check(i < records.len(), "RECORD_HINT")?;
            let prev_end = if i == 0 { 0 } else { records[i - 1].end };
            check(
                prev_end <= local_ticket && local_ticket < records[i].end,
                "RECORD_HINT",
            )?;
            let mut ns = s.phase_raw(Module::Win)?;
            ns.winner_key = Some(records[i].key.clone());
            ns.p = Some(0);
            validate(&ns)?;
            push_state(
                &ns,
                manager_value(&ns)?,
                &mut expected_outputs,
                &mut frozen_outputs,
            )?;
            next = Some(ns);
            data = d.to_vec();
            next_tail = p.frame(Module::Win)?.tail.clone();
            external_cost = Some(fee);
        }
        Action::Timeout => {
            check(
                s.module == Module::Sealed && manager.current_daa >= manager.utxo_daa + TIMEOUT_DELAY,
                "TIMEOUT_EARLY",
            )?;
            check(targets.is_empty(), "UNEXPECTED_TARGETS")?;
            let mut ns = s.phase(Module::Refund)?;
            ns.r = Some(0);
            ns.c = Some(0);
            validate(&ns)?;
            push_state(
                &ns,
                manager_value(&ns)?,
                &mut expected_outputs,
                &mut frozen_outputs,
            )?;
            next = Some(ns);
            next_tail = p.frame(Module::Refund)?.tail.clone();
            external_cost = Some(fee);
        }
        Action::PayOld => {
            check(s.module == Module::Win && external == 0, "PAY_CONTEXT")?;
            let k = targets.len();
            integer(k as i128, 1, 16)?;
            let p_idx = s.p.unwrap_or(0);
            check(p_idx + k <= n, "PAY_BOUNDARY")?;
            let mut gross = 0i128;
            for (idx, f) in targets.iter().enumerate() {
                check_frozen_target(manager, p, &s, f, p_idx + idx)?;
                gross += f.value;
            }
            let last = merge_tail(&s) && p_idx + k == n;
            if last {
                gross += (local as i128) * s.ticket_price;
            } else {
                let mut ns = s.clone();
                ns.p = Some(p_idx + k);
                validate(&ns)?;
                push_state(
                    &ns,
                    manager_value(&ns)?,
                    &mut expected_outputs,
                    &mut frozen_outputs,
                )?;
                next = Some(ns);
            }
            let prize = gross - (k as i128) * REWARD - fee;
            check(prize >= PRICE_MIN, "PRIZE_MIN")?;
            let wk = s.winner_key.as_deref().ok_or_else(|| anyhow!("WIN_KEY"))?;
            push_pay(prize, wk, &mut expected_outputs)?;
            if last {
                push_pay(DEPOSIT, &s.owner_key, &mut expected_outputs)?;
                terminal = Some("PAID".into());
            }
            push_pay((k as i128) * REWARD, &actor_key, &mut expected_outputs)?;
        }
        Action::PaySelf => {
            check(s.module == Module::Win && external == 0, "PAY_CONTEXT")?;
            check(targets.is_empty(), "UNEXPECTED_TARGETS")?;
            check(
                s.p == Some(n) && local > 0 && !merge_tail(&s),
                "SELF_PAY_BOUNDARY",
            )?;
            let prize = (local as i128) * s.ticket_price - REWARD - fee;
            check(prize >= PRICE_MIN, "PRIZE_MIN")?;
            let wk = s.winner_key.as_deref().ok_or_else(|| anyhow!("WIN_KEY"))?;
            push_pay(prize, wk, &mut expected_outputs)?;
            push_pay(DEPOSIT, &s.owner_key, &mut expected_outputs)?;
            push_pay(REWARD, &actor_key, &mut expected_outputs)?;
            terminal = Some("PAID".into());
        }
        Action::RefundOld => {
            let r_idx = s.r.unwrap_or(0);
            let c_idx = s.c.unwrap_or(0);
            check(
                s.module == Module::Refund && r_idx < n && targets.len() == 1 && external == 0,
                "OLD_REFUND_CONTEXT",
            )?;
            let f = &targets[0];
            let fs = check_frozen_target(manager, p, &s, f, r_idx)?;
            let end = c_idx + 32;
            let last = end == 256;
            let final_step = last && r_idx == n - 1 && merge_tail(&s);
            if !final_step {
                let mut ns = s.clone();
                ns.r = Some(if last { r_idx + 1 } else { r_idx });
                ns.c = Some(if last { 0 } else { end });
                validate(&ns)?;
                push_state(
                    &ns,
                    manager_value(&ns)?,
                    &mut expected_outputs,
                    &mut frozen_outputs,
                )?;
                next = Some(ns);
            }
            let gross = (paid_tickets(&fs.records, end)? - paid_tickets(&fs.records, c_idx)?)
                as i128
                * s.ticket_price;
            if !last {
                push_state(
                    &fs,
                    f.value - gross,
                    &mut expected_outputs,
                    &mut frozen_outputs,
                )?;
            }
            for i in c_idx..end {
                let prev_e = if i == 0 { 0 } else { fs.records[i - 1].end };
                let amt = ((fs.records[i].end - prev_e) as i128) * s.ticket_price - REFUND_FEE;
                push_pay(amt, &fs.records[i].key, &mut expected_outputs)?;
            }
            let mut pool = 32 * REFUND_FEE;
            if final_step {
                check(s.records.len() <= 2, "MERGED_REFUND_RECORDS")?;
                for i in 0..s.records.len() {
                    let prev_e = if i == 0 { 0 } else { s.records[i - 1].end };
                    let amt =
                        ((s.records[i].end - prev_e) as i128) * s.ticket_price - REFUND_FEE;
                    push_pay(amt, &s.records[i].key, &mut expected_outputs)?;
                }
                pool += (s.records.len() as i128) * REFUND_FEE;
                push_pay(DEPOSIT, &s.owner_key, &mut expected_outputs)?;
                terminal = Some("REFUNDED".into());
            }
            push_pay(pool - fee, &actor_key, &mut expected_outputs)?;
        }
        Action::RefundSelf => {
            let r_idx = s.r.unwrap_or(0);
            let c_idx = s.c.unwrap_or(0);
            check(
                s.module == Module::Refund
                    && r_idx == n
                    && !s.records.is_empty()
                    && !merge_tail(&s),
                "SELF_REFUND_CONTEXT",
            )?;
            check(targets.is_empty(), "UNEXPECTED_TARGETS")?;
            let end = std::cmp::min(c_idx + 32, s.records.len());
            let last = end == s.records.len();
            if !last {
                let mut ns = s.clone();
                ns.c = Some(end);
                validate(&ns)?;
                push_state(
                    &ns,
                    manager_value(&ns)?,
                    &mut expected_outputs,
                    &mut frozen_outputs,
                )?;
                next = Some(ns);
            }
            for i in c_idx..end {
                let prev_e = if i == 0 { 0 } else { s.records[i - 1].end };
                let amt =
                    ((s.records[i].end - prev_e) as i128) * s.ticket_price - REFUND_FEE;
                push_pay(amt, &s.records[i].key, &mut expected_outputs)?;
            }
            if last {
                push_pay(DEPOSIT - REFUND_FEE, &s.owner_key, &mut expected_outputs)?;
                terminal = Some("REFUNDED".into());
            }
            let exec_amt = ((end - c_idx + if last { 1 } else { 0 }) as i128) * REFUND_FEE
                + external
                - fee;
            push_pay(exec_amt, &actor_key, &mut expected_outputs)?;
        }
    }

    if let Some(cost) = external_cost {
        check(external >= cost, "FUNDING_SHORT")?;
        if external > cost {
            push_pay(external - cost, &actor_key, &mut expected_outputs)?;
        }
    }
    check(
        !expected_outputs.is_empty() && expected_outputs.len() <= MAX_OUTPUTS,
        "OUTPUT_COUNT",
    )?;

    if external_cost.is_some() {
        check(!funds.is_empty() && funds.len() <= 8, "FUNDING_COUNT")?;
    } else if action == Action::RefundSelf {
        check(funds.len() <= 8, "FUNDING_COUNT")?;
    } else {
        check(funds.is_empty(), "FUNDING_COUNT")?;
    }

    if matches!(s.module, Module::Sales | Module::Sealed) {
        check(
            same(get_bytes("data")?, &data) && same(get_bytes("nextTail")?, &next_tail),
            "ACTION_WITNESS",
        )?;
    } else {
        let sizes = get_bytes("targetSizes")?;
        check(sizes.len() == targets.len() * 4, "TARGET_LENGTHS")?;
        for (i, f) in targets.iter().enumerate() {
            check(
                from_le(&sizes[i * 4..i * 4 + 4]) == f.redeem_script.len() as u128,
                "TARGET_LENGTH",
            )?;
        }
    }

    for (i, f) in targets.iter().enumerate() {
        let fw = decode_spend(&tx.inputs[i + 1].signature_script, p)?;
        check(
            fw.state.module == Module::Frozen && same(&fw.raw, &f.redeem_script),
            "FROZEN_WITNESS",
        )?;
        let m_bytes = match fw.args.get("managerRedeemBytes") {
            Some(SpendArg::Int(v)) => *v,
            _ => bail!("MISSING_ARG managerRedeemBytes"),
        };
        let mt_bytes = match fw.args.get("managerTailBytes") {
            Some(SpendArg::Int(v)) => *v,
            _ => bail!("MISSING_ARG managerTailBytes"),
        };
        check(
            m_bytes == manager.redeem_script.len() as i128
                && mt_bytes == p.frame(s.module)?.tail.len() as i128,
            "MANAGER_LOCATOR",
        )?;
        check(
            f.spk == p2sh(&hex(&blake2b256(&fw.raw)))?,
            "FROZEN_P2SH",
        )?;
    }

    check(expected_outputs == tx.outputs, "OUTPUT_EFFECTS")?;
    Ok(AcceptedInterpretation {
        action,
        fee,
        next,
        terminal,
        origin: manager.origin.clone(),
        cid: manager.cid.clone(),
        frozen_outputs,
    })
}

pub fn decode_genesis_payload(payload_hex: &str, expected_profile_id: &str) -> Result<State> {
    let raw = unhex(payload_hex)?;
    let prefix_len = GENESIS_PREFIX.len();
    check(
        raw.len() >= prefix_len + 32 + Module::Sales.header_len(),
        "GENESIS_PAYLOAD_LENGTH",
    )?;
    check(&raw[..prefix_len] == GENESIS_PREFIX, "GENESIS_PREFIX")?;
    let pid = hex(&raw[prefix_len..prefix_len + 32]);
    check(pid == expected_profile_id, "GENESIS_PROFILE")?;
    let s = decode(Module::Sales, &raw[prefix_len + 32..])?;
    check(
        s.indices.is_empty() && s.records.is_empty(),
        "GENESIS_INITIAL_EMPTY",
    )?;
    Ok(s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contracts::rolling_v1::pinned_profile;
    use crate::core::state::push_bytes;
    use crate::core::transaction::Input;

    fn encode_script_num(n: i128) -> Result<Vec<u8>> {
        if n == 0 {
            return push_bytes(&[]);
        }
        let mut v = Vec::new();
        let mut cur = n as u128;
        while cur > 0 {
            v.push((cur & 0xff) as u8);
            cur >>= 8;
        }
        if v.last().is_some_and(|b| b & 0x80 != 0) {
            v.push(0);
        }
        push_bytes(&v)
    }

    #[test]
    fn timeout_requires_external_funding_and_refunds_change() {
        let profile = pinned_profile().unwrap();
        let owner_key = "11".repeat(32);
        let actor_key = "22".repeat(32);
        let buyer_key = "33".repeat(32);
        let origin = Outpoint {
            transaction_id: "44".repeat(32),
            index: 0,
        };
        let sealed = State {
            module: Module::Sealed,
            ticket_price: 100_000_000,
            ticket_cap: 100,
            min_tickets: 10,
            close_eligible_daa: 1_000_000,
            owner_key,
            indices: vec![],
            records: vec![TicketRecord {
                end: 10,
                key: buyer_key,
            }],
            winner_key: None,
            p: None,
            r: None,
            c: None,
        };
        validate(&sealed).unwrap();

        let manager_redeem = redeem(&sealed, &profile).unwrap();
        let manager_sh = hex(&blake2b256(&manager_redeem));
        let manager_spk = p2sh(&manager_sh).unwrap();
        let manager_val = manager_value(&sealed).unwrap();
        let mgr_op = Outpoint {
            transaction_id: "55".repeat(32),
            index: 0,
        };
        let cid = root_id(&origin, &sealed, &profile).unwrap();

        let mut refund = sealed.phase(Module::Refund).unwrap();
        refund.r = Some(0);
        refund.c = Some(0);
        let refund_redeem = redeem(&refund, &profile).unwrap();
        let refund_spk = p2sh(&hex(&blake2b256(&refund_redeem))).unwrap();

        let sealed_frame = profile.frame(Module::Sealed).unwrap();
        let refund_frame = profile.frame(Module::Refund).unwrap();
        let sig_script = hex(&cat(&[
            &encode_script_num(9).unwrap(),
            &encode_script_num(sealed_frame.tail.len() as i128).unwrap(),
            &push_bytes(&refund_frame.tail).unwrap(),
            &push_bytes(&[]).unwrap(),
            &push_bytes(&unhex(&sealed_frame.dispatch_tag).unwrap()).unwrap(),
            &push_bytes(&manager_redeem).unwrap(),
        ]));

        let manager = ManagerContext {
            cid: cid.clone(),
            origin: origin.clone(),
            outpoint: mgr_op.clone(),
            value: manager_val,
            spk: manager_spk.clone(),
            redeem_script: manager_redeem.clone(),
            utxo_daa: 2_000_000,
            current_daa: 2_000_000 + TIMEOUT_DELAY,
        };
        let fee = 10_000i128;
        let fund_val = 50_000_000i128;
        let fund = FundingContext {
            outpoint: Outpoint {
                transaction_id: "66".repeat(32),
                index: 1,
            },
            value: fund_val,
            cid: None,
        };

        let tx_with_fund = Transaction {
            version: 1,
            inputs: vec![
                Input {
                    previous_outpoint: mgr_op.clone(),
                    signature_script: sig_script.clone(),
                    sequence: TIMEOUT_DELAY,
                    compute_budget: 0,
                },
                Input {
                    previous_outpoint: fund.outpoint.clone(),
                    signature_script: String::new(),
                    sequence: 0,
                    compute_budget: 0,
                },
            ],
            outputs: vec![
                Output {
                    value: manager_val,
                    script_public_key: refund_spk.clone(),
                    covenant: Some(Binding {
                        covenant_id: cid.clone(),
                        authorizing_input: 0,
                    }),
                },
                Output {
                    value: fund_val - fee,
                    script_public_key: p2pk(&actor_key).unwrap(),
                    covenant: None,
                },
            ],
            lock_time: 0,
            subnetwork_id: "00".repeat(20),
            gas: 0,
            payload: String::new(),
            storage_mass: 0,
        };

        let res = interpret_accepted(&manager, &[], &[fund.clone()], &tx_with_fund, None, &profile)
            .unwrap();
        assert_eq!(res.action, Action::Timeout);
        assert_eq!(res.fee, fee);
        assert!(res.terminal.is_none());
        let next_state = res.next.unwrap();
        assert_eq!(next_state.module, Module::Refund);
        assert_eq!(next_state.r, Some(0));
        assert_eq!(next_state.c, Some(0));

        // Without external funding: rejected with FUNDING_SHORT.
        let tx_no_fund = Transaction {
            inputs: vec![tx_with_fund.inputs[0].clone()],
            outputs: vec![Output {
                value: manager_val - fee,
                script_public_key: refund_spk,
                covenant: Some(Binding {
                    covenant_id: cid,
                    authorizing_input: 0,
                }),
            }],
            ..tx_with_fund.clone()
        };
        let err = interpret_accepted(&manager, &[], &[], &tx_no_fund, None, &profile)
            .err()
            .unwrap()
            .to_string();
        assert!(err.contains("FUNDING_SHORT"), "unexpected error: {err}");

        // Early timeout DAA: rejected with TIMEOUT_EARLY.
        let mut early_mgr = manager;
        early_mgr.current_daa = early_mgr.utxo_daa + TIMEOUT_DELAY - 1;
        let err_early =
            interpret_accepted(&early_mgr, &[], &[fund], &tx_with_fund, None, &profile)
                .err()
                .unwrap()
                .to_string();
        assert!(err_early.contains("TIMEOUT_EARLY"), "unexpected error: {err_early}");
    }
}
