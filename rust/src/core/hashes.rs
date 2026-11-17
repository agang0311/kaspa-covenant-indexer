//! BLAKE2b-256 (digest-size 32 in the parameter block, optional key) and BLAKE3 (plain / keyed "domain" hash),
//! equivalent to `scripts/core/src/{hashes,blake3}.ts`. Uses the audited `blake2b_simd` and `blake3` crates.
use super::bytes::check;
use anyhow::Result;

pub fn blake2b256(data: &[u8]) -> [u8; 32] {
    blake2b256_keyed(data, &[])
}

pub fn blake2b256_keyed(data: &[u8], key: &[u8]) -> [u8; 32] {
    let h = blake2b_simd::Params::new()
        .hash_length(32)
        .key(key)
        .hash(data);
    let mut out = [0u8; 32];
    out.copy_from_slice(h.as_bytes());
    out
}

pub fn blake3_hash(data: &[u8]) -> [u8; 32] {
    *blake3::hash(data).as_bytes()
}

/// `domainHash(tag, data)`: BLAKE3 keyed with the ASCII tag zero-padded to 32 bytes.
pub fn domain_hash(tag: &str, data: &[u8]) -> Result<[u8; 32]> {
    let t = tag.as_bytes();
    check(t.len() <= 32, "BLAKE3_DOMAIN_LENGTH")?;
    let mut key = [0u8; 32];
    key[..t.len()].copy_from_slice(t);
    Ok(*blake3::keyed_hash(&key, data).as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn blake2b_known_vectors() {
        // BLAKE2b-256("") and keyed with "CovenantID" are cross-checked against the TS core in the differential tests;
        // the empty-input digest is the published BLAKE2b-256 value.
        assert_eq!(
            hex::encode(blake2b256(b"")),
            "0e5751c026e543b2e8ab2eb06099daa1d1e5df47778f7787faab45cdf12fe3a8"
        );
    }
}
