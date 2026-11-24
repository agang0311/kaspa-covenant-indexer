//! Spends accepted below the node's pruning point (Plan B for `Engine::resolve_spend`). The accepting header and block
//! body are gone, so the spending transaction comes from REST. The REST row is content only; it is used only when
//!  1. its consensus fields reproduce the txid (REST does not serve lockTime/sequence/gas: a few candidates are hashed,
//!     only an exact match is kept);
//!  2. every funding input amount comes from a previous transaction that is stored or likewise reproduces its txid;
//!  3. the node still proves the acceptance through one of its outputs - a live UTXO entry, or the Full row of a retained
//!     selected-chain child spending it - which also yields the exact accepting DAA (UTXO entries carry it);
//!  4. that DAA is below the pruning point: consensus-irreversible, so the transition is stored FINAL.
//! The v1 txid does not commit signature scripts: the witness is REST-sourced and the pinned rules bind what it decodes
//! to the txid-committed outputs. A rule rejection here defers (EVIDENCE_PRUNED) instead of marking the round STALE.
//! Live-tip anchors: the newest checkpoint recorded while a tip was seen unspent bounds its spender search once the
//! tip's creating header is pruned.
use super::*;
use crate::core::bytes::{hex, unhex, unhex_len};
use crate::core::hashes::{blake2b256_keyed, domain_hash};
use crate::core::state::{DRAW_DELAY, TIMEOUT_DELAY};

/// EVIDENCE_PRUNED retries are rate limited: missing evidence does not reappear by itself.
const PRUNED_RETRY_MS: i64 = 300_000;
/// Sequences of the pinned V2.0.2 spend paths (open / draw delay / timeout delay) and the default final sequence.
const SEQUENCES: [i128; 4] = [0, DRAW_DELAY, TIMEOUT_DELAY, u64::MAX as i128];
const CHILD_SEARCH_LIMIT: usize = 50;
/// The anchor checkpoint predates the UTXO observation by at least this much (UTXO index lag).
const LIVE_ANCHOR_MARGIN_MS: i64 = 30_000;
const LIVE_ANCHOR_REFRESH_MS: i64 = 600_000;

pub(super) fn is_pruned(e: &anyhow::Error) -> bool {
    e.to_string().contains("cannot find header")
}

fn num(v: Option<&Value>, bits: u32, field: &str) -> Result<u64> {
    let n = js_bigint(v).map_err(|_| anyhow!("TXID_FIELD_{field}"))?;
    if n < 0 || n >= (1i128 << bits) {
        bail!("TXID_FIELD_{field}");
    }
    Ok(n as u64)
}

fn text<'a>(v: Option<&'a Value>, field: &str) -> Result<&'a str> {
    v.and_then(|x| x.as_str())
        .ok_or_else(|| anyhow!("TXID_FIELD_{field}"))
}

/// Consensus transaction id (rusty-kaspa `consensus/core/src/hashing/tx.rs` `id_v0` / `id_v1`) of a node-format JSON
/// transaction. Signature scripts, sig-op counts, compute budgets and mass are not part of the id.
pub(super) fn transaction_id(tx: &Value) -> Result<String> {
    let version = num(tx.get("version"), 16, "version")? as u16;
    let mut b: Vec<u8> = vec![];
    b.extend(version.to_le_bytes());
    let inputs = arr(tx.get("inputs"));
    b.extend((inputs.len() as u64).to_le_bytes());
    for i in inputs {
        let p = i.get("previousOutpoint");
        b.extend(unhex_len(text(p.and_then(|p| p.get("transactionId")), "outpoint")?, 32)?);
        b.extend((num(p.and_then(|p| p.get("index")), 32, "index")? as u32).to_le_bytes());
        b.extend(0u64.to_le_bytes()); // signature script excluded: empty var bytes
        b.extend(num(i.get("sequence"), 64, "sequence")?.to_le_bytes());
    }
    let outputs = arr(tx.get("outputs"));
    b.extend((outputs.len() as u64).to_le_bytes());
    for o in outputs {
        b.extend(num(o.get("value"), 64, "value")?.to_le_bytes());
        let spk = o.get("scriptPublicKey");
        b.extend((num(spk.and_then(|s| s.get("version")), 16, "spkVersion")? as u16).to_le_bytes());
        let script = unhex(text(spk.and_then(|s| s.get("script")), "script")?)?;
        b.extend((script.len() as u64).to_le_bytes());
        b.extend(script);
        if version >= 1 {
            match nn(o.get("covenant")) {
                Some(c) => {
                    b.push(1);
                    b.extend((num(c.get("authorizingInput"), 16, "authorizingInput")? as u16).to_le_bytes());
                    b.extend(unhex_len(text(c.get("covenantId"), "covenantId")?, 32)?);
                }
                None => b.push(0),
            }
        }
    }
    b.extend(num(tx.get("lockTime"), 64, "lockTime")?.to_le_bytes());
    b.extend(unhex_len(text(tx.get("subnetworkId"), "subnetworkId")?, 20)?);
    b.extend(num(tx.get("gas"), 64, "gas")?.to_le_bytes());
    let payload = match nn(tx.get("payload")) {
        None => vec![],
        Some(p) => unhex(text(Some(p), "payload")?)?,
    };
    if version == 0 {
        b.extend((payload.len() as u64).to_le_bytes());
        b.extend(&payload);
        return Ok(hex(&blake2b256_keyed(&b, b"TransactionID")));
    }
    b.extend(0u64.to_le_bytes()); // payload excluded from the rest digest
    let rest = domain_hash("TransactionRest", &b)?;
    let payload_digest = domain_hash("PayloadDigest", &payload)?;
    Ok(hex(&domain_hash(
        "TransactionV1Id",
        &[payload_digest, rest].concat(),
    )?))
}

/// REST rows carry their own `index`; order them before rebuilding.
fn by_index(rows: &[Value]) -> Vec<Value> {
    let mut v = rows.to_vec();
    v.sort_by_key(|r| js_int(r.get("index")).unwrap_or(i64::MAX));
    v
}

/// Per-input sequence candidates: the full product up to 4 inputs, else input 0 free and the rest uniform.
fn sequence_sets(n: usize) -> Vec<Vec<i128>> {
    if n <= 4 {
        let mut sets: Vec<Vec<i128>> = vec![vec![]];
        for _ in 0..n {
            sets = sets
                .into_iter()
                .flat_map(|s| {
                    SEQUENCES.iter().map(move |q| {
                        let mut t = s.clone();
                        t.push(*q);
                        t
                    })
                })
                .collect();
        }
        return sets;
    }
    let mut sets = vec![];
    for a in SEQUENCES {
        for b in SEQUENCES {
            let mut t = vec![a];
            t.extend(vec![b; n - 1]);
            sets.push(t);
        }
    }
    sets
}

/// Node-format transaction rebuilt from a REST `/transactions/{id}` row: Some only for the lockTime/sequence candidate
/// whose consensus id equals `txid`. Values REST does serve are tried first.
fn rebuild(raw: &Value, txid: &str, lock_times: &[i128]) -> Result<Option<Value>> {
    if raw.get("transaction_id").and_then(|t| t.as_str()) != Some(txid) {
        bail!("REST_TXID_MISMATCH");
    }
    let mut row = raw.clone();
    row["inputs"] = Value::Array(by_index(arr(raw.get("inputs"))));
    row["outputs"] = Value::Array(by_index(arr(raw.get("outputs"))));
    let mut t = rest_tx_dto(&row, txid);
    t["verboseData"] = json!({"transactionId": txid, "blockHash": null});
    let ins = arr(row.get("inputs")).to_vec();
    for (n, i) in ins.iter().enumerate() {
        t["inputs"][n]["computeBudget"] = json!(js_int(nn(i.get("compute_budget"))).unwrap_or(0));
        t["inputs"][n]["sigOpCount"] = json!(js_int(nn(i.get("sig_op_count"))).unwrap_or(0));
    }
    let mut lts: Vec<i128> = vec![];
    if let Some(l) = nn(raw.get("lock_time")) {
        lts.push(js_bigint(Some(l))?);
    }
    for l in lock_times {
        if !lts.contains(l) {
            lts.push(*l);
        }
    }
    let mut seqs = sequence_sets(ins.len());
    if !ins.is_empty() && ins.iter().all(|i| nn(i.get("sequence")).is_some()) {
        seqs.insert(0, ins.iter().map(|i| js_bigint(i.get("sequence"))).collect::<Result<_>>()?);
    }
    for lt in &lts {
        t["lockTime"] = json!(lt.to_string());
        for s in &seqs {
            for (k, q) in s.iter().enumerate() {
                t["inputs"][k]["sequence"] = json!(q.to_string());
            }
            if transaction_id(&t)? == txid {
                return Ok(Some(t));
            }
        }
    }
    Ok(None)
}

/// Node-format UTXO entry for a txid-bound previous output (its DAA stays unknown).
fn utxo_entry(out: &Value) -> Result<Value> {
    let spk = &out["scriptPublicKey"];
    let version = num(spk.get("version"), 16, "spkVersion")? as u16;
    Ok(json!({"amount": num(out.get("value"), 64, "value")?, "blockDaaScore": null,
        "covenantId": nn(out.get("covenant")).and_then(|c| c.get("covenantId")).cloned().unwrap_or(Value::Null),
        "scriptPublicKey": format!("{}{}", hex(&version.to_le_bytes()), text(spk.get("script"), "script")?)}))
}

/// A node-reported UTXO entry describes exactly this rebuilt output.
fn entry_matches(amount: i128, spk: &Value, cid: Option<&str>, out: &Value) -> bool {
    let want = nn(out.get("covenant"))
        .and_then(|c| c.get("covenantId"))
        .and_then(|c| c.as_str());
    js_bigint(out.get("value")).ok() == Some(amount)
        && wire_spk(&out["scriptPublicKey"]).is_ok_and(|w| same_spk(spk, &w))
        && cid == want
}

impl Engine {
    async fn rest_get(&self, path: &str) -> Result<Value> {
        let Some(base) = self.config.rest_url.as_deref() else {
            bail!("REST_NOT_CONFIGURED")
        };
        let url = format!("{}{path}", base.trim_end_matches('/'));
        let mut last = anyhow!("REST_QUERY_FAILED");
        for attempt in 0..2u64 {
            match self.http.get(&url).timeout(Duration::from_millis(6000)).send().await {
                Ok(r) if r.status().is_success() => {
                    return r.json::<Value>().await.map_err(|e| anyhow!("REST_BAD_JSON {e}"));
                }
                Ok(r) if r.status().as_u16() == 404 => bail!("REST_NOT_FOUND"),
                Ok(r) => last = anyhow!("REST_STATUS_{}", r.status().as_u16()),
                Err(e) => last = anyhow!("REST_QUERY_FAILED {}", slice_chars(&e.to_string(), 80)),
            }
            tokio::time::sleep(Duration::from_millis(800 * (attempt + 1))).await;
        }
        Err(last)
    }

    async fn pruning_point(&self) -> Result<Acc> {
        let d = self.q("getBlockDagInfo", json!({})).await?;
        self.header(&js_string(d.get("pruningPointHash"))).await
    }

    /// Previous output of a funding input from content bound to its txid: a stored transition (node block body or an
    /// earlier recovery) or a REST row that reproduces the txid.
    async fn verified_output(&self, outpoint: &Value) -> Result<Option<Value>> {
        let txid = js_string(outpoint.get("transactionId"));
        let Some(index) = js_int(outpoint.get("index")).and_then(|i| usize::try_from(i).ok()) else {
            return Ok(None);
        };
        let stored = self.store.db().any_active_transition_by_txid(&txid)?;
        let stored = stored.and_then(|t| {
            ["latestTransaction", "genesisTransaction"]
                .iter()
                .filter_map(|k| t.next.get(*k))
                .find(|x| txid_of(x).as_deref() == Some(txid.as_str()))
                .cloned()
        });
        let tx = match stored {
            Some(t) => Some(t),
            None => {
                let raw = self
                    .rest_get(&format!("/transactions/{txid}?inputs=true&outputs=true&resolve_previous_outpoints=no"))
                    .await?;
                rebuild(&raw, &txid, &[0])?
            }
        };
        match tx.as_ref().and_then(|t| arr(t.get("outputs")).get(index)) {
            Some(o) => Ok(Some(utxo_entry(o)?)),
            None => Ok(None),
        }
    }

    /// Node proof that `tx` (content bound to `txid`) was accepted, with its exact accepting DAA: a live output's UTXO
    /// entry, or the Full row of a retained selected-chain child spending one of its outputs. Covenant outputs first.
    async fn accepted_daa(&self, txid: &str, tx: &Value) -> Result<Option<(i128, Value)>> {
        let outs = arr(tx.get("outputs"));
        let mut order: Vec<usize> = (0..outs.len()).collect();
        order.sort_by_key(|&k| nn(outs[k].get("covenant")).is_none());
        let mut addrs: Vec<(usize, String)> = vec![];
        for k in order {
            if let Ok(a) = self.addr_of(&outs[k]["scriptPublicKey"]) {
                addrs.push((k, a));
            }
        }
        let mut unique: Vec<&str> = vec![];
        for (_, a) in &addrs {
            if !unique.contains(&a.as_str()) {
                unique.push(a);
            }
        }
        if unique.is_empty() {
            return Ok(None);
        }
        let r = self.q("getUtxosByAddresses", json!({ "addresses": unique })).await?;
        for e in arr(r.get("entries")) {
            let u = normalize_utxo(e)?;
            if js_string(u.outpoint.get("transactionId")) != txid {
                continue;
            }
            let o = js_int(u.outpoint.get("index"))
                .and_then(|i| usize::try_from(i).ok())
                .and_then(|k| outs.get(k));
            if !o.is_some_and(|o| entry_matches(u.amount, &u.spk, u.covenant_id.as_deref(), o)) {
                bail!("PRUNED_LIVE_OUTPUT_MISMATCH");
            }
            return Ok(Some((
                u.block_daa_score,
                json!({"kind": "LIVE_OUTPUT", "outpoint": outpoint_key(&u.outpoint)}),
            )));
        }
        for (k, addr) in &addrs {
            let path = format!("/addresses/{addr}/full-transactions?limit={CHILD_SEARCH_LIMIT}&resolve_previous_outpoints=no");
            let list = match self.rest_get(&path).await {
                Ok(l) => l,
                Err(e) => {
                    self.store.log("REST_HINT_QUERY_FAILED", json!({"address": addr, "message": e.to_string()}));
                    continue;
                }
            };
            for c in arr(Some(&list)) {
                let spends_k = arr(c.get("inputs")).iter().any(|i| {
                    i.get("previous_outpoint_hash").and_then(|h| h.as_str()) == Some(txid)
                        && js_int(i.get("previous_outpoint_index")) == Some(*k as i64)
                });
                let (Some(child), Some(acc)) = (
                    c.get("transaction_id").and_then(|t| t.as_str()),
                    c.get("accepting_block_hash").and_then(|a| a.as_str()),
                ) else {
                    continue;
                };
                if !spends_k {
                    continue;
                }
                // A pruned child is no node evidence; a retained one must be accepted by a selected-chain block.
                let Ok(h) = self.header(acc).await else { continue };
                if !h.is_chain {
                    continue;
                }
                let Some(row) = self.accepted_row(child, &h, "Full").await.ok().flatten() else {
                    continue;
                };
                let input = arr(row.tx.get("inputs")).iter().find(|i| {
                    let p = i.get("previousOutpoint");
                    p.and_then(|p| p.get("transactionId")).and_then(|x| x.as_str()) == Some(txid)
                        && js_int(p.and_then(|p| p.get("index"))) == Some(*k as i64)
                });
                let Some(input) = input else {
                    bail!("PRUNED_CHILD_INPUT_MISSING")
                };
                let e = &input["verboseData"]["utxoEntry"];
                if is_nullish(e.get("blockDaaScore")) {
                    continue;
                }
                let cid = nn(e.get("covenantId")).and_then(|c| c.as_str());
                if !entry_matches(js_bigint(e.get("amount"))?, &wire_spk(&e["scriptPublicKey"])?, cid, &outs[*k]) {
                    bail!("PRUNED_CHILD_ENTRY_MISMATCH");
                }
                return Ok(Some((
                    js_bigint(e.get("blockDaaScore"))?,
                    json!({"kind": "RETAINED_CHILD", "child": child, "accepting": h.hash, "outpoint": format!("{txid}:{k}")}),
                )));
            }
        }
        Ok(None)
    }

    fn pruned_miss(&self, rec: &RoundRow, txid: &str, reason: &str, extra: Value) -> Result<Option<String>> {
        self.store.log(
            "PRUNED_EVIDENCE_MISSING",
            spread(&json!({"round": rec.id, "txid": txid, "reason": reason}), extra),
        );
        Ok(None)
    }

    /// Plan B for one REST spender of `rec`'s tip: Some(apply result) once every piece of evidence binds, None to defer.
    async fn recover_pruned(self: &Arc<Self>, rec: &RoundRow, txid: &str) -> Result<Option<String>> {
        let raw = self
            .rest_get(&format!("/transactions/{txid}?inputs=true&outputs=true&resolve_previous_outpoints=no"))
            .await?;
        let accepting = raw
            .get("accepting_block_hash")
            .and_then(|a| a.as_str())
            .filter(|a| !a.is_empty());
        let (Some(accepting), Some(blue), true) = (
            accepting,
            nn(raw.get("accepting_block_blue_score")),
            truthy(raw.get("is_accepted")),
        ) else {
            return self.pruned_miss(rec, txid, "REST_NOT_ACCEPTED", json!({}));
        };
        let blue = js_bigint(Some(blue))?;
        let mut lock_times = vec![0];
        if let Ok(c) = js_bigint(
            rec.data
                .pointer("/state/config/closeEligibleDaa")
                .or_else(|| rec.data.pointer("/settings/closeEligibleDaa")),
        ) {
            if c > 0 {
                lock_times.push(c - 1);
            }
            lock_times.push(c);
        }
        let Some(mut tx) = rebuild(&raw, txid, &lock_times)? else {
            return self.pruned_miss(rec, txid, "TXID_NOT_REPRODUCED", json!({}));
        };
        if !spends(&tx, &rec.data["tip"]) {
            return self.pruned_miss(rec, txid, "NOT_A_TIP_SPEND", json!({}));
        }
        for n in 1..arr(tx.get("inputs")).len() {
            let po = tx["inputs"][n]["previousOutpoint"].clone();
            let Some(entry) = self.verified_output(&po).await? else {
                return self.pruned_miss(rec, txid, "FUNDING_UNBOUND", json!({"outpoint": outpoint_key(&po)}));
            };
            tx["inputs"][n]["verboseData"] = json!({ "utxoEntry": entry });
        }
        let Some((daa, proof)) = self.accepted_daa(txid, &tx).await? else {
            return self.pruned_miss(rec, txid, "ACCEPTANCE_UNPROVEN", json!({}));
        };
        let pp = self.pruning_point().await?;
        if daa >= pp.daa || blue >= pp.blue_score {
            return self.pruned_miss(rec, txid, "NOT_BELOW_PRUNING_POINT",
                json!({"daa": daa.to_string(), "pruningPointDaa": pp.daa.to_string()}));
        }
        let acc = Acc {
            hash: accepting.to_string(),
            daa,
            blue_score: blue,
            is_chain: true,
            selected_parent: None,
        };
        // A rule rejection of REST-sourced witness bytes is no proof of a rule violation: defer instead of STALE.
        let plugin = self.contracts().get(rec.contract.as_deref().unwrap_or("null"))?;
        let b = BlockRef {
            hash: acc.hash.clone(),
            daa: daa.to_string(),
        };
        if let Err(e) = plugin
            .advance(&rec.data, &normalize_consensus_fields(&tx), &b)
            .and_then(|r| check_round(r, &plugin.key))
        {
            return self.pruned_miss(rec, txid, "RULES_REJECTED_REST_WITNESS", json!({"message": e.to_string()}));
        }
        let recovered = json!({"txidReproduced": true, "acceptingDaaProof": proof,
            "pruningPoint": {"hash": pp.hash, "daa": pp.daa.to_string()},
            "acceptingHashSource": "REST", "acceptingBlueScoreSource": "REST",
            "witnessSource": "REST (signature scripts are not committed by the v1 txid)"});
        self.store.log(
            "PRUNED_RECOVERED",
            json!({"round": rec.id, "txid": txid, "acceptingDaa": daa.to_string(), "proof": proof}),
        );
        let c = Candidate {
            txid: txid.to_string(),
            tx,
            containing: None,
            start: None,
            accepting: None,
            source: "rest-pruned".into(),
            recovered: Some(recovered),
        };
        let r = self.apply_spend(rec, c, &acc).await?;
        // A waiting round may have needed this transaction (funding input): retry those at the next tick.
        for p in self.pending.lock().unwrap().values_mut() {
            if p.reason == "EVIDENCE_PRUNED" {
                p.at = 0;
            }
        }
        Ok(Some(r))
    }

    /// Spenders the node can no longer place (accepting header pruned): Plan B, else defer as EVIDENCE_PRUNED.
    pub(super) async fn resolve_pruned(self: &Arc<Self>, rec: &RoundRow, txids: Vec<String>) -> Result<String> {
        if self.config.rest_url.is_some() {
            for txid in &txids {
                match self.recover_pruned(rec, txid).await {
                    Ok(Some(r)) => {
                        self.forget_pending(&format!("spend:{}", rec.id));
                        return Ok(r);
                    }
                    Ok(None) => {}
                    Err(e) => self.store.log(
                        "PRUNED_RECOVERY_FAILED",
                        json!({"round": rec.id, "txid": txid, "message": e.to_string()}),
                    ),
                }
            }
        }
        Ok(self.defer_spend(&rec.id, "EVIDENCE_PRUNED", txids))
    }

    pub(super) fn pruned_backoff(&self, key: &str) -> bool {
        self.pending
            .lock()
            .unwrap()
            .get(key)
            .is_some_and(|p| p.reason == "EVIDENCE_PRUNED" && now_ms() - p.at < PRUNED_RETRY_MS)
    }

    /// Newest checkpoint sink recorded while `tip` was observed unspent: its spender is accepted after it.
    pub(super) fn live_anchor(&self, round_id: &str, tip: &Value) -> Result<Option<String>> {
        let a = self.store.db().get_meta(&format!("live-anchor:{round_id}"))?;
        if a.get("tip").and_then(|t| t.as_str()) != Some(outpoint_key(tip).as_str()) {
            return Ok(None);
        }
        Ok(a.get("sink").and_then(|s| s.as_str()).map(|s| s.to_string()))
    }

    pub(super) fn mark_live_anchor(&self, round_id: &str, tip: &Value, observed_at: i64) -> Result<()> {
        let key = format!("live-anchor:{round_id}");
        let tip_key = outpoint_key(tip);
        let db = self.store.db();
        let cur = db.get_meta(&key)?;
        if cur.get("tip").and_then(|t| t.as_str()) == Some(tip_key.as_str())
            && js_int(cur.get("at")).unwrap_or(0) > observed_at - LIVE_ANCHOR_REFRESH_MS
        {
            return Ok(());
        }
        let Some(cp) = db
            .checkpoints_before(observed_at - LIVE_ANCHOR_MARGIN_MS, 1)?
            .into_iter()
            .next()
        else {
            return Ok(());
        };
        db.set_meta(&key, &json!({"tip": tip_key, "sink": cp.sink, "checkpointAt": cp.at, "at": observed_at}))
    }
}
