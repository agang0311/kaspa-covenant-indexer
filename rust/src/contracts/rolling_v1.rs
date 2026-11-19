//! Rolling v1 contract plugin (TN10 UNVALIDATED profile b56bcd9c...).
//! Hosts `core::rolling` inside KCI's ContractRegistry alongside V2.0.2 without mixing rules or rounds.
use super::registry::{ScopeEntry, registry_scope};
use crate::core::bytes::{hex, unhex};
use crate::core::covenant::{Outpoint, Spk, p2sh};
use crate::core::hashes::blake2b256;
use crate::core::rolling::{
    self as R, Accessor, Action, FundingContext, ManagerContext, Module, Profile, TargetContext,
    decode_genesis_payload, decode_spend, identify, interpret_accepted, load_pinned_profile,
    redeem, root_id,
};
use crate::core::transaction::{Binding, Input, Output, Transaction};
use crate::networks::{Network, network_config};
use crate::normalize::{Utxo, wire_spk};
use crate::util::{is_nullish, js_bigint, js_string};
use anyhow::{Result, anyhow, bail};
use serde_json::{Map, Value, json};
use std::sync::{Arc, OnceLock};

pub const PLUGIN_ID: &str = "rolling-v1";
pub const VERSION: &str = "1.0.0";
pub const PROFILE_ID: &str = R::TN10_PROFILE_ID;
pub const TN10_ROLLING_REGISTRY: &str =
    "kaspatest:qztrjpfpuf6g9jw76e6ay909z43hndpv5maqkker6ur4enz7llarsh82ezggl";

static PROFILE: OnceLock<Arc<Profile>> = OnceLock::new();

pub fn pinned_profile() -> Result<Arc<Profile>> {
    if let Some(p) = PROFILE.get() {
        return Ok(p.clone());
    }
    let p = Arc::new(load_pinned_profile()?);
    Ok(PROFILE.get_or_init(|| p).clone())
}

pub fn action_of(tx: &Value) -> String {
    let Ok(p) = pinned_profile() else {
        return "UNKNOWN".into();
    };
    let Some(sig) = tx
        .get("inputs")
        .and_then(|i| i.get(0))
        .and_then(|i| i.get("signatureScript"))
        .and_then(|s| s.as_str())
    else {
        return "UNKNOWN".into();
    };
    let Ok(w) = decode_spend(sig, &p) else {
        return "UNKNOWN".into();
    };
    match w.args.get("action") {
        Some(R::SpendArg::Int(id)) => Action::from_id(*id)
            .map(|a| a.as_str().to_string())
            .unwrap_or_else(|_| "UNKNOWN".into()),
        _ => "UNKNOWN".into(),
    }
}

pub fn draw_target_hash(tx: &Value) -> Option<String> {
    let p = pinned_profile().ok()?;
    let sig = tx
        .get("inputs")
        .and_then(|i| i.get(0))
        .and_then(|i| i.get("signatureScript"))
        .and_then(|s| s.as_str())?;
    let w = decode_spend(sig, &p).ok()?;
    let Some(R::SpendArg::Int(id)) = w.args.get("action") else {
        return None;
    };
    if *id != Action::Draw as i128 {
        return None;
    }
    let Some(R::SpendArg::Bytes(d)) = w.args.get("data") else {
        return None;
    };
    if d.len() < 32 {
        return None;
    }
    Some(hex(&d[0..32]))
}

pub struct RollingV1 {
    profile: Arc<Profile>,
    pub network: &'static Network,
    scope: Vec<ScopeEntry>,
    watch: Vec<String>,
    pub config: Value,
}

fn outpoint_of(v: Option<&Value>) -> Result<Outpoint> {
    let Some(v) = v.filter(|v| v.is_object()) else {
        bail!("OUTPOINT_MISSING")
    };
    let transaction_id = match v.get("transactionId") {
        Some(Value::String(s)) => s.clone(),
        _ => "?".into(),
    };
    let index = v
        .get("index")
        .and_then(|i| {
            i.as_i64()
                .map(|n| n as i128)
                .or_else(|| i.as_u64().map(|n| n as i128))
        })
        .unwrap_or(-1);
    Ok(Outpoint {
        transaction_id,
        index,
    })
}

fn spk_struct(v: &Value) -> Spk {
    Spk {
        version: v.get("version").and_then(|x| x.as_i64()).unwrap_or(-1) as i128,
        script: v
            .get("script")
            .and_then(|x| x.as_str())
            .unwrap_or("?")
            .to_string(),
    }
}

fn spk_value(s: &Spk) -> Value {
    json!({"version": s.version, "script": s.script})
}

fn parse_tx(t: &Value) -> Result<Transaction> {
    let int_of = |v: Option<&Value>| {
        v.and_then(|x| {
            x.as_i64()
                .map(|n| n as i128)
                .or_else(|| x.as_u64().map(|n| n as i128))
        })
        .unwrap_or(i128::MIN)
    };
    let inputs = t
        .get("inputs")
        .and_then(|i| i.as_array())
        .cloned()
        .unwrap_or_default();
    let mut tx_inputs = Vec::with_capacity(inputs.len());
    for i in &inputs {
        let sig = match i.get("signatureScript") {
            Some(Value::String(s)) => s.to_lowercase(),
            _ => bail!("INPUT_SIGNATURE_SCRIPT"),
        };
        tx_inputs.push(Input {
            previous_outpoint: outpoint_of(i.get("previousOutpoint"))?,
            signature_script: sig,
            sequence: js_bigint(i.get("sequence"))?,
            compute_budget: if is_nullish(i.get("computeBudget")) {
                0
            } else {
                int_of(i.get("computeBudget"))
            },
        });
    }
    let mut tx_outputs = vec![];
    for o in t
        .get("outputs")
        .and_then(|o| o.as_array())
        .cloned()
        .unwrap_or_default()
    {
        let value = js_bigint(o.get("value"))?;
        let spk = spk_struct(&wire_spk(o.get("scriptPublicKey").unwrap_or(&Value::Null))?);
        let covenant = match o.get("covenant") {
            Some(c) if !c.is_null() => Some(Binding {
                covenant_id: c
                    .get("covenantId")
                    .and_then(|x| x.as_str())
                    .unwrap_or("?")
                    .to_string(),
                authorizing_input: int_of(c.get("authorizingInput")),
            }),
            _ => None,
        };
        tx_outputs.push(Output {
            value,
            script_public_key: spk,
            covenant,
        });
    }
    Ok(Transaction {
        version: int_of(t.get("version")),
        inputs: tx_inputs,
        outputs: tx_outputs,
        lock_time: js_bigint(t.get("lockTime"))?,
        subnetwork_id: t
            .get("subnetworkId")
            .and_then(|s| s.as_str())
            .unwrap_or("?")
            .to_string(),
        gas: js_bigint(t.get("gas"))?,
        payload: match t.get("payload") {
            None | Some(Value::Null) => String::new(),
            Some(Value::String(s)) => s.clone(),
            Some(_) => "?".into(),
        },
        storage_mass: if is_nullish(t.get("storageMass")) {
            0
        } else {
            js_bigint(t.get("storageMass"))?
        },
    })
}

impl RollingV1 {
    pub fn create(options: &Value, network_name: Option<&str>) -> Result<Self> {
        let net_name = network_name.unwrap_or("tn10");
        let net = network_config(net_name)?;
        if !matches!(net.key, "tn10" | "mainnet") {
            bail!("ROLLING_V1_UNSUPPORTED_NETWORK {net_name}");
        }
        let profile = pinned_profile()?;
        if let Some(pid) = options.get("profileId").and_then(|v| v.as_str())
            && pid != PROFILE_ID
        {
            bail!("ROLLING_PROFILE_MISMATCH {pid}");
        }
        let addrs: Vec<String> = match options.get("registryAddresses").and_then(|a| a.as_array()) {
            Some(arr) if !arr.is_empty() => arr
                .iter()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect(),
            _ => vec![net.registry.to_string()],
        };
        let scope = registry_scope(&addrs, net.key)?;
        let watch: Vec<String> = scope.iter().map(|s| s.address.clone()).collect();
        let config = json!({
            "profileId": PROFILE_ID,
            "registryAddresses": watch,
        });
        Ok(Self {
            profile,
            network: net,
            scope,
            watch,
            config,
        })
    }

    pub fn version(&self) -> String {
        VERSION.to_string()
    }
    pub fn profile_id(&self) -> &str {
        &self.profile.id
    }
    pub fn watch_addresses(&self) -> Vec<String> {
        self.watch.clone()
    }

    pub fn discover(&self, u: &Utxo) -> Option<String> {
        if u.address
            .as_deref()
            .is_some_and(|a| self.watch.iter().any(|w| w == a))
        {
            Some(u.outpoint_txid().to_string())
        } else {
            None
        }
    }

    pub fn candidate_genesis(&self, tx: &Value) -> bool {
        let Some(payload) = tx.get("payload").and_then(|p| p.as_str()) else {
            return false;
        };
        decode_genesis_payload(payload, PROFILE_ID).is_ok()
    }

    pub fn inspect_genesis(&self, tx: &Value) -> (bool, String) {
        let payload = tx.get("payload").and_then(|p| p.as_str()).unwrap_or("");
        match decode_genesis_payload(payload, PROFILE_ID) {
            Ok(_) => (true, "GENESIS_PAYLOAD_OK".into()),
            Err(e) => (false, e.to_string()),
        }
    }

    pub fn genesis(&self, tx: &Value, b: &super::v202::BlockRef) -> Result<Option<Value>> {
        let payload = tx.get("payload").and_then(|p| p.as_str()).unwrap_or("");
        let Ok(state) = decode_genesis_payload(payload, PROFILE_ID) else {
            return Ok(None);
        };
        let parsed = parse_tx(tx)?;
        if parsed.version != 1 || parsed.inputs.is_empty() || parsed.outputs.is_empty() {
            return Ok(None);
        }
        let origin = parsed.inputs[0].previous_outpoint.clone();
        let cid = root_id(&origin, &state, &self.profile)?;
        let redeem_bytes = redeem(&state, &self.profile)?;
        let sh = hex(&blake2b256(&redeem_bytes));
        let expected_spk = p2sh(&sh)?;
        let out0 = &parsed.outputs[0];
        if out0.value != R::DEPOSIT || out0.script_public_key != expected_spk {
            return Ok(None);
        }
        let Some(cov) = &out0.covenant else {
            return Ok(None);
        };
        if cov.covenant_id != cid || cov.authorizing_input != 0 {
            return Ok(None);
        }

        let txid = tx["verboseData"]["transactionId"].clone();
        let block_hash = tx["verboseData"].get("blockHash").cloned();
        let mut r = Map::new();
        r.insert("id".into(), txid.clone());
        r.insert("cid".into(), json!(cid));
        r.insert("profileId".into(), json!(PROFILE_ID));
        r.insert("version".into(), json!(VERSION));
        r.insert("network".into(), json!(self.network.key));
        r.insert("networkGenesis".into(), json!(self.network.genesis));
        r.insert("genesisAccepting".into(), json!(b.hash));
        if let Some(h) = &block_hash {
            r.insert("genesisContaining".into(), h.clone());
        }
        r.insert("genesisTransaction".into(), tx.clone());
        r.insert("latestTxid".into(), txid.clone());
        r.insert("latestTransaction".into(), tx.clone());
        r.insert("accepting".into(), json!(b.hash));
        if let Some(h) = block_hash {
            r.insert("containing".into(), h);
        }
        r.insert("tip".into(), json!({"transactionId": txid, "index": 0}));
        r.insert(
            "origin".into(),
            json!({"transactionId": origin.transaction_id, "index": origin.index}),
        );
        r.insert("spk".into(), spk_value(&expected_spk));
        r.insert("value".into(), json!(R::DEPOSIT.to_string()));
        r.insert("utxoDaa".into(), json!(b.daa));
        r.insert("module".into(), json!(state.module.as_str()));
        r.insert("redeemHex".into(), json!(hex(&redeem_bytes)));
        r.insert("state".into(), state.to_json());
        r.insert("frozen".into(), json!([]));
        r.insert("preimages".into(), json!({}));
        r.insert("ownerKey".into(), json!(state.owner_key));
        r.insert(
            "settings".into(),
            json!({
                "ticketPrice": state.ticket_price.to_string(),
                "ticketCap": state.ticket_cap,
                "minTickets": state.min_tickets,
                "closeEligibleDaa": state.close_eligible_daa.to_string(),
            }),
        );
        let in_sum: i128 = tx["inputs"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|i| {
                        js_bigint(
                            i.get("verboseData")
                                .and_then(|v| v.get("utxoEntry"))
                                .and_then(|u| u.get("amount")),
                        )
                        .ok()
                    })
                    .sum()
            })
            .unwrap_or(0);
        let out_sum: i128 = parsed.outputs.iter().map(|o| o.value).sum();
        let genesis_fee = if in_sum >= out_sum {
            in_sum - out_sum
        } else {
            0
        };

        r.insert("terminal".into(), Value::Null);
        r.insert("step".into(), json!(1));
        r.insert("buys".into(), json!(0));
        r.insert("bought".into(), json!(0));
        r.insert("fees".into(), json!(genesis_fee.to_string()));
        r.insert(
            "receipts".into(),
            json!([{
                "txid": txid,
                "action": "GENESIS",
                "accepted": true,
                "candidate": false,
                "finalized": true,
                "acceptingBlock": b.hash,
                "acceptingDaa": b.daa,
                "feeSompi": genesis_fee.to_string(),
            }]),
        );
        let _ = &self.scope;
        Ok(Some(Value::Object(r)))
    }

    pub fn advance(&self, r: &Value, t: &Value, b: &super::v202::BlockRef) -> Result<Value> {
        self.advance_with_accessor(r, t, b, None)
    }

    pub fn advance_with_accessor(
        &self,
        r: &Value,
        t: &Value,
        b: &super::v202::BlockRef,
        accessor: Option<&Accessor>,
    ) -> Result<Value> {
        let cid = r
            .get("cid")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("ROLLING_CID"))?
            .to_string();
        let tip = r.get("tip").ok_or_else(|| anyhow!("ROLLING_TIP"))?;
        let tx = parse_tx(t)?;
        let tip_op = outpoint_of(Some(tip))?;
        if tx.inputs.is_empty()
            || tx.inputs[0].previous_outpoint != tip_op
            || r.get("profileId").and_then(|p| p.as_str()) != Some(PROFILE_ID)
        {
            bail!("ROLLING_ROUND_INPUT");
        }

        let origin = outpoint_of(r.get("origin"))?;
        let redeem_hex = r
            .get("redeemHex")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("ROLLING_REDEEM_HEX"))?;
        let manager = ManagerContext {
            cid: cid.clone(),
            origin,
            outpoint: tip_op,
            value: js_bigint(r.get("value"))?,
            spk: spk_struct(r.get("spk").unwrap_or(&Value::Null)),
            redeem_script: unhex(redeem_hex)?,
            utxo_daa: js_bigint(r.get("utxoDaa"))?,
            current_daa: js_bigint(Some(&json!(b.daa)))?,
        };

        let raw_inputs = t
            .get("inputs")
            .and_then(|i| i.as_array())
            .cloned()
            .unwrap_or_default();
        let mut targets = Vec::new();
        let mut funds = Vec::new();
        let mut funding_started = false;
        let mut spent_target_keys = std::collections::HashSet::new();

        for idx in 1..tx.inputs.len() {
            let utxo_entry = raw_inputs[idx]
                .get("verboseData")
                .and_then(|v| v.get("utxoEntry"));
            let entry_cid = utxo_entry
                .and_then(|u| u.get("covenantId"))
                .and_then(|c| c.as_str())
                .map(str::to_owned);
            let value = js_bigint(utxo_entry.and_then(|u| u.get("amount")))?;
            let decoded_frozen = decode_spend(&tx.inputs[idx].signature_script, &self.profile)
                .ok()
                .filter(|w| w.state.module == Module::Frozen);

            if decoded_frozen.is_some()
                || entry_cid.as_deref().is_some_and(|c| c == cid)
            {
                if funding_started {
                    bail!("TARGET_LAYOUT");
                }
                let fw = decoded_frozen.ok_or_else(|| anyhow!("TARGET_WITNESS"))?;
                let spk = match utxo_entry.and_then(|u| u.get("scriptPublicKey")) {
                    Some(v) if !v.is_null() => spk_struct(&wire_spk(v)?),
                    _ => p2sh(&hex(&blake2b256(&fw.raw)))?,
                };
                let op = tx.inputs[idx].previous_outpoint.clone();
                spent_target_keys.insert(format!("{}:{}", op.transaction_id, op.index));
                targets.push(TargetContext {
                    cid: cid.clone(),
                    outpoint: op,
                    value,
                    spk,
                    redeem_script: fw.raw,
                });
            } else {
                funding_started = true;
                funds.push(FundingContext {
                    outpoint: tx.inputs[idx].previous_outpoint.clone(),
                    value,
                    cid: entry_cid,
                });
            }
        }

        let res = interpret_accepted(
            &manager,
            &targets,
            &funds,
            &tx,
            accessor,
            &self.profile,
        )?;

        let id = t["verboseData"]["transactionId"].clone();
        let txid_str = id.as_str().unwrap_or("").to_string();
        let mut next = r.as_object().cloned().unwrap_or_default();
        next.insert("latestTxid".into(), id.clone());
        next.insert("latestTransaction".into(), t.clone());
        next.insert("accepting".into(), json!(b.hash));
        match t["verboseData"].get("blockHash") {
            Some(h) => {
                next.insert("containing".into(), h.clone());
            }
            None => {
                next.shift_remove("containing");
            }
        }
        next.insert("utxoDaa".into(), json!(b.daa));
        next.insert("actualFee".into(), json!(res.fee.to_string()));
        next.insert(
            "terminal".into(),
            res.terminal.clone().map(|x| json!(x)).unwrap_or(Value::Null),
        );

        let mut preimages = match next.shift_remove("preimages") {
            Some(Value::Object(m)) => m,
            _ => serde_json::Map::new(),
        };
        for (h, raw) in &res.frozen_outputs {
            preimages.insert(h.clone(), json!(hex(raw)));
        }

        // Update counters (buys, bought, fees, step, receipts).
        let old_step = next.get("step").and_then(|v| v.as_u64()).unwrap_or(1);
        next.insert("step".into(), json!(old_step + 1));
        let old_fees = js_bigint(next.get("fees")).unwrap_or(0);
        next.insert("fees".into(), json!((old_fees + res.fee).to_string()));
        if res.action == Action::Buy
            && let Ok(w) = decode_spend(&tx.inputs[0].signature_script, &self.profile)
            && let Some(R::SpendArg::Bytes(d)) = w.args.get("data")
            && d.len() >= 4
        {
            let q = crate::core::bytes::from_le(&d[0..4]);
            let buys = next.get("buys").and_then(|v| v.as_u64()).unwrap_or(0);
            let bought = next.get("bought").and_then(|v| v.as_u64()).unwrap_or(0);
            next.insert("buys".into(), json!(buys + 1));
            next.insert("bought".into(), json!(bought + q as u64));
        }
        let mut receipts = match next.shift_remove("receipts") {
            Some(Value::Array(a)) => a,
            _ => Vec::new(),
        };
        receipts.push(json!({
            "txid": txid_str,
            "action": res.action.as_str(),
            "accepted": true,
            "candidate": false,
            "finalized": true,
            "acceptingBlock": b.hash,
            "acceptingDaa": b.daa,
            "feeSompi": res.fee.to_string(),
        }));
        next.insert("receipts".into(), Value::Array(receipts));

        // Preserve any frozen preimage and draw metadata if DRAW revealed a winning preimage.
        if res.action == Action::Draw
            && let Ok(w) = decode_spend(&tx.inputs[0].signature_script, &self.profile)
            && let Some(R::SpendArg::Bytes(d)) = w.args.get("data")
            && d.len() >= 244
        {
            if d.len() > 248 {
                let raw = &d[248..];
                if identify(raw, &self.profile)
                    .map(|s| s.module == Module::Frozen)
                    .unwrap_or(false)
                {
                    let h = hex(&blake2b256(raw));
                    preimages.insert(h, json!(hex(raw)));
                }
            }
            if let Some(acc) = accessor
                && let Ok(ticket) = R::draw_sample(
                    &cid,
                    &manager.outpoint,
                    manager.utxo_daa,
                    manager.current_daa,
                    &w.state,
                    &d[0..240],
                    acc,
                )
            {
                let shard = crate::core::bytes::from_le(&d[240..242]) as u64;
                let record = crate::core::bytes::from_le(&d[242..244]) as u64;
                next.insert(
                    "draw".into(),
                    json!({
                        "txid": txid_str,
                        "shard": shard,
                        "record": record,
                        "ticket": ticket,
                    }),
                );
            }
        }
        if let Some(term) = &res.terminal
            && (term == "PAID" || term == "REFUNDED")
            && let Ok(prev_state) = identify(&manager.redeem_script, &self.profile)
        {
            let shards: Vec<Value> = prev_state
                .indices
                .iter()
                .map(|idx| {
                    let recs = preimages
                        .get(&idx.hash)
                        .and_then(|v| v.as_str())
                        .and_then(|h| unhex(h).ok())
                        .and_then(|raw| identify(&raw, &self.profile).ok())
                        .map(|fs| {
                            Value::Array(
                                fs.records
                                    .iter()
                                    .map(|rec| json!({"end": rec.end, "key": rec.key}))
                                    .collect(),
                            )
                        })
                        .unwrap_or(Value::Null);
                    json!({
                        "hash": idx.hash,
                        "end": idx.end,
                        "records": recs,
                    })
                })
                .collect();
            let local_records: Vec<Value> = prev_state
                .records
                .iter()
                .map(|rec| json!({"end": rec.end, "key": rec.key}))
                .collect();
            next.insert(
                "history".into(),
                json!({
                    "terminal": term,
                    "winnerKey": prev_state.winner_key,
                    "localRecords": local_records,
                    "shards": shards,
                }),
            );
        }
        next.insert("preimages".into(), Value::Object(preimages));

        let mut frozen_list: Vec<Value> = next
            .get("frozen")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter(|f| {
                let tid = f
                    .get("outpoint")
                    .and_then(|o| o.get("transactionId"))
                    .and_then(|s| s.as_str())
                    .unwrap_or("");
                let idx = f
                    .get("outpoint")
                    .and_then(|o| o.get("index"))
                    .and_then(|n| n.as_i64())
                    .unwrap_or(-1);
                !spent_target_keys.contains(&format!("{tid}:{idx}"))
            })
            .collect();

        if let Some(ns) = &res.next {
            let rbytes = redeem(ns, &self.profile)?;
            next.insert("tip".into(), json!({"transactionId": id, "index": 0}));
            next.insert("module".into(), json!(ns.module.as_str()));
            next.insert("redeemHex".into(), json!(hex(&rbytes)));
            next.insert("state".into(), ns.to_json());
            next.insert("spk".into(), spk_value(&tx.outputs[0].script_public_key));
            next.insert("value".into(), json!(tx.outputs[0].value.to_string()));

            if !res.frozen_outputs.is_empty() && tx.outputs.len() > 1 {
                let (fhash, _) = &res.frozen_outputs[0];
                frozen_list.push(json!({
                    "outpoint": {"transactionId": txid_str, "index": 1},
                    "value": tx.outputs[1].value.to_string(),
                    "spk": spk_value(&tx.outputs[1].script_public_key),
                    "scriptHash": fhash,
                    "utxoDaa": b.daa,
                    "cid": cid,
                }));
            } else if res.action == Action::RefundOld
                && tx.outputs.len() > 1
                && tx.outputs[1].covenant.is_some()
                && !targets.is_empty()
            {
                let fhash = hex(&blake2b256(&targets[0].redeem_script));
                frozen_list.push(json!({
                    "outpoint": {"transactionId": txid_str, "index": 1},
                    "value": tx.outputs[1].value.to_string(),
                    "spk": spk_value(&tx.outputs[1].script_public_key),
                    "scriptHash": fhash,
                    "utxoDaa": b.daa,
                    "cid": cid,
                }));
            }
        } else {
            next.insert("tip".into(), Value::Null);
            next.insert("value".into(), json!("0"));
            next.insert("frozen".into(), json!([]));
        }
        if res.next.is_some() {
            next.insert("frozen".into(), Value::Array(frozen_list));
        }

        Ok(Value::Object(next))
    }
}

/// Build `roundText` (tagged JSON compatible with `web/rolling-tn10/codec.mjs`) and `historicalShards`.
pub fn export_round_text(d: &Value, virtual_daa: Option<&str>) -> Result<(String, Value)> {
    export_round_text_for_network(d, virtual_daa, "testnet-10")
}

pub fn export_round_text_for_network(
    d: &Value,
    virtual_daa: Option<&str>,
    network_id: &str,
) -> Result<(String, Value)> {
    let cid = d.get("cid").and_then(|v| v.as_str()).unwrap_or("");
    let terminal = d.get("terminal").cloned().unwrap_or(Value::Null);
    let utxo_daa = d.get("utxoDaa").and_then(|v| v.as_str()).unwrap_or("0");
    let current_daa = match virtual_daa {
        Some(v) if v.parse::<u64>().unwrap_or(0) > utxo_daa.parse::<u64>().unwrap_or(0) => v,
        _ => utxo_daa,
    };
    let settings = &d["settings"];
    let preimages = d.get("preimages").and_then(|v| v.as_object());

    let manager = if terminal.is_null() && d.get("tip").is_some_and(|t| !t.is_null()) {
        let redeem_hex = d.get("redeemHex").and_then(|v| v.as_str()).unwrap_or("");
        json!({
            "outpoint": d["tip"],
            "origin": d["origin"],
            "cid": cid,
            "spk": d["spk"],
            "value": {"$bigint": js_string(d.get("value"))},
            "utxoDaa": {"$bigint": utxo_daa},
            "currentDaa": {"$bigint": current_daa},
            "redeemScript": {"$bytes": redeem_hex},
        })
    } else {
        Value::Null
    };

    let frozen_arr: Vec<Value> = if manager.is_null() {
        vec![]
    } else {
        d.get("frozen")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|f| {
                let sh = f.get("scriptHash").and_then(|v| v.as_str())?;
                let rhex = preimages
                    .and_then(|m| m.get(sh))
                    .and_then(|v| v.as_str())?;
                let f_daa = f
                    .get("utxoDaa")
                    .and_then(|v| v.as_str())
                    .unwrap_or(utxo_daa);
                Some(json!({
                    "outpoint": f["outpoint"],
                    "value": {"$bigint": js_string(f.get("value"))},
                    "utxoDaa": {"$bigint": f_daa},
                    "spk": f["spk"],
                    "cid": cid,
                    "redeemScript": {"$bytes": rhex},
                }))
            })
            .collect()
    };

    let mut tagged = Map::new();
    tagged.insert("v".into(), json!(1));
    tagged.insert("network".into(), json!(network_id));
    tagged.insert("profileId".into(), json!(PROFILE_ID));
    tagged.insert("cid".into(), json!(cid));
    tagged.insert("origin".into(), d["origin"].clone());
    tagged.insert(
        "settings".into(),
        json!({
            "ticketPrice": {"$bigint": js_string(settings.get("ticketPrice"))},
            "ticketCap": settings["ticketCap"],
            "minTickets": settings["minTickets"],
            "closeEligibleDaa": {"$bigint": js_string(settings.get("closeEligibleDaa"))},
        }),
    );
    tagged.insert("manager".into(), manager);
    tagged.insert("frozen".into(), Value::Array(frozen_arr));
    tagged.insert("accepting".into(), d["accepting"].clone());
    tagged.insert("step".into(), d.get("step").cloned().unwrap_or(json!(1)));
    tagged.insert("buys".into(), d.get("buys").cloned().unwrap_or(json!(0)));
    tagged.insert("bought".into(), d.get("bought").cloned().unwrap_or(json!(0)));
    tagged.insert(
        "fees".into(),
        json!({"$bigint": d.get("fees").and_then(|v| v.as_str()).unwrap_or("0")}),
    );
    tagged.insert("terminal".into(), terminal);
    tagged.insert(
        "receipts".into(),
        d.get("receipts").cloned().unwrap_or(json!([])),
    );
    tagged.insert("pending".into(), json!([]));
    tagged.insert("halted".into(), Value::Null);
    tagged.insert("source".into(), json!("imported"));
    tagged.insert("intent".into(), Value::Null);
    if let Some(draw) = d.get("draw").filter(|v| !v.is_null()) {
        tagged.insert("draw".into(), draw.clone());
    }
    if let Some(history) = d.get("history").filter(|v| !v.is_null()) {
        tagged.insert("history".into(), history.clone());
    }

    let round_text = format!("{}\n", serde_json::to_string_pretty(&Value::Object(tagged))?);

    let shards: Vec<Value> = preimages
        .map(|m| {
            m.iter()
                .filter_map(|(h, v)| {
                    v.as_str().map(|rh| json!({"hash": h, "redeemHex": rh}))
                })
                .collect()
        })
        .unwrap_or_default();

    Ok((round_text, json!({"shards": shards})))
}
