//! Node pool selection for dual-node mode (`src/node-pool.mjs`): probe candidates of ONE network, drop bad ones, and
//! pick two members that are NOT the same backend (distinct p2pId). Several public hostnames can front one node.
use crate::networks::Network;
use futures_util::future::{BoxFuture, join_all};
use serde_json::{Value, json};
use std::time::Duration;

#[derive(Clone, Debug)]
pub struct Probe {
    pub p2p_id: Option<String>,
    pub blue_score: u64,
    pub rtt_ms: u64,
}

#[derive(Clone, Debug)]
pub struct Healthy {
    pub url: String,
    pub p2p_id: String,
    pub blue_score: u64,
    pub rtt_ms: u64,
    pub lag: u64,
}

pub struct PoolResult {
    pub healthy: Vec<Healthy>,
    pub rejected: Vec<Value>,
}

pub type ProbeFn =
    std::sync::Arc<dyn Fn(String) -> BoxFuture<'static, anyhow::Result<Probe>> + Send + Sync>;

pub fn dedup(urls: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    for u in urls {
        let u = crate::rpc::normalize_url(&u);
        if !out.contains(&u) {
            out.push(u);
        }
    }
    out
}

/// Probe all candidates in parallel (deadline 12 s each) and rank the healthy ones (rtt, lag, url).
pub async fn probe_pool(urls: Vec<String>, probe: ProbeFn, max_lag: u64) -> PoolResult {
    let uniq = dedup(urls);
    let settled = join_all(uniq.iter().map(|u| {
        let p = probe.clone();
        let u = u.clone();
        async move {
            match tokio::time::timeout(Duration::from_millis(12_000), p(u.clone())).await {
                Err(_) => (u, Err("PROBE_TIMEOUT".to_string())),
                Ok(Err(e)) => (u, Err(crate::util::slice_chars(&e.to_string(), 80))),
                Ok(Ok(r)) => (u, Ok(r)),
            }
        }
    }))
    .await;
    let mut rejected: Vec<Value> = settled
        .iter()
        .filter_map(|(u, r)| r.as_ref().err().map(|e| json!({"url": u, "reason": e})))
        .collect();
    let ok: Vec<(String, Probe)> = settled
        .into_iter()
        .filter_map(|(u, r)| {
            r.ok()
                .filter(|p| p.p2p_id.as_deref().map(|s| !s.is_empty()).unwrap_or(false))
                .map(|p| (u, p))
        })
        .collect();
    let top = ok.iter().map(|(_, p)| p.blue_score).max().unwrap_or(0);
    let mut healthy = vec![];
    for (u, p) in ok {
        let lag = top - p.blue_score;
        if lag > max_lag {
            rejected.push(json!({"url": u, "reason": format!("LAGGING_{lag}")}));
        } else {
            healthy.push(Healthy {
                url: u,
                p2p_id: p.p2p_id.unwrap(),
                blue_score: p.blue_score,
                rtt_ms: p.rtt_ms,
                lag,
            });
        }
    }
    healthy.sort_by(|a, b| {
        a.rtt_ms
            .cmp(&b.rtt_ms)
            .then(a.lag.cmp(&b.lag))
            .then(a.url.cmp(&b.url))
    });
    PoolResult { healthy, rejected }
}

/// [primary, standby], never two on the same backend; `exclude` = p2pIds to avoid.
pub fn choose_pair(healthy: &[Healthy], exclude: &[String]) -> Vec<Healthy> {
    let mut seen: Vec<String> = exclude.to_vec();
    let mut out = vec![];
    for c in healthy {
        if seen.contains(&c.p2p_id) {
            continue;
        }
        seen.push(c.p2p_id.clone());
        out.push(c.clone());
        if out.len() == 2 {
            break;
        }
    }
    out
}

pub fn choose_replacement(healthy: &[Healthy], exclude: &[String]) -> Option<Healthy> {
    choose_pair(healthy, exclude).into_iter().next()
}

/// Resolver hosts from rusty-kaspa rpc/wrpc/client/Resolvers.toml (4 groups x 4, fetched 2026-09-30).
pub fn resolver_hosts() -> Vec<String> {
    let mut v = vec![];
    for (names, dom) in [
        (["eric", "maxim", "sean", "troy"], "kaspa.stream"),
        (["john", "mike", "paul", "alex"], "kaspa.red"),
        (["jake", "mark", "adam", "liam"], "kaspa.green"),
        (["noah", "ryan", "jack", "luke"], "kaspa.blue"),
    ] {
        for n in names {
            v.push(format!("{n}.{dom}"));
        }
    }
    v
}

/// Ask kaspa-resolver services for node URLs of ONE network. Best effort; failures ignored.
pub async fn discover_from_resolvers(
    http: &reqwest::Client,
    net: &Network,
    rounds: usize,
) -> Vec<String> {
    let re =
        regex::Regex::new(r"^wss://[a-z0-9.-]+/kaspa/([a-z0-9-]+)/wrpc/(borsh|json)$").unwrap();
    let mut found: Vec<String> = vec![];
    for _ in 0..rounds {
        let answers = join_all(resolver_hosts().into_iter().map(|host| {
            let http = http.clone();
            let path = net.resolver_path;
            async move {
                let url = format!("https://{host}/v2/kaspa/{path}/tls/wrpc/borsh");
                let r = http
                    .get(&url)
                    .timeout(Duration::from_millis(4000))
                    .send()
                    .await
                    .ok()?;
                if !r.status().is_success() {
                    return None;
                }
                let j: Value = r.json().await.ok()?;
                j.get("url").and_then(|u| u.as_str()).map(|s| s.to_string())
            }
        }))
        .await;
        for u in answers.into_iter().flatten() {
            if let Some(c) = re.captures(&u)
                && &c[1] == net.resolver_path
                && !found.contains(&u)
            {
                found.push(u);
            }
        }
    }
    found
}
