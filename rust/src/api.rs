//! HTTP cache and Admin Studio. All queries/probes are public; mutations require the administrator token.
//! Public query routes under `/v1/{info,rounds,txid}` (with optional `/{net}` prefix or `/{net}/{info,rounds,txid}` shorthand,
//! plus legacy `/{net}/rolling/v1/{profileId}/...`) serve Rolling v1 directly.
use crate::{
    contracts::{DEFAULT_CONTRACTS_JSON, LoadCtx, load_contracts_json},
    rpc::RpcClient,
    runtime::View,
    store::RoundRow,
    util::{is_hash, js_bigint, js_string, now_ms},
};
use anyhow::{Result, bail};
use axum::{
    Router,
    body::{Body, to_bytes},
    extract::{Request, State},
    http::{HeaderMap, Method, StatusCode},
    response::{IntoResponse, Response},
};
use serde_json::{Value, json};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

pub struct Api {
    pub views: Vec<Arc<View>>,
    pub token: String,
    pub contracts_path: Option<String>,
    pub started: Instant,
    pub mutations: tokio::sync::Mutex<()>,
    pub export_cache: std::sync::Mutex<Option<((String, String, i64), axum::body::Bytes)>>,
}
impl Api {
    pub fn pick(&self, key: Option<&str>) -> Option<Arc<View>> {
        match key {
            None => self.views.first().cloned(),
            Some(k) => self
                .views
                .iter()
                .find(|v| v.net().key == k || v.net().network_id == k)
                .cloned(),
        }
    }
    fn keys(&self) -> Vec<&str> {
        self.views.iter().map(|v| v.net().key).collect()
    }
}
pub fn router(api: Arc<Api>) -> Router {
    Router::new().fallback(handler).with_state(api)
}
fn response(code: u16, body: Value) -> Response {
    let mut r = (StatusCode::from_u16(code).unwrap(), axum::Json(body)).into_response();
    r.headers_mut()
        .insert("cache-control", "no-store".parse().unwrap());
    r
}
fn first<'a>(q: &'a [(String, String)], key: &str) -> Option<&'a str> {
    q.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
}
fn authorized(api: &Api, h: &HeaderMap, q: &[(String, String)]) -> bool {
    if api.token.is_empty() {
        return false;
    }
    if let Some(a) = h.get("authorization").and_then(|a| a.to_str().ok()) {
        let p: Vec<_> = a.split_whitespace().collect();
        if p.len() == 2 && p[0].eq_ignore_ascii_case("bearer") && p[1] == api.token {
            return true;
        }
    }
    if first(q, "token") == Some(api.token.as_str()) {
        return true;
    }
    if let Some(c) = h.get("cookie").and_then(|c| c.to_str().ok()) {
        for pair in c.split(';') {
            if let Some(("admin_token", v)) = pair.trim().split_once('=') {
                let decoded = percent_decode(v);
                if decoded == api.token {
                    return true;
                }
            }
        }
    }
    false
}
fn percent_decode(s: &str) -> String {
    // Cookie values are percent encoded, never form encoded ('+' is literal).
    url::form_urlencoded::parse(format!("x={}", s.replace('+', "%2B")).as_bytes())
        .next()
        .map(|(_, v)| v.into_owned())
        .unwrap_or_default()
}
async fn handler(State(api): State<Arc<Api>>, req: Request) -> Response {
    let is_options = req.method() == Method::OPTIONS;
    let mut res = if is_options {
        (StatusCode::NO_CONTENT, Body::empty()).into_response()
    } else {
        match dispatch(&api, req).await {
            Ok(r) => r,
            Err(e) => response(
                500,
                json!({"ok":false,"detail":"INTERNAL_ERROR","error":e.to_string()}),
            ),
        }
    };
    let h = res.headers_mut();
    for (k, v) in [
        ("access-control-allow-origin", "*"),
        ("access-control-allow-methods", "GET, POST, OPTIONS"),
        (
            "access-control-allow-headers",
            "Content-Type, Authorization",
        ),
        ("x-content-type-options", "nosniff"),
    ] {
        h.insert(k, v.parse().unwrap());
    }
    res
}
async fn dispatch(api: &Arc<Api>, req: Request) -> Result<Response> {
    let path = req.uri().path().to_owned();
    let query: Vec<(String, String)> =
        url::form_urlencoded::parse(req.uri().query().unwrap_or("").as_bytes())
            .into_owned()
            .collect();
    let method = req.method().clone();
    if matches!(path.as_str(), "/admin" | "/admin/" | "/admin/index.html") {
        return Ok((
            [
                ("content-type", "text/html; charset=utf-8"),
                ("cache-control", "no-store"),
            ],
            include_str!("../assets/admin.html"),
        )
            .into_response());
    }
    let parts: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    let mut key = first(&query, "network");
    let mut route = path.clone();
    let mut rolling_route: Option<(String, Vec<String>)> = None;
    if parts.last() == Some(&"status") && !path.starts_with("/admin") {
        match parts.as_slice() {
            ["status"] | ["v1", "status"] => {}
            [n, "status"] | [n, "v1", "status"] | ["v1", n, "status"] => {
                key = Some(n);
            }
            _ => return Ok(response(404, json!({"detail":"NOT_FOUND"}))),
        }
        route = "/admin/api/status".into();
    } else if !path.starts_with("/admin") {
        match parts.as_slice() {
            [n, "rolling", "v1", pid, rest @ ..] => {
                key = Some(n);
                rolling_route = Some((
                    (*pid).to_string(),
                    rest.iter().map(|s| (*s).to_string()).collect(),
                ));
            }
            [n, "v1", rest @ ..] if *n != "v1" => {
                key = Some(n);
                route = format!("/v1/{}", rest.join("/"));
            }
            ["v1", n, rest @ ..]
                if !matches!(*n, "info" | "rounds" | "txid" | "networks") =>
            {
                key = Some(n);
                route = format!("/v1/{}", rest.join("/"));
            }
            [n, rest @ ..]
                if *n != "v1"
                    && rest.first().is_some_and(|r| {
                        matches!(
                            *r,
                            "info" | "rounds" | "txid" | "networks" | "wallets" | "covenants"
                        )
                    }) =>
            {
                key = Some(n);
                route = format!("/v1/{}", rest.join("/"));
            }
            _ => {}
        }
    }
    let Some(view) = api.pick(key) else {
        return Ok(response(
            400,
            json!({"ok":false,"detail":"UNKNOWN_NETWORK","error":"UNKNOWN_NETWORK","networks":api.keys()}),
        ));
    };
    if let Some(sub) = route.strip_prefix("/admin/api") {
        let auth = authorized(api, req.headers(), &query);
        if sub == "/verify-token" && method == Method::POST {
            return Ok(response(if auth { 200 } else { 401 }, json!({"ok":auth})));
        }
        let mutation = matches!(
            sub,
            "/track" | "/recheck-discoveries" | "/compile" | "/generate-plugin" | "/install-plugin"
        ) || sub.starts_with("/plugins/")
            || sub.starts_with("/pool/");
        if mutation && !auth {
            return Ok(response(
                401,
                json!({"ok":false,"error":"UNAUTHORIZED","detail":"管理操作需要管理员密码"}),
            ));
        }
        let body = if method == Method::POST {
            let bytes = match tokio::time::timeout(
                Duration::from_secs(10),
                to_bytes(req.into_body(), 1024 * 1024),
            )
            .await
            {
                Ok(Ok(b)) => b,
                _ => {
                    return Ok(response(
                        413,
                        json!({"ok":false,"error":"PAYLOAD_TOO_LARGE_OR_TIMEOUT"}),
                    ));
                }
            };
            if bytes.is_empty() {
                json!({})
            } else {
                match serde_json::from_slice::<Value>(&bytes) {
                    Ok(v) => v,
                    Err(_) => return Ok(response(400, json!({"ok":false,"error":"INVALID_JSON"}))),
                }
            }
        } else {
            json!({})
        };
        return admin(api, &view, sub, &method, &query, &body).await;
    }
    if method != Method::GET {
        return Ok(response(405, json!({"detail":"METHOD_NOT_ALLOWED"})));
    }
    if route == "/v1/networks" {
        let selected: Vec<_> = if path == "/v1/networks" {
            api.views.clone()
        } else {
            vec![view]
        };
        let networks = selected
            .iter()
            .map(|v| catalog(v, false))
            .collect::<Result<Vec<_>>>()?;
        return Ok(response(
            200,
            json!({"ok":true,"defaultNetwork":api.views[0].net().key,"networks":networks}),
        ));
    }
    let e = &view.engine;
    let cp = e
        .store
        .db()
        .checkpoints_before(now_ms() + 1, 1)?
        .into_iter()
        .next();
    let virtual_daa = cp.as_ref().map(|c| c.virtual_daa.as_str());
    if let Some((pid, rest)) = rolling_route {
        let rest_ref: Vec<&str> = rest.iter().map(String::as_str).collect();
        return rolling_api(api, &view, &pid, &rest_ref, &query, virtual_daa, cp.as_ref());
    }
    if let Some(sub_v1) = route.strip_prefix("/v1/") {
        let rest_ref: Vec<&str> = sub_v1.split('/').filter(|s| !s.is_empty()).collect();
        match rest_ref.as_slice() {
            ["wallets", pk, "activity"] => {
                return kusdt_v2_wallet_activity(&view, pk, &query, virtual_daa);
            }
            ["covenants", cid] => {
                return kusdt_v2_covenant_lookup(&view, cid);
            }
            _ => {}
        }
        if matches!(
            rest_ref.first().copied(),
            Some("info" | "rounds" | "txid")
        ) {
            return rolling_api(
                api,
                &view,
                crate::contracts::rolling_v1::PROFILE_ID,
                &rest_ref,
                &query,
                virtual_daa,
                cp.as_ref(),
            );
        }
    }
    Ok(response(404, json!({"detail":"NOT_FOUND"})))
}
pub fn category(r: &RoundRow) -> &'static str {
    if r
        .contract
        .as_deref()
        .is_some_and(|s| s.starts_with("rolling-v1@"))
    {
        return rolling_category(r);
    }
    if r
        .contract
        .as_deref()
        .is_some_and(|s| s.starts_with("kusdt-v2@"))
    {
        return kusdt_v2_category(r);
    }
    if r.status == "STALE"
        || !r
            .contract
            .as_deref()
            .is_some_and(|s| s.starts_with("kaswin-v2.0.2@"))
    {
        return "unknown";
    }
    if r.status == "TERMINAL" {
        return "close";
    }
    match r.data["state"]["phase"].as_i64() {
        Some(1) => "open",
        Some(2) => "sealed",
        Some(5) => "close",
        _ => "unknown",
    }
}
/// Rolling v1: a non-null `terminal` closes the round; otherwise the manager module gives the phase.
/// `refund` is grouped with `close`, matching how V2.0.2 groups its refunding phase.
fn rolling_category(r: &RoundRow) -> &'static str {
    if r.status == "STALE" || r.status == "ROLLED_BACK" {
        return "unknown";
    }
    if r.status == "TERMINAL" || !r.data["terminal"].is_null() {
        return "close";
    }
    match r.data["module"].as_str() {
        Some("sales") => "open",
        Some("sealed" | "win" | "frozen") => "sealed",
        Some("refund") => "close",
        _ => "unknown",
    }
}
fn kusdt_v2_category(r: &RoundRow) -> &'static str {
    if r.status == "STALE" || r.status == "ROLLED_BACK" {
        return "unknown";
    }
    if r.status == "TERMINAL" || !r.data["terminal"].is_null() {
        return "close";
    }
    match r.data["stage"].as_str() {
        Some("M_OPEN") => "open",
        Some("M_SEALED") => "sealed",
        Some("M_DISPATCH" | "SHARD_ONLY" | "DONE") => "close",
        _ => "unknown",
    }
}
fn catalog(v: &View, admin: bool) -> Result<Value> {
    let n = v.net();
    let e = &v.engine;
    let id = e.node.identity();
    let mut out = json!({"key":n.key,"label":n.label,"networkId":n.network_id,"genesis":n.genesis,"rest":n.rest,"registry":n.registry,
        "rounds":e.store.db().count_rounds(false)?,"live":e.store.db().count_rounds(true)?,"activeNode":if admin {id.clone()}else{id["url"].clone()}});
    if !admin {
        out["virtualDaaScore"] = id["virtualDaaScore"].clone();
    }
    Ok(out)
}
fn rss_mb() -> Option<f64> {
    let s = std::fs::read_to_string("/proc/self/status").ok()?;
    let line = s.lines().find(|l| l.starts_with("VmRSS:"))?;
    let kb = line.split_whitespace().nth(1)?.parse::<f64>().ok()?;
    Some((kb / 1024. * 10.).round() / 10.)
}
fn status(api: &Api, v: &View) -> Result<Value> {
    let e = &v.engine;
    let all = e.store.db().rounds(false, false)?;
    let mut by_contract = json!({});
    let mut by_status = json!({"open":0,"sealed":0,"close":0,"unknown":0});
    for r in &all {
        let k = r.contract.as_deref().unwrap_or("null");
        by_contract[k] = json!(by_contract[k].as_u64().unwrap_or(0) + 1);
        let k = category(r);
        by_status[k] = json!(by_status[k].as_u64().unwrap_or(0) + 1);
    }
    let cp = e
        .store
        .db()
        .checkpoints_before(now_ms() + 1, 1)?
        .into_iter()
        .next();
    Ok(
        json!({"ok":true,"name":"kaspa-covenant-indexer","version":"v0.2.0-rust","runtime":"rust","uptimeSec":api.started.elapsed().as_secs(),
        "memory":{"rssMb":rss_mb(),"heapUsedMb":null,"heapTotalMb":null},"network":v.net().network_id,"networkKey":v.net().key,"networkGenesis":v.net().genesis,"networks":api.keys(),
        "activeNode":e.node.identity(),"rest":{"url":e.config.rest_url,"enabled":e.config.rest_url.is_some()},"nodePool":e.node.status(),
        "rounds":{"total":all.len(),"live":all.iter().filter(|r|r.status=="LIVE").count(),"byContract":by_contract,"byStatus":by_status},
        "checkpoint":{"at":cp.as_ref().map(|c|c.at),"sink":cp.as_ref().map(|c|&c.sink),"virtualDaa":cp.as_ref().map(|c|&c.virtual_daa)},
        "contracts":e.contracts().describe(),"journal":e.store.db().journal(25)?,"errors":e.errors.load(std::sync::atomic::Ordering::SeqCst),"queued":e.queued(),"degraded":v.degraded.load(std::sync::atomic::Ordering::SeqCst)}),
    )
}

async fn probe(url: &str) -> Result<Value> {
    let start = Instant::now();
    let (rpc, _) = RpcClient::connect(url, Duration::from_secs(4)).await?;
    let rtt = start.elapsed().as_millis();
    let info = rpc.call_default("getServerInfo", json!({})).await?;
    let p2p = rpc
        .call_default("getInfo", json!({}))
        .await
        .ok()
        .map(|v| v["p2pId"].clone())
        .unwrap_or(Value::Null);
    let dag = rpc
        .call_default("getBlockDagInfo", json!({}))
        .await
        .unwrap_or(Value::Null);
    rpc.close();
    Ok(
        json!({"ok":true,"rttMs":rtt as u64,"info":{"p2pId":p2p,"serverVersion":info["serverVersion"],"networkId":info["networkId"],"isSynced":info["isSynced"],"hasUtxoIndex":info["hasUtxoIndex"],"virtualDaaScore":info.get("virtualDaaScore").map(|x|js_string(Some(x)))},"dag":dag}),
    )
}
async fn bounded_probe(url: &str) -> Value {
    match tokio::time::timeout(Duration::from_secs(5), probe(url)).await {
        Ok(Ok(v)) => v,
        Ok(Err(e)) => json!({"ok":false,"error":e.to_string()}),
        Err(_) => json!({"ok":false,"error":"PROBE_TIMEOUT"}),
    }
}
fn ws_url(s: &str) -> bool {
    url::Url::parse(s).is_ok_and(|u| {
        matches!(u.scheme(), "ws" | "wss")
            && u.host_str().is_some()
            && u.username().is_empty()
            && u.password().is_none()
    })
}

async fn admin(
    api: &Arc<Api>,
    v: &Arc<View>,
    sub: &str,
    method: &Method,
    q: &[(String, String)],
    body: &Value,
) -> Result<Response> {
    let e = &v.engine;
    match (method.as_str(), sub) {
        ("GET", "/status") => Ok(response(200, status(api, v)?)),
        ("GET", "/networks") => Ok(response(
            200,
            json!({"ok":true,"networks":api.views.iter().map(|v|catalog(v,true)).collect::<Result<Vec<_>>>()?}),
        )),
        ("GET", "/node-pool") => Ok(response(
            200,
            json!({"ok":true,"network":v.net().network_id,"networkKey":v.net().key,"seeds":v.net().seed,"members":e.node.status(),"active":e.node.identity(),"rest":{"url":e.config.rest_url,"enabled":e.config.rest_url.is_some()}}),
        )),
        ("GET", "/pool") => {
            let members = e.node.status();
            let standby = members
                .iter()
                .find(|m| m["up"] == true && m["active"] == false)
                .map(|m| m["url"].clone());
            Ok(response(
                200,
                json!({"ok":true,"network":v.net().network_id,"networkKey":v.net().key,"seeds":v.net().seed,"managed":e.store.db().pool_nodes()?,"connected":members.iter().filter(|m|m["up"]==true).map(|m|m["url"].clone()).collect::<Vec<_>>(),"active":e.node.url(),"activeUrl":e.node.url(),"standbyUrl":standby,"nodeStatus":members}),
            ))
        }
        ("GET", "/plugins") => {
            let raw = read_config(api)?;
            let plugins = e.contracts().describe();
            let mut cfg = raw["contracts"].as_array().cloned().unwrap_or_default();
            for (i, c) in cfg.iter_mut().enumerate() {
                if c["options"]["registryAddresses"]
                    .as_array()
                    .is_none_or(|a| a.is_empty())
                {
                    c["options"]["registryAddresses"] = plugins[i]["watchAddresses"].clone();
                }
            }
            Ok(response(
                200,
                json!({"ok":true,"contractsPath":api.contracts_path,"network":v.net().network_id,"networkKey":v.net().key,"networks":api.keys(),"plugins":plugins,"config":cfg}),
            ))
        }
        ("POST", "/test-node") => {
            let u = body["url"].as_str().unwrap_or("").trim();
            if !ws_url(u) {
                return Ok(response(400, json!({"ok":false,"error":"INVALID_URL"})));
            }
            Ok(response(200, bounded_probe(u).await))
        }
        ("POST", "/probe-rest") => {
            let Some(base) = &e.config.rest_url else {
                return Ok(response(
                    200,
                    json!({"ok":false,"enabled":false,"message":"REST acceleration is disabled"}),
                ));
            };
            let start = Instant::now();
            let result = async {
                let r = v
                    .pool
                    .http
                    .get(format!("{}/info/kaspad", base.trim_end_matches('/')))
                    .timeout(Duration::from_secs(4))
                    .send()
                    .await?
                    .error_for_status()?;
                r.json::<Value>().await
            };
            let mut out = match result.await {
                Ok(info) => {
                    let mut o = info;
                    if !o.is_object() {
                        o = json!({});
                    }
                    o["ok"] = json!(true);
                    o
                }
                Err(err) => json!({"ok":false,"error":err.to_string()}),
            };
            out["enabled"] = json!(true);
            out["restUrl"] = json!(base);
            out["latencyMs"] = json!(start.elapsed().as_millis() as u64);
            Ok(response(200, out))
        }
        ("POST", "/probe-pool") => {
            let managed = e.store.db().pool_nodes()?;
            let mut urls = Vec::new();
            for u in body["urls"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .chain(managed.iter().filter_map(|m| m["url"].as_str()))
                .chain(v.net().seed.iter().copied())
            {
                if ws_url(u) && !urls.contains(&u.to_string()) {
                    urls.push(u.to_string());
                }
            }
            for m in e.node.status() {
                if let Some(u) = m["url"].as_str() {
                    if !urls.contains(&u.to_string()) {
                        urls.push(u.to_string());
                    }
                }
            }
            if urls.len() > 64 {
                return Ok(response(
                    400,
                    json!({"ok":false,"error":"TOO_MANY_TARGETS"}),
                ));
            }
            let mut rows=futures_util::future::join_all(urls.iter().map(|u|async move {
                let p=bounded_probe(u).await;let i=&p["info"];
                json!({"url":u,"up":p["ok"]==true,"rttMs":p["rttMs"],"error":p["error"],"p2pId":i["p2pId"],"serverVersion":i["serverVersion"],"networkId":i["networkId"],"isSynced":i["isSynced"],"hasUtxoIndex":i["hasUtxoIndex"],"virtualDaaScore":i["virtualDaaScore"],"sink":p["dag"]["sink"]})
            })).await;
            let max = rows
                .iter()
                .filter(|r| r["up"] == true && r["networkId"] == v.net().network_id)
                .filter_map(|r| js_bigint(r.get("virtualDaaScore")).ok())
                .max()
                .unwrap_or(0);
            let ids: Vec<_> = rows.iter().map(|r| r["p2pId"].clone()).collect();
            for r in &mut rows {
                r["managed"] = json!(managed.iter().any(|m| m["url"] == r["url"]));
                r["sharedBackend"] = json!(
                    !r["p2pId"].is_null() && ids.iter().filter(|id| **id == r["p2pId"]).count() > 1
                );
                r["lag"] = json!(
                    (max - js_bigint(r.get("virtualDaaScore")).unwrap_or(max))
                        .max(0)
                        .to_string()
                );
            }
            rows.sort_by_key(|r| (r["up"] != true, r["rttMs"].as_u64().unwrap_or(u64::MAX)));
            Ok(response(
                200,
                json!({"ok":true,"network":v.net().network_id,"networkKey":v.net().key,"candidates":rows,"managed":managed.iter().map(|m|m["url"].clone()).collect::<Vec<_>>(),"maxDaa":max.to_string()}),
            ))
        }
        ("POST", "/pool/add") => {
            let u = body["url"].as_str().unwrap_or("").trim();
            if !ws_url(u) {
                return Ok(response(400, json!({"ok":false,"error":"INVALID_URL"})));
            }
            let opened =
                tokio::time::timeout(Duration::from_secs(12), (v.pool.open)(u.into())).await;
            let (node, _) = match opened {
                Ok(Ok(x)) => x,
                Ok(Err(err)) => {
                    return Ok(response(
                        400,
                        json!({"ok":false,"error":"PROBE_FAILED","detail":err.to_string()}),
                    ));
                }
                Err(_) => return Ok(response(400, json!({"ok":false,"error":"PROBE_TIMEOUT"}))),
            };
            let id = node.identity();
            node.close();
            let note = body["note"]
                .as_str()
                .map(|s| crate::util::slice_chars(s, 120));
            let list = e.store.db().add_pool_node(u, note.as_deref(), now_ms())?;
            Ok(response(
                200,
                json!({"ok":true,"managed":list,"verified":id}),
            ))
        }
        ("POST", "/pool/remove") => {
            let u = body["url"].as_str().unwrap_or("").trim();
            if u.is_empty() {
                return Ok(response(400, json!({"ok":false,"error":"URL_REQUIRED"})));
            }
            Ok(response(
                200,
                json!({"ok":true,"managed":e.store.db().remove_pool_node(u)?,"note":"已从候选池移除，当前连接保持"}),
            ))
        }
        ("POST", "/pool/activate") => {
            let _guard = api.mutations.lock().await;
            let u = body["url"].as_str().unwrap_or("").trim();
            if !ws_url(u) {
                return Ok(response(400, json!({"ok":false,"error":"INVALID_URL"})));
            }
            let status = e.node.promote(u).await?;
            e.run_reconcile();
            Ok(response(
                200,
                json!({"ok":true,"active":e.node.url(),"nodeStatus":status}),
            ))
        }
        ("POST", "/pool/elect") => {
            let _guard = api.mutations.lock().await;
            let mut r = v.pool.elect(Some(&e.node)).await?;
            r["ok"] = json!(true);
            r["active"] = json!(e.node.url());
            r["nodeStatus"] = json!(e.node.status());
            e.run_reconcile();
            Ok(response(200, r))
        }
        ("POST", "/track") => {
            match crate::tracer::track(e, body["txid"].as_str().unwrap_or("")).await {
                Ok(mut r) => {
                    r["ok"] = json!(true);
                    Ok(response(200, r))
                }
                Err(err) => Ok(response(
                    400,
                    json!({"ok":false,"error":"TRACK_FAILED","detail":err.to_string()}),
                )),
            }
        }
        ("POST", "/recheck-discoveries") => {
            let n = e.store.db().reset_ignored_discoveries()?;
            e.retry_pending()?;
            Ok(response(
                200,
                json!({"ok":true,"recheckedCount":n,"message":format!("已重置 {n} 条任务，后台重新核验")}),
            ))
        }
        ("POST", "/compile") => {
            let _guard = api.mutations.lock().await;
            match crate::compiler::compile(body).await {
                Ok(r) => Ok(response(if r["ok"] == true { 200 } else { 400 }, r)),
                Err(err) => Ok(response(
                    400,
                    json!({"ok":false,"error":"COMPILATION_ERROR","detail":err.to_string()}),
                )),
            }
        }
        ("POST", "/generate-plugin" | "/install-plugin") => Ok(response(
            422,
            json!({"ok":false,"error":"NATIVE_PLUGINS_ONLY","detail":"Rust 版仅运行固定的 Kaswin V2.0.2 原生规则，不生成或安装 JavaScript 插件。SilverScript 编译可独立使用。"}),
        )),
        ("POST", "/plugins/toggle" | "/plugins/update-config" | "/plugins/remove") => {
            update_config(api, sub, body).await
        }
        ("GET", "/preset-contract") => {
            let _name = first(q, "name");
            Ok(response(
                404,
                json!({"ok":false,"error":"PRESET_NOT_FOUND","detail":"本版本不提供旧版 V2.0.1 预设"}),
            ))
        }
        _ => Ok(response(404, json!({"ok":false,"error":"NOT_FOUND"}))),
    }
}
fn read_config(api: &Api) -> Result<Value> {
    let s = match &api.contracts_path {
        Some(p) => std::fs::read_to_string(p)?,
        None => DEFAULT_CONTRACTS_JSON.into(),
    };
    Ok(serde_json::from_str(&s)?)
}
async fn update_config(api: &Arc<Api>, sub: &str, body: &Value) -> Result<Response> {
    let _guard = api.mutations.lock().await;
    let Some(path) = &api.contracts_path else {
        return Ok(response(
            400,
            json!({"ok":false,"error":"CONTRACTS_FILE_NOT_FOUND"}),
        ));
    };
    let mut raw = read_config(api)?;
    let Some(index) = body["index"].as_u64().and_then(|i| usize::try_from(i).ok()) else {
        return Ok(response(
            400,
            json!({"ok":false,"error":"INVALID_PLUGIN_INDEX"}),
        ));
    };
    let Some(list) = raw["contracts"].as_array_mut() else {
        bail!("CONTRACTS_CONFIG_EMPTY");
    };
    if index >= list.len() {
        return Ok(response(
            400,
            json!({"ok":false,"error":"INVALID_PLUGIN_INDEX"}),
        ));
    }
    if sub.ends_with("remove") {
        list.remove(index);
    } else {
        if sub.ends_with("toggle") || body.get("retired").is_some() {
            list[index]["retired"] = json!(crate::util::truthy(body.get("retired")));
        }
        if let Some(o) = body["options"].as_object() {
            list[index]["options"] = crate::util::spread(&list[index]["options"], json!(o));
        }
    }
    let text = serde_json::to_string_pretty(&raw)? + "\n";
    // Validate every network before touching disk. A shared config must never break the other network's registry.
    let mut registries = vec![];
    for v in &api.views {
        let registry = Arc::new(load_contracts_json(
            &text,
            &LoadCtx {
                network: v.net().key,
                registry_addresses: None,
            },
        )?);
        for r in v.engine.store.db().rounds(true, false)? {
            if !registry.has(r.contract.as_deref().unwrap_or("")) {
                return Ok(response(
                    409,
                    json!({"ok":false,"error":"LIVE_ROUND_CONTRACT_NOT_LOADED","round":r.id}),
                ));
            }
        }
        registries.push(registry);
    }
    // Stage subscriptions through each engine queue before committing the shared configuration.
    for (v, c) in api.views.iter().zip(&registries) {
        let c = c.clone();
        let (tx, rx) = tokio::sync::oneshot::channel();
        v.engine.run("stage-contracts".into(), move |e| async move {
            let result: Result<()> = async {
                for a in c.watch_addresses() {
                    if !e.subscriptions().contains(&a) {
                        e.subscribe(Some(&a)).await?;
                    }
                }
                Ok(())
            }
            .await;
            let _ = tx.send(result);
            Ok(())
        });
        rx.await??;
    }
    let parent = std::path::Path::new(path)
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(std::path::Path::new("."));
    let mut tmp = tempfile::NamedTempFile::new_in(parent)?;
    use std::io::Write;
    tmp.write_all(text.as_bytes())?;
    tmp.as_file().sync_all()?;
    tmp.persist(path)?;
    for (v, c) in api.views.iter().zip(registries) {
        let (tx, rx) = tokio::sync::oneshot::channel();
        v.engine
            .run("reload-contracts".into(), move |e| async move {
                e.set_contracts(c);
                let _ = tx.send(());
                Ok(())
            });
        rx.await?;
    }
    Ok(response(
        200,
        json!({"ok":true,"message":"配置已验证并加载","config":raw["contracts"],"plugins":api.views[0].engine.contracts().describe()}),
    ))
}

/// Rolling v1 public read endpoints (`/info`, `/rounds`, `/rounds/{cid}`, `/rounds/{cid}/history`, `/rounds/{cid}/export`, `/txid/{txid}`).
fn rolling_api(
    api: &Api,
    view: &View,
    profile_id: &str,
    rest_ref: &[&str],
    query: &[(String, String)],
    virtual_daa: Option<&str>,
    cp: Option<&crate::store::Checkpoint>,
) -> Result<Response> {
    if !view.engine.contracts().has_rolling()
        || profile_id != crate::contracts::rolling_v1::PROFILE_ID
    {
        return Ok(response(404, json!({"detail":"NOT_FOUND"})));
    }
    let db = view.engine.store.db();
    let is_rolling = |r: &RoundRow| {
        r.contract
            .as_deref()
            .is_some_and(|s| s.starts_with("rolling-v1@"))
            && r.status != "STALE"
            && r.status != "ROLLED_BACK"
    };
    let is_kusdt_v2 = |r: &RoundRow| {
        r.contract
            .as_deref()
            .is_some_and(|s| s.starts_with("kusdt-v2@"))
            && r.status != "STALE"
            && r.status != "ROLLED_BACK"
    };
    let proto_filter = first(query, "protocol");
    let pid_filter = first(query, "profileId");
    match rest_ref {
        ["info"] => {
            let rounds: Vec<_> = db
                .rounds(false, false)?
                .into_iter()
                .filter(|r| {
                    let match_proto = match proto_filter {
                        Some("kusdt-v2") => is_kusdt_v2(r),
                        Some("rolling-v1") => is_rolling(r),
                        _ => is_rolling(r) || is_kusdt_v2(r),
                    };
                    let match_pid = pid_filter
                        .map(|p| r.data.get("profileId").and_then(|v| v.as_str()) == Some(p))
                        .unwrap_or(true);
                    match_proto && match_pid
                })
                .collect();
            let revision = rounds.iter().map(|r| r.updated_at).max().unwrap_or(0);
            let cursor = cp
                .map(|c| {
                    json!({
                        "blockHash": c.sink,
                        "daa": c.virtual_daa,
                        "blueScore": c.sink_blue_score,
                        "virtualDaa": c.virtual_daa,
                    })
                })
                .unwrap_or(Value::Null);
            let profiles: Vec<Value> = view
                .engine
                .contracts()
                .describe()
                .as_array()
                .into_iter()
                .flatten()
                .filter(|c| !c.get("retired").and_then(|v| v.as_bool()).unwrap_or(false))
                .filter_map(|c| {
                    let protocol_id = c.get("module").and_then(|v| v.as_str())?;
                    let pid = c.get("profileId").and_then(|v| v.as_str())?;
                    Some(json!({
                        "protocolId": protocol_id,
                        "profileId": pid,
                    }))
                })
                .collect();
            Ok(response(
                200,
                json!({
                    "schema": 1,
                    "network": view.net().network_id,
                    "genesisHash": view.net().genesis,
                    "profileId": profile_id,
                    "profiles": profiles,
                    "status": "UNVALIDATED",
                    "readOnly": true,
                    "stale": false,
                    "cursor": cursor,
                    "revision": revision.to_string(),
                    "roundsCount": rounds.len(),
                }),
            ))
        }
        ["rounds"] => {
            let limit = first(query, "limit")
                .unwrap_or("30")
                .parse::<usize>()
                .unwrap_or(30)
                .clamp(1, 100);
            let cursor = first(query, "cursor");
            let mut rounds: Vec<_> = db
                .rounds(false, false)?
                .into_iter()
                .filter(|r| {
                    let match_proto = match proto_filter {
                        Some("kusdt-v2") => is_kusdt_v2(r),
                        Some("rolling-v1") => is_rolling(r),
                        _ => is_rolling(r) || is_kusdt_v2(r),
                    };
                    let match_pid = pid_filter
                        .map(|p| r.data.get("profileId").and_then(|v| v.as_str()) == Some(p))
                        .unwrap_or(true);
                    match_proto && match_pid
                })
                .collect();
            rounds.sort_by(|a, b| {
                let a_term = !a.data["terminal"].is_null();
                let b_term = !b.data["terminal"].is_null();
                a_term
                    .cmp(&b_term)
                    .then_with(|| b.updated_at.cmp(&a.updated_at))
                    .then_with(|| {
                        a.data["cid"]
                            .as_str()
                            .unwrap_or("")
                            .cmp(b.data["cid"].as_str().unwrap_or(""))
                    })
            });
            let start = if let Some(c) = cursor {
                rounds
                    .iter()
                    .position(|r| r.data["cid"].as_str() == Some(c))
                    .map(|i| i + 1)
                    .unwrap_or(rounds.len())
            } else {
                0
            };
            let slice: Vec<_> = rounds.iter().skip(start).take(limit).collect();
            let next_cursor = if start + slice.len() < rounds.len() {
                slice
                    .last()
                    .map(|r| r.data["cid"].clone())
                    .unwrap_or(Value::Null)
            } else {
                Value::Null
            };
            let items: Vec<Value> = slice
                .iter()
                .map(|r| {
                    let d = &r.data;
                    if is_kusdt_v2(r) {
                        let summary =
                            crate::contracts::kusdt_v2::build_summary_fields(d, virtual_daa);
                        let accepting_daa = d
                            .get("acceptingDaa")
                            .filter(|v| !v.is_null())
                            .cloned()
                            .unwrap_or_else(|| d["utxoDaa"].clone());
                        let mut item = json!({
                            "cid": d["cid"],
                            "terminal": d["terminal"],
                            "stage": d["stage"],
                            "managerValue": d["value"],
                            "acceptingDaa": accepting_daa,
                            "updatedAt": r.updated_at,
                        });
                        if let (Some(dst), Some(src)) = (item.as_object_mut(), summary.as_object())
                        {
                            for (k, v) in src {
                                dst.insert(k.clone(), v.clone());
                            }
                        }
                        return item;
                    }
                    let s = &d["state"];
                    let indices = s["indices"].as_array();
                    let shards = indices.map(|a| a.len()).unwrap_or(0);
                    let base_tickets = indices
                        .and_then(|a| a.last())
                        .and_then(|x| x["end"].as_i64())
                        .unwrap_or(0);
                    let local_tickets = s["records"]
                        .as_array()
                        .and_then(|a| a.last())
                        .and_then(|x| x["end"].as_i64())
                        .unwrap_or(0);
                    let tickets = base_tickets + local_tickets;
                    let round_profile_id = d
                        .get("profileId")
                        .and_then(|v| v.as_str())
                        .unwrap_or(profile_id);
                    let ticket_price = rolling_ticket_price(d);
                    let (state_deposit, frozen_value, pool_value, frozen_count) =
                        rolling_pool_breakdown(d);
                    json!({
                        "cid": d["cid"],
                        "terminal": d["terminal"],
                        "module": d["module"],
                        "protocolId": "rolling-v1",
                        "profileId": round_profile_id,
                        "tickets": tickets,
                        "shards": shards,
                        "bought": d["bought"],
                        "buys": d["buys"],
                        "ticketPrice": ticket_price,
                        "managerValue": d["value"],
                        "stateDeposit": state_deposit.to_string(),
                        "frozenValue": frozen_value.to_string(),
                        "poolValue": pool_value.to_string(),
                        "frozenCount": frozen_count,
                        "acceptingDaa": d["utxoDaa"],
                        "updatedAt": r.updated_at,
                    })
                })
                .collect();
            Ok(response(
                200,
                json!({
                    "schema": 1,
                    "network": view.net().network_id,
                    "genesisHash": view.net().genesis,
                    "profileId": profile_id,
                    "rounds": items,
                    "nextCursor": next_cursor,
                }),
            ))
        }
        ["rounds", cid] => {
            if !is_hash(cid) {
                return Ok(response(400, json!({"detail":"INVALID_CID"})));
            }
            let rows = db.by_cid(cid)?;
            let Some(r) = rows.into_iter().find(|r| is_rolling(r) || is_kusdt_v2(r)) else {
                return Ok(response(404, json!({"detail":"NOT_FOUND"})));
            };
            let d = &r.data;
            if is_kusdt_v2(&r) {
                return Ok(response(
                    200,
                    build_kusdt_v2_round_response(
                        &db,
                        &r,
                        cid,
                        &view.net().network_id,
                        &view.net().genesis,
                        virtual_daa,
                    )?,
                ));
            }
            let s = &d["state"];
            let settings = &d["settings"];
            let indices = s["indices"].as_array();
            let shards = indices.map(|a| a.len()).unwrap_or(0);
            let base_tickets = indices
                .and_then(|a| a.last())
                .and_then(|x| x["end"].as_i64())
                .unwrap_or(0);
            let local_records = s["records"].as_array().map(|a| a.len()).unwrap_or(0);
            let local_tickets = s["records"]
                .as_array()
                .and_then(|a| a.last())
                .and_then(|x| x["end"].as_i64())
                .unwrap_or(0);
            let tickets = base_tickets + local_tickets;
            let frozen = d.get("frozen").cloned().unwrap_or_else(|| json!([]));
            let (state_deposit, frozen_value, pool_value, frozen_count) =
                rolling_pool_breakdown(d);
            let ticket_price = rolling_ticket_price(d);
            let revision = db.transition_count(&r.id)?;
            let round_profile_id = d
                .get("profileId")
                .and_then(|v| v.as_str())
                .unwrap_or(profile_id);
            Ok(response(
                200,
                json!({
                    "schema": 1,
                    "network": view.net().network_id,
                    "genesisHash": view.net().genesis,
                    "profileId": round_profile_id,
                    "status": "UNVALIDATED",
                    "cid": cid,
                    "origin": d.get("origin").cloned().unwrap_or(Value::Null),
                    "revision": revision.to_string(),
                    "terminal": d.get("terminal").cloned().unwrap_or(Value::Null),
                    "manager": {
                        "outpoint": d.get("tip").cloned().unwrap_or(Value::Null),
                        "scriptHash": d["spk"].get("script").cloned().unwrap_or(Value::Null),
                        "module": d.get("module").cloned().unwrap_or(Value::Null),
                        "value": d.get("value").cloned().unwrap_or(Value::Null),
                        "utxoDaa": d.get("utxoDaa").cloned().unwrap_or(Value::Null),
                    },
                    "frozen": frozen,
                    "summary": {
                        "module": d.get("module").cloned().unwrap_or(Value::Null),
                        "ticketPrice": ticket_price,
                        "ticketCap": settings.get("ticketCap").cloned().unwrap_or(Value::Null),
                        "minTickets": settings.get("minTickets").cloned().unwrap_or(Value::Null),
                        "closeEligibleDaa": settings.get("closeEligibleDaa").cloned().unwrap_or(Value::Null),
                        "tickets": tickets,
                        "shards": shards,
                        "localRecords": local_records,
                        "frozenCount": frozen_count,
                        "stateDeposit": state_deposit.to_string(),
                        "frozenValue": frozen_value.to_string(),
                        "poolValue": pool_value.to_string(),
                        "step": d.get("step").cloned().unwrap_or(Value::Null),
                        "buys": d.get("buys").cloned().unwrap_or(Value::Null),
                        "bought": d.get("bought").cloned().unwrap_or(Value::Null),
                        "fees": d.get("fees").cloned().unwrap_or(Value::Null),
                        "winnerKey": s.get("winnerKey").cloned().unwrap_or(Value::Null),
                        "payCursor": s.get("payCursor").cloned().unwrap_or(Value::Null),
                        "refundShard": s.get("refundShard").cloned().unwrap_or(Value::Null),
                        "refundCursor": s.get("refundCursor").cloned().unwrap_or(Value::Null),
                    },
                    "head": {
                        "txid": d.get("latestTxid").cloned().unwrap_or(Value::Null),
                        "acceptingBlock": d.get("accepting").cloned().unwrap_or(Value::Null),
                        "acceptingDaa": d.get("utxoDaa").cloned().unwrap_or(Value::Null),
                        "observedVirtualDaa": virtual_daa,
                    },
                }),
            ))
        }
        ["rounds", cid, "history"] => {
            if !is_hash(cid) {
                return Ok(response(400, json!({"detail":"INVALID_CID"})));
            }
            let rows = db.by_cid(cid)?;
            let Some(r) = rows.into_iter().find(|r| is_rolling(r) || is_kusdt_v2(r)) else {
                return Ok(response(404, json!({"detail":"NOT_FOUND"})));
            };
            let effective_pid = r
                .data
                .get("profileId")
                .and_then(|v| v.as_str())
                .unwrap_or(profile_id);
            let limit = first(query, "limit")
                .unwrap_or("50")
                .parse::<usize>()
                .unwrap_or(50)
                .clamp(1, 200);
            let offset = first(query, "cursor")
                .and_then(|c| c.parse::<usize>().ok())
                .unwrap_or(0);
            let revision = db.transition_count(&r.id)?;
            let rows = db.transition_events(&r.id, limit as i64, offset as i64)?;
            let events: Vec<Value> = rows
                .iter()
                .map(|ev| {
                    json!({
                        "type": if ev.status == "ROLLED_BACK" { "ROLLBACK" } else { "ACCEPT" },
                        "txid": ev.txid,
                        "position": {
                            "acceptingBlockHash": ev.accepting,
                            "acceptingDaa": ev.accepting_daa,
                        },
                        "action": ev.kind,
                        "status": ev.status,
                        "observedAt": ev.observed_at,
                        "revision": ev.seq.to_string(),
                    })
                })
                .collect();
            let next_offset = offset + events.len();
            let next = if (next_offset as i64) < revision {
                json!(next_offset.to_string())
            } else {
                Value::Null
            };
            Ok(response(
                200,
                json!({
                    "schema": 1,
                    "network": view.net().network_id,
                    "genesisHash": view.net().genesis,
                    "profileId": effective_pid,
                    "cid": cid,
                    "revision": revision.to_string(),
                    "events": events,
                    "next": next,
                    "nextCursor": next,
                }),
            ))
        }
        ["rounds", cid, "export"] => {
            if !is_hash(cid) {
                return Ok(response(400, json!({"detail":"INVALID_CID"})));
            }
            let rows = db.by_cid(cid)?;
            let Some(mut r) = rows.into_iter().find(is_rolling) else {
                return Ok(response(404, json!({"detail":"NOT_FOUND"})));
            };
            let revision = db.transition_count(&r.id)?;
            let head = json!({
                "txid": r.data["latestTxid"],
                "acceptingBlock": r.data["accepting"],
                "acceptingDaa": r.data["utxoDaa"],
            });

            let since_txid = query
                .iter()
                .find(|(k, _)| k == "sinceTxid")
                .map(|(_, v)| v.as_str())
                .filter(|s| is_hash(s));
            let since_step_raw = query
                .iter()
                .find(|(k, _)| k == "sinceStep")
                .map(|(_, v)| v.as_str());
            let since_step = since_step_raw.and_then(|s| s.parse::<u64>().ok());
            let step_param_ok = since_step_raw.is_none() || since_step.is_some();

            if let Some(stx) = since_txid
                && step_param_ok
            {
                let current_step = r.data.get("step").and_then(|v| v.as_u64()).unwrap_or(0);
                if r.data.get("latestTxid").and_then(|v| v.as_str()) == Some(stx)
                    && since_step.is_none_or(|ss| ss == current_step)
                {
                    return Ok(response(
                        200,
                        json!({
                            "schema": 1,
                            "network": view.net().network_id,
                            "genesisHash": view.net().genesis,
                            "profileId": profile_id,
                            "cid": cid,
                            "mode": "unchanged",
                            "revision": revision.to_string(),
                            "head": head,
                        }),
                    ));
                }

                if let Some(anchor_tr) = db.active_transition(&r.id, stx)? {
                    let receipt_seq = if db
                        .round_receipt_txid_at(&r.id, anchor_tr.seq)?
                        .as_deref()
                        == Some(stx)
                    {
                        Some(anchor_tr.seq)
                    } else if anchor_tr.seq > 0
                        && db
                            .round_receipt_txid_at(&r.id, anchor_tr.seq - 1)?
                            .as_deref()
                            == Some(stx)
                    {
                        Some(anchor_tr.seq - 1)
                    } else {
                        None
                    };

                    if let Some(rseq) = receipt_seq {
                        let anchor_count = db.round_receipt_count_up_to(&r.id, rseq)?;
                        if anchor_count >= 0
                            && (anchor_count as u64) < current_step
                            && since_step.is_none_or(|ss| ss == anchor_count as u64)
                        {
                            let new_receipts = db.round_receipts_after(&r.id, rseq)?;
                            let new_preimages =
                                db.round_preimages_after(&r.id, anchor_tr.seq)?;
                            let delta = crate::contracts::rolling_v1::export_round_delta(
                                &r.data,
                                virtual_daa,
                                new_receipts,
                                new_preimages,
                            );
                            return Ok(response(
                                200,
                                json!({
                                    "schema": 1,
                                    "network": view.net().network_id,
                                    "genesisHash": view.net().genesis,
                                    "profileId": profile_id,
                                    "cid": cid,
                                    "mode": "delta",
                                    "revision": revision.to_string(),
                                    "head": head,
                                    "base": {
                                        "txid": stx,
                                        "step": anchor_count,
                                    },
                                    "delta": delta,
                                }),
                            ));
                        }
                    }
                } else if since_step.is_some_and(|ss| ss > current_step)
                    && !db.has_rolled_back_transition(&r.id, stx)?
                {
                    return Ok(response(
                        200,
                        json!({
                            "schema": 1,
                            "network": view.net().network_id,
                            "genesisHash": view.net().genesis,
                            "profileId": profile_id,
                            "cid": cid,
                            "mode": "ahead",
                            "revision": revision.to_string(),
                            "head": head,
                            "step": current_step,
                        }),
                    ));
                }
            }

            let cache_key = (view.net().key.to_string(), cid.to_string(), revision);
            if let Some((k, bytes)) = api.export_cache.lock().unwrap().as_ref()
                && k == &cache_key
            {
                return Ok((
                    StatusCode::OK,
                    [("content-type", "application/json; charset=utf-8")],
                    bytes.clone(),
                )
                    .into_response());
            }
            if let Some(obj) = r.data.as_object_mut() {
                if !obj.contains_key("receipts") {
                    obj.insert("receipts".into(), Value::Array(db.round_receipts(&r.id)?));
                }
                if !obj.contains_key("preimages") {
                    obj.insert("preimages".into(), Value::Object(db.round_preimages(&r.id)?));
                }
            }
            let (round_text, historical_shards) =
                crate::contracts::rolling_v1::export_round_text_for_network_owned(
                    &mut r.data,
                    virtual_daa,
                    view.net().network_id,
                )?;
            let body = json!({
                "schema": 1,
                "network": view.net().network_id,
                "genesisHash": view.net().genesis,
                "profileId": profile_id,
                "cid": cid,
                "mode": "full",
                "revision": revision.to_string(),
                "head": head,
                "roundText": round_text,
                "historicalShards": historical_shards,
            });
            let bytes = axum::body::Bytes::from(serde_json::to_vec(&body)?);
            *api.export_cache.lock().unwrap() = Some((cache_key, bytes.clone()));
            Ok((
                StatusCode::OK,
                [("content-type", "application/json; charset=utf-8")],
                bytes,
            )
                .into_response())
        }
        ["txid", txid] => {
            if !is_hash(txid) {
                return Ok(response(400, json!({"detail":"INVALID_TXID"})));
            }
            let Some(tr) = db.any_active_transition_by_txid(txid)? else {
                return Ok(response(404, json!({"detail":"NOT_FOUND"})));
            };
            let Some(r) = db.round(&tr.round_id)? else {
                return Ok(response(404, json!({"detail":"NOT_FOUND"})));
            };
            if !is_rolling(&r) && !is_kusdt_v2(&r) {
                return Ok(response(404, json!({"detail":"NOT_FOUND"})));
            }
            let effective_pid = r
                .data
                .get("profileId")
                .and_then(|v| v.as_str())
                .unwrap_or(profile_id);
            Ok(response(
                200,
                json!({
                    "schema": 1,
                    "network": view.net().network_id,
                    "genesisHash": view.net().genesis,
                    "profileId": effective_pid,
                    "cid": r.data["cid"],
                    "txid": txid,
                }),
            ))
        }
        _ => Ok(response(404, json!({"detail":"NOT_FOUND"}))),
    }
}

/// Wallet activity aggregation across kUSDT v2 rounds (`buyer`, `creator`, `firstBuyerDeposit`).
fn kusdt_v2_wallet_activity(
    view: &View,
    pk: &str,
    query: &[(String, String)],
    virtual_daa: Option<&str>,
) -> Result<Response> {
    if !is_hash(pk) {
        return Ok(response(400, json!({"detail":"INVALID_PK"})));
    }
    let pid_filter = first(query, "profileId");
    let db = view.engine.store.db();
    let rounds: Vec<_> = db
        .rounds(false, false)?
        .into_iter()
        .filter(|r| {
            r.contract
                .as_deref()
                .is_some_and(|s| s.starts_with("kusdt-v2@"))
                && r.status != "STALE"
                && r.status != "ROLLED_BACK"
                && pid_filter
                    .map(|p| r.data.get("profileId").and_then(|v| v.as_str()) == Some(p))
                    .unwrap_or(true)
        })
        .collect();

    let mut activities = Vec::new();
    let mut by_profile: serde_json::Map<String, Value> = serde_json::Map::new();
    for r in rounds {
        let roles = crate::contracts::kusdt_v2::wallet_roles_in_round(&r.data, pk);
        if roles.is_empty() {
            continue;
        }
        let summary = crate::contracts::kusdt_v2::build_summary_fields(&r.data, virtual_daa);
        let pid = summary
            .get("profileId")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let entry = json!({
            "cid": r.data["cid"],
            "profileId": pid,
            "roles": roles,
            "phase": summary["phase"],
            "subPhase": summary["subPhase"],
            "terminal": summary["terminal"],
            "asset": summary["asset"],
            "settlementProgress": summary["settlementProgress"],
            "protocolData": summary["protocolData"],
            "updatedAt": r.updated_at,
        });
        by_profile
            .entry(pid)
            .or_insert_with(|| json!([]))
            .as_array_mut()
            .unwrap()
            .push(entry.clone());
        activities.push(entry);
    }

    Ok(response(
        200,
        json!({
            "schema": 1,
            "network": view.net().network_id,
            "genesisHash": view.net().genesis,
            "pk": pk,
            "activities": activities,
            "byProfile": by_profile,
        }),
    ))
}

/// Live UTXO + redeemHex lookup by `covenantId` (manager `cid` or shard `shardCid`).
fn kusdt_v2_covenant_lookup(view: &View, cid: &str) -> Result<Response> {
    if !is_hash(cid) {
        return Ok(response(400, json!({"detail":"INVALID_CID"})));
    }
    let db = view.engine.store.db();
    let prefix = view.net().address_prefix;
    let is_kv2 = |r: &RoundRow| {
        r.contract
            .as_deref()
            .is_some_and(|s| s.starts_with("kusdt-v2@"))
            && r.status != "STALE"
            && r.status != "ROLLED_BACK"
    };
    for r in db.by_cid(cid)? {
        if is_kv2(&r)
            && let Some(found) = crate::contracts::kusdt_v2::find_live_covenant(&r.data, cid, prefix)
        {
            return Ok(response(
                200,
                json!({
                    "schema": 1,
                    "network": view.net().network_id,
                    "genesisHash": view.net().genesis,
                    "covenant": found,
                }),
            ));
        }
    }
    for r in db.rounds(true, false)? {
        if is_kv2(&r)
            && let Some(found) = crate::contracts::kusdt_v2::find_live_covenant(&r.data, cid, prefix)
        {
            return Ok(response(
                200,
                json!({
                    "schema": 1,
                    "network": view.net().network_id,
                    "genesisHash": view.net().genesis,
                    "covenant": found,
                }),
            ));
        }
    }
    Ok(response(404, json!({"detail":"NOT_FOUND"})))
}

fn build_kusdt_v2_round_response(
    db: &crate::store::Db,
    r: &RoundRow,
    cid: &str,
    network_id: &str,
    genesis: &str,
    virtual_daa: Option<&str>,
) -> Result<Value> {
    let d = &r.data;
    let revision = db.transition_count(&r.id)?;
    let raw_receipts = db.round_receipts(&r.id)?;
    let need_enrich = raw_receipts.is_empty()
        || raw_receipts
            .iter()
            .any(crate::contracts::kusdt_v2::receipt_needs_enrichment);
    let receipts = if need_enrich {
        let trs = db.transitions(&r.id)?;
        crate::contracts::kusdt_v2::enrich_kusdt_v2_receipts(d, raw_receipts, &trs)
    } else {
        raw_receipts
    };
    let head_accepting_daa = d
        .get("acceptingDaa")
        .filter(|v| !v.is_null())
        .cloned()
        .or_else(|| {
            receipts
                .last()
                .and_then(|rec| rec.get("acceptingDaa"))
                .filter(|v| !v.is_null())
                .cloned()
        })
        .unwrap_or_else(|| d.get("utxoDaa").cloned().unwrap_or(Value::Null));
    let summary = crate::contracts::kusdt_v2::build_summary_fields(d, virtual_daa);
    let mut out = json!({
        "schema": 1,
        "network": network_id,
        "genesisHash": genesis,
        "status": "UNVALIDATED",
        "cid": cid,
        "origin": d.get("origin").cloned().unwrap_or(Value::Null),
        "revision": revision.to_string(),
        "terminal": d.get("terminal").cloned().unwrap_or(Value::Null),
        "manager": d.get("manager").cloned().unwrap_or(Value::Null),
        "shards": d.get("shards").cloned().unwrap_or_else(|| json!({})),
        "draw": d.get("draw").cloned().unwrap_or(Value::Null),
        "receipts": receipts,
        "summary": summary,
        "head": {
            "txid": d.get("latestTxid").cloned().unwrap_or(Value::Null),
            "acceptingBlock": d.get("accepting").cloned().unwrap_or(Value::Null),
            "acceptingDaa": head_accepting_daa,
            "observedVirtualDaa": virtual_daa,
        },
    });
    if let (Some(dst), Some(src)) = (out.as_object_mut(), summary.as_object()) {
        for (k, v) in src {
            dst.insert(k.clone(), v.clone());
        }
    }
    Ok(out)
}

#[cfg(test)]
mod kusdt_v2_api_tests {
    use super::*;
    use crate::store::Store;

    #[test]
    fn round_response_exposes_draw_and_receipts() {
        let store = Store::open(":memory:", None, "mainnet").unwrap();
        let db = store.db();
        let cid = "90".repeat(32);
        let draw_txid = "dd".repeat(32);
        let winner_pk = "33".repeat(32);
        let row = RoundRow {
            id: "aa".repeat(32),
            contract: Some("kusdt-v2@1.0.0".into()),
            status: "LIVE".into(),
            address: None,
            data: json!({
                "cid": cid,
                "terminal": null,
                "winnerPk": winner_pk,
                "draw": {
                    "txid": draw_txid,
                    "shard": 0,
                    "record": 1,
                    "ticket": 2,
                    "winnerPk": winner_pk,
                },
                "shards": {},
                "settings": {
                    "kusdtCid": "8f".repeat(32),
                    "creatorPk": "11".repeat(32),
                    "ticketPrice": "1000000",
                    "closeDaa": "900",
                },
            }),
            updated_at: 1,
            live_seen_at: Some(1),
        };
        let out = build_kusdt_v2_round_response(
            &db,
            &row,
            &cid,
            "mainnet",
            "58c2d4199e21f910d1571d114969cecef48f09f934d42ccb6a281a15868f2999",
            Some("1000"),
        )
        .unwrap();
        assert_eq!(out["draw"]["txid"], draw_txid);
        assert_eq!(out["draw"]["shard"], 0);
        assert_eq!(out["draw"]["record"], 1);
        assert_eq!(out["draw"]["ticket"], 2);
        assert_eq!(out["draw"]["winnerPk"], winner_pk);
        assert!(out["receipts"].is_array());

        // Historical fallback: utxoDaa stale, no acceptingDaa -> falls back to latest receipt acceptingDaa.
        drop(db);
        let id_hist = "ab".repeat(32);
        store
            .tx(|wdb| {
                wdb.add_round_receipt(
                    &id_hist,
                    1,
                    &json!({"txid": "01".repeat(32), "action": "DISPATCH_BATCH", "acceptingDaa": "600632354"}),
                )?;
                wdb.add_round_receipt(
                    &id_hist,
                    2,
                    &json!({"txid": "02".repeat(32), "action": "PAYOUT_SELF", "acceptingDaa": "600633213"}),
                )
            })
            .unwrap();
        let db = store.db();
        let hist_row = RoundRow {
            id: id_hist,
            contract: Some("kusdt-v2@1.0.0".into()),
            status: "PAID".into(),
            address: None,
            data: json!({
                "cid": cid,
                "terminal": "PAID",
                "utxoDaa": "600632354",
                "latestTxid": "02".repeat(32),
                "shards": {},
            }),
            updated_at: 2,
            live_seen_at: None,
        };
        let out_hist = build_kusdt_v2_round_response(
            &db,
            &hist_row,
            &cid,
            "mainnet",
            "58c2d4199e21f910d1571d114969cecef48f09f934d42ccb6a281a15868f2999",
            Some("600633250"),
        )
        .unwrap();
        assert_eq!(out_hist["head"]["acceptingDaa"], "600633213");
    }

    #[test]
    fn rolling_pool_breakdown_separates_deposit_and_includes_frozen_shards() {
        let open_tail_only = json!({
            "terminal": null,
            "value": "9400000000",
            "settings": {"ticketPrice": "100000000"},
            "frozen": []
        });
        assert_eq!(rolling_ticket_price(&open_tail_only), json!("100000000"));
        assert_eq!(
            rolling_pool_breakdown(&open_tail_only),
            (400_000_000, 0, 9_000_000_000, 0)
        );

        let open_with_frozen = json!({
            "terminal": null,
            "value": "9400000000",
            "state": {"ticketPrice": "100000000"},
            "frozen": [
                {"value": "50000000000"},
                {"value": "25000000000"}
            ]
        });
        assert_eq!(rolling_ticket_price(&open_with_frozen), json!("100000000"));
        assert_eq!(
            rolling_pool_breakdown(&open_with_frozen),
            (400_000_000, 75_000_000_000, 84_000_000_000, 2)
        );

        let closed = json!({
            "terminal": "PAID",
            "value": "0",
            "settings": {"ticketPrice": "100000000"},
            "frozen": []
        });
        assert_eq!(rolling_pool_breakdown(&closed), (0, 0, 0, 0));
    }
}

fn parse_sompi_i128(v: Option<&Value>) -> i128 {
    match v {
        Some(Value::String(s)) => s.parse::<i128>().unwrap_or(0),
        Some(Value::Number(n)) => n.as_i64().map(i128::from).unwrap_or(0),
        _ => 0,
    }
}

fn rolling_ticket_price(d: &Value) -> Value {
    d.get("settings")
        .and_then(|st| st.get("ticketPrice"))
        .filter(|v| !v.is_null())
        .or_else(|| {
            d.get("state")
                .and_then(|st| st.get("ticketPrice"))
                .filter(|v| !v.is_null())
        })
        .cloned()
        .unwrap_or(Value::Null)
}

/// Compute `(state_deposit, frozen_value, pool_value, frozen_count)` in sompi for a `rolling-v1` round.
/// `pool_value` excludes the refundable manager state deposit (`DEPOSIT = 400_000_000`) and includes all unspent frozen shards.
fn rolling_pool_breakdown(d: &Value) -> (i128, i128, i128, usize) {
    let deposit = crate::core::rolling::DEPOSIT;
    let terminal = d.get("terminal").is_some_and(|v| !v.is_null());
    let manager_val = parse_sompi_i128(d.get("value"));
    let (state_deposit, tail_pool) = if !terminal && manager_val >= deposit {
        (deposit, manager_val - deposit)
    } else {
        (0, 0)
    };
    let frozen_arr = d.get("frozen").and_then(|v| v.as_array());
    let frozen_count = frozen_arr.map(|a| a.len()).unwrap_or(0);
    let frozen_value: i128 = frozen_arr
        .map(|arr| arr.iter().map(|f| parse_sompi_i128(f.get("value"))).sum())
        .unwrap_or(0);
    (state_deposit, frozen_value, tail_pool + frozen_value, frozen_count)
}
