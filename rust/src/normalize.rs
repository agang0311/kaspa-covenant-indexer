//! Transaction / UTXO helpers shared by the engine (`src/normalize.mjs`). No network access here.
use crate::util::{canonical, is_nullish, js_bigint, js_int, js_string, truthy};
use anyhow::{Result, bail};
use serde_json::{Value, json};

pub const ZERO_HASH: &str = "0000000000000000000000000000000000000000000000000000000000000000";

/// JS strict equality (`===`) for JSON primitives; objects/arrays are never `===` unless identical, treat as false.
pub fn jeq(a: Option<&Value>, b: Option<&Value>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(Value::Null), Some(Value::Null)) => true,
        (Some(Value::Bool(x)), Some(Value::Bool(y))) => x == y,
        (Some(Value::String(x)), Some(Value::String(y))) => x == y,
        (Some(Value::Number(x)), Some(Value::Number(y))) => match (x.as_i64(), y.as_i64()) {
            (Some(p), Some(q)) => p == q,
            _ => match (x.as_u64(), y.as_u64()) {
                (Some(p), Some(q)) => p == q,
                _ => x.as_f64() == y.as_f64(),
            },
        },
        _ => false,
    }
}

/// `${o.transactionId}:${o.index}`
pub fn outpoint_key(o: &Value) -> String {
    format!(
        "{}:{}",
        js_string(o.get("transactionId")),
        js_string(o.get("index"))
    )
}

pub fn txid_of(tx: &Value) -> Option<String> {
    match tx.get("verboseData").and_then(|v| v.get("transactionId")) {
        None | Some(Value::Null) => None,
        Some(v) => Some(js_string(Some(v))),
    }
}

fn is_hex_ci(s: &str) -> bool {
    s.bytes().all(|c| c.is_ascii_hexdigit())
}

/// Accept both the wire string `vvvv<script>` (little-endian version) and {version, script}. Returns {version, script}.
pub fn wire_spk(x: &Value) -> Result<Value> {
    match x {
        Value::String(s) => {
            if s.len() < 4 || s.len() % 2 != 0 || !is_hex_ci(s) {
                bail!("SPK_FORMAT");
            }
            let v = u32::from_str_radix(&format!("{}{}", &s[2..4], &s[0..2]), 16).unwrap();
            Ok(json!({"version": v, "script": s[4..].to_lowercase()}))
        }
        Value::Object(o) => {
            let version = o.get("version").and_then(|v| {
                v.as_i64()
                    .or_else(|| v.as_f64().filter(|f| f.fract() == 0.0).map(|f| f as i64))
            });
            match (version, o.get("script")) {
                (Some(v), Some(Value::String(s))) => {
                    Ok(json!({"version": v, "script": s.to_lowercase()}))
                }
                _ => bail!("SPK_FORMAT"),
            }
        }
        _ => bail!("SPK_FORMAT"),
    }
}

pub fn same_spk(a: &Value, b: &Value) -> bool {
    truthy(Some(a))
        && truthy(Some(b))
        && jeq(a.get("version"), b.get("version"))
        && match (
            a.get("script").and_then(|s| s.as_str()),
            b.get("script").and_then(|s| s.as_str()),
        ) {
            (Some(x), Some(y)) => x.to_lowercase() == y.to_lowercase(),
            _ => false,
        }
}

pub fn spends(tx: &Value, outpoint: &Value) -> bool {
    tx.get("inputs").and_then(|i| i.as_array()).map(|ins| {
        ins.iter().any(|i| {
            let p = i.get("previousOutpoint");
            jeq(
                p.and_then(|p| p.get("transactionId")),
                outpoint.get("transactionId"),
            ) && jeq(p.and_then(|p| p.get("index")), outpoint.get("index"))
        })
    }) == Some(true)
}

/// Tip-spend match on raw RPC rows (index may arrive as number or string; hash case normalized).
pub fn spends_loose(tx: &Value, tip: &Value) -> bool {
    let tid = js_string(tip.get("transactionId"));
    let tidx = js_int(tip.get("index"));
    tx.get("inputs").and_then(|i| i.as_array()).map(|ins| {
        ins.iter().any(|i| match i.get("previousOutpoint") {
            Some(p) if truthy(Some(p)) => {
                js_string(p.get("transactionId")).to_lowercase() == tid
                    && tidx.is_some()
                    && js_int(p.get("index")) == tidx
            }
            _ => false,
        })
    }) == Some(true)
}

/// Cheap routing only: no witness/payload decoding or trust decision. Always retain a known tip spend.
pub fn related_transaction(tx: &Value, tip: Option<&Value>, cid: Option<&str>) -> bool {
    if let Some(t) = tip
        && truthy(Some(t))
        && spends(tx, t)
    {
        return true;
    }
    let Some(cid) = cid else { return false };
    let cid_v = json!(cid);
    let outs = tx
        .get("outputs")
        .and_then(|o| o.as_array())
        .map(|a| {
            a.iter().any(|o| {
                jeq(
                    o.get("covenant").and_then(|c| c.get("covenantId")),
                    Some(&cid_v),
                )
            })
        })
        .unwrap_or(false);
    outs || tx
        .get("inputs")
        .and_then(|i| i.as_array())
        .map(|a| {
            a.iter().any(|i| {
                jeq(
                    i.get("verboseData")
                        .and_then(|v| v.get("utxoEntry"))
                        .and_then(|u| u.get("covenantId")),
                    Some(&cid_v),
                )
            })
        })
        .unwrap_or(false)
}

/// V2 'High' rows carry version/lockTime as null; those are not full transactions.
pub fn is_full_transaction(tx: &Value) -> bool {
    txid_of(tx).map(|t| !t.is_empty()).unwrap_or(false)
        && !is_nullish(tx.get("version"))
        && !is_nullish(tx.get("lockTime"))
        && tx.get("outputs").map(|o| o.is_array()).unwrap_or(false)
        && tx
            .get("inputs")
            .and_then(|i| i.as_array())
            .map(|a| {
                a.iter().all(|i| {
                    i.get("signatureScript")
                        .map(|s| s.is_string())
                        .unwrap_or(false)
                })
            })
            .unwrap_or(false)
}

/// Canonicalize and record which block included it, if known.
pub fn prepare_transaction(tx: &Value, containing: Option<&str>) -> Value {
    let mut t = canonical(tx);
    let o = crate::util::obj_mut(&mut t);
    let mut vd = o
        .get("verboseData")
        .and_then(|v| v.as_object())
        .cloned()
        .unwrap_or_default();
    let hash = vd.get("blockHash").cloned();
    if !truthy(hash.as_ref()) || hash.as_ref().and_then(|h| h.as_str()) == Some(ZERO_HASH) {
        vd.insert(
            "blockHash".into(),
            containing.map(|c| json!(c)).unwrap_or(Value::Null),
        );
    }
    o.insert("verboseData".into(), Value::Object(vd));
    t
}

/// Normalized live UTXO row.
#[derive(Clone, Debug)]
pub struct Utxo {
    pub outpoint: Value,
    pub amount: i128,
    pub spk: Value,
    pub block_daa_score: i128,
    pub covenant_id: Option<String>,
    pub address: Option<String>,
}

impl Utxo {
    pub fn outpoint_txid(&self) -> &str {
        self.outpoint["transactionId"].as_str().unwrap_or("")
    }
    pub fn outpoint_index(&self) -> Option<i64> {
        self.outpoint["index"].as_i64()
    }
}

pub fn normalize_utxo(e: &Value) -> Result<Utxo> {
    let empty = json!({});
    let entry = e
        .get("entry")
        .filter(|v| !v.is_null())
        .or_else(|| e.get("utxoEntry").filter(|v| !v.is_null()))
        .unwrap_or(&empty);
    let outpoint = e
        .get("outpoint")
        .filter(|v| !v.is_null())
        .or_else(|| entry.get("outpoint").filter(|v| !v.is_null()));
    let Some(outpoint) = outpoint else {
        bail!("Cannot read properties of undefined (reading 'transactionId')")
    };
    let pick = |k: &str| e.get(k).filter(|v| !v.is_null()).or_else(|| entry.get(k));
    let cid = entry
        .get("covenantId")
        .filter(|v| !v.is_null())
        .or_else(|| e.get("covenantId"));
    let spk_raw = pick("scriptPublicKey").cloned().unwrap_or(Value::Null);
    let spk_obj = match &spk_raw {
        Value::String(_) => spk_raw.clone(),
        Value::Object(o) => {
            let v = crate::util::js_number(o.get("version"));
            json!({"version": v.map(|f| if f.fract() == 0.0 { json!(f as i64) } else { json!(f) }).unwrap_or(Value::Null), "script": js_string(o.get("script"))})
        }
        _ => bail!("Cannot read properties of undefined (reading 'version')"),
    };
    let idx = js_int(outpoint.get("index"));
    Ok(Utxo {
        outpoint: json!({"transactionId": js_string(outpoint.get("transactionId")).to_lowercase(), "index": idx.map(Value::from).unwrap_or(Value::Null)}),
        amount: js_bigint(pick("amount"))?,
        spk: wire_spk(&spk_obj)?,
        block_daa_score: js_bigint(pick("blockDaaScore"))?,
        covenant_id: if is_nullish(cid) {
            None
        } else {
            Some(js_string(cid).to_lowercase())
        },
        address: e
            .get("address")
            .filter(|a| truthy(Some(a)))
            .map(|a| js_string(Some(a))),
    })
}

/// V2 'High' verbosity omits version/lockTime (null) and may omit per-output spk version. Fill consensus defaults,
/// with the same JSON types the JS build stores (strings for lockTime/gas/value/sequence, numbers for indices).
pub fn normalize_consensus_fields(tx: &Value) -> Value {
    let Some(src) = tx.as_object() else {
        return tx.clone();
    };
    let mut out = src.clone();
    if is_nullish(out.get("version")) {
        out.insert("version".into(), json!(1));
    }
    let lt = if is_nullish(out.get("lockTime")) {
        "0".to_string()
    } else {
        js_string(out.get("lockTime"))
    };
    out.insert("lockTime".into(), json!(lt));
    let gas = if is_nullish(out.get("gas")) {
        "0".to_string()
    } else {
        js_string(out.get("gas"))
    };
    out.insert("gas".into(), json!(gas));
    if is_nullish(out.get("payload")) {
        out.insert("payload".into(), json!(""));
    }
    if is_nullish(out.get("subnetworkId")) {
        out.insert(
            "subnetworkId".into(),
            json!("0000000000000000000000000000000000000000"),
        );
    }
    if let Some(Value::Array(outs)) = out.get("outputs").cloned() {
        let mapped: Vec<Value> = outs
            .iter()
            .map(|o| {
                let mut spk = json!({"version": 0, "script": ""});
                if let Some(s) = o.get("scriptPublicKey")
                    && truthy(Some(s))
                    && let Ok(w) = wire_spk(s)
                {
                    spk = w;
                }
                let cov = match o.get("covenant") {
                    Some(c) if truthy(Some(c)) => json!({
                        "authorizingInput": num_or_nan(c.get("authorizingInput").filter(|v| !v.is_null()).unwrap_or(&json!(0))),
                        "covenantId": js_string(Some(c.get("covenantId").filter(|v| !v.is_null()).unwrap_or(&json!("")))),
                    }),
                    _ => match o.get("covenant_id") {
                        Some(c) if truthy(Some(c)) => json!({
                            "authorizingInput": num_or_nan(o.get("covenant_authorizing_input").filter(|v| !v.is_null()).unwrap_or(&json!(0))),
                            "covenantId": js_string(Some(c)),
                        }),
                        _ => Value::Null,
                    },
                };
                let mut m = o.as_object().cloned().unwrap_or_default();
                let value = if is_nullish(o.get("value")) { "0".to_string() } else { js_string(o.get("value")) };
                m.insert("value".into(), json!(value));
                m.insert("covenant".into(), cov);
                m.insert("scriptPublicKey".into(), spk);
                Value::Object(m)
            })
            .collect();
        out.insert("outputs".into(), Value::Array(mapped));
    }
    if let Some(Value::Array(ins)) = out.get("inputs").cloned() {
        let mapped: Vec<Value> = ins
            .iter()
            .map(|i| {
                let mut m = i.as_object().cloned().unwrap_or_default();
                let seq = if is_nullish(i.get("sequence")) {
                    "0".to_string()
                } else {
                    js_string(i.get("sequence"))
                };
                m.insert("sequence".into(), json!(seq));
                let sig = i
                    .get("signatureScript")
                    .filter(|v| !v.is_null())
                    .cloned()
                    .unwrap_or(json!(""));
                m.insert("signatureScript".into(), sig);
                let mut po = i
                    .get("previousOutpoint")
                    .and_then(|p| p.as_object())
                    .cloned()
                    .unwrap_or_default();
                let idx = num_or_nan(
                    po.get("index")
                        .filter(|v| !v.is_null())
                        .unwrap_or(&json!(0)),
                );
                po.insert("index".into(), idx);
                m.insert("previousOutpoint".into(), Value::Object(po));
                Value::Object(m)
            })
            .collect();
        out.insert("inputs".into(), Value::Array(mapped));
    }
    Value::Object(out)
}

/// `Number(x)` as a JSON value (integers stay integral; NaN becomes null as JSON.stringify would write it).
fn num_or_nan(v: &Value) -> Value {
    match crate::util::js_number(Some(v)) {
        Some(f) if f.fract() == 0.0 && f.abs() < 9.007_199_254_740_992e15 => json!(f as i64),
        Some(f) if f.is_finite() => json!(f),
        _ => Value::Null,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wire_spk_forms() {
        assert_eq!(
            wire_spk(&json!("0000aa20ff87")).unwrap(),
            json!({"version":0,"script":"aa20ff87"})
        );
        assert_eq!(
            wire_spk(&json!({"version":0,"script":"AB"})).unwrap(),
            json!({"version":0,"script":"ab"})
        );
        assert!(wire_spk(&json!("000")).is_err());
    }
    #[test]
    fn normalizes_high_rows() {
        let t = json!({"version": null, "lockTime": null, "gas": 0, "outputs":[{"value": 5, "scriptPublicKey":"000020aa", "covenant": null}],
            "inputs":[{"previousOutpoint":{"transactionId":"aa","index":1},"sequence":0,"signatureScript":"41"}]});
        let n = normalize_consensus_fields(&t);
        assert_eq!(n["version"], json!(1));
        assert_eq!(n["lockTime"], json!("0"));
        assert_eq!(n["gas"], json!("0"));
        assert_eq!(n["outputs"][0]["value"], json!("5"));
        assert_eq!(
            n["outputs"][0]["scriptPublicKey"],
            json!({"version":0,"script":"20aa"})
        );
        assert_eq!(n["inputs"][0]["sequence"], json!("0"));
    }
}
