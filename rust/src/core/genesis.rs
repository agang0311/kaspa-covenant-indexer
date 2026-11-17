//! Pure Genesis announcement authentication, contract V2.0.2 (`scripts/core/src/genesis-discovery.ts`).
//! The caller separately establishes acceptance/live/reorg facts.
use super::bytes::{check, hex, same, unhex};
use super::covenant::{Outpoint, Spk, p2sh};
use super::hashes::blake2b256;
use super::state::{self as S, Ledger, Profile};
use anyhow::Result;

pub struct GenesisEvidence<'a> {
    pub accepted: bool,
    pub payload: &'a str,
    pub authorizing_outpoint: &'a Outpoint,
    pub output_index: i128,
    pub value: i128,
    pub spk: &'a Spk,
    pub covenant_id: &'a str,
    pub authorizing_input: i128,
    /// All outputs sharing this CID, not all ordinary outputs.
    pub covenant_output_indices: Vec<usize>,
}

pub const GENESIS_TAG: &str = "KASWIN_GENESIS_V2";

pub fn verify_genesis_announcement(e: &GenesisEvidence, p: &Profile) -> Result<Ledger> {
    check(e.accepted, "GENESIS_NOT_ACCEPTED")?;
    let bytes = unhex(e.payload)?;
    let tag = GENESIS_TAG.as_bytes();
    check(
        bytes.len() == tag.len() + 32 + S::HEADER_BYTES,
        "GENESIS_ANNOUNCEMENT_LENGTH",
    )?;
    check(same(&bytes[..tag.len()], tag), "GENESIS_ANNOUNCEMENT_TAG")?;
    check(
        hex(&bytes[tag.len()..tag.len() + 32]) == p.id(),
        "GENESIS_PROFILE",
    )?;
    let ledger = &bytes[tag.len() + 32..];
    let s = S::decode_ledger(ledger)?;
    S::validate_ledger(&s)?;
    let initial = S::new_open(&s.owner_key, &s.config);
    check(
        same(ledger, &S::encode_ledger(&initial)?),
        "NONCANONICAL_GENESIS_STATE",
    )?;
    check(
        e.output_index == 0 && e.authorizing_input == 0 && e.value == S::DEPOSIT,
        "GENESIS_OUTPUT",
    )?;
    check(
        e.covenant_output_indices.len() == 1 && e.covenant_output_indices[0] == 0,
        "GENESIS_OUTPUT_GROUP",
    )?;
    let script = S::root_script(&s, p)?;
    let spk = p2sh(&hex(&blake2b256(&script)))?;
    check(
        e.spk.version == spk.version && e.spk.script == spk.script,
        "GENESIS_SPK",
    )?;
    check(
        S::root_id(e.authorizing_outpoint, &script)? == e.covenant_id,
        "OPEN_GENESIS_CID_MISMATCH",
    )?;
    Ok(s)
}
