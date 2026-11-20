//! Runtime composition; independent engine/store/node set per network.
use crate::{
    contracts::{LoadCtx, load_contracts},
    dual_node::{DualNode, NodeEvent, OpenFn, PickFn},
    engine::{Engine, EngineConfig},
    log::log,
    networks::Network,
    node::open_node,
    node_pool::{self, Probe, ProbeFn},
    store::Store,
    util::{js_bigint, now_ms},
};
use anyhow::{Result, bail};
use serde_json::{Value, json};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::task::JoinHandle;

pub struct View {
    pub engine: Arc<Engine>,
    pub pool: Arc<Pool>,
    pub degraded: Arc<AtomicBool>,
}
impl View {
    pub fn net(&self) -> &'static Network {
        self.engine.net
    }

    /// Periodically switch back to a managed local node once it finishes syncing and catches up.
    pub async fn reclaim_local_if_ready(&self) -> Result<()> {
        let cur = self.engine.node.url();
        if cur.as_deref().is_some_and(node_pool::is_local_url) {
            return Ok(());
        }
        let locals: Vec<String> = self
            .engine
            .store
            .db()
            .pool_nodes()?
            .iter()
            .filter_map(|v| v["url"].as_str().map(str::to_owned))
            .filter(|u| node_pool::is_local_url(u))
            .collect();
        if locals.is_empty() {
            return Ok(());
        }
        let active_blue = u64::try_from(js_bigint(
            self.engine
                .node
                .query("getSinkBlueScore", json!({}))
                .await?
                .get("blueScore"),
        )?)?;
        let max_lag = self.pool.max_lag();
        for url in locals {
            if let Ok(Ok(p)) =
                tokio::time::timeout(Duration::from_secs(6), (self.pool.probe)(url.clone())).await
                && active_blue.saturating_sub(p.blue_score) <= max_lag
            {
                let res = self.pool.elect(Some(&self.engine.node)).await?;
                let next = self.engine.node.url();
                if next.as_deref().is_some_and(node_pool::is_local_url) {
                    log(
                        "LOCAL_NODE_RECLAIMED",
                        json!({"network": self.net().key, "from": cur, "to": next, "chosen": res["chosen"]}),
                    );
                    self.engine.run_reconcile();
                }
                break;
            }
        }
        Ok(())
    }
}
pub struct Pool {
    pub net: &'static Network,
    pub store: Arc<Store>,
    pub http: reqwest::Client,
    pub extra: Vec<String>,
    pub open: OpenFn,
    pub probe: ProbeFn,
}
impl Pool {
    pub fn new(net: &'static Network, store: Arc<Store>, extra: Vec<String>) -> Arc<Self> {
        let open: OpenFn = Arc::new(move |url| {
            Box::pin(async move {
                open_node(
                    net,
                    &url,
                    Duration::from_secs(6),
                    Some(Duration::from_secs(8)),
                )
                .await
            })
        });
        let op = open.clone();
        let probe: ProbeFn = Arc::new(move |url| {
            let op = op.clone();
            Box::pin(async move {
                let (n, _events) = op(url).await?;
                let start = Instant::now();
                let r = n.call("getSinkBlueScore", json!({})).await?;
                let blue = u64::try_from(js_bigint(r.get("blueScore"))?)?;
                let rtt_ms = start.elapsed().as_millis() as u64;
                let p = Probe {
                    p2p_id: n.p2p_id(),
                    blue_score: blue,
                    rtt_ms,
                };
                n.close();
                Ok(p)
            })
        });
        Arc::new(Self {
            net,
            store,
            http: reqwest::Client::new(),
            extra,
            open,
            probe,
        })
    }
    pub async fn candidates(&self) -> Result<Vec<String>> {
        let managed: Vec<String> = self
            .store
            .db()
            .pool_nodes()?
            .iter()
            .filter_map(|v| v["url"].as_str().map(str::to_owned))
            .collect();
        let has_local = managed.iter().any(|u| node_pool::is_local_url(u));
        let discovered = if has_local {
            vec![]
        } else {
            node_pool::discover_from_resolvers(&self.http, self.net, 2).await
        };
        Ok(node_pool::dedup(
            managed
                .into_iter()
                .chain(self.extra.clone())
                .chain(self.net.seed.iter().map(|s| s.to_string()))
                .chain(discovered),
        ))
    }
    pub async fn elect(&self, node: Option<&Arc<DualNode>>) -> Result<Value> {
        let p = node_pool::probe_pool(self.candidates().await?, self.probe.clone(), self.max_lag())
            .await;
        crate::log::log(
            "POOL_ELECT",
            json!({"network": self.net.key, "healthy": p.healthy.iter().map(|h| json!({"url": h.url, "rtt": h.rtt_ms, "lag": h.lag})).collect::<Vec<_>>(), "rejected": p.rejected}),
        );
        let pair = node_pool::choose_pair(&p.healthy, &[]);
        if pair.is_empty() {
            bail!("POOL_NO_HEALTHY_NODE {}", self.net.key);
        }
        let mut urls: Vec<String> = pair.iter().map(|x| x.url.clone()).collect();
        if urls.len() == 1 {
            if let Some(s) = self.net.seed.iter().find(|s| **s != urls[0]) {
                urls.push(s.to_string());
            }
        }
        if let Some(n) = node {
            n.switch_urls(urls.clone()).await?;
        }
        let healthy: Vec<Value> = p.healthy.iter().map(|x| json!({"url":x.url,"p2pId":x.p2p_id,"rttMs":x.rtt_ms,"lag":x.lag.to_string(),"blueScore":x.blue_score.to_string()})).collect();
        Ok(json!({"chosen":urls,"healthy":healthy,"rejected":p.rejected}))
    }
    fn max_lag(&self) -> u64 {
        if self.net.key == "tn10" { 3000 } else { 600 }
    }
    pub fn replacement(self: &Arc<Self>) -> PickFn {
        let p = self.clone();
        Arc::new(move |r| {
            let p = p.clone();
            Box::pin(async move {
                let urls = p
                    .candidates()
                    .await?
                    .into_iter()
                    .filter(|u| !r.avoid_urls.contains(u))
                    .collect();
                let pool = node_pool::probe_pool(urls, p.probe.clone(), p.max_lag()).await;
                Ok(node_pool::choose_replacement(&pool.healthy, &r.avoid_p2p_ids).map(|x| x.url))
            })
        })
    }
}

pub async fn open_view(
    net: &'static Network,
    db: &str,
    contracts: Option<&str>,
    urls: Option<Vec<String>>,
    extra: Vec<String>,
    config: EngineConfig,
) -> Result<(Arc<View>, tokio::sync::mpsc::UnboundedReceiver<NodeEvent>)> {
    let store = Arc::new(Store::open(db, None, net.key)?);
    let plugins = Arc::new(load_contracts(
        contracts,
        &LoadCtx {
            network: net.key,
            registry_addresses: None,
        },
    )?);
    let pool = Pool::new(net, store.clone(), extra);
    let automatic = urls.is_none();
    let urls = match urls {
        Some(u) => u,
        None => pool.elect(None).await?["chosen"]
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x.as_str().unwrap().to_owned())
            .collect(),
    };
    let (node, events) = DualNode::open(
        urls,
        pool.open.clone(),
        if automatic {
            Some(pool.replacement())
        } else {
            None
        },
    )
    .await?;
    let engine = Engine::new(store, plugins, node, net, config);
    Ok((
        Arc::new(View {
            engine,
            pool,
            degraded: Arc::new(AtomicBool::new(false)),
        }),
        events,
    ))
}

pub async fn start_view(
    view: Arc<View>,
    mut events: tokio::sync::mpsc::UnboundedReceiver<NodeEvent>,
    checkpoint_ms: u64,
    reconcile_ms: u64,
) -> Result<Vec<JoinHandle<()>>> {
    let e = view.engine.clone();
    // Start subscriptions/reconciliation before consuming buffered node notifications, to preserve startup ordering.
    e.start().await?;
    match e.node.subscribe_chain().await {
        Ok(_) => {
            e.chain_subscribed.store(true, Ordering::SeqCst);
        }
        Err(err) => log(
            "CHAIN_SUBSCRIBE_FAILED",
            json!({"network":view.net().key,"message":err.to_string()}),
        ),
    }
    let mut tasks = vec![];
    let v = view.clone();
    tasks.push(tokio::spawn(async move {
        while let Some(event) = events.recv().await {
            let e = &v.engine;
            let result = match event {
                NodeEvent::Utxos(data) => e.on_utxos_changed(&data),
                NodeEvent::Chain(removed) => e.on_chain_changed(&removed),
                NodeEvent::Disconnect => {
                    v.degraded.store(true, Ordering::SeqCst);
                    log("DISCONNECTED", json!({"network":v.net().key}));
                    Ok(())
                }
                NodeEvent::Connect => {
                    let v = v.clone();
                    e.run("reconnect".into(), move |e| async move {
                        if v.degraded.load(Ordering::SeqCst) {
                            e.node.verify().await?;
                            e.resubscribe_all().await?;
                            v.degraded.store(false, Ordering::SeqCst);
                        }
                        Ok(())
                    });
                    Ok(())
                }
                NodeEvent::Failover { from, to } => {
                    log(
                        "FAILOVER_RECONCILE",
                        json!({"network":v.net().key,"from":from,"to":to}),
                    );
                    e.run_reconcile();
                    e.retry_pending()
                }
            };
            if let Err(err) = result {
                log(
                    "HANDLER_ERROR",
                    json!({"network":v.net().key,"message":err.to_string()}),
                );
            }
        }
    }));
    for (ms, kind) in [
        (checkpoint_ms, "checkpoint"),
        (reconcile_ms, "reconcile"),
        (5000, "retry"),
        (60_000, "local_reclaim"),
    ] {
        let v = view.clone();
        tasks.push(tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_millis(ms.max(100)));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            interval.tick().await;
            loop {
                interval.tick().await;
                if v.degraded.load(Ordering::SeqCst) {
                    continue;
                }
                let e = &v.engine;
                let r = match kind {
                    "checkpoint" => e.checkpoint().await.map(|_| ()),
                    "reconcile" => {
                        e.run_reconcile();
                        e.run_finality();
                        Ok(())
                    }
                    "local_reclaim" => v.reclaim_local_if_ready().await,
                    _ => e.retry_pending(),
                };
                if let Err(err) = r {
                    log(
                        "MAINTENANCE_ERROR",
                        json!({"network":v.net().key,"kind":kind,"message":err.to_string()}),
                    );
                }
            }
        }));
    }
    log(
        "STARTED",
        json!({"network":view.net().key,"at":now_ms(),"node":e.node.identity(),"live":e.store.db().count_rounds(true)?}),
    );
    Ok(tasks)
}

pub fn db_path(base: &str, net: &Network, multi: bool) -> String {
    if !multi {
        return base.into();
    }
    let p = std::path::Path::new(base);
    let stem = p.file_stem().unwrap_or_default().to_string_lossy();
    let ext = p
        .extension()
        .map(|s| format!(".{}", s.to_string_lossy()))
        .unwrap_or_default();
    p.with_file_name(format!("{stem}.{}{ext}", net.key))
        .to_string_lossy()
        .into_owned()
}
