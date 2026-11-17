//! Kaswin KW21 ledger codec and Profile identity, contract V2.0.2 (`scripts/core/src/state.ts`).
//! Fixed 80-byte header + 36 bytes per purchase record. A Profile selects exactly one codec.
use super::bytes::{ascii, cat, check, from_le, hex, integer, le, same, unhex, unhex_len};
use super::covenant::{AuthorizedOutput, Outpoint, Spk, covenant_id, p2sh};
use super::hashes::{blake2b256, blake3_hash};
use anyhow::Result;

pub const CONTRACT_VERSION: &str = "V2.0.2";
pub const APPROVED_PROFILE_ID: &str =
    "ed0387c9e77bb4e0a8f1f4654a326c316d83663f455283ec6a046a095c94856d";
pub const COMPILER_COMMIT: &str = "3ed973335b59269293564805cc2c58a14595ec03";
pub const DEPOSIT: i128 = 400_000_000;
pub const MIN_PRICE: i128 = 100_000_000;
pub const MAX_REFUND_RECORDS: i128 = 32;
pub const FINALIZER: i128 = 100_000_000;
pub const MAX_PAY_FEE: i128 = 50_000_000;
pub const REFUND_FEE: i128 = 2_000_000;
pub const DRAW_DELAY: i128 = 100;
pub const TIMEOUT_DELAY: i128 = 432_000;
pub const DAA_LIMIT: i128 = 500_000_000_000;
pub const MAX_PURCHASES: i128 = 256;
pub const MAX_TICKETS: i128 = 100_000;
pub const VALUE_LIMIT: i128 = 9_000_000_000_000_000;
pub const INPUT_LIMIT: usize = 9;
pub const ZERO: &str = "0000000000000000000000000000000000000000000000000000000000000000";

// Ledger layout (projection of contracts/ledger-layout.json).
pub const MAGIC: &str = "KW21";
pub const OFFSET_MAGIC: usize = 0;
pub const OFFSET_PHASE: usize = 4;
pub const OFFSET_TICKET_PRICE: usize = 8;
pub const OFFSET_TICKET_CAP: usize = 16;
pub const OFFSET_PURCHASE_CAP: usize = 20;
pub const OFFSET_MIN_TICKETS: usize = 24;
pub const OFFSET_CLOSE_ELIGIBLE_DAA: usize = 28;
pub const OFFSET_SOLD: usize = 36;
pub const OFFSET_PURCHASE_COUNT: usize = 40;
pub const OFFSET_REFUND_CURSOR: usize = 44;
pub const OFFSET_OWNER_KEY: usize = 48;
pub const HEADER_BYTES: usize = 80;
pub const RECORD_BYTES: usize = 36;
pub const RECORD_OFFSET_END: usize = 0;
pub const RECORD_OFFSET_BUYER_KEY: usize = 4;
pub const U32_BYTES: usize = 4;
pub const U63_BYTES: usize = 8;
pub const KEY_BYTES: usize = 32;

pub const PHASE_OPEN: i128 = 1;
pub const PHASE_SEALED: i128 = 2;
pub const PHASE_REFUNDING: i128 = 5;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Module {
    Open,
    Sealed,
    Refunding,
}
pub const MODULES: [Module; 3] = [Module::Open, Module::Sealed, Module::Refunding];

impl Module {
    pub fn name(self) -> &'static str {
        match self {
            Module::Open => "open",
            Module::Sealed => "sealed",
            Module::Refunding => "refunding",
        }
    }
    pub fn idx(self) -> usize {
        match self {
            Module::Open => 0,
            Module::Sealed => 1,
            Module::Refunding => 2,
        }
    }
    /// Per-template output caps retained in V2.0.2.
    pub fn output_limit(self) -> usize {
        match self {
            Module::Open => 2,
            Module::Sealed => 3,
            Module::Refunding => 34,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    pub ticket_price: i128,
    pub ticket_cap: i128,
    pub purchase_cap: i128,
    pub min_tickets: i128,
    pub close_eligible_daa: i128,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ledger {
    pub phase: i128,
    pub owner_key: String,
    pub config: Config,
    pub sold: i128,
    pub purchase_count: i128,
    pub cursor: i128,
    pub directory: Vec<u8>,
}

#[derive(Clone, Debug)]
pub struct Frame {
    pub tail: Vec<u8>,
    pub template_hash: String,
    pub dispatch_tag: String,
    pub source_sha256: String,
}

/// A Profile can only be built through [`Profile::new`], which recomputes the identity from the actual bytecode and
/// requires the approved id. Frames are immutable afterwards, so the per-operation re-check the TS core performs
/// (needed there because JS objects and Uint8Arrays are mutable) is unnecessary here.
#[derive(Clone, Debug)]
pub struct Profile {
    id: String,
    frames: [Frame; 3],
    compiler_commit: String,
}

impl Profile {
    pub fn new(frames: [Frame; 3]) -> Result<Profile> {
        let id = profile_id(&frames)?;
        check(APPROVED_PROFILE_ID.len() == 64, "PROFILE_NOT_APPROVED")?;
        check(id == APPROVED_PROFILE_ID, "INCOMPATIBLE_PROFILE")?;
        Ok(Profile {
            id,
            frames,
            compiler_commit: COMPILER_COMMIT.to_string(),
        })
    }
    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn compiler_commit(&self) -> &str {
        &self.compiler_commit
    }
    pub fn frame(&self, m: Module) -> &Frame {
        &self.frames[m.idx()]
    }
}

pub struct Snapshot {
    pub ledger: Vec<u8>,
    pub tip: Outpoint,
    pub script_public_key: Spk,
    pub covenant_id: String,
    pub value: i128,
    pub utxo_daa: i128,
    pub current_daa: i128,
    pub origin: Option<Outpoint>,
}

pub fn check_phase(phase: i128) -> Result<i128> {
    check(
        phase == PHASE_OPEN || phase == PHASE_SEALED || phase == PHASE_REFUNDING,
        "PHASE",
    )?;
    Ok(phase)
}

pub fn phase_module(phase: i128) -> Result<Module> {
    check_phase(phase)?;
    Ok(if phase == PHASE_OPEN {
        Module::Open
    } else if phase == PHASE_REFUNDING {
        Module::Refunding
    } else {
        Module::Sealed
    })
}

pub fn template_hash(tail: &[u8]) -> Result<String> {
    check(tail.first() == Some(&0x6c), "TEMPLATE_BOUNDARY")?;
    Ok(hex(&blake3_hash(&cat(&[
        &le(1, 8)?,
        &[0x6b],
        &le(tail.len() as i128, 8)?,
        tail,
    ]))))
}

/// Recompute identity from actual bytecode, not caller-provided template hashes.
pub fn profile_id(frames: &[Frame; 3]) -> Result<String> {
    for f in frames {
        check(template_hash(&f.tail)? == f.template_hash, "ARTIFACT_HASH")?;
    }
    let distinct: std::collections::HashSet<&str> =
        frames.iter().map(|f| f.template_hash.as_str()).collect();
    check(distinct.len() == 3, "FRAME_COLLISION")?;
    let mut pre = ascii("KASWIN_PROFILE_V2");
    for f in frames {
        pre.extend(unhex_len(&f.source_sha256, 32)?);
        pre.extend(unhex_len(&f.template_hash, 32)?);
        pre.extend(unhex_len(&f.dispatch_tag, 4)?);
    }
    Ok(hex(&blake2b256(&pre)))
}

/// Data push encoding matching the pinned compiler (also used for arbitrary witness items).
pub fn push_bytes(b: &[u8]) -> Result<Vec<u8>> {
    let n = b.len();
    check(n <= 0x7fff_ffff, "PUSH_LIMIT")?;
    let mut out = if n == 0 {
        vec![0]
    } else if n <= 75 {
        vec![n as u8]
    } else if n <= 255 {
        vec![0x4c, n as u8]
    } else if n <= 65535 {
        cat(&[&[0x4d], &le(n as i128, 2)?])
    } else {
        cat(&[&[0x4e], &le(n as i128, 4)?])
    };
    out.extend_from_slice(b);
    Ok(out)
}

pub fn script_for(ledger: &[u8], tail: &[u8]) -> Result<Vec<u8>> {
    check(tail.first() == Some(&0x6c), "TEMPLATE_BOUNDARY")?;
    Ok(cat(&[&[0x6b], &push_bytes(ledger)?, tail]))
}

pub fn new_open(owner_key: &str, config: &Config) -> Ledger {
    Ledger {
        phase: PHASE_OPEN,
        owner_key: owner_key.to_string(),
        config: config.clone(),
        sold: 0,
        purchase_count: 0,
        cursor: 0,
        directory: vec![],
    }
}

pub fn encode_ledger(s: &Ledger) -> Result<Vec<u8>> {
    let p = cat(&[
        &ascii(MAGIC),
        &le(s.phase, U32_BYTES)?,
        &le(s.config.ticket_price, U63_BYTES)?,
        &le(s.config.ticket_cap, U32_BYTES)?,
        &le(s.config.purchase_cap, U32_BYTES)?,
        &le(s.config.min_tickets, U32_BYTES)?,
        &le(s.config.close_eligible_daa, U63_BYTES)?,
        &le(s.sold, U32_BYTES)?,
        &le(s.purchase_count, U32_BYTES)?,
        &le(s.cursor, U32_BYTES)?,
        &unhex_len(&s.owner_key, KEY_BYTES)?,
        &s.directory,
    ]);
    check(p.len() == HEADER_BYTES + s.directory.len(), "STATE_LAYOUT")?;
    Ok(p)
}

pub fn decode_ledger(b: &[u8]) -> Result<Ledger> {
    check(
        b.len() >= HEADER_BYTES && b.len() <= HEADER_BYTES + MAX_PURCHASES as usize * RECORD_BYTES,
        "LEDGER_LENGTH",
    )?;
    check(
        same(&b[OFFSET_MAGIC..OFFSET_PHASE], MAGIC.as_bytes()),
        "STATE_MAGIC",
    )?;
    let u = |a: usize, w: usize| -> Result<i128> {
        let n = from_le(&b[a..a + w]);
        check(n < (1u128 << (w * 8 - 1)), "SIGNED_DOMAIN")?;
        Ok(n as i128)
    };
    let n = |a: usize| u(a, U32_BYTES);
    let pc = n(OFFSET_PURCHASE_COUNT)?;
    check(
        pc <= MAX_PURCHASES && b.len() == HEADER_BYTES + RECORD_BYTES * pc as usize,
        "DIRECTORY_LENGTH",
    )?;
    Ok(Ledger {
        phase: n(OFFSET_PHASE)?,
        config: Config {
            ticket_price: u(OFFSET_TICKET_PRICE, U63_BYTES)?,
            ticket_cap: n(OFFSET_TICKET_CAP)?,
            purchase_cap: n(OFFSET_PURCHASE_CAP)?,
            min_tickets: n(OFFSET_MIN_TICKETS)?,
            close_eligible_daa: u(OFFSET_CLOSE_ELIGIBLE_DAA, U63_BYTES)?,
        },
        sold: n(OFFSET_SOLD)?,
        purchase_count: pc,
        cursor: n(OFFSET_REFUND_CURSOR)?,
        owner_key: hex(&b[OFFSET_OWNER_KEY..HEADER_BYTES]),
        directory: b[HEADER_BYTES..].to_vec(),
    })
}

pub fn validate_config(c: &Config) -> Result<()> {
    integer(c.ticket_cap, 3, MAX_TICKETS)?;
    integer(c.purchase_cap, 1, MAX_PURCHASES)?;
    integer(c.min_tickets, 3, c.ticket_cap)?;
    check(
        c.ticket_price >= MIN_PRICE && c.ticket_price <= (VALUE_LIMIT - DEPOSIT) / c.ticket_cap,
        "TICKET_PRICE",
    )?;
    check(
        c.close_eligible_daa > 0 && c.close_eligible_daa < DAA_LIMIT - TIMEOUT_DELAY,
        "CLOSE_DAA",
    )?;
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Record {
    pub end: i128,
    pub key: String,
    pub count: i128,
}

pub fn records(s: &Ledger) -> Result<Vec<Record>> {
    check(
        s.directory.len() as i128 == s.purchase_count * RECORD_BYTES as i128,
        "DIRECTORY_LENGTH",
    )?;
    let mut previous = 0i128;
    let mut out = Vec::with_capacity(s.purchase_count as usize);
    for i in 0..s.purchase_count as usize {
        let at = i * RECORD_BYTES;
        let end =
            from_le(&s.directory[at + RECORD_OFFSET_END..at + RECORD_OFFSET_BUYER_KEY]) as i128;
        integer(end, previous + 1, s.sold)?;
        out.push(Record {
            end,
            key: hex(&s.directory[at + RECORD_OFFSET_BUYER_KEY..at + RECORD_BYTES]),
            count: end - previous,
        });
        previous = end;
    }
    Ok(out)
}

pub fn validate_ledger(s: &Ledger) -> Result<()> {
    check_phase(s.phase)?;
    unhex_len(&s.owner_key, KEY_BYTES)?;
    validate_config(&s.config)?;
    integer(s.sold, 0, s.config.ticket_cap)?;
    integer(s.purchase_count, 0, s.config.purchase_cap.min(s.sold))?;
    check((s.sold == 0) == (s.purchase_count == 0), "EMPTY_MISMATCH")?;
    let rs = records(s)?;
    check(
        rs.last().map(|r| r.end).unwrap_or(0) == s.sold,
        "DIRECTORY_TOTAL",
    )?;
    if s.phase == PHASE_REFUNDING {
        integer(s.cursor, 0, s.purchase_count - 1)?;
    } else {
        check(s.cursor == 0, "CURSOR_NONREFUND")?;
    }
    if s.phase == PHASE_SEALED {
        check(s.sold >= s.config.min_tickets, "DRAW_BELOW_MINIMUM")?;
    }
    Ok(())
}

/// Tickets already refunded (records before the cursor).
pub fn paid_tickets(s: &Ledger) -> Result<i128> {
    if s.cursor == 0 {
        return Ok(0);
    }
    let rs = records(s)?;
    Ok(rs
        .get((s.cursor - 1) as usize)
        .map(|r| r.end)
        .ok_or_else(|| anyhow::anyhow!("RECORD_INDEX"))?)
}

pub fn value_of(s: &Ledger) -> Result<i128> {
    Ok(DEPOSIT + (s.sold - paid_tickets(s)?) * s.config.ticket_price)
}

/// Canonical zero-ticket OPEN script the OPEN contract rebuilds to authenticate the round CID.
pub fn root_script(s: &Ledger, p: &Profile) -> Result<Vec<u8>> {
    script_for(
        &encode_ledger(&new_open(&s.owner_key, &s.config))?,
        &p.frame(Module::Open).tail,
    )
}

pub fn root_id(origin: &Outpoint, genesis_script: &[u8]) -> Result<String> {
    let spk = p2sh(&hex(&blake2b256(genesis_script)))?;
    covenant_id(
        origin,
        &[AuthorizedOutput {
            index: 0,
            value: DEPOSIT,
            spk: &spk,
        }],
    )
}

pub fn script_of(s: &Ledger, p: &Profile) -> Result<Vec<u8>> {
    script_for(&encode_ledger(s)?, &p.frame(phase_module(s.phase)?).tail)
}

/// Client bootstrap trust: every live phase must match the canonical OPEN Genesis CID.
pub fn verify_snapshot(x: &Snapshot, p: &Profile) -> Result<Ledger> {
    unhex_len(&x.tip.transaction_id, 32)?;
    integer(x.tip.index, 0, 0x7fff_ffff)?;
    let origin = x
        .origin
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("ORIGIN_REQUIRED"))?;
    unhex_len(&origin.transaction_id, 32)?;
    check(origin.transaction_id != ZERO, "ORIGIN_ZERO")?;
    integer(origin.index, 0, 0x7fff_ffff)?;
    unhex_len(&x.covenant_id, 32)?;
    check(x.utxo_daa >= 0 && x.utxo_daa < DAA_LIMIT, "UTXO_DAA")?;
    check(
        x.current_daa >= 0 && x.current_daa < DAA_LIMIT,
        "CURRENT_DAA",
    )?;
    let s = decode_ledger(&x.ledger)?;
    validate_ledger(&s)?;
    let root = root_script(&s, p)?;
    check(
        root_id(origin, &root)? == x.covenant_id,
        "OPEN_GENESIS_CID_MISMATCH",
    )?;
    check(x.value == value_of(&s)?, "STATE_INPUT_AMOUNT")?;
    let expected = p2sh(&hex(&blake2b256(&script_of(&s, p)?)))?;
    check(x.script_public_key == expected, "STATE_INPUT_SPK")?;
    Ok(s)
}

pub fn append_purchase(s: &Ledger, quantity: i128, buyer: &str) -> Result<Ledger> {
    validate_ledger(s)?;
    check(s.phase == PHASE_OPEN, "NOT_OPEN")?;
    integer(quantity, 1, s.config.ticket_cap - s.sold)?;
    check(s.purchase_count < s.config.purchase_cap, "DIRECTORY_FULL")?;
    let key = unhex_len(buyer, KEY_BYTES)?;
    let sold = s.sold + quantity;
    let mut directory = s.directory.clone();
    directory.extend(le(sold, U32_BYTES)?);
    directory.extend(key);
    Ok(Ledger {
        sold,
        purchase_count: s.purchase_count + 1,
        directory,
        ..s.clone()
    })
}

#[allow(dead_code)]
pub fn ensure_hex(value: &str) -> Result<()> {
    unhex(value).map(|_| ())
}
