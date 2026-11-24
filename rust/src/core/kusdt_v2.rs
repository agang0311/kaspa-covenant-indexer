//! Kaswin kUSDT v2 (256 parallel shards) core constants, state layouts, witness decoding, and draw sampling.
//! Pure computation, no I/O. Matches `contracts/kusdt-v2/artifacts/template-pins.json` and `KUSDT-PARALLEL-SHARDS-SPEC.md`.

use super::accepted::{script_number, script_pushes};
use super::bytes::{cat, check, from_le, hex, le, unhex, unhex_len};
use super::covenant::{AuthorizedOutput, Outpoint, Spk, covenant_id, p2sh};
use super::hashes::{blake2b256, blake3_hash, domain_hash};
use super::rolling::Accessor;
use super::state::{script_for, template_hash};
use anyhow::{Result, anyhow, bail};
use serde_json::{Value, json};
use std::collections::BTreeMap;

pub const PROTOCOL_ID: &str = "kusdt-v2";
pub const VERSION: u8 = 0x01;
pub const GENESIS_PAYLOAD_MAGIC: &[u8] = b"KASWIN_KUSDT_V2_GENESIS";
const M_OPEN_ARTIFACT_JSON: &[u8] = include_bytes!("../../../artifacts/kusdt-v2/m_open.json");
pub const MAX_BATCH: usize = 8;
pub const MAX_SHARDS: usize = 256;
pub const MAX_RECORDS_PER_SHARD: usize = 256;
pub const MIN_DRAW_TICKETS: u32 = 3;
pub const DRAW_DELAY_DAA: i128 = 100;
pub const REROLL_TIMEOUT_DAA: i128 = 432_000;
pub const DAA_LIMIT: i128 = i64::MAX as i128;

pub const MAINNET_KUSDT_CID: &str =
    "50b3477e7368d0c5bfbd64483b4607350ec411377972f33e40b2d4b6d1513fd3";
pub const KCC20_EXT_TEMPLATE_HASH: &str =
    "734850b0af0aeef49f009167bf9ddd238fd8c5cdbe214234f97181ac6f5cc498";
pub const KCC20_PREFIX_HEX: &str =
    "6b209703112ee6e3555107cd168858992b77d3b74f655205b2b463b1f9ec2ec73cf7";
pub const KCC20_SUFFIX_LEN: usize = 3885;

pub const MANAGER_DEPOSIT: i128 = 400_000_000;
pub const SHARD_LEDGER_DEPOSIT: i128 = 200_000_000;
pub const SHARD_VAULT_DEPOSIT: i128 = 200_000_000;
pub const SHARD_TOTAL_DEPOSIT: i128 = SHARD_LEDGER_DEPOSIT + SHARD_VAULT_DEPOSIT;
pub const WINNER_CARRIER_MIN: i128 = 150_000_000;
pub const PRICE_MIN: i128 = 1_000_000;
pub const EXECUTOR_REWARD: i128 = 500_000;
pub const FEE_CAP: i128 = 50_000_000;

pub const ZERO32: &str = "0000000000000000000000000000000000000000000000000000000000000000";

/// Frozen template hashes from `contracts/kusdt-v2/artifacts/template-pins.json` (`MAX_BATCH=8`).
pub const M_OPEN_HASH: &str = "559e67acf808e5d48283421e01ac7dc3134d91109f09ac1257324fa85feb8f2a";
pub const M_SEALED_HASH: &str = "e987429ff1ee8d5905fe6dc7c7a01ea5747f7e036d26d2a2e37e35c6d031f20f";
pub const M_DISPATCH_HASH: &str = "c19b021080e31fd1408bd7aa7455cf11cfd4324d1cae0d94dd8ce6139db00050";
pub const S_OPEN_HASH: &str = "30213f2234df0b1f67794ef5b8fd6e91f144a3d745ffa282d679218ac52165fc";
pub const S_FROZEN_HASH: &str = "5f6a591bd11fc5d4f873ab17aa94cc5b41e688ff7b9bae34e157dd5f86dfc76a";
pub const S_PAYOUT_HASH: &str = "d9c495e42d7730616441e05335ab8e30a742e7004e67f467ad4498111452e950";
pub const S_REFUND_HASH: &str = "63f0f28aee985e1b3393c3722de1cbf1c4bec8eb565de75ee5125165c9a79983";

#[derive(Clone, Debug)]
pub struct TemplateHashes {
    pub m_open: String,
    pub m_sealed: String,
    pub m_dispatch: String,
    pub s_open: String,
    pub s_frozen: String,
    pub s_payout: String,
    pub s_refund: String,
}

impl Default for TemplateHashes {
    fn default() -> Self {
        Self {
            m_open: M_OPEN_HASH.to_string(),
            m_sealed: M_SEALED_HASH.to_string(),
            m_dispatch: M_DISPATCH_HASH.to_string(),
            s_open: S_OPEN_HASH.to_string(),
            s_frozen: S_FROZEN_HASH.to_string(),
            s_payout: S_PAYOUT_HASH.to_string(),
            s_refund: S_REFUND_HASH.to_string(),
        }
    }
}

impl TemplateHashes {
    pub fn from_config(v: Option<&Value>) -> Result<Self> {
        let mut h = Self::default();
        let Some(obj) = v.and_then(|x| x.as_object()) else {
            return Ok(h);
        };
        let apply = |key: &str, dst: &mut String| -> Result<()> {
            if let Some(s) = obj.get(key).and_then(|x| x.as_str()) {
                unhex_len(s, 32)?;
                *dst = s.to_string();
            }
            Ok(())
        };
        apply("m_open", &mut h.m_open)?;
        apply("m_sealed", &mut h.m_sealed)?;
        apply("m_dispatch", &mut h.m_dispatch)?;
        apply("s_open", &mut h.s_open)?;
        apply("s_frozen", &mut h.s_frozen)?;
        apply("s_payout", &mut h.s_payout)?;
        apply("s_refund", &mut h.s_refund)?;
        Ok(h)
    }

    pub fn stage_for_hash(&self, th: &str) -> Option<Stage> {
        if th == self.m_open {
            Some(Stage::MOpen)
        } else if th == self.m_sealed {
            Some(Stage::MSealed)
        } else if th == self.m_dispatch {
            Some(Stage::MDispatch)
        } else if th == self.s_open {
            Some(Stage::SOpen)
        } else if th == self.s_frozen {
            Some(Stage::SFrozen)
        } else if th == self.s_payout {
            Some(Stage::SPayout)
        } else if th == self.s_refund {
            Some(Stage::SRefund)
        } else {
            None
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
#[repr(u8)]
pub enum Stage {
    MOpen = 0x01,
    MSealed = 0x02,
    MDispatch = 0x03,
    SOpen = 0x11,
    SFrozen = 0x12,
    SPayout = 0x13,
    SRefund = 0x14,
}

impl Stage {
    pub fn as_str(self) -> &'static str {
        match self {
            Stage::MOpen => "M_OPEN",
            Stage::MSealed => "M_SEALED",
            Stage::MDispatch => "M_DISPATCH",
            Stage::SOpen => "S_OPEN",
            Stage::SFrozen => "S_FROZEN",
            Stage::SPayout => "S_PAYOUT",
            Stage::SRefund => "S_REFUND",
        }
    }

    pub fn is_manager(self) -> bool {
        matches!(self, Stage::MOpen | Stage::MSealed | Stage::MDispatch)
    }

    pub fn is_shard(self) -> bool {
        !self.is_manager()
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum Decision {
    Undecided = 0x00,
    Payout = 0x01,
    Refund = 0x02,
}

impl Decision {
    pub fn from_u8(b: u8) -> Result<Self> {
        match b {
            0x00 => Ok(Decision::Undecided),
            0x01 => Ok(Decision::Payout),
            0x02 => Ok(Decision::Refund),
            _ => bail!("KUSDT_V2_DECISION"),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Decision::Undecided => "UNDECIDED",
            Decision::Payout => "PAYOUT",
            Decision::Refund => "REFUND",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ManagerEntry {
    pub shard_cid: String,
    pub ledger_hash: String,
    pub cum_tickets: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TicketRecord {
    pub cum_tickets: u32,
    pub buyer_pk: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ManagerHeader {
    pub stage: Stage,
    pub version: u8,
    pub creator_pk: String,
    pub price: i128,
    pub close_daa: i128,
    pub opened_count: u16,
    pub cursor: u16,
    pub total_tickets: u32,
    pub decision: Decision,
    pub reserved: u8,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ManagerState {
    pub header: ManagerHeader,
    pub entries: Vec<ManagerEntry>,
    pub winner_pk: Option<String>,
    pub shard_cids: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ShardState {
    OpenOrFrozen {
        stage: Stage,
        version: u8,
        manager_cid: String,
        first_buyer_pk: String,
        price: i128,
        records: Vec<TicketRecord>,
    },
    Payout {
        version: u8,
        first_buyer_pk: String,
        winner_pk: String,
        vault_amount: i128,
        manager_cid: String,
        shard_tickets: u32,
    },
    Refund {
        version: u8,
        first_buyer_pk: String,
        price: i128,
        record_count: u8,
        claimed_mask: u8,
        records: Vec<TicketRecord>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ContractState {
    Manager(ManagerState),
    Shard(ShardState),
}

impl ManagerState {
    pub fn to_json(&self) -> Value {
        manager_to_json(self)
    }
}

impl ShardState {
    pub fn stage(&self) -> Stage {
        match self {
            ShardState::OpenOrFrozen { stage, .. } => *stage,
            ShardState::Payout { .. } => Stage::SPayout,
            ShardState::Refund { .. } => Stage::SRefund,
        }
    }

    pub fn to_json(&self) -> Value {
        shard_to_json(self)
    }
}

impl ContractState {
    pub fn stage(&self) -> Stage {
        match self {
            ContractState::Manager(m) => m.header.stage,
            ContractState::Shard(s) => s.stage(),
        }
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        match self {
            ContractState::Manager(m) => encode_manager_state(m),
            ContractState::Shard(s) => encode_shard_state(s),
        }
    }

    pub fn to_json(&self) -> Value {
        match self {
            ContractState::Manager(m) => manager_to_json(m),
            ContractState::Shard(s) => shard_to_json(s),
        }
    }
}

fn read_u63(b: &[u8]) -> Result<i128> {
    check(b.len() == 8, "U63_LEN")?;
    let v = u64::from_le_bytes(b.try_into().unwrap());
    check(v < (1u64 << 63), "U63_SIGN")?;
    Ok(v as i128)
}

fn read_u16(b: &[u8]) -> Result<u16> {
    check(b.len() == 2, "U16_LEN")?;
    Ok(u16::from_le_bytes(b.try_into().unwrap()))
}

fn read_u32(b: &[u8]) -> Result<u32> {
    check(b.len() == 4, "U32_LEN")?;
    Ok(u32::from_le_bytes(b.try_into().unwrap()))
}

pub fn decode_manager_state(stage: Stage, raw: &[u8]) -> Result<ManagerState> {
    check(stage.is_manager(), "NOT_MANAGER_STAGE")?;
    check(raw.len() >= 60, "M_HEADER_LEN")?;
    check(raw[0] == stage as u8, "M_STAGE_BYTE")?;
    check(raw[1] == VERSION, "M_VERSION_BYTE")?;
    let creator_pk = hex(&raw[2..34]);
    let price = read_u63(&raw[34..42])?;
    let close_daa = read_u63(&raw[42..50])?;
    let opened_count = read_u16(&raw[50..52])?;
    let cursor = read_u16(&raw[52..54])?;
    let total_tickets = read_u32(&raw[54..58])?;
    let decision = Decision::from_u8(raw[58])?;
    let reserved = raw[59];
    check(opened_count as usize <= MAX_SHARDS, "M_OPENED_COUNT")?;

    let header = ManagerHeader {
        stage,
        version: raw[1],
        creator_pk,
        price,
        close_daa,
        opened_count,
        cursor,
        total_tickets,
        decision,
        reserved,
    };

    if matches!(stage, Stage::MOpen | Stage::MSealed) {
        let expected_len = 60 + 68 * (opened_count as usize);
        check(raw.len() == expected_len, "M_OPEN_SEALED_LEN")?;
        let mut entries = Vec::with_capacity(opened_count as usize);
        for i in 0..opened_count as usize {
            let off = 60 + i * 68;
            entries.push(ManagerEntry {
                shard_cid: hex(&raw[off..off + 32]),
                ledger_hash: hex(&raw[off + 32..off + 64]),
                cum_tickets: read_u32(&raw[off + 64..off + 68])?,
            });
        }
        Ok(ManagerState {
            header,
            entries,
            winner_pk: None,
            shard_cids: vec![],
        })
    } else {
        let expected_len = 92 + 32 * (opened_count as usize);
        check(raw.len() == expected_len, "M_DISPATCH_LEN")?;
        let winner_pk = hex(&raw[60..92]);
        let mut shard_cids = Vec::with_capacity(opened_count as usize);
        for i in 0..opened_count as usize {
            let off = 92 + i * 32;
            shard_cids.push(hex(&raw[off..off + 32]));
        }
        Ok(ManagerState {
            header,
            entries: vec![],
            winner_pk: Some(winner_pk),
            shard_cids,
        })
    }
}

pub fn encode_manager_state(m: &ManagerState) -> Result<Vec<u8>> {
    let h = &m.header;
    let mut out = Vec::with_capacity(60 + 68 * m.entries.len() + 32 * m.shard_cids.len() + 32);
    out.push(h.stage as u8);
    out.push(h.version);
    out.extend(unhex_len(&h.creator_pk, 32)?);
    out.extend(le(h.price, 8)?);
    out.extend(le(h.close_daa, 8)?);
    out.extend(le(h.opened_count as i128, 2)?);
    out.extend(le(h.cursor as i128, 2)?);
    out.extend(le(h.total_tickets as i128, 4)?);
    out.push(h.decision as u8);
    out.push(h.reserved);
    if matches!(h.stage, Stage::MOpen | Stage::MSealed) {
        check(m.entries.len() == h.opened_count as usize, "M_ENTRY_COUNT")?;
        for e in &m.entries {
            out.extend(unhex_len(&e.shard_cid, 32)?);
            out.extend(unhex_len(&e.ledger_hash, 32)?);
            out.extend(le(e.cum_tickets as i128, 4)?);
        }
    } else {
        check(m.shard_cids.len() == h.opened_count as usize, "M_DISPATCH_COUNT")?;
        let w = m.winner_pk.as_deref().unwrap_or(ZERO32);
        out.extend(unhex_len(w, 32)?);
        for sc in &m.shard_cids {
            out.extend(unhex_len(sc, 32)?);
        }
    }
    Ok(out)
}

pub fn decode_shard_state(stage: Stage, raw: &[u8]) -> Result<ShardState> {
    check(stage.is_shard(), "NOT_SHARD_STAGE")?;
    check(raw.len() >= 2 && raw[0] == stage as u8 && raw[1] == VERSION, "S_HEADER")?;
    match stage {
        Stage::SOpen | Stage::SFrozen => {
            check(raw.len() >= 76, "S_OPEN_LEN")?;
            let manager_cid = hex(&raw[2..34]);
            let first_buyer_pk = hex(&raw[34..66]);
            let price = read_u63(&raw[66..74])?;
            let record_count = read_u16(&raw[74..76])? as usize;
            check((1..=MAX_RECORDS_PER_SHARD).contains(&record_count), "S_RECORD_COUNT")?;
            check(raw.len() == 76 + 36 * record_count, "S_OPEN_RECORDS_LEN")?;
            let mut records = Vec::with_capacity(record_count);
            for i in 0..record_count {
                let off = 76 + i * 36;
                records.push(TicketRecord {
                    cum_tickets: read_u32(&raw[off..off + 4])?,
                    buyer_pk: hex(&raw[off + 4..off + 36]),
                });
            }
            Ok(ShardState::OpenOrFrozen {
                stage,
                version: raw[1],
                manager_cid,
                first_buyer_pk,
                price,
                records,
            })
        }
        Stage::SPayout => {
            check(raw.len() == 110, "S_PAYOUT_LEN")?;
            Ok(ShardState::Payout {
                version: raw[1],
                first_buyer_pk: hex(&raw[2..34]),
                winner_pk: hex(&raw[34..66]),
                vault_amount: read_u63(&raw[66..74])?,
                manager_cid: hex(&raw[74..106]),
                shard_tickets: read_u32(&raw[106..110])?,
            })
        }
        Stage::SRefund => {
            check(raw.len() >= 44, "S_REFUND_LEN")?;
            let first_buyer_pk = hex(&raw[2..34]);
            let price = read_u63(&raw[34..42])?;
            let record_count = raw[42];
            let claimed_mask = raw[43];
            check(record_count == 1 || record_count == 2, "S_REFUND_COUNT")?;
            check(raw.len() == 44 + 36 * (record_count as usize), "S_REFUND_RECS_LEN")?;
            let mut records = Vec::with_capacity(record_count as usize);
            for i in 0..record_count as usize {
                let off = 44 + i * 36;
                records.push(TicketRecord {
                    cum_tickets: read_u32(&raw[off..off + 4])?,
                    buyer_pk: hex(&raw[off + 4..off + 36]),
                });
            }
            Ok(ShardState::Refund {
                version: raw[1],
                first_buyer_pk,
                price,
                record_count,
                claimed_mask,
                records,
            })
        }
        _ => bail!("INVALID_SHARD_STAGE"),
    }
}

pub fn encode_shard_state(s: &ShardState) -> Result<Vec<u8>> {
    match s {
        ShardState::OpenOrFrozen {
            stage,
            version,
            manager_cid,
            first_buyer_pk,
            price,
            records,
        } => {
            let mut out = Vec::with_capacity(76 + 36 * records.len());
            out.push(*stage as u8);
            out.push(*version);
            out.extend(unhex_len(manager_cid, 32)?);
            out.extend(unhex_len(first_buyer_pk, 32)?);
            out.extend(le(*price, 8)?);
            out.extend(le(records.len() as i128, 2)?);
            for r in records {
                out.extend(le(r.cum_tickets as i128, 4)?);
                out.extend(unhex_len(&r.buyer_pk, 32)?);
            }
            Ok(out)
        }
        ShardState::Payout {
            version,
            first_buyer_pk,
            winner_pk,
            vault_amount,
            manager_cid,
            shard_tickets,
        } => {
            let mut out = Vec::with_capacity(110);
            out.push(Stage::SPayout as u8);
            out.push(*version);
            out.extend(unhex_len(first_buyer_pk, 32)?);
            out.extend(unhex_len(winner_pk, 32)?);
            out.extend(le(*vault_amount, 8)?);
            out.extend(unhex_len(manager_cid, 32)?);
            out.extend(le(*shard_tickets as i128, 4)?);
            Ok(out)
        }
        ShardState::Refund {
            version,
            first_buyer_pk,
            price,
            record_count,
            claimed_mask,
            records,
        } => {
            let mut out = Vec::with_capacity(44 + 36 * records.len());
            out.push(Stage::SRefund as u8);
            out.push(*version);
            out.extend(unhex_len(first_buyer_pk, 32)?);
            out.extend(le(*price, 8)?);
            out.push(*record_count);
            out.push(*claimed_mask);
            for r in records {
                out.extend(le(r.cum_tickets as i128, 4)?);
                out.extend(unhex_len(&r.buyer_pk, 32)?);
            }
            Ok(out)
        }
    }
}

pub fn split_redeem_script(raw: &[u8]) -> Result<(Vec<u8>, Vec<u8>)> {
    check(raw.len() > 4 && raw[0] == 0x6b, "REDEEM_PREFIX")?;
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
    check(at + len < raw.len(), "REDEEM_FRAME")?;
    let state_bytes = raw[at..at + len].to_vec();
    let tail = raw[at + len..].to_vec();
    check(!tail.is_empty() && tail[0] == 0x6c, "REDEEM_TAIL_FROMALTSTACK")?;
    Ok((state_bytes, tail))
}

pub fn identify_redeem(raw: &[u8], hashes: &TemplateHashes) -> Result<(ContractState, Vec<u8>)> {
    let (state_bytes, tail) = split_redeem_script(raw)?;
    let th = template_hash(&tail)?;
    let stage = hashes
        .stage_for_hash(&th)
        .ok_or_else(|| anyhow!("KUSDT_V2_UNSUPPORTED_TEMPLATE {th}"))?;
    let st = if stage.is_manager() {
        ContractState::Manager(decode_manager_state(stage, &state_bytes)?)
    } else {
        ContractState::Shard(decode_shard_state(stage, &state_bytes)?)
    };
    check(script_for(&state_bytes, &tail)? == raw, "REDEEM_CANONICAL")?;
    Ok((st, tail))
}

#[derive(Clone, Debug)]
pub enum WitnessArg {
    Int(i64),
    Bytes(Vec<u8>),
}

#[derive(Clone, Debug)]
pub struct DecodedSpend {
    pub state: ContractState,
    pub tail: Vec<u8>,
    pub raw_redeem: Vec<u8>,
    pub args: BTreeMap<&'static str, WitnessArg>,
}

fn spend_abi_spec(stage: Stage) -> &'static [(&'static str, &'static str)] {
    match stage {
        Stage::MOpen => &[
            ("action", "int"),
            ("ownTailBytes", "int"),
            ("shardTail", "bytes"),
            ("extraBytes", "bytes"),
            ("data", "bytes"),
        ],
        Stage::MSealed => &[
            ("action", "int"),
            ("ownTailBytes", "int"),
            ("nextTail", "bytes"),
            ("data", "bytes"),
        ],
        Stage::MDispatch => &[
            ("k", "int"),
            ("ownTailBytes", "int"),
            ("shardTail", "bytes"),
            ("shardMeta", "bytes"),
        ],
        Stage::SOpen => &[
            ("action", "int"),
            ("qty", "int"),
            ("buyerPk", "bytes"),
            ("ownTailBytes", "int"),
            ("nextTail", "bytes"),
            ("kcc20Suffix", "bytes"),
        ],
        Stage::SFrozen => &[
            ("decision", "int"),
            ("winnerPk", "bytes"),
            ("nextTail", "bytes"),
        ],
        Stage::SPayout => &[("action", "int"), ("kcc20Suffix", "bytes")],
        Stage::SRefund => &[
            ("recIdx", "int"),
            ("ownTailBytes", "int"),
            ("kcc20Suffix", "bytes"),
        ],
    }
}

pub fn decode_spend(signature_script: &str, hashes: &TemplateHashes) -> Result<DecodedSpend> {
    let pushes = script_pushes(&unhex(signature_script)?)?;
    let raw_redeem = pushes
        .last()
        .ok_or_else(|| anyhow!("EMPTY_WITNESS"))?
        .clone();
    let (state, tail) = identify_redeem(&raw_redeem, hashes)?;
    let spec = spend_abi_spec(state.stage());
    check(
        pushes.len() == spec.len() + 1 || pushes.len() == spec.len() + 2,
        "KUSDT_V2_WITNESS_ABI",
    )?;
    let mut args = BTreeMap::new();
    for (i, (name, kind)) in spec.iter().enumerate() {
        let b = &pushes[i];
        if *kind == "int" {
            let n = script_number(b)?;
            check(n >= 0 && n <= i64::MAX as i128, "NEGATIVE_ARGUMENT")?;
            args.insert(*name, WitnessArg::Int(n as i64));
        } else {
            args.insert(*name, WitnessArg::Bytes(b.clone()));
        }
    }
    Ok(DecodedSpend {
        state,
        tail,
        raw_redeem,
        args,
    })
}

pub fn shard_tickets_from_state(s: &ShardState) -> u32 {
    match s {
        ShardState::OpenOrFrozen { records, .. } => {
            records.last().map(|r| r.cum_tickets).unwrap_or(0)
        }
        ShardState::Payout { shard_tickets, .. } => *shard_tickets,
        ShardState::Refund { records, .. } => records.last().map(|r| r.cum_tickets).unwrap_or(0),
    }
}

pub fn redeem_script_hash(raw: &[u8]) -> String {
    hex(&blake2b256(raw))
}

pub fn redeem_p2sh(raw: &[u8]) -> Result<Spk> {
    p2sh(&redeem_script_hash(raw))
}

/// Extract pinned `m_open` tail from embedded `artifacts/kusdt-v2/m_open.json` and verify its template hash.
pub fn pinned_m_open_tail() -> Result<Vec<u8>> {
    let doc: Value = serde_json::from_slice(M_OPEN_ARTIFACT_JSON)?;
    let comp = doc
        .pointer("/contracts/KusdtManagerOpen/compiled")
        .ok_or_else(|| anyhow!("M_OPEN_ARTIFACT_SHAPE"))?;
    let arr = comp
        .get("bytecode")
        .and_then(|b| b.as_array())
        .ok_or_else(|| anyhow!("M_OPEN_ARTIFACT_BYTECODE"))?;
    let mut bc = Vec::with_capacity(arr.len());
    for v in arr {
        let n = v
            .as_u64()
            .filter(|n| *n <= 255)
            .ok_or_else(|| anyhow!("M_OPEN_ARTIFACT_BYTE"))?;
        bc.push(n as u8);
    }
    let (_, tail) = split_redeem_script(&bc)?;
    check(template_hash(&tail)? == M_OPEN_HASH, "M_OPEN_TEMPLATE_HASH")?;
    Ok(tail)
}

/// Decode 115-byte `kusdt-v2` genesis payload:
/// `KASWIN_KUSDT_V2_GENESIS` (23B) || `profileId` (32B) || `m_open` initial state (60B).
pub fn decode_genesis_payload(payload_hex: &str, expected_profile_id: &str) -> Result<ManagerState> {
    let raw = unhex(payload_hex)?;
    check(raw.len() == 115, "KUSDT_V2_GENESIS_PAYLOAD_LEN")?;
    check(&raw[0..23] == GENESIS_PAYLOAD_MAGIC, "KUSDT_V2_GENESIS_MAGIC")?;
    check(
        hex(&raw[23..55]) == expected_profile_id,
        "KUSDT_V2_GENESIS_PROFILE",
    )?;
    let st = decode_manager_state(Stage::MOpen, &raw[55..115])?;
    let h = &st.header;
    check(
        h.opened_count == 0
            && h.cursor == 0
            && h.total_tickets == 0
            && h.decision == Decision::Undecided
            && h.reserved == 0
            && h.price >= PRICE_MIN
            && h.close_daa > 0,
        "KUSDT_V2_GENESIS_INITIAL_STATE",
    )?;
    Ok(st)
}

pub fn manager_genesis_cid(origin: &Outpoint, m: &ManagerState, m_open_tail: &[u8]) -> Result<String> {
    let state_bytes = encode_manager_state(m)?;
    let raw = script_for(&state_bytes, m_open_tail)?;
    let spk = redeem_p2sh(&raw)?;
    covenant_id(
        origin,
        &[AuthorizedOutput {
            index: 0,
            value: MANAGER_DEPOSIT,
            spk: &spk,
        }],
    )
}

pub fn shard_genesis_cid(
    manager_outpoint: &Outpoint,
    shard_output_index: i128,
    s: &ShardState,
    s_open_tail: &[u8],
) -> Result<String> {
    let state_bytes = encode_shard_state(s)?;
    let raw = script_for(&state_bytes, s_open_tail)?;
    let spk = redeem_p2sh(&raw)?;
    covenant_id(
        manager_outpoint,
        &[AuthorizedOutput {
            index: shard_output_index,
            value: SHARD_LEDGER_DEPOSIT,
            spk: &spk,
        }],
    )
}

/// Verify KCC20 vault scriptPubKey against `KCC20_PREFIX || encoded_kcc20_state (112B) || kcc20Suffix`.
pub fn kcc20_vault_spk(
    owner: &[u8; 32],
    owner_scheme: u8,
    amount: i128,
    kcc20_suffix: &[u8],
) -> Result<Spk> {
    check(kcc20_suffix.len() == KCC20_SUFFIX_LEN, "KCC20_SUFFIX_LEN")?;
    let prefix = unhex(KCC20_PREFIX_HEX)?;
    let ext_hash = hex(&blake3_hash(&cat(&[
        &le(prefix.len() as i128, 8)?,
        &prefix,
        &le(kcc20_suffix.len() as i128, 8)?,
        kcc20_suffix,
    ])));
    check(ext_hash == KCC20_EXT_TEMPLATE_HASH, "KCC20_TEMPLATE_HASH")?;
    let mut encoded = Vec::with_capacity(112);
    encoded.push(0x08);
    encoded.extend(le(amount, 8)?);
    encoded.push(0x20);
    encoded.extend_from_slice(owner);
    encoded.extend([0x01, owner_scheme]);
    encoded.extend([0x01, 0x00]); // borrow_scheme = 0x00
    encoded.push(0x20);
    encoded.extend([0u8; 32]); // borrow_guard = ZERO
    encoded.push(0x20);
    encoded.extend([0u8; 32]); // extension_commitment = ZERO
    check(encoded.len() == 112, "KCC20_STATE_LEN")?;
    let redeem = cat(&[&prefix, &encoded, kcc20_suffix]);
    redeem_p2sh(&redeem)
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

/// Verify DRAW anchor proof and return winning ticket index in `0..total_tickets`.
pub fn draw_sample(
    cid: &str,
    outpoint: &Outpoint,
    utxo_daa: i128,
    current_daa: i128,
    total_tickets: u32,
    proof: &[u8],
    accessor: &Accessor,
) -> Result<u32> {
    check(proof.len() == 240, "DRAW_PROOF")?;
    check(total_tickets >= MIN_DRAW_TICKETS, "DRAW_MIN_TICKETS")?;
    check(
        utxo_daa >= 0
            && utxo_daa < DAA_LIMIT - REROLL_TIMEOUT_DAA
            && current_daa >= utxo_daa + DRAW_DELAY_DAA,
        "DRAW_MATURITY",
    )?;
    let boundary = utxo_daa + DRAW_DELAY_DAA;
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
        b"KASWIN_KUSDT_DRAW_V1",
        &unhex_len(cid, 32)?,
        &unhex_len(&outpoint.transaction_id, 32)?,
        &le(outpoint.index, 4)?,
        &le(boundary, 8)?,
        &proof[0..32],
        &seq,
    ]));
    let digest = blake2b256(&cat(&[b"KASWIN_KUSDT_SAMPLE_V1", &seed, &[0u8; 8]]));
    let sample = from_le(&digest[0..7]) as i128;
    let t = total_tickets as i128;
    Ok((sample % t) as u32)
}

pub fn manager_to_json(m: &ManagerState) -> Value {
    let h = &m.header;
    let mut obj = serde_json::Map::new();
    obj.insert("stage".into(), json!(h.stage.as_str()));
    obj.insert("stageCode".into(), json!(h.stage as u8));
    obj.insert("version".into(), json!(h.version));
    obj.insert("creatorPk".into(), json!(h.creator_pk));
    obj.insert("price".into(), json!(h.price.to_string()));
    obj.insert("closeDaa".into(), json!(h.close_daa.to_string()));
    obj.insert("openedCount".into(), json!(h.opened_count));
    obj.insert("cursor".into(), json!(h.cursor));
    obj.insert("totalTickets".into(), json!(h.total_tickets));
    obj.insert("decision".into(), json!(h.decision.as_str()));
    obj.insert("decisionCode".into(), json!(h.decision as u8));
    if matches!(h.stage, Stage::MOpen | Stage::MSealed) {
        obj.insert(
            "entries".into(),
            Value::Array(
                m.entries
                    .iter()
                    .map(|e| {
                        json!({
                            "shardCid": e.shard_cid,
                            "ledgerHash": e.ledger_hash,
                            "cumTickets": e.cum_tickets,
                        })
                    })
                    .collect(),
            ),
        );
    } else {
        obj.insert(
            "winnerPk".into(),
            m.winner_pk
                .as_deref()
                .filter(|w| *w != ZERO32)
                .map(|w| json!(w))
                .unwrap_or(Value::Null),
        );
        obj.insert("shardCids".into(), json!(m.shard_cids));
    }
    Value::Object(obj)
}

pub fn shard_to_json(s: &ShardState) -> Value {
    match s {
        ShardState::OpenOrFrozen {
            stage,
            version,
            manager_cid,
            first_buyer_pk,
            price,
            records,
        } => json!({
            "stage": stage.as_str(),
            "stageCode": *stage as u8,
            "version": version,
            "managerCid": manager_cid,
            "firstBuyerPk": first_buyer_pk,
            "price": price.to_string(),
            "recordCount": records.len(),
            "tickets": records.last().map(|r| r.cum_tickets).unwrap_or(0),
            "records": records.iter().map(|r| json!({
                "cumTickets": r.cum_tickets,
                "buyerPk": r.buyer_pk,
            })).collect::<Vec<_>>(),
        }),
        ShardState::Payout {
            version,
            first_buyer_pk,
            winner_pk,
            vault_amount,
            manager_cid,
            shard_tickets,
        } => json!({
            "stage": Stage::SPayout.as_str(),
            "stageCode": Stage::SPayout as u8,
            "version": version,
            "firstBuyerPk": first_buyer_pk,
            "winnerPk": winner_pk,
            "vaultAmount": vault_amount.to_string(),
            "managerCid": manager_cid,
            "shardTickets": shard_tickets,
        }),
        ShardState::Refund {
            version,
            first_buyer_pk,
            price,
            record_count,
            claimed_mask,
            records,
        } => json!({
            "stage": Stage::SRefund.as_str(),
            "stageCode": Stage::SRefund as u8,
            "version": version,
            "firstBuyerPk": first_buyer_pk,
            "price": price.to_string(),
            "recordCount": record_count,
            "claimedMask": claimed_mask,
            "records": records.iter().map(|r| json!({
                "cumTickets": r.cum_tickets,
                "buyerPk": r.buyer_pk,
            })).collect::<Vec<_>>(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_manager_states() {
        let m_open = ManagerState {
            header: ManagerHeader {
                stage: Stage::MOpen,
                version: VERSION,
                creator_pk: "11".repeat(32),
                price: 1_000_000,
                close_daa: 600_000_000,
                opened_count: 1,
                cursor: 0,
                total_tickets: 0,
                decision: Decision::Undecided,
                reserved: 0,
            },
            entries: vec![ManagerEntry {
                shard_cid: "22".repeat(32),
                ledger_hash: ZERO32.to_string(),
                cum_tickets: 0,
            }],
            winner_pk: None,
            shard_cids: vec![],
        };
        let enc = encode_manager_state(&m_open).unwrap();
        assert_eq!(enc.len(), 60 + 68);
        assert_eq!(decode_manager_state(Stage::MOpen, &enc).unwrap(), m_open);

        let m_disp = ManagerState {
            header: ManagerHeader {
                stage: Stage::MDispatch,
                version: VERSION,
                creator_pk: "11".repeat(32),
                price: 1_000_000,
                close_daa: 600_000_000,
                opened_count: 2,
                cursor: 1,
                total_tickets: 5,
                decision: Decision::Payout,
                reserved: 0,
            },
            entries: vec![],
            winner_pk: Some("33".repeat(32)),
            shard_cids: vec!["aa".repeat(32), "bb".repeat(32)],
        };
        let enc_d = encode_manager_state(&m_disp).unwrap();
        assert_eq!(enc_d.len(), 92 + 64);
        assert_eq!(decode_manager_state(Stage::MDispatch, &enc_d).unwrap(), m_disp);
    }

    #[test]
    fn roundtrip_shard_states() {
        let s_open = ShardState::OpenOrFrozen {
            stage: Stage::SOpen,
            version: VERSION,
            manager_cid: "11".repeat(32),
            first_buyer_pk: "22".repeat(32),
            price: 1_000_000,
            records: vec![TicketRecord {
                cum_tickets: 3,
                buyer_pk: "22".repeat(32),
            }],
        };
        let enc = encode_shard_state(&s_open).unwrap();
        assert_eq!(enc.len(), 76 + 36);
        assert_eq!(decode_shard_state(Stage::SOpen, &enc).unwrap(), s_open);

        let s_payout = ShardState::Payout {
            version: VERSION,
            first_buyer_pk: "22".repeat(32),
            winner_pk: "33".repeat(32),
            vault_amount: 3_000_000,
            manager_cid: "11".repeat(32),
            shard_tickets: 3,
        };
        let enc_p = encode_shard_state(&s_payout).unwrap();
        assert_eq!(enc_p.len(), 110);
        assert_eq!(decode_shard_state(Stage::SPayout, &enc_p).unwrap(), s_payout);

        let s_refund = ShardState::Refund {
            version: VERSION,
            first_buyer_pk: "22".repeat(32),
            price: 1_000_000,
            record_count: 2,
            claimed_mask: 1,
            records: vec![
                TicketRecord {
                    cum_tickets: 1,
                    buyer_pk: "22".repeat(32),
                },
                TicketRecord {
                    cum_tickets: 2,
                    buyer_pk: "44".repeat(32),
                },
            ],
        };
        let enc_r = encode_shard_state(&s_refund).unwrap();
        assert_eq!(enc_r.len(), 44 + 72);
        assert_eq!(decode_shard_state(Stage::SRefund, &enc_r).unwrap(), s_refund);
    }

    #[test]
    fn embedded_m_open_tail_and_genesis_payload_verify() {
        let tail = pinned_m_open_tail().unwrap();
        assert_eq!(tail.len(), 8688);
        let init = ManagerState {
            header: ManagerHeader {
                stage: Stage::MOpen,
                version: VERSION,
                creator_pk: "11".repeat(32),
                price: PRICE_MIN,
                close_daa: 600_000_000,
                opened_count: 0,
                cursor: 0,
                total_tickets: 0,
                decision: Decision::Undecided,
                reserved: 0,
            },
            entries: vec![],
            winner_pk: None,
            shard_cids: vec![],
        };
        let state_bytes = encode_manager_state(&init).unwrap();
        assert_eq!(state_bytes.len(), 60);
        let mut payload = Vec::new();
        payload.extend_from_slice(GENESIS_PAYLOAD_MAGIC);
        payload.extend_from_slice(&unhex_len(M_OPEN_HASH, 32).unwrap());
        payload.extend_from_slice(&state_bytes);
        assert_eq!(payload.len(), 115);
        let decoded = decode_genesis_payload(&hex(&payload), M_OPEN_HASH).unwrap();
        assert_eq!(decoded, init);
    }

    #[test]
    fn draw_sample_accepts_240b_pass_a_proof_and_matches_open_seq() {
        let mut proof = vec![0u8; 240];
        // Target block hash at [0..32]
        proof[0..32].fill(0x11);
        // Td opening: [32..64] right sibling, [64..96] left sibling, [96..120] mergeset ctx (nonce=1, daa=1000, blue=200)
        proof[32..64].fill(0x22);
        proof[64..96].fill(0x33);
        proof[96..104].copy_from_slice(&1u64.to_le_bytes());
        proof[104..112].copy_from_slice(&1000u64.to_le_bytes());
        proof[112..120].copy_from_slice(&200u64.to_le_bytes());
        // Parent block hash at [120..152]
        proof[120..152].fill(0x44);
        // Parent opening: [152..184] left, [184..216] right, [216..240] mergeset ctx (nonce=2, daa=999, blue=199)
        proof[152..184].fill(0x55);
        proof[184..216].fill(0x66);
        proof[216..224].copy_from_slice(&2u64.to_le_bytes());
        proof[224..232].copy_from_slice(&999u64.to_le_bytes());
        proof[232..240].copy_from_slice(&199u64.to_le_bytes());

        let seq1 = open_seq(&proof, 152, &proof[120..152]).unwrap();
        let seq = open_seq(&proof, 32, &seq1).unwrap();
        let acc = Accessor {
            block_hash: hex(&proof[0..32]),
            sequence_commitment: hex(&seq),
        };
        let cid = "aa".repeat(32);
        let op = Outpoint {
            transaction_id: "bb".repeat(32),
            index: 0,
        };
        let utxo_daa = 1000 - DRAW_DELAY_DAA;
        let current_daa = 1000;
        let ticket = draw_sample(&cid, &op, utxo_daa, current_daa, 7, &proof, &acc).unwrap();
        assert!(ticket < 7);
    }
}
