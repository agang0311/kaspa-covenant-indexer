//! Binary helpers mirroring `scripts/core/src/bytes.ts`. Error messages are the same codes as the TypeScript core
//! (`KaswinError.code`), so a STALE record written by this build reads exactly like one written by the JS build.
use anyhow::{Result, anyhow};

/// Fail with `code` unless `ok`.
#[inline]
pub fn check(ok: bool, code: &str) -> Result<()> {
    if ok {
        Ok(())
    } else {
        Err(anyhow!(code.to_string()))
    }
}

/// `uint(value, bits)`: 0 <= value < 2^bits.
pub fn uint(value: i128, bits: u32) -> Result<i128> {
    check(
        value >= 0 && (bits >= 127 || value < (1i128 << bits)),
        "INTEGER_RANGE",
    )?;
    Ok(value)
}

/// `integer(n, min, max)`: inclusive range (all call sites pass values well inside the JS safe-integer domain).
pub fn integer(n: i128, min: i128, max: i128) -> Result<i128> {
    check(
        n >= min && n <= max && n.unsigned_abs() <= (1u128 << 53) - 1,
        "INTEGER_RANGE",
    )?;
    Ok(n)
}

pub fn hex(bytes: &[u8]) -> String {
    hex::encode(bytes)
}

/// Canonical (lower-case, even length) hex only, like the TS core's `unhex`.
pub fn unhex(value: &str) -> Result<Vec<u8>> {
    check(
        value.len() % 2 == 0
            && value
                .bytes()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)),
        "NONCANONICAL_HEX",
    )?;
    Ok(hex::decode(value).expect("validated hex"))
}

pub fn unhex_len(value: &str, len: usize) -> Result<Vec<u8>> {
    let out = unhex(value)?;
    check(out.len() == len, "BYTE_LENGTH")?;
    Ok(out)
}

pub fn cat(parts: &[&[u8]]) -> Vec<u8> {
    let mut out = Vec::with_capacity(parts.iter().map(|p| p.len()).sum());
    for p in parts {
        out.extend_from_slice(p);
    }
    out
}

pub fn ascii(s: &str) -> Vec<u8> {
    s.as_bytes().to_vec()
}

/// Little-endian fixed width; `uint(n, width * 8)` first.
pub fn le(n: i128, width: usize) -> Result<Vec<u8>> {
    uint(n, (width * 8) as u32)?;
    let mut v = n as u128;
    let mut b = vec![0u8; width];
    for x in b.iter_mut() {
        *x = (v & 0xff) as u8;
        v >>= 8;
    }
    Ok(b)
}

pub fn from_le(b: &[u8]) -> u128 {
    let mut n: u128 = 0;
    for &x in b.iter().rev() {
        n = (n << 8) | x as u128;
    }
    n
}

pub fn same(a: &[u8], b: &[u8]) -> bool {
    a == b
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn le_roundtrip_and_range() {
        assert_eq!(le(0x0102, 2).unwrap(), vec![2, 1]);
        assert_eq!(from_le(&[2, 1]), 0x0102);
        assert!(le(256, 1).is_err());
        assert!(le(-1, 4).is_err());
        assert_eq!(le((1i128 << 64) - 1, 8).unwrap(), vec![0xff; 8]);
    }
    #[test]
    fn unhex_is_canonical_only() {
        assert!(unhex("AB").is_err());
        assert!(unhex("abc").is_err());
        assert_eq!(unhex("00ff").unwrap(), vec![0, 255]);
    }
}
