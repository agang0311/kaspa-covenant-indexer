//! Kaspa Bech32 (CashAddr-style) address codec. P2PK (version 0) and P2SH (version 8), any network prefix.
//! The prefix is presentation only (the same x-only key yields the same SPK on every network), so every decode
//! must pass the network's own prefix.
use anyhow::{Result, bail};

const CHARSET: &[u8; 32] = b"qpzry9x8gf2tvdw0s3jn54khce6mua7l";

fn polymod(values: &[u8]) -> u64 {
    let mut c: u64 = 1;
    for &d in values {
        let c0 = c >> 35;
        c = ((c & 0x07_ffff_ffff) << 5) ^ d as u64;
        if c0 & 0x01 != 0 {
            c ^= 0x98_f2bc_8e61;
        }
        if c0 & 0x02 != 0 {
            c ^= 0x79_b76d_99e2;
        }
        if c0 & 0x04 != 0 {
            c ^= 0xf3_3e5f_b3c4;
        }
        if c0 & 0x08 != 0 {
            c ^= 0xae_2eab_e2a8;
        }
        if c0 & 0x10 != 0 {
            c ^= 0x1e_4f43_e470;
        }
    }
    c ^ 1
}

fn checksum(payload5: &[u8], prefix: &str) -> u64 {
    let mut v: Vec<u8> = prefix.bytes().map(|c| c & 0x1f).collect();
    v.push(0);
    v.extend_from_slice(payload5);
    v.extend_from_slice(&[0; 8]);
    polymod(&v)
}

fn conv8to5(bytes: &[u8]) -> Vec<u8> {
    let (mut out, mut buff, mut bits) = (Vec::new(), 0u32, 0u32);
    for &b in bytes {
        buff = (buff << 8) | b as u32;
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(((buff >> bits) & 31) as u8);
            buff &= (1 << bits) - 1;
        }
    }
    if bits > 0 {
        out.push(((buff << (5 - bits)) & 31) as u8);
    }
    out
}

fn conv5to8(five: &[u8]) -> Vec<u8> {
    let (mut out, mut buff, mut bits) = (Vec::new(), 0u32, 0u32);
    for &c in five {
        buff = (buff << 5) | c as u32;
        bits += 5;
        while bits >= 8 {
            bits -= 8;
            out.push(((buff >> bits) & 255) as u8);
            buff &= (1 << bits) - 1;
        }
    }
    out
}

pub fn encode_address(version: u8, payload: &[u8], prefix: &str) -> String {
    let mut data = vec![version];
    data.extend_from_slice(payload);
    let five = conv8to5(&data);
    let sum = checksum(&five, prefix);
    let sum_bytes: Vec<u8> = (0..5)
        .rev()
        .map(|i| ((sum >> (i * 8)) & 0xff) as u8)
        .collect();
    let mut s = String::from(prefix);
    s.push(':');
    for i in five.iter().chain(conv8to5(&sum_bytes).iter()) {
        s.push(CHARSET[*i as usize] as char);
    }
    s
}

pub struct Decoded {
    #[allow(dead_code)]
    pub prefix: String,
    pub version: u8,
    pub payload: Vec<u8>,
}

pub fn decode_address(address: &str, expected_prefix: Option<&str>) -> Result<Decoded> {
    if address.len() > 120 || !address.contains(':') {
        bail!("INVALID_KASPA_ADDRESS");
    }
    let lower = address.to_lowercase();
    let mut parts = lower.split(':');
    let prefix = parts.next().unwrap_or("").to_string();
    let body = parts.next().unwrap_or("");
    if let Some(p) = expected_prefix
        && prefix != p
    {
        bail!("PREFIX_MISMATCH: expected {p}");
    }
    if !["kaspa", "kaspatest", "kaspadev", "kaspasim"].contains(&prefix.as_str()) {
        bail!("UNKNOWN_PREFIX: {prefix}");
    }
    let mut five = Vec::with_capacity(body.len());
    for c in body.chars() {
        match CHARSET.iter().position(|x| *x as char == c) {
            Some(v) => five.push(v as u8),
            None => bail!("INVALID_CHAR_IN_ADDRESS: {c}"),
        }
    }
    if five.len() < 8 {
        bail!("ADDRESS_TOO_SHORT");
    }
    let (payload5, sum5) = five.split_at(five.len() - 8);
    let mut given: u64 = 0;
    for b in conv5to8(sum5) {
        given = (given << 8) | b as u64;
    }
    if checksum(payload5, &prefix) != given {
        bail!("ADDRESS_CHECKSUM_ERROR");
    }
    let bytes = conv5to8(payload5);
    let version = *bytes.first().unwrap_or(&255);
    let payload = bytes.get(1..).unwrap_or(&[]).to_vec();
    if (version != 0 && version != 8) || payload.len() != 32 {
        bail!("UNSUPPORTED_ADDRESS_VERSION_OR_LENGTH");
    }
    Ok(Decoded {
        prefix,
        version,
        payload,
    })
}

/// Address to standard scriptPublicKey (version 0).
pub fn address_to_spk(address: &str, expected_prefix: Option<&str>) -> Result<(i64, String)> {
    let d = decode_address(address, expected_prefix)?;
    let h = hex::encode(&d.payload);
    Ok(if d.version == 0 {
        (0, format!("20{h}ac"))
    } else {
        (0, format!("aa20{h}87"))
    })
}

/// ScriptPublicKey to address (P2PK or P2SH). None for non-standard scripts.
pub fn spk_to_address(version: i64, script: &str, prefix: &str) -> Option<String> {
    if version != 0 {
        return None;
    }
    let s = script.to_lowercase();
    let is_hex = |x: &str| x.bytes().all(|c| c.is_ascii_hexdigit());
    if s.len() == 68 && s.starts_with("20") && s.ends_with("ac") && is_hex(&s[2..66]) {
        return Some(encode_address(0, &hex::decode(&s[2..66]).ok()?, prefix));
    }
    if s.len() == 70 && s.starts_with("aa20") && s.ends_with("87") && is_hex(&s[4..68]) {
        return Some(encode_address(8, &hex::decode(&s[4..68]).ok()?, prefix));
    }
    None
}

pub fn address_of(version: i64, script: &str, network_id: &str) -> Result<String> {
    let prefix = if network_id == "mainnet" {
        "kaspa"
    } else {
        "kaspatest"
    };
    match spk_to_address(version, script, prefix) {
        Some(a) => Ok(a),
        None => bail!("SPK_HAS_NO_ADDRESS"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn registry_addresses_roundtrip() {
        let tn = "kaspatest:qztrjpfpuf6g9jw76e6ay909z43hndpv5maqkker6ur4enz7llarsh82ezggl";
        let (v, s) = address_to_spk(tn, Some("kaspatest")).unwrap();
        assert_eq!(
            s,
            "2096390521e27482c9ded675d215e5156379b42ca6fa0b5b23d7075ccc5efffa38ac"
        );
        assert_eq!(spk_to_address(v, &s, "kaspatest").unwrap(), tn);
        let main = "kaspa:qzccgw790r0mnfe9uvy7nwd7a8990029ycg8e0manehhgruqm2r9v528nz0qr";
        let (v, s) = address_to_spk(main, Some("kaspa")).unwrap();
        assert_eq!(
            s,
            "20b1843bc578dfb9a725e309e9b9bee9ca57bd4526107cbf7d9e6f740f80da8656ac"
        );
        assert_eq!(spk_to_address(v, &s, "kaspa").unwrap(), main);
        assert!(address_to_spk(main, Some("kaspatest")).is_err());
        let p2sh = "kaspatest:pp0qugaz5d283qsfv5k6qlzgwhvp86ghdcpv5lhg5nf5zzwkfem6zlfuwwjyn";
        let (_, s) = address_to_spk(p2sh, None).unwrap();
        assert_eq!(
            s,
            "aa205e0e23a2a354788209652da07c4875d813e9176e02ca7ee8a4d34109d64e77a187"
        );
    }
}
