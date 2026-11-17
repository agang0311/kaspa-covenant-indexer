//! Optional F3.1-style registration output: an ordinary discovery marker, never a covenant authorization.
//! The registry address and its prefix are NETWORK facts: they are injected, never assumed (`src/adapter/registry.mjs`).
use crate::address::{address_to_spk, spk_to_address};
use crate::networks::network_config;
use crate::util::{js_string, truthy};
use anyhow::{Result, bail};
use serde_json::{Value, json};

#[derive(Clone, Debug)]
pub struct ScopeEntry {
    pub address: String,
    pub spk_version: i64,
    pub spk_script: String,
}

pub fn registry_scope(addresses: &[String], network: &str) -> Result<Vec<ScopeEntry>> {
    let net = network_config(network)?;
    let uniq: std::collections::HashSet<&String> = addresses.iter().collect();
    if addresses.is_empty() || addresses.len() > 16 || uniq.len() != addresses.len() {
        bail!("REGISTRY_ADDRESSES");
    }
    addresses
        .iter()
        .map(|address| {
            if !address.starts_with(&format!("{}:", net.address_prefix)) || address.len() > 120 {
                bail!("REGISTRY_ADDRESS_PREFIX {}", net.address_prefix);
            }
            let (v, s) = address_to_spk(address, Some(net.address_prefix))?;
            if spk_to_address(v, &s, net.address_prefix).as_deref() != Some(address.as_str()) {
                bail!("REGISTRY_ADDRESS_ROUNDTRIP");
            }
            Ok(ScopeEntry {
                address: address.clone(),
                spk_version: v,
                spk_script: s,
            })
        })
        .collect()
}

/// The registry module's `wireSpk`: {version, script} -> little-endian version hex + script (no case folding).
fn reg_wire(p: Option<&Value>) -> Option<String> {
    match p {
        Some(Value::String(s)) => Some(s.clone()),
        Some(Value::Object(o)) => {
            let script = o.get("script").and_then(|s| s.as_str())?;
            if !script
                .bytes()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
            {
                return None;
            }
            let v = o
                .get("version")
                .and_then(|v| v.as_i64())
                .filter(|v| (0..=65535).contains(v))?;
            let le = format!("{:04x}", v);
            Some(format!("{}{}{}", &le[2..4], &le[0..2], script))
        }
        _ => None,
    }
}

fn is_positive_decimal(s: &str) -> bool {
    !s.is_empty() && !s.starts_with('0') && s.bytes().all(|c| c.is_ascii_digit())
}

pub fn registration_of(
    tx: &Value,
    scope: &[ScopeEntry],
    owner_key: &str,
    amount_sompi: &str,
) -> Result<Option<Value>> {
    if !is_positive_decimal(amount_sompi) {
        bail!("REGISTRATION_AMOUNT");
    }
    let empty = vec![];
    let outputs = tx
        .get("outputs")
        .and_then(|o| o.as_array())
        .unwrap_or(&empty);
    let o = outputs.get(1);
    let spk = o.and_then(|o| o.get("scriptPublicKey"));
    if outputs.len() != 2 && outputs.len() != 3 {
        return Ok(None);
    }
    if outputs.len() == 3 {
        let change = &outputs[2];
        if owner_key.is_empty()
            || !is_positive_decimal(&js_string(change.get("value")))
            || truthy(change.get("covenant"))
            || reg_wire(change.get("scriptPublicKey")) != Some(format!("000020{owner_key}ac"))
        {
            return Ok(None);
        }
    }
    let ov = js_string(o.and_then(|o| o.get("value")));
    let matches: Vec<&ScopeEntry> = scope
        .iter()
        .filter(|r| {
            ov == amount_sompi
                && reg_wire(spk)
                    == reg_wire(Some(
                        &json!({"version": r.spk_version, "script": r.spk_script}),
                    ))
                && !truthy(o.and_then(|o| o.get("covenant")))
        })
        .collect();
    if matches.len() != 1 {
        return Ok(None);
    }
    let m = matches[0];
    Ok(Some(json!({
        "address": m.address,
        "outpoint": {"transactionId": tx.get("verboseData").and_then(|v| v.get("transactionId")).cloned().unwrap_or(Value::Null), "index": 1},
        "amountSompi": amount_sompi,
        "spk": {"version": m.spk_version, "script": m.spk_script},
    })))
}
