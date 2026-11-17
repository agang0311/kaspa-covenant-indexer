//! KIP-20 covenant id preimage and standard script templates (`scripts/core/src/covenant-id.ts`).
use super::bytes::{cat, check, hex, integer, le, uint, unhex, unhex_len};
use super::hashes::blake2b256_keyed;
use anyhow::Result;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outpoint {
    pub transaction_id: String,
    pub index: i128,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Spk {
    pub version: i128,
    pub script: String,
}

pub struct AuthorizedOutput<'a> {
    pub index: i128,
    pub value: i128,
    pub spk: &'a Spk,
}

pub fn p2sh(script_hash: &str) -> Result<Spk> {
    unhex_len(script_hash, 32)?;
    Ok(Spk {
        version: 0,
        script: format!("aa20{script_hash}87"),
    })
}

pub fn p2pk(public_key: &str) -> Result<Spk> {
    unhex_len(public_key, 32)?;
    Ok(Spk {
        version: 0,
        script: format!("20{public_key}ac"),
    })
}

pub fn covenant_preimage(origin: &Outpoint, outputs: &[AuthorizedOutput]) -> Result<Vec<u8>> {
    let txid = unhex_len(&origin.transaction_id, 32)?;
    integer(origin.index, 0, 0xffff_ffff)?;
    check(!outputs.is_empty() && outputs.len() <= 65536, "AUTH_COUNT")?;
    let mut out = cat(&[&txid, &le(origin.index, 4)?, &le(outputs.len() as i128, 8)?]);
    let mut previous: i128 = -1;
    for o in outputs {
        integer(o.index, 0, 0xffff_ffff)?;
        check(o.index > previous, "AUTH_ORDER")?;
        previous = o.index;
        uint(o.value, 64)?;
        integer(o.spk.version, 0, 65535)?;
        let script = unhex(&o.spk.script)?;
        check(script.len() <= 1_000_000, "SPK_LIMIT")?;
        out.extend(le(o.index, 4)?);
        out.extend(le(o.value, 8)?);
        out.extend(le(o.spk.version, 2)?);
        out.extend(le(script.len() as i128, 8)?);
        out.extend(script);
    }
    Ok(out)
}

pub fn covenant_id(origin: &Outpoint, outputs: &[AuthorizedOutput]) -> Result<String> {
    Ok(hex(&blake2b256_keyed(
        &covenant_preimage(origin, outputs)?,
        b"CovenantID",
    )))
}
