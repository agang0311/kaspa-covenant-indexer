//! Offline differential self-check: re-derive every recorded V2.0.2 transition of a database with the native rules and
//! compare with what was stored (by either build). Read-only; no network.
use crate::contracts::{BlockRef, LoadCtx, check_round, load_contracts};
use crate::normalize::normalize_consensus_fields;
use crate::store::enc;
use anyhow::{Result, anyhow};
use rusqlite::{Connection, OpenFlags};
use serde_json::{Value, json};
use std::collections::BTreeMap;

fn diff_paths(a: &Value, b: &Value, path: String, out: &mut Vec<String>) {
    if out.len() > 12 {
        return;
    }
    match (a, b) {
        (Value::Object(x), Value::Object(y)) => {
            for (k, v) in x {
                match y.get(k) {
                    Some(w) => diff_paths(v, w, format!("{path}.{k}"), out),
                    None => out.push(format!("{path}.{k}: only in rust")),
                }
            }
            for k in y.keys() {
                if !x.contains_key(k) {
                    out.push(format!("{path}.{k}: only in stored"));
                }
            }
        }
        (Value::Array(x), Value::Array(y)) if x.len() == y.len() => {
            for (i, (v, w)) in x.iter().zip(y).enumerate() {
                diff_paths(v, w, format!("{path}[{i}]"), out);
            }
        }
        _ if a != b => out.push(format!(
            "{path}: rust={} stored={}",
            crate::util::slice_chars(&a.to_string(), 120),
            crate::util::slice_chars(&b.to_string(), 120)
        )),
        _ => {}
    }
}

/// Whether a recorded transaction is in the fully normalized form the current engine writes
/// (string lockTime, {version, script} output SPKs).
fn normalized_form(tx: &Value) -> bool {
    tx.get("lockTime").map(|v| v.is_string()).unwrap_or(false)
        && tx
            .get("outputs")
            .and_then(|o| o.as_array())
            .map(|a| {
                a.iter().all(|o| {
                    o.get("scriptPublicKey")
                        .map(|s| s.is_object())
                        .unwrap_or(false)
                })
            })
            .unwrap_or(false)
}

pub fn check_cases(path: &str) -> Result<Value> {
    let cases: Vec<Value> = serde_json::from_slice(&std::fs::read(path)?)?;
    let mut out = vec![];
    for case in cases {
        let net = case["network"]
            .as_str()
            .ok_or_else(|| anyhow!("CASE_NETWORK"))?;
        let registry = load_contracts(
            None,
            &LoadCtx {
                network: net,
                registry_addresses: None,
            },
        )?;
        let p = registry.for_genesis(None).remove(0);
        let b = BlockRef {
            hash: case["block"]["hash"].as_str().unwrap_or("").into(),
            daa: case["block"]["daa"].as_str().unwrap_or("").into(),
        };
        let result = if case["kind"] == "GENESIS" {
            p.genesis(&case["tx"], &b)
                .and_then(|r| r.ok_or_else(|| anyhow!("GENESIS_NOT_CLAIMED")))
        } else {
            p.advance(&case["previous"], &case["tx"], &b)
        };
        out.push(match result.and_then(|r| check_round(r, &p.key)) {
            Ok(r) => json!({"name":case["name"],"accepted":true,"result":r}),
            Err(e) => json!({"name":case["name"],"accepted":false,"error":e.to_string()}),
        });
    }
    Ok(json!(out))
}

pub fn replay(db_path: &str, verbose: bool) -> Result<Value> {
    let c = Connection::open_with_flags(db_path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let network: String = c
        .query_row("SELECT value FROM meta WHERE key='network'", [], |r| {
            r.get::<_, String>(0)
        })
        .map(|s| serde_json::from_str::<String>(&s).unwrap_or(s))?;
    let registry = load_contracts(
        None,
        &LoadCtx {
            network: &network,
            registry_addresses: None,
        },
    )?;
    let plugin = registry
        .for_genesis(None)
        .into_iter()
        .next()
        .ok_or_else(|| anyhow!("NO_PLUGIN"))?;
    let mut st = c.prepare("SELECT round_id, seq, kind, status, accepting, accepting_daa, containing, previous, next, txid FROM transitions WHERE contract=? ORDER BY round_id, seq")?;
    let rows = st.query_map([&plugin.key], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, i64>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, String>(4)?,
            r.get::<_, String>(5)?,
            r.get::<_, Option<String>>(6)?,
            r.get::<_, Option<String>>(7)?,
            r.get::<_, String>(8)?,
            r.get::<_, String>(9)?,
        ))
    })?;
    let mut stats: BTreeMap<String, [u64; 4]> = BTreeMap::new(); // [total, semantic ok, byte-identical, failed]
    let mut failures = vec![];
    let mut idem = [0u64; 2]; // [fully normalized rows, normalize(row) == row]
    let t0 = std::time::Instant::now();
    for row in rows {
        let (round_id, seq, kind, status, accepting, daa, containing, previous, next_raw, txid) =
            row?;
        let expected: Value = serde_json::from_str(&next_raw)?;
        let b = BlockRef {
            hash: accepting.clone(),
            daa: daa.clone(),
        };
        let got: Result<Value> = if kind == "GENESIS" {
            let tx = expected
                .get("genesisTransaction")
                .cloned()
                .unwrap_or(Value::Null);
            plugin
                .genesis(&tx, &b)
                .and_then(|r| r.ok_or_else(|| anyhow!("GENESIS_NOT_CLAIMED")))
                .and_then(|r| check_round(r, &plugin.key))
        } else {
            let prev: Value = previous
                .as_deref()
                .map(serde_json::from_str)
                .transpose()?
                .unwrap_or(Value::Null);
            // Feed the accepted transaction exactly as it was recorded: rows written before the JS engine normalized
            // consensus fields (numbers / wire SPK strings) must still re-derive identically; newer rows are already
            // normalized and normalization is idempotent.
            let tx = expected
                .get("latestTransaction")
                .cloned()
                .unwrap_or(Value::Null);
            if normalized_form(&tx) {
                idem[0] += 1;
                if normalize_consensus_fields(&tx) == tx {
                    idem[1] += 1;
                }
            }
            plugin
                .advance(&prev, &tx, &b)
                .and_then(|r| check_round(r, &plugin.key))
                .map(|mut r| {
                    crate::util::obj_mut(&mut r).insert(
                        "containing".into(),
                        containing.clone().map(Value::String).unwrap_or(Value::Null),
                    );
                    r
                })
        };
        let e = stats.entry(format!("{kind}/{status}")).or_insert([0; 4]);
        e[0] += 1;
        match got {
            Ok(v) => {
                if v == expected {
                    e[1] += 1;
                    if enc(&v) == next_raw {
                        e[2] += 1;
                    }
                } else {
                    e[3] += 1;
                    let mut d = vec![];
                    diff_paths(&v, &expected, String::new(), &mut d);
                    if failures.len() < 20 {
                        failures.push(json!({"round": round_id, "seq": seq, "kind": kind, "txid": txid, "diff": d}));
                    }
                }
            }
            Err(err) => {
                e[3] += 1;
                if failures.len() < 20 {
                    failures.push(json!({"round": round_id, "seq": seq, "kind": kind, "status": status, "txid": txid, "error": err.to_string()}));
                }
            }
        }
        if verbose && e[0] % 500 == 0 {
            eprintln!("{kind}/{status}: {} replayed", e[0]);
        }
    }
    let by_kind: serde_json::Map<String, Value> = stats
        .iter()
        .map(|(k, v)| (k.clone(), json!({"total": v[0], "semanticMatch": v[1], "byteIdentical": v[2], "mismatchOrError": v[3]})))
        .collect();
    let total: u64 = stats.values().map(|v| v[0]).sum();
    let ok: u64 = stats.values().map(|v| v[1]).sum();
    let bytes: u64 = stats.values().map(|v| v[2]).sum();
    Ok(
        json!({"db": db_path, "network": network, "contract": plugin.key, "total": total, "semanticMatch": ok, "byteIdentical": bytes,
        "mismatchOrError": total - ok, "normalizedRows": idem[0], "normalizationIdempotent": idem[1], "elapsedMs": t0.elapsed().as_millis() as u64, "byKind": by_kind, "failures": failures}),
    )
}
