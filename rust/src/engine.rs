//! Event-driven covenant-lineage follower core (`src/engine.mjs`). Contract rules come from plugins; the node is the
//! injected dual-node set. Truth = selected-chain acceptance + covenant rules: this cache never decides entitlement.
//! All chain work runs on ONE serial queue (`run`, deduplicated by key while waiting); event intake and checkpoints stay
//! outside the queue. Jobs that enqueue other jobs go through the non-async `run_*` helpers (no async type cycles).
use crate::contracts::{BlockRef, ContractRegistry, check_round};
use crate::dual_node::DualNode;
use crate::log::log;
use crate::networks::Network;
use crate::normalize::{
    is_full_transaction, normalize_consensus_fields, normalize_utxo, outpoint_key,
    prepare_transaction, related_transaction, same_spk, spends, spends_loose, txid_of, wire_spk,
};
use crate::store::{Checkpoint, NewTransition, RoundRow, Store, TransitionRow, enc};
use crate::util::{
    canonical, is_nullish, js_bigint, js_int, js_number, js_string, now_ms, slice_chars, spread,
    truthy,
};
use crate::window_scan::{Acc, Cursor, Found, WindowResult, scan_window, strings};
use anyhow::{Result, anyhow, bail};
use futures_util::FutureExt;
use futures_util::future::BoxFuture;
use indexmap::IndexMap;
use serde_json::{Map, Value, json};
use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::panic::AssertUnwindSafe;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};

pub const TRUST_NOTE: &str = "Observed by this indexer via one RPC node; consumers must verify acceptance/UTXO on their own node before value decisions";

#[derive(Clone, Debug)]
pub struct EngineConfig {
    pub checkpoint_keep: i64,
    pub checkpoints_per_search: i64,
    /// No fixed page cap for the v1 acceptance scan: only a wall-clock budget stops it (then PENDING and retry).
    pub scan_budget_ms: i64,
    /// Project policy, NOT a consensus finality rule.
    pub finality_blue_score: i128,
    pub genesis_lookback_ms: i64,
    pub fast_lookback_ms: i64,
    /// Older start -> the single Full page would be too large; use the bounded window walk instead.
    pub fast_max_age_ms: i64,
    pub fast_max_pages: usize,
    pub window_blue: i128,
    /// Max time one resolveSpend holds the serial queue in the window walk.
    pub window_slice_ms: i64,
    /// Pages per window chunk (the JS build ran one child process per chunk; Rust walks in-process).
    pub window_pages: usize,
    pub auto_catchup: bool,
    /// Spender / Genesis hint oracle (REST). Hints only: consensus fields always come from the node.
    pub rest_url: Option<String>,
    /// Only enqueue relay outbox rows when a relay consumer (--relay / --relay-file) is configured.
    pub relay_enabled: bool,
}

impl Default for EngineConfig {
    fn default() -> Self {
        EngineConfig {
            checkpoint_keep: 720,
            checkpoints_per_search: 3,
            scan_budget_ms: 600_000,
            finality_blue_score: 600,
            genesis_lookback_ms: 30_000,
            fast_lookback_ms: 4000,
            fast_max_age_ms: 120_000,
            fast_max_pages: 6,
            window_blue: 100,
            window_slice_ms: 20_000,
            window_pages: 200,
            auto_catchup: true,
            rest_url: None,
            relay_enabled: false,
        }
    }
}

/// Uncertain spend work kept in memory (`engine.pending`); it never expires by wall time.
#[derive(Clone, Debug)]
pub struct PendingSpend {
    pub id: String,
    pub since: i64,
    /// Last deferral (EVIDENCE_PRUNED retries are rate limited from it).
    pub at: i64,
    pub reason: String,
    pub candidates: Vec<String>,
}

#[derive(Clone, Debug)]
struct Candidate {
    txid: String,
    tx: Value,
    containing: Option<String>,
    start: Option<String>,
    accepting: Option<Acc>,
    source: String,
    /// Plan B evidence for a spend accepted below the pruning point (`pruned.rs`).
    recovered: Option<Value>,
}

/// A located accepted transaction: accepting chain header, the transaction row and its containing block.
#[derive(Clone, Debug)]
struct Hit {
    acc: Acc,
    tx: Value,
    containing: Option<String>,
}

impl From<Found> for Hit {
    fn from(f: Found) -> Hit {
        Hit {
            acc: f.acc,
            tx: f.tx,
            containing: Some(f.containing),
        }
    }
}

type JobFn = Box<dyn FnOnce() -> BoxFuture<'static, Result<()>> + Send>;
enum Job {
    Run(String, JobFn),
    Barrier(oneshot::Sender<()>),
}

pub struct Engine {
    pub store: Arc<Store>,
    contracts: RwLock<Arc<ContractRegistry>>,
    pub node: Arc<DualNode>,
    pub net: &'static Network,
    pub config: EngineConfig,
    http: reqwest::Client,
    subs: Mutex<IndexMap<String, usize>>,
    queue: mpsc::UnboundedSender<Job>,
    scheduled: Arc<Mutex<HashSet<String>>>,
    pending: Mutex<IndexMap<String, PendingSpend>>,
    pub errors: Arc<AtomicU64>,
    pub chain_subscribed: AtomicBool,
    seen_rest_hints: Mutex<HashSet<String>>,
    watch: Mutex<(String, Arc<HashSet<String>>)>,
}

fn arr(v: Option<&Value>) -> &[Value] {
    v.and_then(|x| x.as_array())
        .map(|a| a.as_slice())
        .unwrap_or(&[])
}

fn nn(v: Option<&Value>) -> Option<&Value> {
    v.filter(|x| !x.is_null())
}

/// `Number(x)` as JSON (NaN serializes as null, like JSON.stringify).
fn js_num_json(v: Option<&Value>) -> Value {
    match js_number(v) {
        Some(f) if f.is_finite() && f.fract() == 0.0 && f.abs() < 9.007_199_254_740_992e15 => {
            json!(f as i64)
        }
        Some(f) if f.is_finite() => json!(f),
        _ => Value::Null,
    }
}

/// `JSON.stringify(x)` (undefined -> None).
fn ser(v: Option<&Value>) -> Option<String> {
    v.map(enc)
}

/// `JSON.stringify(rows.map(({verboseData, ...rest}) => rest))`.
fn ser_stripped(v: Option<&Value>) -> Result<Option<String>> {
    match v {
        Some(Value::Array(a)) => {
            let rows: Vec<Value> = a
                .iter()
                .map(|r| {
                    let mut r = r.clone();
                    if let Some(o) = r.as_object_mut() {
                        o.shift_remove("verboseData");
                    }
                    r
                })
                .collect();
            Ok(Some(enc(&Value::Array(rows))))
        }
        _ => bail!("rows.map is not a function"),
    }
}

/// `{...tx, verboseData: {...tx.verboseData, blockHash: containing}}`.
fn with_block_hash(tx: &Value, containing: Value) -> Value {
    let vd = spread(
        tx.get("verboseData").unwrap_or(&Value::Null),
        json!({ "blockHash": containing }),
    );
    spread(tx, json!({ "verboseData": vd }))
}

/// `tx.inputs.map((i, n) => high.inputs[n]?.verboseData ? {...i, verboseData: high.inputs[n].verboseData} : i)`.
fn with_input_verbose(tx: &mut Value, high: &Value) -> Result<()> {
    let hin = arr(high.get("inputs")).to_vec();
    let Some(Value::Array(ins)) = tx.get_mut("inputs") else {
        bail!("tx.inputs.map is not a function")
    };
    for (n, i) in ins.iter_mut().enumerate() {
        if let Some(vd) = hin
            .get(n)
            .and_then(|h| h.get("verboseData"))
            .filter(|v| truthy(Some(v)))
        {
            *i = spread(i, json!({ "verboseData": vd }));
        }
    }
    Ok(())
}

fn panic_text(p: Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = p.downcast_ref::<&str>() {
        format!("PANIC: {s}")
    } else if let Some(s) = p.downcast_ref::<String>() {
        format!("PANIC: {s}")
    } else {
        "PANIC".into()
    }
}

impl Engine {
    pub fn new(
        store: Arc<Store>,
        contracts: Arc<ContractRegistry>,
        node: Arc<DualNode>,
        net: &'static Network,
        config: EngineConfig,
    ) -> Arc<Engine> {
        let (tx, mut rx) = mpsc::unbounded_channel::<Job>();
        let scheduled: Arc<Mutex<HashSet<String>>> = Arc::new(Mutex::new(HashSet::new()));
        let errors = Arc::new(AtomicU64::new(0));
        {
            let (sched, errs, st) = (scheduled.clone(), errors.clone(), store.clone());
            tokio::spawn(async move {
                while let Some(job) = rx.recv().await {
                    match job {
                        Job::Barrier(done) => {
                            let _ = done.send(());
                        }
                        Job::Run(key, f) => {
                            sched.lock().unwrap().remove(&key);
                            let r = AssertUnwindSafe(async move { f().await })
                                .catch_unwind()
                                .await;
                            let err = match r {
                                Ok(Ok(())) => None,
                                Ok(Err(e)) => Some(e.to_string()),
                                Err(p) => Some(panic_text(p)),
                            };
                            if let Some(m) = err {
                                errs.fetch_add(1, Ordering::SeqCst);
                                st.log("ERROR", json!({"key": key, "message": m}));
                                log("ERROR", json!([key, m]));
                            }
                        }
                    }
                }
            });
        }
        let http = reqwest::Client::builder()
            .user_agent("kaspa-covenant-indexer")
            .build()
            .expect("http client");
        Arc::new(Engine {
            store,
            contracts: RwLock::new(contracts),
            node,
            net,
            config,
            http,
            subs: Mutex::new(IndexMap::new()),
            queue: tx,
            scheduled,
            pending: Mutex::new(IndexMap::new()),
            errors,
            chain_subscribed: AtomicBool::new(false),
            seen_rest_hints: Mutex::new(HashSet::new()),
            watch: Mutex::new((String::new(), Arc::new(HashSet::new()))),
        })
    }

    pub fn contracts(&self) -> Arc<ContractRegistry> {
        self.contracts.read().unwrap().clone()
    }
    pub fn set_contracts(&self, c: Arc<ContractRegistry>) {
        *self.contracts.write().unwrap() = c;
    }
    pub fn pending_spends(&self) -> Vec<PendingSpend> {
        self.pending.lock().unwrap().values().cloned().collect()
    }
    pub fn subscriptions(&self) -> Vec<String> {
        self.subs.lock().unwrap().keys().cloned().collect()
    }
    pub fn queued(&self) -> usize {
        self.scheduled.lock().unwrap().len()
    }

    fn addr_of(&self, spk: &Value) -> Result<String> {
        match (
            spk.get("version").and_then(|v| v.as_i64()),
            spk.get("script").and_then(|s| s.as_str()),
        ) {
            (Some(v), Some(s)) => crate::address::address_of(v, s, self.net.network_id),
            _ => bail!("SPK_HAS_NO_ADDRESS"),
        }
    }

    async fn q(&self, method: &'static str, params: Value) -> Result<Value> {
        self.node.query(method, params).await
    }

    async fn sink_blue(&self) -> Result<i128> {
        js_bigint(
            self.q("getSinkBlueScore", json!({}))
                .await?
                .get("blueScore"),
        )
    }

    // ---------- serial queue ----------
    /// Enqueue `f` unless a job with the same key is already waiting (JS `run`).
    pub fn run<F, Fut>(self: &Arc<Self>, key: String, f: F)
    where
        F: FnOnce(Arc<Engine>) -> Fut + Send + 'static,
        Fut: Future<Output = Result<()>> + Send + 'static,
    {
        if !self.scheduled.lock().unwrap().insert(key.clone()) {
            return;
        }
        let me = self.clone();
        let job: JobFn = Box::new(move || Box::pin(f(me)));
        if self.queue.send(Job::Run(key.clone(), job)).is_err() {
            self.scheduled.lock().unwrap().remove(&key);
        }
    }
    /// Resolves when every job queued before this call has finished.
    pub async fn idle(&self) {
        let (tx, rx) = oneshot::channel();
        if self.queue.send(Job::Barrier(tx)).is_ok() {
            let _ = rx.await;
        }
    }
    pub fn run_spend(self: &Arc<Self>, id: &str, event_at: Option<i64>) {
        let id = id.to_string();
        self.run(format!("spend:{id}"), move |e| async move {
            e.resolve_spend(&id, event_at).await.map(|_| ())
        });
    }
    pub fn run_genesis(self: &Arc<Self>, txid: &str, hint: Value, force: bool) {
        let txid = txid.to_string();
        self.run(format!("genesis:{txid}"), move |e| async move {
            e.discover_genesis(&txid, &hint, force).await.map(|_| ())
        });
    }
    /// Genesis after a rolled-back Genesis: starts are read when the job runs (as the JS closure did).
    fn run_genesis_after_rollback(self: &Arc<Self>, txid: &str, observed_at: i64) {
        let txid = txid.to_string();
        self.run(format!("genesis:{txid}"), move |e| async move {
            let starts: Vec<String> = e
                .store
                .db()
                .checkpoints_before(observed_at, e.config.checkpoints_per_search)?
                .into_iter()
                .map(|c| c.sink)
                .collect();
            e.discover_genesis(&txid, &json!({ "starts": starts }), false)
                .await
                .map(|_| ())
        });
    }
    pub fn run_reconcile(self: &Arc<Self>) {
        self.run("reconcile".into(), |e| async move { e.reconcile().await });
    }
    pub fn run_finality(self: &Arc<Self>) {
        self.run(
            "finality".into(),
            |e| async move { e.check_finality().await },
        );
    }
    fn run_reorg(self: &Arc<Self>, key: String, gone: HashSet<String>) {
        self.run(key, move |e| async move {
            let mut rolled: HashSet<String> = HashSet::new();
            let list = e
                .store
                .db()
                .unfinalized_heads()?
                .into_iter()
                .filter(|x| gone.contains(&x.accepting))
                .collect::<Vec<_>>();
            for t in list {
                if rolled.contains(&t.round_id) {
                    continue;
                }
                let h = e.header(&t.accepting).await?; // confirm on the node, never trust the event alone
                if !h.is_chain {
                    let full = e.store.db().transition(&t.round_id, t.seq)?;
                    if let Some(full) = full {
                        e.rollback(&full).await?;
                    }
                    rolled.insert(t.round_id.clone());
                }
            }
            Ok(())
        });
    }

    // ---------- lifecycle ----------
    pub async fn start(self: &Arc<Self>) -> Result<()> {
        let pruned = self.store.tx(|db| db.prune_final_transitions())?;
        if pruned > 0 {
            self.store.db().vacuum_and_truncate_wal()?;
            self.store
                .log("PRUNED_FINAL_TRANSITIONS", json!({"count": pruned}));
        }
        // Every stored round must still have its own contract plugin loaded; never re-interpret it with another one.
        let c = self.contracts();
        let all = self.store.db().round_headers()?;
        for (id, contract, _address, status) in &all {
            if status == "LIVE" && !c.has(contract.as_deref().unwrap_or("\u{0}")) {
                bail!(
                    "LIVE_ROUND_CONTRACT_NOT_LOADED {} {}",
                    id,
                    contract.as_deref().unwrap_or("null")
                );
            }
        }
        let mut addrs: Vec<String> = c.watch_addresses();
        for (_id, _contract, address, status) in &all {
            if status == "LIVE"
                && let Some(a) = address.as_ref().filter(|a| !a.is_empty())
            {
                addrs.push(a.clone());
            }
        }
        self.subscribe_many(&addrs).await?;
        self.initialize_discovery_scan().await?;
        self.checkpoint().await?;
        self.reconcile().await?;
        self.retry_pending()?;
        Ok(())
    }

    /// After reconnect: server-side subscriptions are gone.
    pub async fn resubscribe_all(self: &Arc<Self>) -> Result<()> {
        let addrs = self.subscriptions();
        if !addrs.is_empty() {
            self.node.subscribe_utxos(&addrs).await?;
        }
        if self.chain_subscribed.load(Ordering::SeqCst) {
            let _ = self.node.subscribe_chain().await;
        }
        self.reconcile().await?;
        self.check_finality().await // reorgs that happened while disconnected
    }

    // ---------- subscriptions ----------
    pub async fn subscribe_many(&self, addresses: &[String]) -> Result<()> {
        let mut first_addrs = Vec::new();
        {
            let mut s = self.subs.lock().unwrap();
            for a in addresses {
                if a.is_empty() {
                    continue;
                }
                let n = s.entry(a.clone()).or_insert(0);
                *n += 1;
                if *n == 1 {
                    first_addrs.push(a.clone());
                }
            }
        }
        if !first_addrs.is_empty()
            && let Err(e) = self.node.subscribe_utxos(&first_addrs).await
        {
            let mut s = self.subs.lock().unwrap();
            for a in &first_addrs {
                if let Some(n) = s.get_mut(a) {
                    *n -= 1;
                    if *n == 0 {
                        s.shift_remove(a);
                    }
                }
            }
            return Err(e);
        }
        Ok(())
    }

    pub async fn subscribe(&self, address: Option<&str>) -> Result<()> {
        let Some(a) = address.filter(|a| !a.is_empty()) else {
            return Ok(());
        };
        let first = {
            let mut s = self.subs.lock().unwrap();
            let n = s.entry(a.to_string()).or_insert(0);
            *n += 1;
            *n == 1
        };
        if first && let Err(e) = self.node.subscribe_utxos(&[a.to_string()]).await {
            let mut s = self.subs.lock().unwrap();
            if let Some(n) = s.get_mut(a) {
                *n -= 1;
                if *n == 0 {
                    s.shift_remove(a);
                }
            }
            return Err(e);
        }
        Ok(())
    }
    pub async fn unsubscribe(&self, address: Option<&str>) -> Result<()> {
        let Some(a) = address else { return Ok(()) };
        let last = {
            let mut s = self.subs.lock().unwrap();
            let n = s.get(a).copied().unwrap_or(0);
            if n <= 1 {
                s.shift_remove(a);
                n == 1
            } else {
                s.insert(a.to_string(), n - 1);
                false
            }
        };
        if last {
            self.node.unsubscribe_utxos(&[a.to_string()]).await?;
        }
        Ok(())
    }

    // ---------- events ----------
    pub fn on_utxos_changed(self: &Arc<Self>, data: &Value) -> Result<()> {
        let at = now_ms();
        let tips: HashMap<String, String> = self
            .store
            .db()
            .rounds(true, false)?
            .into_iter()
            .map(|r| (outpoint_key(&r.data["tip"]), r.id))
            .collect();
        for raw in arr(data.get("removed")) {
            let u = normalize_utxo(raw)?;
            if let Some(id) = tips.get(&outpoint_key(&u.outpoint)) {
                if !crate::util::truthy(Some(&self.store.db().tip_spent(id, &u.outpoint)?)) {
                    let cid = self
                        .store
                        .db()
                        .round(id)?
                        .and_then(|r| nn(r.data.get("cid")).cloned())
                        .unwrap_or(Value::Null);
                    self.store.log_at(
                        "SPENT_EVENT",
                        json!({"round": id, "cid": cid, "outpoint": u.outpoint}),
                        at,
                    );
                    self.store.db().mark_tip_spent(id, &u.outpoint, at)?;
                }
                self.run_spend(id, Some(at));
            }
        }
        for raw in arr(data.get("added")) {
            let u = normalize_utxo(raw)?;
            for p in self.contracts().for_genesis(None) {
                let Some(txid) = p.discover(&u).filter(|t| !t.is_empty()) else {
                    continue;
                };
                if self.store.db().round(&txid)?.is_some() {
                    continue;
                }
                let old = self.store.db().discovery(&txid)?;
                if old.map(|o| o.status != "ROLLED_BACK").unwrap_or(false) {
                    continue;
                }
                let daa = u.block_daa_score.to_string();
                self.store.log_at(
                    "DISCOVERY_MARKER",
                    json!({"contract": p.key, "txid": txid, "daa": daa}),
                    at,
                );
                let task = self.store.db().discover_task(
                    &txid,
                    &json!({"eventAt": at, "daa": daa}),
                    at,
                )?;
                if task.status == "PENDING" {
                    self.run_genesis(&txid, json!({}), false);
                }
            }
        }
        Ok(())
    }

    pub async fn initialize_discovery_scan(&self) -> Result<()> {
        if truthy(Some(&self.store.db().get_meta("discoveryScan")?)) {
            return Ok(());
        }
        let oldest = self.store.db().oldest_checkpoint_sink()?;
        let cursor = match oldest {
            Some(s) => s,
            None => js_string(self.q("getBlockDagInfo", json!({})).await?.get("sink")),
        };
        self.store
            .db()
            .set_meta("discoveryScan", &json!({"cursor": cursor, "anchor": cursor, "status": "READY", "at": now_ms(), "error": null}))
    }

    // ---------- checkpoints ----------
    pub async fn checkpoint(&self) -> Result<Checkpoint> {
        let d = self.q("getBlockDagInfo", json!({})).await?;
        let c = Checkpoint {
            at: now_ms(),
            sink: js_string(d.get("sink")),
            virtual_daa: js_bigint(d.get("virtualDaaScore"))?.to_string(),
            sink_blue_score: None,
        };
        self.store
            .db()
            .add_checkpoint(&c, self.config.checkpoint_keep)?;
        Ok(c)
    }

    // ---------- fast path (V2 Full) ----------
    /// Newest checkpoint recorded before `eventAt - fastLookbackMs`, if recent enough (vs the event AND now).
    fn fast_start(&self, event_at: i64) -> Result<Option<String>> {
        let cp = self
            .store
            .db()
            .checkpoints_before(event_at - self.config.fast_lookback_ms, 1)?
            .into_iter()
            .next();
        let now = now_ms();
        Ok(cp
            .filter(|c| {
                event_at - c.at <= self.config.fast_max_age_ms
                    && now - c.at <= self.config.fast_max_age_ms
            })
            .map(|c| c.sink))
    }

    /// Walk the virtual selected chain from `start` with V2 Full; first accepted transaction matching `m`. None when
    /// scanned to the sink blue score observed at call time (termination by blue score, never by a captured sink hash).
    async fn fast_find(
        &self,
        m: &(dyn Fn(&Value) -> bool + Send + Sync),
        start: &str,
    ) -> Result<Option<Found>> {
        let sink_blue = self.sink_blue().await?;
        let mut from = start.to_string();
        let mut from_blue = self.header(&from).await?.blue_score;
        let mut w: i128 = 32;
        for _ in 0..self.config.fast_max_pages {
            let current = if from_blue + w >= sink_blue {
                self.sink_blue().await?
            } else {
                sink_blue
            };
            let want = from_blue + w;
            let min = if current > want { current - want } else { 0 };
            let r = self.q("getVirtualChainFromBlockV2", json!({"startHash": from, "dataVerbosityLevel": "Full", "minConfirmationCount": min as u64})).await?;
            let added = strings(r.get("addedChainBlockHashes"));
            let rows = arr(r.get("chainBlockAcceptedTransactions"));
            let removed = !arr(r.get("removedChainBlockHashes")).is_empty();
            if rows.len() != added.len() {
                bail!("VC_ROWS_NOT_ALIGNED");
            }
            if added.is_empty() {
                if from_blue >= sink_blue || current - from_blue <= w {
                    return Ok(None);
                }
                w = (w * 2).min(256);
                continue;
            }
            for (i, row) in rows.iter().enumerate() {
                let h = row.get("chainBlockHeader");
                if js_string(h.and_then(|h| h.get("hash"))) != added[i] {
                    bail!("VC_HEADER_MISMATCH");
                }
                for t in arr(row.get("acceptedTransactions")) {
                    if !m(t) {
                        continue;
                    }
                    let h = h.unwrap();
                    if is_nullish(h.get("daaScore")) || is_nullish(h.get("blueScore")) {
                        bail!("VC_HEADER_FIELDS_MISSING");
                    }
                    let tx = canonical(t);
                    let containing = tx
                        .get("verboseData")
                        .and_then(|v| v.get("blockHash"))
                        .filter(|b| truthy(Some(b)))
                        .map(|b| js_string(Some(b)));
                    if txid_of(&tx).filter(|s| !s.is_empty()).is_none() || containing.is_none() {
                        bail!("VC_FULL_ROW_INCOMPLETE");
                    }
                    let selected_parent = if i > 0 {
                        Some(added[i - 1].clone())
                    } else if removed {
                        None
                    } else {
                        Some(from.clone())
                    };
                    let acc = Acc {
                        hash: added[i].clone(),
                        daa: js_bigint(h.get("daaScore"))?,
                        blue_score: js_bigint(h.get("blueScore"))?,
                        is_chain: true,
                        selected_parent,
                    };
                    return Ok(Some(Found {
                        acc,
                        tx,
                        containing: containing.unwrap(),
                    }));
                }
            }
            let top = match nn(rows
                .last()
                .and_then(|r| r.get("chainBlockHeader"))
                .and_then(|h| h.get("blueScore")))
            {
                Some(v) => Some(js_bigint(Some(v))?),
                None => None,
            };
            match top {
                Some(t) if !added.is_empty() && t < sink_blue => {
                    from = added.last().unwrap().clone();
                    from_blue = t;
                    w = (w * 2).min(128);
                }
                _ => return Ok(None),
            }
        }
        Ok(None)
    }

    async fn try_fast(
        &self,
        label: &str,
        m: &(dyn Fn(&Value) -> bool + Send + Sync),
        start: Option<&str>,
    ) -> Option<Found> {
        let start = start?;
        match self.fast_find(m, start).await {
            Ok(x) => x,
            Err(e) => {
                self.store.log(
                    "FAST_PATH_FAILED",
                    json!({"label": label, "start": start, "message": e.to_string()}),
                );
                None
            }
        }
    }

    // ---------- bounded window walk ----------
    fn window_save(&self, resume_key: Option<&str>, start: &str, v: Option<Value>) -> Result<()> {
        let Some(k) = resume_key else { return Ok(()) };
        let db = self.store.db();
        let mut all: Map<String, Value> = db.get_meta(k)?.as_object().cloned().unwrap_or_default();
        match v {
            Some(v) => {
                all.insert(start.to_string(), v);
            }
            None => {
                all.shift_remove(start);
            }
        }
        db.set_meta(
            k,
            &if all.is_empty() {
                Value::Null
            } else {
                Value::Object(all)
            },
        )
    }
    fn window_mark(&self, resume_key: Option<&str>, start: &str, cur: &Cursor) -> Result<()> {
        let w = cur.w.map(|w| json!(w.to_string())).unwrap_or(Value::Null);
        self.window_save(
            resume_key,
            start,
            Some(json!({"from": cur.from, "w": w, "at": now_ms()})),
        )
    }

    /// Find the accepted spender of `tip` on the selected chain after `start`, in windows of <= windowBlue blue score.
    /// Holds the queue at most windowSliceMs: then saves a durable per-start cursor under `resume_key` and fails with
    /// WINDOW_SCAN_CONTINUES (the caller defers; the 5 s retry resumes). A cursor is a position hint, never evidence.
    async fn window_find(
        &self,
        tip: &Value,
        start: &str,
        start_blue: Option<i128>,
        resume_key: Option<&str>,
    ) -> Result<Option<Found>> {
        let sink_blue = self.sink_blue().await?;
        let until = now_ms() + self.config.window_slice_ms;
        let saved = match resume_key {
            Some(k) => self.store.db().get_meta(k)?.get(start).cloned(),
            None => None,
        };
        let mut cur_opt: Option<Cursor> = None;
        if let Some(f) = saved
            .as_ref()
            .and_then(|s| s.get("from"))
            .and_then(|f| f.as_str())
            .map(|s| s.to_string())
            && let Ok(h) = self.header(&f).await
            && h.is_chain
        {
            let w_raw = saved.as_ref().and_then(|s| s.get("w"));
            let w = if truthy(w_raw) {
                js_bigint(w_raw).ok().map(Some)
            } else {
                Some(None)
            };
            if let Some(w) = w {
                cur_opt = Some(Cursor {
                    from: f,
                    from_blue: h.blue_score,
                    w,
                });
            }
        }
        let mut cur = match cur_opt {
            Some(c) => c,
            None => Cursor {
                from: start.to_string(),
                from_blue: match start_blue {
                    Some(b) => b,
                    None => self.header(start).await?.blue_score,
                },
                w: None,
            },
        };
        loop {
            if now_ms() > until {
                self.window_mark(resume_key, start, &cur)?;
                bail!("WINDOW_SCAN_CONTINUES");
            }
            let r = scan_window(
                &self.node,
                tip,
                cur.from.clone(),
                cur.from_blue,
                sink_blue,
                self.config.window_blue,
                self.config.window_pages,
                until,
                cur.w,
            )
            .await;
            let r = match r {
                Ok(r) => r,
                Err(e) => {
                    if e.to_string().contains("WINDOW_START_NOT_CHAIN") {
                        self.window_save(resume_key, start, None)?;
                    } else {
                        self.window_mark(resume_key, start, &cur)?;
                    }
                    return Err(e);
                }
            };
            match r {
                WindowResult::Found(f) => return Ok(Some(f)),
                WindowResult::Done => {
                    self.window_save(resume_key, start, None)?;
                    return Ok(None);
                }
                WindowResult::Cursor(c) => {
                    cur = c;
                    self.window_mark(resume_key, start, &cur)?;
                }
            }
        }
    }

    // ---------- spend discovery ----------
    async fn from_mempool(
        &self,
        address: Option<&str>,
        tip: &Value,
        cid: Option<&str>,
    ) -> Vec<Candidate> {
        let res = match self.q("getMempoolEntriesByAddresses", json!({"addresses": [address], "includeOrphanPool": false, "filterTransactionPool": false})).await {
            Ok(r) => r,
            Err(e) => {
                self.store.log("MEMPOOL_QUERY_FAILED", json!({"message": e.to_string()}));
                return vec![];
            }
        };
        // Fixed rusty-kaspa returns [{address, sending[], receiving[]}]; a flat list is accepted as well.
        let mut entries: Vec<&Value> = vec![];
        for e in arr(res.get("entries")) {
            if truthy(e.get("transaction")) {
                entries.push(e);
            } else {
                entries.extend(arr(e.get("sending")));
                entries.extend(arr(e.get("receiving")));
            }
        }
        let mut out: IndexMap<String, Candidate> = IndexMap::new();
        for e in entries {
            if truthy(e.get("isOrphan")) {
                continue;
            }
            let t = e.get("transaction").cloned().unwrap_or(Value::Null);
            if !related_transaction(&t, Some(tip), cid)
                || !spends(&t, tip)
                || txid_of(&t).filter(|s| !s.is_empty()).is_none()
            {
                continue;
            }
            let tx = prepare_transaction(&t, None);
            if let Some(id) = txid_of(&tx).filter(|s| !s.is_empty()) {
                out.insert(
                    id.clone(),
                    Candidate {
                        txid: id,
                        tx,
                        containing: None,
                        start: None,
                        accepting: None,
                        source: "mempool".into(),
                        recovered: None,
                    },
                );
            }
        }
        out.into_values().collect()
    }

    async fn from_rest(
        &self,
        address: Option<&str>,
        tip: &Value,
        cid: Option<&str>,
    ) -> Vec<Candidate> {
        let (Some(base), Some(address)) = (
            self.config.rest_url.as_deref(),
            address.filter(|a| !a.is_empty()),
        ) else {
            return vec![];
        };
        if !truthy(Some(tip)) {
            return vec![];
        }
        let url = format!(
            "{}/addresses/{address}/full-transactions?limit=20",
            base.trim_end_matches('/')
        );
        let list: Value = match self
            .http
            .get(&url)
            .timeout(Duration::from_millis(3500))
            .send()
            .await
        {
            Err(e) => {
                self.store.log(
                    "REST_HINT_QUERY_FAILED",
                    json!({"address": address, "message": slice_chars(&e.to_string(), 80)}),
                );
                return vec![];
            }
            Ok(res) if !res.status().is_success() => {
                self.store.log(
                    "REST_HINT_QUERY_FAILED",
                    json!({"address": address, "status": res.status().as_u16()}),
                );
                return vec![];
            }
            Ok(res) => match res.json::<Value>().await {
                Ok(v) => v,
                Err(e) => {
                    self.store.log(
                        "REST_HINT_QUERY_FAILED",
                        json!({"address": address, "message": slice_chars(&e.to_string(), 80)}),
                    );
                    return vec![];
                }
            },
        };
        let Some(list) = list.as_array() else {
            return vec![];
        };
        let tip_index = js_number(tip.get("index"));
        let mut out = vec![];
        for raw in list {
            let spends_tip = arr(raw.get("inputs")).iter().any(|i| {
                matches!((i.get("previous_outpoint_hash"), tip.get("transactionId")), (Some(Value::String(a)), Some(Value::String(b))) if a == b)
                    && js_number(i.get("previous_outpoint_index")).is_some_and(|n| Some(n) == tip_index)
            });
            if !spends_tip || !truthy(raw.get("transaction_id")) {
                continue;
            }
            let txid = js_string(raw.get("transaction_id"));
            let accepting = raw
                .get("accepting_block_hash")
                .filter(|a| truthy(Some(a)))
                .map(|a| js_string(Some(a)));
            let mut start = None;
            let mut accepting_acc = None;
            if let Some(a) = &accepting
                && let Ok(h) = self.header(a).await
                && h.is_chain
            {
                start = Some(h.selected_parent.clone().unwrap_or_else(|| a.clone()));
                accepting_acc = Some(h);
            }
            let tx = rest_tx_dto(raw, &txid);
            let containing = tx["verboseData"]["blockHash"]
                .as_str()
                .map(|s| s.to_string());
            out.push(Candidate {
                txid: txid.clone(),
                tx,
                containing,
                start,
                accepting: accepting_acc,
                source: "rest-hint".into(),
                recovered: None,
            });
            let mut d = json!({"round": cid, "cid": cid, "txid": txid});
            if let Some(a) = raw.get("accepting_block_hash") {
                d["accepting"] = a.clone();
            }
            self.store.log("REST_HINT_SPENDER_FOUND", d);
        }
        out
    }

    /// REST Genesis hint: returns a selected-chain start before the accepting block, or None. Never evidence.
    async fn genesis_from_rest(&self, txid: &str) -> Option<String> {
        let base = self.config.rest_url.as_deref()?;
        if txid.is_empty() {
            return None;
        }
        let url = format!("{}/transactions/{txid}", base.trim_end_matches('/'));
        let res = self
            .http
            .get(&url)
            .timeout(Duration::from_millis(3500))
            .send()
            .await
            .ok()?;
        if !res.status().is_success() {
            return None; // 404 = not (yet) indexed by REST
        }
        let data: Value = res.json().await.ok()?;
        if !truthy(data.get("is_accepted")) || !truthy(data.get("accepting_block_hash")) {
            return None;
        }
        let ab = js_string(data.get("accepting_block_hash"));
        let start = match self.header(&ab).await {
            Ok(h) if h.is_chain => Some(h.selected_parent.clone().unwrap_or_else(|| ab.clone())),
            _ => None,
        };
        if self
            .seen_rest_hints
            .lock()
            .unwrap()
            .insert(format!("{txid}:{ab}"))
        {
            self.store.log(
                "REST_HINT_GENESIS_FOUND",
                json!({"txid": txid, "accepting": ab}),
            );
        }
        start
    }

    pub async fn header(&self, hash: &str) -> Result<Acc> {
        let r = self
            .q(
                "getBlock",
                json!({"hash": hash, "includeTransactions": false}),
            )
            .await?;
        let Some(b) = nn(r.get("block")) else {
            bail!("Cannot read properties of undefined (reading 'header')")
        };
        let returned = nn(b.get("header").and_then(|h| h.get("hash")))
            .or_else(|| nn(b.get("verboseData").and_then(|v| v.get("hash"))));
        if let Some(x) = returned
            && js_string(Some(x)) != hash
        {
            bail!("HEADER_HASH_MISMATCH");
        }
        let Some(h) = nn(b.get("header")) else {
            bail!("Cannot read properties of undefined (reading 'daaScore')")
        };
        let vd = b.get("verboseData");
        Ok(Acc {
            hash: hash.to_string(),
            daa: js_bigint(h.get("daaScore"))?,
            blue_score: js_bigint(h.get("blueScore"))?,
            is_chain: vd.and_then(|v| v.get("isChainBlock")) == Some(&Value::Bool(true)),
            selected_parent: vd
                .and_then(|v| v.get("selectedParentHash"))
                .filter(|s| truthy(Some(s)))
                .map(|s| js_string(Some(s))),
        })
    }

    /// Selected-chain acceptance of txid, searched forward (v1, ids only) from hashes known to precede it. Bounded to the
    /// sink observed now: stop on the captured sink hash OR its blue score (the captured sink may be reorged off).
    async fn acceptance(&self, txid: &str, starts: &[String]) -> Result<Option<Acc>> {
        let (dag_info, sink_blue) =
            tokio::try_join!(self.q("getBlockDagInfo", json!({})), self.sink_blue())?;
        let sink = js_string(dag_info.get("sink"));
        let mut scanned = false;
        for start in starts {
            let res: Result<Option<Acc>> = async {
                let mut from = start.clone();
                let until = now_ms() + self.config.scan_budget_ms;
                loop {
                    if now_ms() > until {
                        bail!("VC_SCAN_BUDGET_EXCEEDED");
                    }
                    let r = self
                        .q(
                            "getVirtualChainFromBlock",
                            json!({"startHash": from, "includeAcceptedTransactionIds": true}),
                        )
                        .await?;
                    let added = strings(r.get("addedChainBlockHashes"));
                    let ids = arr(r.get("acceptedTransactionIds"));
                    if ids.len() != added.len() {
                        bail!("VC_ROWS_NOT_ALIGNED");
                    }
                    for row in ids {
                        if arr(row.get("acceptedTransactionIds"))
                            .iter()
                            .any(|x| x.as_str() == Some(txid))
                        {
                            let h = self
                                .header(&js_string(row.get("acceptingBlockHash")))
                                .await?;
                            if !h.is_chain {
                                bail!("ACCEPTING_NOT_CHAIN");
                            }
                            return Ok(Some(h));
                        }
                    }
                    if added.is_empty() || added.contains(&sink) || *start == sink {
                        break;
                    }
                    let last = added.last().unwrap().clone();
                    if self.header(&last).await?.blue_score >= sink_blue {
                        break;
                    }
                    from = last;
                }
                Ok(None)
            }
            .await;
            match res {
                Ok(Some(h)) => return Ok(Some(h)),
                Ok(None) => scanned = true, // scanned to sink from this start: not accepted after it
                Err(e) => {
                    let msg = e.to_string();
                    if msg.contains("cannot find header") {
                        let db = self.store.db();
                        let mut m = db.get_meta("discoveryScan")?;
                        if m.get("anchor").and_then(|a| a.as_str()) == Some(start.as_str()) {
                            m["anchor"] = Value::Null;
                            db.set_meta("discoveryScan", &m)?;
                        }
                    }
                    self.store
                        .log("VC_QUERY_FAILED", json!({"start": start, "message": msg}));
                }
            }
        }
        if scanned {
            return Ok(None);
        }
        bail!("ACCEPTANCE_UNKNOWN_ALL_STARTS_FAILED")
    }

    async fn live_utxo(
        &self,
        address: Option<&str>,
        outpoint: &Value,
    ) -> Result<Option<crate::normalize::Utxo>> {
        let r = self
            .q("getUtxosByAddresses", json!({"addresses": [address]}))
            .await?;
        let all = arr(r.get("entries"))
            .iter()
            .map(normalize_utxo)
            .collect::<Result<Vec<_>>>()?;
        let key = outpoint_key(outpoint);
        Ok(all.into_iter().find(|u| outpoint_key(&u.outpoint) == key))
    }

    /// The latest ROLLED_BACK transition that spent the current live tip (durable, survives a restart).
    fn rolled_back_spend(&self, rec: &RoundRow) -> Result<Option<TransitionRow>> {
        let tip = outpoint_key(&rec.data["tip"]);
        Ok(self
            .store
            .db()
            .rolled_back_transitions(&rec.id)?
            .into_iter()
            .find(|t| {
                truthy(t.previous.get("tip"))
                    && outpoint_key(&t.previous["tip"]) == tip
                    && truthy(t.next.get("latestTransaction"))
                    && t.next.get("latestTxid").and_then(|x| x.as_str()) == Some(t.txid.as_str())
            }))
    }

    /// First selected-chain block at or above `hash` by selected parents (bounded).
    async fn chain_ancestor(&self, hash: &str, limit: usize) -> Result<Option<Acc>> {
        let mut hash = Some(hash.to_string());
        for _ in 0..limit {
            let Some(h) = hash.filter(|h| !h.is_empty()) else {
                return Ok(None);
            };
            let hd = self.header(&h).await?;
            if hd.is_chain {
                return Ok(Some(hd));
            }
            hash = hd.selected_parent;
        }
        Ok(None)
    }

    /// Re-resolution after a rollback: check the known spender with the light v1 scan from its still-chain ancestor.
    async fn replay_rolled_back(
        self: &Arc<Self>,
        rec: &RoundRow,
        hint: &TransitionRow,
    ) -> Result<(Option<String>, Option<String>)> {
        let Some(start_acc) = self.chain_ancestor(&hint.accepting, 256).await? else {
            return Ok((None, None));
        };
        let start = start_acc.hash;
        let Some(acc) = self
            .acceptance(&hint.txid, std::slice::from_ref(&start))
            .await?
        else {
            return Ok((Some(start), None));
        };
        let tx = prepare_transaction(&hint.next["latestTransaction"], None);
        if txid_of(&tx).as_deref() != Some(hint.txid.as_str())
            || !is_full_transaction(&tx)
            || !spends(&tx, &rec.data["tip"])
        {
            bail!("ROLLED_BACK_SPENDER_INVALID");
        }
        self.store.log(
            "ROLLBACK_REPLAY",
            json!({"round": rec.id, "txid": hint.txid, "from": start, "accepting": acc.hash}),
        );
        let c = Candidate {
            txid: hint.txid.clone(),
            tx,
            containing: None,
            start: None,
            accepting: Some(acc.clone()),
            source: "rolled-back-replay".into(),
            recovered: None,
        };
        let chosen = self.bind_accepted(c, &acc).await?;
        let applied = self.apply_spend(rec, chosen, &acc).await?;
        Ok((Some(start), Some(applied)))
    }

    fn forget_pending(&self, key: &str) {
        self.pending.lock().unwrap().shift_remove(key);
    }

    pub async fn resolve_spend(
        self: &Arc<Self>,
        round_id: &str,
        event_at: Option<i64>,
    ) -> Result<String> {
        let event_at = event_at.unwrap_or_else(now_ms);
        let Some(rec) = self
            .store
            .db()
            .round(round_id)?
            .filter(|r| r.status == "LIVE")
        else {
            return Ok("NOT_LIVE".into());
        };
        let key = format!("spend:{round_id}");
        if self.pruned_backoff(&key) {
            return Ok("PENDING".into());
        }
        let tip = rec.data["tip"].clone();
        let tip_daa = rec
            .data
            .get("utxoDaa")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0);
        let cur_daa = self
            .store
            .db()
            .checkpoints_before(now_ms() + 1, 1)?
            .into_iter()
            .next()
            .and_then(|c| c.virtual_daa.parse::<u64>().ok())
            .unwrap_or(0);
        let is_historical = cur_daa > tip_daa + 1000;
        let mut replay_start = None;
        if let Some(hint) = self.rolled_back_spend(&rec)? {
            let (start, applied) = self.replay_rolled_back(&rec, &hint).await?;
            if let Some(a) = applied {
                self.forget_pending(&key);
                return Ok(a);
            }
            replay_start = start;
        }
        let tip2 = tip.clone();
        let m = move |t: &Value| spends_loose(t, &tip2);
        let fast_from = if is_historical {
            None
        } else {
            self.fast_start(event_at)?
        };
        if let Some(fast) = self.try_fast(&key, &m, fast_from.as_deref()).await {
            self.forget_pending(&key);
            let tx = with_block_hash(&fast.tx, json!(fast.containing));
            if !is_full_transaction(&tx) {
                bail!("CANDIDATE_MISSING_CONSENSUS_FIELDS");
            }
            let c = Candidate {
                txid: txid_of(&tx).unwrap_or_default(),
                tx,
                containing: Some(fast.containing.clone()),
                start: None,
                accepting: Some(fast.acc.clone()),
                source: "vc-full".into(),
                recovered: None,
            };
            return self.apply_spend(&rec, c, &fast.acc).await;
        }
        // A current tip cannot be spent before its creating transaction was accepted: start at that block's selected
        // parent (also covers same-block descendants), never at today's checkpoint. Below the pruning point that header
        // is gone (and the creation irreversible): the newest live-tip anchor bounds the search instead.
        let accepting = js_string(rec.data.get("accepting"));
        let created = match self.header(&accepting).await {
            Ok(h) => Some(h),
            Err(e) if pruned::is_pruned(&e) => None,
            Err(e) => return Err(e),
        };
        if let Some(created) = &created
            && !created.is_chain
        {
            let t = self.store.db().latest_active(round_id)?;
            let Some(t) = t.filter(|t| t.accepting == accepting) else {
                bail!("TIP_ACCEPTANCE_CONTEXT")
            };
            self.rollback(&t).await?;
            return Ok("ROLLED_BACK".into());
        }
        let mut starts: Vec<String> = vec![];
        for s in [
            replay_start,
            self.live_anchor(round_id, &tip)?,
            created
                .as_ref()
                .map(|c| c.selected_parent.clone().unwrap_or_else(|| c.hash.clone())),
        ]
        .into_iter()
        .flatten()
        {
            if !s.is_empty() && !starts.contains(&s) {
                starts.push(s);
            }
        }
        let tip_key = outpoint_key(&tip);
        {
            let db = self.store.db();
            if db.get_meta(&format!("window-tip:{round_id}"))? != json!(tip_key) {
                db.set_meta(&format!("window:{round_id}"), &Value::Null)?;
                db.set_meta(&format!("window-tip:{round_id}"), &json!(tip_key))?;
            }
        }
        let cid = rec
            .data
            .get("cid")
            .and_then(|c| c.as_str())
            .map(|s| s.to_string());
        let mut candidates = if is_historical {
            vec![]
        } else {
            self.from_mempool(rec.address.as_deref(), &tip, cid.as_deref())
                .await
        };
        if candidates.is_empty() && self.config.rest_url.is_some() {
            candidates = self
                .from_rest(rec.address.as_deref(), &tip, cid.as_deref())
                .await;
        }
        let mut window_continues = false;
        if candidates.is_empty() {
            // Bounded accepted-chain walk on the node when fast path, mempool and REST hints all missed.
            let resume = format!("window:{round_id}");
            for hint in &starts {
                // A start that left the selected chain is replaced by its selected-chain ancestor (a superset scan).
                let start = self.chain_ancestor(hint, 256).await.ok().flatten();
                let Some(start) = start else {
                    self.store.log(
                        "WINDOW_SCAN_FAILED",
                        json!({"start": hint, "message": "NO_CHAIN_ANCESTOR"}),
                    );
                    continue;
                };
                let w = match self
                    .window_find(&tip, &start.hash, Some(start.blue_score), Some(&resume))
                    .await
                {
                    Ok(w) => w,
                    Err(e) => {
                        let message = e.to_string();
                        if message == "WINDOW_SCAN_CONTINUES" {
                            window_continues = true;
                            break;
                        }
                        self.store.log(
                            "WINDOW_SCAN_FAILED",
                            json!({"start": start.hash, "message": message}),
                        );
                        continue;
                    }
                };
                let Some(w) = w else { continue };
                self.forget_pending(&key);
                self.store.db().set_meta(&resume, &Value::Null)?; // all starts' cursors
                let mut tx = with_block_hash(&w.tx, json!(w.containing));
                let mut source = "vc-window".to_string();
                if !is_full_transaction(&tx) {
                    tx = self.hydrate(&tx).await?;
                    with_input_verbose(&mut tx, &w.tx)?;
                    source.push_str("+block");
                }
                let c = Candidate {
                    txid: txid_of(&tx).unwrap_or_default(),
                    tx,
                    containing: Some(w.containing.clone()),
                    start: None,
                    accepting: Some(w.acc.clone()),
                    source,
                    recovered: None,
                };
                return self.apply_spend(&rec, c, &w.acc).await;
            }
        }
        if candidates.is_empty() {
            let observed = now_ms();
            if self
                .live_utxo(rec.address.as_deref(), &tip)
                .await?
                .is_some()
            {
                self.store.db().mark_live_seen(round_id, now_ms())?;
                self.mark_live_anchor(round_id, &tip, observed)?;
                self.forget_pending(&key);
                return Ok("STILL_LIVE".into());
            }
            let reason = if window_continues {
                "WINDOW_SCAN_CONTINUES"
            } else if starts.is_empty() {
                "EVIDENCE_PRUNED"
            } else {
                "SPENDER_NOT_FOUND"
            };
            return Ok(self.defer_spend(round_id, reason, vec![]));
        }
        let mut chosen = None;
        // Candidates the node can no longer place: no start survives pruning, or every start's header is pruned.
        let mut unplaced = vec![];
        for c in &candidates {
            if let Some(acc) = &c.accepting {
                chosen = Some((c.clone(), acc.clone()));
                break;
            }
            let from = match &c.start {
                Some(s) if !s.is_empty() => vec![s.clone()],
                _ => starts.clone(),
            };
            if from.is_empty() {
                unplaced.push(c.txid.clone());
                continue;
            }
            match self.acceptance(&c.txid, &from).await {
                Ok(Some(acc)) => {
                    chosen = Some((c.clone(), acc));
                    break;
                }
                Ok(None) => {}
                Err(_) if created.is_none() => unplaced.push(c.txid.clone()),
                Err(e) => return Err(e),
            }
        }
        let Some((c, acc)) = chosen else {
            if !unplaced.is_empty() {
                return self.resolve_pruned(&rec, unplaced).await;
            }
            return Ok(self.defer_spend(
                round_id,
                "NOT_YET_ACCEPTED",
                candidates.iter().map(|c| c.txid.clone()).collect(),
            ));
        };
        self.forget_pending(&key);
        if !is_full_transaction(&c.tx) {
            bail!("CANDIDATE_MISSING_CONSENSUS_FIELDS");
        }
        let c = self.bind_accepted(c, &acc).await?;
        self.apply_spend(&rec, c, &acc).await
    }

    async fn apply_spend(
        self: &Arc<Self>,
        rec: &RoundRow,
        chosen: Candidate,
        acc: &Acc,
    ) -> Result<String> {
        self.apply_spend_mode(rec, chosen, acc, false).await
    }

    async fn apply_spend_mode(
        self: &Arc<Self>,
        rec: &RoundRow,
        chosen: Candidate,
        acc: &Acc,
        skip_live_probe: bool,
    ) -> Result<String> {
        self.apply_spend_mode_row(rec, chosen, acc, skip_live_probe)
            .await
            .map(|r| r.0)
    }

    async fn apply_spend_mode_row(
        self: &Arc<Self>,
        rec: &RoundRow,
        mut chosen: Candidate,
        acc: &Acc,
        skip_live_probe: bool,
    ) -> Result<(String, Option<RoundRow>)> {
        let plugin = self
            .contracts()
            .get(rec.contract.as_deref().unwrap_or("null"))?; // the rules this round was created under
        // V2 'High' rows carry version/lockTime as null: normalize at the boundary for every path.
        chosen.tx = normalize_consensus_fields(&chosen.tx);
        let b = BlockRef {
            hash: acc.hash.clone(),
            daa: acc.daa.to_string(),
        };
        let accessor = if plugin.is_rolling()
            && let Some(target_hash) = crate::contracts::rolling_v1::draw_target_hash(&chosen.tx)
        {
            let r = self
                .q(
                    "getBlock",
                    json!({"hash": target_hash, "includeTransactions": false}),
                )
                .await?;
            let block = nn(r.get("block"));
            let is_chain = block
                .and_then(|blk| blk.get("verboseData"))
                .and_then(|v| v.get("isChainBlock"))
                == Some(&Value::Bool(true));
            let seq = block
                .and_then(|blk| blk.get("header"))
                .and_then(|h| h.get("acceptedIdMerkleRoot"))
                .and_then(|s| s.as_str())
                .map(str::to_owned);
            if is_chain && let Some(sequence_commitment) = seq {
                Some(crate::core::rolling::Accessor {
                    block_hash: target_hash,
                    sequence_commitment,
                })
            } else {
                None
            }
        } else {
            None
        };
        let next = match plugin
            .advance_with_accessor(&rec.data, &chosen.tx, &b, accessor.as_ref())
            .and_then(|r| check_round(r, &plugin.key))
        {
            Ok(n) => n,
            Err(e) => {
                let r = self.mark_stale(rec, &chosen, acc, &e.to_string())?;
                self.unsubscribe(rec.address.as_deref()).await?;
                return Ok((r, None));
            }
        };
        if next.get("id").and_then(|i| i.as_str()) != Some(rec.id.as_str()) {
            let r = self.mark_stale(rec, &chosen, acc, "PLUGIN_CHANGED_ROUND_ID")?;
            self.unsubscribe(rec.address.as_deref()).await?;
            return Ok((r, None));
        }
        // Consumers must still see which block actually included the transaction when known.
        let next = spread(&next, json!({ "containing": chosen.containing }));
        let kind = {
            let k = slice_chars(&plugin.kind_of(&chosen.tx), 32);
            if k.is_empty() {
                "UNKNOWN".to_string()
            } else {
                k
            }
        };
        self.commit(
            Some(rec),
            next,
            chosen,
            acc,
            &kind,
            &plugin.key,
            skip_live_probe,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn commit(
        self: &Arc<Self>,
        rec: Option<&RoundRow>,
        next: Value,
        chosen: Candidate,
        acc: &Acc,
        kind: &str,
        contract: &str,
        skip_live_probe: bool,
    ) -> Result<(String, Option<RoundRow>)> {
        let next_id = next["id"].as_str().unwrap_or_default().to_string();
        let has_tip = truthy(next.get("tip"));
        let next_address = if has_tip {
            Some(self.addr_of(&next["spk"])?)
        } else {
            None
        };
        let cur_daa = self
            .store
            .db()
            .checkpoints_before(now_ms() + 1, 1)?
            .into_iter()
            .next()
            .and_then(|c| c.virtual_daa.parse::<u64>().ok())
            .unwrap_or(0);
        let is_historical = skip_live_probe || cur_daa as i128 > acc.daa + 100;
        if next_address.is_some() && !is_historical {
            self.subscribe(next_address.as_deref()).await?; // subscribe first: no notification gap
        }
        let utxo_check;
        if skip_live_probe {
            utxo_check = if has_tip {
                "SKIPPED_BATCH_HISTORICAL"
            } else {
                "TERMINAL_BATCH_HISTORICAL"
            };
        } else if has_tip {
            if let Some(u) = self
                .live_utxo(next_address.as_deref(), &next["tip"])
                .await?
            {
                if is_historical && next_address.is_some() {
                    self.subscribe(next_address.as_deref()).await?;
                }
                // GHOSTDAG: containing DAA <= accepting DAA (parallel blocks are merged later); never strict equality.
                let daa_ok = u.block_daa_score <= acc.daa && acc.daa - u.block_daa_score <= 2000;
                let cid_ok = match (&u.covenant_id, next.get("cid")) {
                    (Some(a), Some(Value::String(b))) => a == b,
                    (None, Some(Value::Null)) => true,
                    _ => false,
                };
                let ok = u.amount == js_bigint(next.get("value"))?
                    && same_spk(&u.spk, &wire_spk(&next["spk"])?)
                    && cid_ok
                    && daa_ok;
                if !ok {
                    if !is_historical {
                        self.unsubscribe(next_address.as_deref()).await?;
                    }
                    bail!("SUCCESSOR_UTXO_MISMATCH");
                }
                utxo_check = "SUCCESSOR_LIVE_MATCHES";
            } else {
                utxo_check = "SUCCESSOR_NOT_LIVE_(ALREADY_SPENT_OR_INDEX_LAG)";
            }
        } else {
            if let Some(r) = rec
                && self
                    .live_utxo(r.address.as_deref(), &r.data["tip"])
                    .await?
                    .is_some()
            {
                bail!("TERMINAL_BUT_TIP_STILL_LIVE");
            }
            utxo_check = "TERMINAL_OLD_TIP_GONE";
        }
        let now = now_ms();
        let witness = if chosen.source == "mempool" {
            "MEMPOOL_COPY_TXID_MATCH"
        } else if chosen.recovered.is_some() {
            "REST_COPY_TXID_MATCH"
        } else {
            "BLOCK_BODY"
        };
        let mut evidence =
            json!({"source": chosen.source, "utxoCheck": utxo_check, "witnessSource": witness});
        if let Some(r) = &chosen.recovered {
            evidence["prunedRecovery"] = r.clone();
        }
        // Accepted below the pruning point: consensus-irreversible, and no accepting header is left to recheck.
        let status = if chosen.recovered.is_some() { "FINAL" } else { "ACCEPTED" };
        let mut next = next;
        let delta_receipt = next
            .as_object_mut()
            .and_then(|m| m.shift_remove("_deltaReceipt"));
        let delta_preimages: Vec<(String, String)> = next
            .as_object_mut()
            .and_then(|m| m.shift_remove("_deltaPreimages"))
            .and_then(|v| v.as_array().cloned())
            .map(|arr| {
                arr.into_iter()
                    .filter_map(|item| {
                        let a = item.as_array()?;
                        Some((a.first()?.as_str()?.to_string(), a.get(1)?.as_str()?.to_string()))
                    })
                    .collect()
            })
            .unwrap_or_default();
        let previous = rec.map(|r| r.data.clone()).unwrap_or(Value::Null);
        let (daa_s, blue_s) = (acc.daa.to_string(), acc.blue_score.to_string());
        // Idempotent: a txid already applied to this round is not re-applied.
        let outcome: std::result::Result<i64, i64> = self.store.tx(|db| {
            if let Some(d) = db.active_transition(&next_id, &chosen.txid)? {
                return Ok(Err(d.seq));
            }
            let seq = db.last_seq(&next_id)? + 1; // allocated inside the write transaction
            if next.get("history").is_some() {
                let mut pre = db.round_preimages(&next_id)?;
                for (h, rh) in &delta_preimages {
                    pre.insert(h.clone(), Value::String(rh.clone()));
                }
                crate::contracts::rolling_v1::fill_terminal_history_preimages(&mut next, &pre);
            }
            db.put_round(&next, next_address.as_deref(), now, contract)?;
            if let Some(ref dr) = delta_receipt {
                db.add_round_receipt(&next_id, seq, dr)?;
            }
            if !delta_preimages.is_empty() {
                db.add_round_preimages(&next_id, seq, &delta_preimages)?;
            }
            if utxo_check == "SUCCESSOR_LIVE_MATCHES" {
                db.mark_live_seen(&next_id, now)?;
            }
            db.add_transition(&NewTransition {
                txid: &chosen.txid,
                round_id: &next_id,
                contract,
                seq,
                kind,
                status,
                accepting: &acc.hash,
                accepting_daa: &daa_s,
                accepting_blue_score: &blue_s,
                containing: chosen.containing.as_deref(),
                previous: Some(&previous),
                next: &next,
                evidence: &evidence,
                observed_at: now,
            })?;
            if kind == "GENESIS" {
                db.discovery_result(&next_id, "APPLIED", now, None)?;
            }
            if self.config.relay_enabled {
                let row = TransitionRow {
                    txid: chosen.txid.clone(),
                    round_id: next_id.clone(),
                    contract: Some(contract.to_string()),
                    seq,
                    kind: kind.to_string(),
                    status: status.into(),
                    accepting: acc.hash.clone(),
                    accepting_daa: daa_s.clone(),
                    accepting_blue_score: blue_s.clone(),
                    containing: chosen.containing.clone(),
                    previous: previous.clone(),
                    next: next.clone(),
                    evidence: evidence.clone(),
                    observed_at: now,
                };
                db.enqueue(
                    &self.relay_payload("transition", &row, next_address.as_deref()),
                    now,
                )?;
            }
            Ok(Ok(seq))
        })?;
        let seq = match outcome {
            Err(existing) => {
                if next_address.is_some() {
                    self.unsubscribe(next_address.as_deref()).await?; // balance the subscribe above
                }
                self.store.log_at(
                    "DUPLICATE_SKIPPED",
                    json!({"round": next_id, "txid": chosen.txid, "existingSeq": existing}),
                    now,
                );
                return Ok(("KNOWN".into(), None));
            }
            Ok(seq) => seq,
        };
        if let Some(r) = rec
            && r.address.is_some()
            && r.status == "LIVE"
        {
            self.unsubscribe(r.address.as_deref()).await?;
        }
        let mut d = Map::new();
        d.insert("round".into(), json!(next_id));
        d.insert("roundId".into(), json!(next_id));
        if let Some(c) = next.get("cid") {
            d.insert("cid".into(), c.clone());
        }
        for (k, v) in [
            ("seq", json!(seq)),
            ("kind", json!(kind)),
            ("txid", json!(chosen.txid)),
            ("accepting", json!(acc.hash)),
            ("utxoCheck", json!(utxo_check)),
        ] {
            d.insert(k.into(), v);
        }
        self.store.log_at("APPLIED", Value::Object(d), now);
        if has_tip
            && utxo_check != "SUCCESSOR_LIVE_MATCHES"
            && self.config.auto_catchup
            && !chosen.source.starts_with("vc-batch")
        {
            self.run_spend(&next_id, None);
        }
        let updated = if has_tip {
            Some(RoundRow {
                id: next_id,
                contract: Some(contract.to_string()),
                data: next,
                address: next_address,
                status: "LIVE".into(),
                live_seen_at: None,
                updated_at: now,
            })
        } else {
            None
        };
        Ok(("APPLIED".into(), updated))
    }

    fn mark_stale(
        &self,
        rec: &RoundRow,
        chosen: &Candidate,
        acc: &Acc,
        message: &str,
    ) -> Result<String> {
        let now = now_ms();
        self.store.tx(|db| {
            db.set_round_status(&rec.id, "STALE", now)?;
            if self.config.relay_enabled {
                db.enqueue(
                    &json!({"schema": "kaswin-event-indexer/1", "type": "stale", "network": self.net.key, "roundId": rec.id,
                        "contract": rec.contract, "txid": chosen.txid, "accepting": acc.hash, "error": message,
                        "note": "Accepted spend did not match the pinned contract plugin rules; state NOT advanced"}),
                    now,
                )?;
            }
            db.log("STALE", &json!({"round": rec.id, "txid": chosen.txid, "message": message}), now)
        })?;
        Ok("STALE".into())
    }

    fn defer_spend(&self, id: &str, reason: &str, candidates: Vec<String>) -> String {
        // A missing spender does not expire with wall time: the tip and the durable window cursor survive a restart and
        // reconcile requeues it. Never drop uncertain work by TTL.
        let mut p = self.pending.lock().unwrap();
        let e = p
            .entry(format!("spend:{id}"))
            .or_insert_with(|| PendingSpend {
                id: id.to_string(),
                since: now_ms(),
                at: 0,
                reason: String::new(),
                candidates: vec![],
            });
        e.at = now_ms();
        e.reason = reason.to_string();
        e.candidates = candidates;
        "PENDING".into()
    }
    fn defer_genesis(&self, txid: &str, reason: &str) -> Result<String> {
        self.store
            .db()
            .discovery_result(txid, "PENDING", now_ms(), Some(reason))?;
        Ok("PENDING".into())
    }

    pub fn retry_pending(self: &Arc<Self>) -> Result<()> {
        let list = self.store.db().pending_discoveries()?;
        for (id, _) in list {
            self.run_genesis(&id, json!({}), false);
        }
        let pend: Vec<PendingSpend> = self.pending.lock().unwrap().values().cloned().collect();
        for p in pend {
            self.run_spend(&p.id, Some(p.since));
        }
        Ok(())
    }

    // ---------- genesis discovery (registry marker event or manual track) ----------
    pub async fn discover_genesis(
        self: &Arc<Self>,
        txid: &str,
        hint: &Value,
        force: bool,
    ) -> Result<String> {
        let task = self.store.db().discover_task(txid, hint, now_ms())?;
        let mut attempts = task.attempts;
        if force {
            attempts = 0;
            self.store.db().force_discovery_pending(txid, false)?;
        }
        if !force && attempts >= 40 {
            let err = task
                .error
                .clone()
                .unwrap_or_else(|| "EXCEEDED_MAX_ATTEMPTS".into());
            self.store
                .db()
                .discovery_result(txid, "IGNORED", now_ms(), Some(&err))?;
            return Ok("IGNORED".into());
        }
        match self.discover_genesis_attempt(txid, &task.hint).await {
            Ok(result) => {
                let cur = self.store.db().discovery(txid)?.map(|d| enc(&d.hint));
                let newer_hint = cur != Some(enc(&task.hint));
                let status = if result == "KNOWN" || result == "APPLIED" {
                    "APPLIED"
                } else if result == "IGNORED" && !newer_hint {
                    "IGNORED"
                } else {
                    "PENDING"
                };
                self.store
                    .db()
                    .discovery_result(txid, status, now_ms(), None)?;
                Ok(result)
            }
            Err(e) => {
                let next = if attempts >= 14 { "IGNORED" } else { "PENDING" };
                self.store
                    .db()
                    .discovery_result(txid, next, now_ms(), Some(&e.to_string()))?;
                Err(e)
            }
        }
    }

    fn log_daa_difference(
        &self,
        txid: &str,
        daa: &Option<Value>,
        found: &Hit,
        path: Option<&str>,
    ) -> Result<()> {
        if let Some(d) = daa {
            let n = js_bigint(Some(d))?;
            if n > found.acc.daa || found.acc.daa - n > 200 {
                let mut x = json!({"txid": txid, "notificationDaa": js_string(Some(d)), "acceptingDaa": found.acc.daa.to_string(), "accepting": found.acc.hash});
                if let Some(p) = path {
                    x["path"] = json!(p);
                }
                self.store.log("REGISTRY_EVENT_DAA_DIFFERENCE", x);
            }
        }
        Ok(())
    }

    async fn discover_genesis_attempt(
        self: &Arc<Self>,
        txid: &str,
        hint: &Value,
    ) -> Result<String> {
        let event_at = match hint.get("eventAt") {
            None => now_ms(),
            Some(v) => js_int(Some(v)).unwrap_or(0),
        };
        let daa = nn(hint.get("daa")).cloned();
        let mut starts: Option<Vec<String>> = if truthy(hint.get("starts")) {
            Some(
                arr(hint.get("starts"))
                    .iter()
                    .map(|s| js_string(Some(s)))
                    .collect(),
            )
        } else {
            None
        };
        let contract = hint
            .get("contract")
            .and_then(|c| c.as_str())
            .map(|s| s.to_string());
        if let Some(existing) = self.store.db().round(txid)?
            && existing.status != "ROLLED_BACK"
        {
            return Ok("KNOWN".into());
        }
        // Fast path only for event-driven discovery (no explicit starts): the V2 Full row carries the full accepted tx.
        let fast_from = if starts.is_some() {
            None
        } else {
            self.fast_start(event_at)?
        };
        let want = txid.to_string();
        let m = move |t: &Value| {
            t.get("verboseData")
                .and_then(|v| v.get("transactionId"))
                .and_then(|x| x.as_str())
                == Some(want.as_str())
        };
        if let Some(found) = self
            .try_fast(&format!("genesis:{txid}"), &m, fast_from.as_deref())
            .await
        {
            let found: Hit = found.into();
            self.log_daa_difference(txid, &daa, &found, Some("fast"))?;
            if is_full_transaction(&found.tx) {
                self.forget_pending(&format!("genesis:{txid}"));
                let full = found.tx.clone();
                return self
                    .apply_genesis(txid, found, contract.as_deref(), Some(full), "vc-full")
                    .await;
            } // never guess consensus fields: the slow path hydrates
        }
        let mut from: Vec<String> = match &starts {
            Some(s) => s.clone(),
            None => self
                .store
                .db()
                .checkpoints_before(
                    event_at - self.config.genesis_lookback_ms,
                    self.config.checkpoints_per_search,
                )?
                .into_iter()
                .map(|c| c.sink)
                .collect(),
        };
        if from.is_empty() {
            let anchor = self
                .store
                .db()
                .get_meta("discoveryScan")?
                .get("anchor")
                .cloned();
            if truthy(anchor.as_ref()) {
                from.push(js_string(anchor.as_ref()));
                if starts.is_some() {
                    starts = Some(from.clone()); // the JS array is shared
                }
            }
        }
        // 1. Local DAG acceptance from recent checkpoints (purely on-node, avoids REST lag).
        let mut acc = if from.is_empty() {
            None
        } else {
            self.acceptance(txid, &from).await.ok().flatten()
        };
        // 2. Not found there and REST configured: ask REST for an older anchor hint.
        if acc.is_none()
            && self.config.rest_url.is_some()
            && starts.is_none()
            && let Some(start) = self.genesis_from_rest(txid).await.filter(|s| !s.is_empty())
            && !from.contains(&start)
        {
            from.insert(0, start.clone());
            acc = self
                .acceptance(txid, std::slice::from_ref(&start))
                .await
                .ok()
                .flatten();
        }
        if from.is_empty() && acc.is_none() {
            return self.defer_genesis(txid, "NO_CHECKPOINT");
        }
        let found = match acc {
            Some(a) => self.accepted_high(txid, &a).await?,
            None => None,
        };
        let Some(found) = found else {
            return self.defer_genesis(txid, "GENESIS_NOT_FOUND");
        };
        self.forget_pending(&format!("genesis:{txid}"));
        self.log_daa_difference(txid, &daa, &found, None)?;
        self.apply_genesis(txid, found, contract.as_deref(), None, "vc-high+block")
            .await
    }

    async fn apply_genesis(
        self: &Arc<Self>,
        txid: &str,
        found: Hit,
        contract: Option<&str>,
        full_tx: Option<Value>,
        source: &str,
    ) -> Result<String> {
        let all = self.contracts().for_genesis(contract);
        let mut rejected = vec![];
        let plugins: Vec<_> = all
            .iter()
            .filter(|p| {
                let (ok, reason) = p.inspect_genesis(&found.tx);
                if !ok {
                    rejected.push(json!({"contract": p.key, "reason": reason}));
                }
                ok
            })
            .cloned()
            .collect();
        if plugins.is_empty() {
            self.store.log("GENESIS_IGNORED", json!({"txid": txid, "reason": "no pinned creation template matched", "rejected": rejected}));
            return Ok("IGNORED".into());
        }
        let tx = match full_tx {
            Some(t) => t,
            None => self.hydrate(&found.tx).await?,
        };
        // Exactly one active plugin may claim a Genesis. Ambiguity is a configuration error, not a choice to make here.
        let b = BlockRef {
            hash: found.acc.hash.clone(),
            daa: found.acc.daa.to_string(),
        };
        let mut claims = vec![];
        for p in &plugins {
            if let Some(r) = p.genesis(&tx, &b)? {
                claims.push((p.clone(), check_round(r, &p.key)?));
            }
        }
        if claims.is_empty() {
            let tried: Vec<String> = self
                .contracts()
                .for_genesis(contract)
                .iter()
                .map(|p| p.key.clone())
                .collect();
            self.store.log("GENESIS_IGNORED", json!({"txid": txid, "tried": tried, "reason": "no loaded contract plugin authenticated this Genesis"}));
            return Ok("IGNORED".into());
        }
        if claims.len() > 1 {
            bail!(
                "GENESIS_CLAIMED_BY_MULTIPLE_CONTRACTS {}",
                claims
                    .iter()
                    .map(|(p, _)| p.key.clone())
                    .collect::<Vec<_>>()
                    .join(",")
            );
        }
        let (plugin, round) = claims.remove(0);
        if round.get("id").and_then(|i| i.as_str()) != Some(txid) {
            bail!("PLUGIN_GENESIS_ID_MISMATCH");
        }
        let c = Candidate {
            txid: txid.to_string(),
            tx,
            containing: found.containing.clone(),
            start: None,
            accepting: Some(found.acc.clone()),
            source: source.to_string(),
            recovered: None,
        };
        self.commit(None, round, c, &found.acc, "GENESIS", &plugin.key, false)
            .await
            .map(|r| r.0)
    }

    async fn accepted_high(&self, txid: &str, acc: &Acc) -> Result<Option<Hit>> {
        self.accepted_row(txid, acc, "High").await
    }

    /// V2 row (`level` High, or Full when input UTXO entries must carry their DAA) for exactly one accepting chain block:
    /// start at its selected parent and strip the head with minConfirmationCount (2 blocks of margin; retry with 0 if
    /// the boundary still pruned the target).
    async fn accepted_row(&self, txid: &str, acc: &Acc, level: &str) -> Result<Option<Hit>> {
        let Some(sp) = acc.selected_parent.clone() else {
            bail!("SELECTED_PARENT_MISSING")
        };
        let distance = self.sink_blue().await? - acc.blue_score;
        let min_conf: i128 = if distance > 2 { distance - 2 } else { 0 };
        let mut r = self.q("getVirtualChainFromBlockV2", json!({"startHash": sp, "dataVerbosityLevel": level, "minConfirmationCount": min_conf as u64})).await?;
        let mut added = strings(r.get("addedChainBlockHashes"));
        if arr(r.get("chainBlockAcceptedTransactions")).len() != added.len() {
            bail!("VC_ROWS_NOT_ALIGNED");
        }
        let mut i = added.iter().position(|h| *h == acc.hash);
        if i.is_none() && min_conf > 0 {
            let distance2 = self.sink_blue().await? - acc.blue_score;
            let retry_conf: i128 = if distance2 > 32 { distance2 - 32 } else { 0 };
            r = self.q("getVirtualChainFromBlockV2", json!({"startHash": sp, "dataVerbosityLevel": level, "minConfirmationCount": retry_conf as u64})).await?;
            added = strings(r.get("addedChainBlockHashes"));
            if arr(r.get("chainBlockAcceptedTransactions")).len() != added.len() {
                bail!("VC_ROWS_NOT_ALIGNED");
            }
            i = added.iter().position(|h| *h == acc.hash);
        }
        let Some(i) = i else { return Ok(None) };
        let rows = arr(r.get("chainBlockAcceptedTransactions"));
        let row = &rows[i];
        if !arr(r.get("removedChainBlockHashes")).is_empty()
            || row["chainBlockHeader"]["hash"].as_str() != Some(acc.hash.as_str())
            || js_bigint(row["chainBlockHeader"].get("daaScore"))? != acc.daa
            || js_bigint(row["chainBlockHeader"].get("blueScore"))? != acc.blue_score
        {
            bail!("ACCEPTED_HEADER_CHANGED");
        }
        let t = arr(rows[i].get("acceptedTransactions")).iter().find(|x| {
            x.get("verboseData")
                .and_then(|v| v.get("transactionId"))
                .and_then(|v| v.as_str())
                == Some(txid)
        });
        let Some(t) = t else {
            bail!("ACCEPTED_ID_WITHOUT_HIGH_ROW")
        };
        let containing = t["verboseData"]
            .get("blockHash")
            .and_then(|b| b.as_str())
            .map(|s| s.to_string());
        Ok(Some(Hit {
            acc: acc.clone(),
            tx: canonical(t),
            containing,
        }))
    }

    /// signatureScript is excluded from the v1 txid, so a copy with the same txid may carry a different witness than
    /// the accepted instance. Bind the candidate to the accepted instance (V2 High row) before parsing.
    async fn bind_accepted(&self, candidate: Candidate, acc: &Acc) -> Result<Candidate> {
        let Some(high) = self.accepted_high(&candidate.txid, acc).await? else {
            bail!("ACCEPTED_ROW_NOT_FOUND")
        };
        let mut same = true;
        for k in ["inputs", "outputs", "payload"] {
            let eq = if k == "inputs" {
                ser_stripped(high.tx.get(k))? == ser_stripped(candidate.tx.get(k))?
            } else {
                ser(high.tx.get(k)) == ser(candidate.tx.get(k))
            };
            if !eq {
                same = false;
                break;
            }
        }
        let mut tx = candidate.tx.clone();
        let mut source = candidate.source.clone();
        // REST hints lack real lock_time and consensus covenant objects: ALWAYS hydrate them from the containing block.
        let must_hydrate = source == "rest-hint" || !same;
        if must_hydrate {
            // Fail closed: a different witness or REST hint must never survive failed accepted-instance hydration.
            tx = self.hydrate(&high.tx).await?;
            source.push_str("+accepted-witness");
        }
        let containing = high
            .containing
            .clone()
            .map(Value::String)
            .unwrap_or(Value::Null);
        let mut tx = with_block_hash(&tx, containing);
        with_input_verbose(&mut tx, &high.tx)?;
        Ok(Candidate {
            tx,
            containing: high.containing,
            source,
            ..candidate
        })
    }

    async fn hydrate(&self, high: &Value) -> Result<Value> {
        let hash = high
            .get("verboseData")
            .and_then(|vd| vd.get("blockHash"))
            .cloned()
            .unwrap_or(Value::Null);
        let r = self
            .q("getBlock", json!({"hash": hash, "includeTransactions": true}))
            .await?;
        Self::hydrate_from_block(high, &r)
    }

    /// Add version/lockTime from a fetched block body after exact comparison of all shared consensus fields.
    fn hydrate_from_block(high: &Value, r: &Value) -> Result<Value> {
        let vd = high.get("verboseData").cloned().unwrap_or(Value::Null);
        let want = vd
            .get("transactionId")
            .and_then(|t| t.as_str())
            .map(|s| s.to_string());
        let raw = arr(r.get("block").and_then(|b| b.get("transactions")))
            .iter()
            .find(|t| txid_of(t).is_some() && txid_of(t) == want);
        let Some(raw) = raw else {
            bail!("CONTAINING_TX_MISSING")
        };
        let full = canonical(raw);
        for (k, hv) in high.as_object().cloned().unwrap_or_default() {
            if k == "verboseData" {
                continue;
            }
            // V2 High rows carry version/lockTime as null: exactly what this hydration fetches.
            if (k == "version" || k == "lockTime") && hv.is_null() {
                continue;
            }
            let eq = if k == "inputs" {
                ser_stripped(Some(&hv))? == ser_stripped(full.get(&k))?
            } else {
                ser(Some(&hv)) == ser(full.get(&k))
            };
            if !eq {
                bail!("HIGH_FULL_MISMATCH_{k}");
            }
        }
        if full.get("version").is_none() || full.get("lockTime").is_none() {
            bail!("FULL_TRANSACTION_FIELDS");
        }
        let mut merged = spread(&full, json!({ "verboseData": vd }));
        with_input_verbose(&mut merged, high)?;
        Ok(merged)
    }

    // ---------- acceptance maintenance ----------
    fn finalize(&self, t: &TransitionRow) -> Result<()> {
        let now = now_ms();
        let mut f = t.clone();
        f.status = "FINAL".into();
        self.store.tx(|db| {
            db.set_transition_status(&t.round_id, t.seq, "FINAL")?;
            if self.config.relay_enabled {
                db.enqueue(&self.relay_payload("final", &f, None), now)?;
            }
            Ok(())
        })
    }

    pub async fn check_finality(self: &Arc<Self>) -> Result<()> {
        let open = self.store.db().unfinalized_heads()?;
        if open.is_empty() {
            return Ok(());
        }
        let sink = self.sink_blue().await?;
        let mut rolled: HashSet<String> = HashSet::new();
        let mut chain_set: HashSet<String> = HashSet::new();
        let mut header_cache: HashMap<String, Acc> = HashMap::new();
        let mut finalized = 0usize;
        for t in open {
            if rolled.contains(&t.round_id) {
                continue;
            }
            let is_chain = if chain_set.contains(&t.accepting) {
                true
            } else {
                let h = if let Some(cached) = header_cache.get(&t.accepting) {
                    cached.clone()
                } else {
                    match self.header(&t.accepting).await {
                        Ok(h) => {
                            header_cache.insert(t.accepting.clone(), h.clone());
                            h
                        }
                        Err(e) => {
                            // Missing/pruned headers are absence of evidence, never a finality proof.
                            self.store.log(
                                "FINALITY_UNVERIFIED",
                                json!({"round":t.round_id,"accepting":t.accepting,"message":e.to_string()}),
                            );
                            continue;
                        }
                    }
                };
                if h.is_chain && chain_set.is_empty()
                    && let Ok(vc) = self
                        .q(
                            "getVirtualChainFromBlock",
                            json!({"startHash": h.hash, "includeAcceptedTransactionIds": false}),
                        )
                        .await
                {
                    chain_set.insert(h.hash.clone());
                    for ah in strings(vc.get("addedChainBlockHashes")) {
                        chain_set.insert(ah);
                    }
                }
                h.is_chain
            };
            if !is_chain {
                let full = self.store.db().transition(&t.round_id, t.seq)?;
                if let Some(full) = full {
                    self.rollback(&full).await?;
                }
                rolled.insert(t.round_id.clone());
                continue;
            }
            if sink - js_bigint(Some(&json!(t.accepting_blue_score)))?
                >= self.config.finality_blue_score
            {
                if self.config.relay_enabled {
                    let full = self.store.db().transition(&t.round_id, t.seq)?;
                    if let Some(full) = full {
                        self.finalize(&full)?;
                        finalized += 1;
                    }
                } else {
                    self.store
                        .tx(|db| db.set_transition_status(&t.round_id, t.seq, "FINAL"))?;
                    finalized += 1;
                }
            }
        }
        if finalized > 0 {
            let _ = self.store.db().incremental_vacuum(0);
        }
        self.trim_heap();
        Ok(())
    }

    fn trim_heap(&self) {
        self.store.db().shrink_memory();
        #[cfg(target_env = "gnu")]
        {
            unsafe extern "C" {
                fn malloc_trim(pad: usize) -> i32;
            }
            unsafe {
                malloc_trim(0);
            }
        }
    }

    /// Accepting block left the selected chain: undo this and later transitions, then re-resolve from the chain.
    pub async fn rollback(self: &Arc<Self>, t: &TransitionRow) -> Result<()> {
        let undo = self.store.db().active_after(&t.round_id, t.seq)?;
        let now = now_ms();
        if undo.is_empty() {
            return Ok(());
        }
        let current = self.store.db().round(&t.round_id)?;
        let prev = &t.previous;
        let prev_address = if truthy(prev.get("tip")) {
            Some(self.addr_of(&prev["spk"])?)
        } else {
            None
        };
        let contract = t
            .contract
            .clone()
            .or_else(|| current.as_ref().and_then(|c| c.contract.clone()))
            .unwrap_or_default();
        self.store.tx(|db| {
            for u in &undo {
                db.set_transition_status(&u.round_id, u.seq, "ROLLED_BACK")?;
                let mut r = u.clone();
                r.status = "ROLLED_BACK".into();
                if self.config.relay_enabled {
                    db.enqueue(&self.relay_payload("rollback", &r, None), now)?;
                }
            }
            if truthy(Some(prev)) {
                db.put_round(prev, prev_address.as_deref(), now, &contract)?;
            } else {
                db.mark_round_rolled_back(&t.round_id, now)?;
                db.discovery_result(&t.round_id, "PENDING", now, Some("GENESIS_ROLLED_BACK"))?;
            }
            db.rollback_round_extras(&t.round_id, t.seq)?;
            let txids: Vec<&str> = undo.iter().map(|u| u.txid.as_str()).collect();
            db.log(
                "ROLLBACK",
                &json!({"round": t.round_id, "fromSeq": t.seq, "txids": txids}),
                now,
            )
        })?;
        if let Some(c) = &current
            && c.status == "LIVE"
            && c.address.is_some()
        {
            self.unsubscribe(c.address.as_deref()).await?;
        }
        if prev_address.is_some() {
            self.subscribe(prev_address.as_deref()).await?;
            self.run_spend(&t.round_id, None);
        } else {
            self.run_genesis_after_rollback(&t.round_id, t.observed_at);
        }
        Ok(())
    }

    /// Reorg fast path: roll back at once every non-final transition whose accepting block the node reported removed.
    /// Matches against an in-memory set of non-final accepting hashes; no DB/RPC unless one matches.
    pub fn on_chain_changed(self: &Arc<Self>, removed: &[String]) -> Result<()> {
        if removed.is_empty() {
            return Ok(());
        }
        let watched = self.watched_accepting()?;
        let gone: HashSet<String> = removed
            .iter()
            .filter(|h| watched.contains(*h))
            .cloned()
            .collect();
        if gone.is_empty() {
            return Ok(());
        }
        let hit: Vec<Value> = self
            .store
            .db()
            .unfinalized_heads()?
            .into_iter()
            .filter(|t| gone.contains(&t.accepting))
            .map(|t| json!({"round": t.round_id, "seq": t.seq, "txid": t.txid}))
            .collect();
        if hit.is_empty() {
            return Ok(());
        }
        self.store.log(
            "REORG_EVENT",
            json!({"removed": removed.len(), "transitions": hit}),
        );
        self.run_reorg(format!("reorg:{}", removed[0]), gone);
        Ok(())
    }

    /// Accepting hashes of non-final transitions, cached until the transitions table changes.
    fn watched_accepting(&self) -> Result<Arc<HashSet<String>>> {
        let key = self.store.db().transitions_fingerprint()?;
        let mut w = self.watch.lock().unwrap();
        if w.0 != key {
            let set: HashSet<String> = self
                .store
                .db()
                .unfinalized_heads()?
                .into_iter()
                .map(|t| t.accepting)
                .collect();
            *w = (key, Arc::new(set));
        }
        Ok(w.1.clone())
    }

    /// Missed-notification safety net: one batched UTXO query for every active (LIVE/STALE with a tip) round.
    pub async fn reconcile(self: &Arc<Self>) -> Result<()> {
        let active = self.store.db().rounds(false, true)?;
        if active.is_empty() {
            return Ok(());
        }
        let mut addrs: Vec<String> = vec![];
        for a in active
            .iter()
            .filter_map(|x| x.address.clone())
            .filter(|a| !a.is_empty())
        {
            if !addrs.contains(&a) {
                addrs.push(a);
            }
        }
        if addrs.is_empty() {
            return Ok(());
        }
        let observed = now_ms();
        let r = self
            .q("getUtxosByAddresses", json!({ "addresses": addrs }))
            .await?;
        let present: HashSet<String> = arr(r.get("entries"))
            .iter()
            .map(normalize_utxo)
            .collect::<Result<Vec<_>>>()?
            .iter()
            .map(|u| outpoint_key(&u.outpoint))
            .collect();
        let now = now_ms();
        let mut spent_rounds = vec![];
        for x in active {
            if present.contains(&outpoint_key(&x.data["tip"])) {
                self.store.db().mark_live_seen(&x.id, now)?;
                self.mark_live_anchor(&x.id, &x.data["tip"], observed)?;
            } else {
                // Tip spent on chain. A STALE round (earlier transient error) is un-staled so it can self-heal.
                if x.status == "STALE" {
                    self.store.db().set_round_status(&x.id, "LIVE", now)?;
                }
                self.store.db().mark_tip_spent(&x.id, &x.data["tip"], now)?;
                spent_rounds.push(x);
            }
        }
        let mut batch_applied = 0usize;
        let mut batch_remaining = 0usize;
        if !spent_rounds.is_empty() {
            match self.batch_catchup(&spent_rounds).await {
                Ok((n, rem)) => {
                    batch_applied = n;
                    batch_remaining = rem;
                }
                Err(e) => {
                    self.store.log(
                        "BATCH_CATCHUP_ERROR",
                        json!({"message": e.to_string()}),
                    );
                }
            }
        }
        if batch_applied > 0 && batch_remaining > 0 {
            self.run_reconcile();
        }
        if batch_applied == 0 {
            for x in spent_rounds {
                if let Ok(Some(cur)) = self.store.db().round(&x.id)
                    && cur.status == "LIVE"
                    && !present.contains(&outpoint_key(&cur.data["tip"]))
                {
                    self.run_spend(&cur.id, None);
                }
            }
        }
        self.trim_heap();
        Ok(())
    }

    /// Single-pass sequential chain scanner: walks V2 High chain blocks once and advances all matching active tips.
    async fn batch_catchup(self: &Arc<Self>, rounds: &[RoundRow]) -> Result<(usize, usize)> {
        let mut active_tips: HashMap<String, RoundRow> = HashMap::new();
        let mut earliest: Option<(i128, String)> = None;
        let mut acc_cache: HashMap<String, Acc> = HashMap::new();
        for r in rounds {
            let Some(cur) = self.store.db().round(&r.id)? else {
                continue;
            };
            if cur.status != "LIVE" || !truthy(cur.data.get("tip")) {
                continue;
            }
            let tip_key = outpoint_key(&cur.data["tip"]);
            active_tips.insert(tip_key, cur.clone());
            let accepting = js_string(cur.data.get("accepting"));
            if !accepting.is_empty() {
                let h = if let Some(cached) = acc_cache.get(&accepting) {
                    Some(cached.clone())
                } else if let Ok(fetched) = self.header(&accepting).await {
                    acc_cache.insert(accepting.clone(), fetched.clone());
                    Some(fetched)
                } else {
                    None
                };
                if let Some(h) = h
                    && h.is_chain
                {
                    let start = h.selected_parent.unwrap_or_else(|| h.hash.clone());
                    if earliest.as_ref().is_none_or(|(b, _)| h.blue_score < *b) {
                        earliest = Some((h.blue_score, start));
                    }
                }
            }
        }
        if active_tips.is_empty() {
            return Ok((0, 0));
        }
        let Some((earliest_blue, raw_start)) = earliest else {
            return Ok((0, active_tips.len()));
        };
        let (mut from, mut from_blue) = match self.chain_ancestor(&raw_start, 256).await.ok().flatten() {
            Some(acc) => (acc.hash, acc.blue_score),
            None => (raw_start, earliest_blue),
        };
        let (dag_info, sink_blue) =
            tokio::try_join!(self.q("getBlockDagInfo", json!({})), self.sink_blue())?;
        let sink = js_string(dag_info.get("sink"));
        let until = now_ms() + 15_000;
        let max_window = self.config.window_blue.max(16);
        let mut w = max_window.min(16);
        let target = crate::window_scan::page_tx_target();
        let mut blocks_scanned = 0usize;
        let mut applied_count = 0usize;
        let mut block_cache: HashMap<String, Value> = HashMap::new();

        while !active_tips.is_empty() && now_ms() < until {
            let current = if from_blue + w >= sink_blue {
                js_bigint(
                    self.q("getSinkBlueScore", json!({}))
                        .await?
                        .get("blueScore"),
                )?
            } else {
                sink_blue
            };
            let want = from_blue + w;
            let min = if current > want { current - want } else { 0 };
            let r = self
                .q(
                    "getVirtualChainFromBlockV2",
                    json!({
                        "startHash": from,
                        "dataVerbosityLevel": "High",
                        "minConfirmationCount": min as u64
                    }),
                )
                .await?;
            let added = strings(r.get("addedChainBlockHashes"));
            let Some(rows) = r
                .get("chainBlockAcceptedTransactions")
                .and_then(|v| v.as_array())
            else {
                break;
            };
            if added.is_empty() {
                if from_blue >= sink_blue || current - from_blue <= w {
                    break;
                }
                w *= 2;
                continue;
            }
            if rows.len() != added.len() {
                break;
            }
            blocks_scanned += added.len();
            for (block_hash, row) in added.iter().zip(rows.iter()) {
                if active_tips.is_empty() {
                    break;
                }
                let Some(txs) = row.get("acceptedTransactions").and_then(|v| v.as_array()) else {
                    continue;
                };
                for raw_tx in txs {
                    let Some(inputs) = raw_tx.get("inputs").and_then(|v| v.as_array()) else {
                        continue;
                    };
                    let matched_key = inputs.iter().find_map(|inp| {
                        let k = outpoint_key(&inp["previousOutpoint"]);
                        if active_tips.contains_key(&k) {
                            Some(k)
                        } else {
                            None
                        }
                    });
                    let Some(tip_key) = matched_key else {
                        continue;
                    };
                    let Some(rec) = active_tips.remove(&tip_key) else {
                        continue;
                    };
                    let cbh = row.get("chainBlockHeader");
                    let acc = if let Some(h) = cbh
                        && js_string(h.get("hash")) == *block_hash
                        && !crate::util::is_nullish(h.get("daaScore"))
                        && !crate::util::is_nullish(h.get("blueScore"))
                    {
                        Acc {
                            hash: block_hash.clone(),
                            daa: js_bigint(h.get("daaScore"))?,
                            blue_score: js_bigint(h.get("blueScore"))?,
                            is_chain: true,
                            selected_parent: None,
                        }
                    } else {
                        match self.header(block_hash).await {
                            Ok(h) if h.is_chain => h,
                            _ => continue,
                        }
                    };
                    let mut tx = canonical(&raw_tx);
                    let containing = tx["verboseData"]
                        .get("blockHash")
                        .and_then(|b| b.as_str())
                        .map(str::to_owned);
                    let mut source = "vc-batch".to_string();
                    if !is_full_transaction(&tx) {
                        if let Some(bh) = containing.as_deref() {
                            if !block_cache.contains_key(bh) {
                                if block_cache.len() >= 16 {
                                    block_cache.clear();
                                }
                                let blk = match self
                                    .q(
                                        "getBlock",
                                        json!({"hash": bh, "includeTransactions": true}),
                                    )
                                    .await
                                {
                                    Ok(b) => b,
                                    Err(_) => continue,
                                };
                                block_cache.insert(bh.to_string(), blk);
                            }
                            tx = match Self::hydrate_from_block(&tx, &block_cache[bh]) {
                                Ok(h) => h,
                                Err(_) => continue,
                            };
                        } else {
                            tx = match self.hydrate(&tx).await {
                                Ok(h) => h,
                                Err(_) => continue,
                            };
                        }
                        let _ = with_input_verbose(&mut tx, raw_tx);
                        source.push_str("+block");
                    }
                    let c = Candidate {
                        txid: txid_of(&tx).unwrap_or_default(),
                        tx,
                        containing,
                        start: None,
                        accepting: Some(acc.clone()),
                        source,
                        recovered: None,
                    };
                    let skip_live = acc.blue_score + 20 < sink_blue;
                    if let Ok((res, updated_opt)) =
                        self.apply_spend_mode_row(&rec, c, &acc, skip_live).await
                        && res == "APPLIED"
                    {
                        applied_count += 1;
                        if let Some(updated) = updated_opt
                            && updated.status == "LIVE"
                            && truthy(updated.data.get("tip"))
                        {
                            active_tips.insert(outpoint_key(&updated.data["tip"]), updated);
                        }
                    }
                }
            }
            let page_txs: usize = rows
                .iter()
                .map(|row| {
                    row.get("acceptedTransactions")
                        .and_then(|x| x.as_array())
                        .map(|a| a.len())
                        .unwrap_or(0)
                })
                .sum();
            if w > max_window {
                w = max_window;
            }
            if page_txs > target && w > 1 {
                w = (w / 2).max(1);
            } else if page_txs * 4 < target && w < max_window {
                w = (w * 2).min(max_window);
            }
            let last_blue = rows
                .last()
                .and_then(|row| row.get("chainBlockHeader"))
                .and_then(|h| h.get("blueScore"))
                .and_then(|b| js_bigint(Some(b)).ok());
            let last = added.last().unwrap().clone();
            let hit_sink = last == sink || added.contains(&sink);
            drop(r);
            if hit_sink {
                break;
            }
            let top = match last_blue {
                Some(b) => b,
                None => match self.header(&last).await {
                    Ok(h) => h.blue_score,
                    Err(_) => break,
                },
            };
            if top >= sink_blue {
                break;
            }
            from = last;
            from_blue = top;
        }
        // Ensure any round left active after skipping historical subscriptions is subscribed for live events.
        let sub_addrs: Vec<String> = active_tips
            .values()
            .filter_map(|r| r.address.clone())
            .collect();
        let _ = self.subscribe_many(&sub_addrs).await;
        let remaining = active_tips.len();
        self.store.log(
            "BATCH_CATCHUP_DONE",
            json!({
                "blocks": blocks_scanned,
                "applied": applied_count,
                "remaining": remaining
            }),
        );
        drop(block_cache);
        self.trim_heap();
        Ok((applied_count, remaining))
    }

    pub fn relay_payload(&self, kind: &str, t: &TransitionRow, address: Option<&str>) -> Value {
        let plugin = t
            .contract
            .as_deref()
            .and_then(|c| self.contracts().get(c).ok());
        let mut p = json!({"schema": "kaswin-event-indexer/1", "type": kind, "network": self.net.key, "contract": t.contract,
            "profileId": plugin.map(|p| json!(p.profile_id())).unwrap_or(Value::Null),
            "roundId": t.round_id, "seq": t.seq, "txid": t.txid, "kind": t.kind, "status": t.status,
            "accepting": {"hash": t.accepting, "daa": t.accepting_daa, "blueScore": t.accepting_blue_score},
            "containing": t.containing, "address": address});
        if kind == "transition" {
            let mut summary = t.next.as_object().cloned().unwrap_or_default();
            for k in [
                "latestTransaction",
                "genesisTransaction",
                "receipts",
                "history",
                "preimages",
                "redeemHex",
            ] {
                summary.shift_remove(k);
            }
            p["round"] = canonical(&Value::Object(summary));
        }
        p["trust"] = json!(TRUST_NOTE);
        p
    }
}

/// Node-like transaction DTO built from a REST row (hint only; hydrated from the block body before any use).
fn rest_tx_dto(raw: &Value, txid: &str) -> Value {
    let or = |v: Option<&Value>, d: Value| nn(v).cloned().unwrap_or(d);
    let inputs: Vec<Value> = arr(raw.get("inputs"))
        .iter()
        .map(|i| {
            let mut po = Map::new();
            if let Some(h) = i.get("previous_outpoint_hash") {
                po.insert("transactionId".into(), h.clone());
            }
            po.insert("index".into(), js_num_json(Some(&or(i.get("previous_outpoint_index"), json!(0)))));
            json!({"previousOutpoint": po,
                "signatureScript": i.get("signature_script").filter(|s| truthy(Some(s))).cloned().unwrap_or(json!("")),
                "sequence": js_string(Some(&or(i.get("sequence"), json!(0)))),
                "sigOpCount": or(i.get("sig_op_count"), json!(1))})
        })
        .collect();
    let outputs: Vec<Value> = arr(raw.get("outputs"))
        .iter()
        .map(|o| {
            let spk = match nn(o.get("script_public_key")) {
                Some(s) if s.is_object() => {
                    json!({"version": js_num_json(Some(&or(s.get("version"), json!(0)))), "script": js_string(Some(&or(s.get("script"), json!(""))))})
                }
                _ => json!({"version": js_num_json(Some(&or(o.get("script_public_key_version"), json!(0)))),
                    "script": js_string(Some(&or(o.get("script_public_key"), json!(""))))}),
            };
            let cov_obj = o.get("covenant");
            let cov_id = [o.get("covenant_id"), cov_obj.and_then(|c| c.get("covenantId"))].into_iter().flatten().find(|v| truthy(Some(v))).cloned();
            let cov_in = nn(o.get("covenant_authorizing_input")).or_else(|| nn(cov_obj.and_then(|c| c.get("authorizingInput")))).cloned().unwrap_or(json!(0));
            let covenant = match cov_id {
                Some(id) => json!({"authorizingInput": js_num_json(Some(&cov_in)), "covenantId": js_string(Some(&id))}),
                None => Value::Null,
            };
            json!({"value": js_string(Some(&or(o.get("amount"), json!(0)))), "scriptPublicKey": spk, "covenant": covenant,
                "verboseData": {"scriptPublicKeyAddress": o.get("script_public_key_address").filter(|a| truthy(Some(a))).cloned().unwrap_or(Value::Null)}})
        })
        .collect();
    let block_hash = raw
        .get("accepting_block_hash")
        .filter(|a| truthy(Some(a)))
        .cloned()
        .or_else(|| {
            raw.get("block_hash")
                .and_then(|b| b.as_array())
                .map(|a| a.first().cloned().unwrap_or(Value::Null))
        })
        .unwrap_or(Value::Null);
    json!({"version": or(raw.get("version"), json!(0)), "lockTime": js_string(Some(&or(raw.get("lock_time"), json!(0)))),
        "subnetworkId": or(raw.get("subnetwork_id"), json!("0000000000000000000000000000000000000000")),
        "gas": js_string(Some(&or(raw.get("gas"), json!(0)))), "payload": or(raw.get("payload"), json!("")),
        "inputs": inputs, "outputs": outputs, "verboseData": {"transactionId": txid, "blockHash": block_hash}})
}

#[path = "pruned.rs"]
mod pruned;

#[cfg(test)]
#[path = "engine_tests.rs"]
mod tests;

/// Convenience for callers that only need an error string.
pub fn err_text(e: &anyhow::Error) -> String {
    e.to_string()
}

#[allow(dead_code)]
fn _assert_send(e: Arc<Engine>) {
    fn is_send<T: Send>(_: T) {}
    is_send(async move { e.resolve_spend("", None).await });
    let _ = anyhow!("");
}
