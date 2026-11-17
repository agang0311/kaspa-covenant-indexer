//! Primary + hot standby over one-connection nodes (`src/dual-node.mjs`). Both members carry the same UTXO
//! subscriptions (duplicate events are harmless: the engine dedups by queue key and round state). Queries go to the
//! active member; when it drops, the other becomes active at once and a Failover event is emitted. A dropped member
//! is recreated with a fresh client per attempt; in pool mode a member that keeps failing is replaced.
//! Public endpoints behind Cloudflare drop a WebSocket idle ~60 s, so every member is pinged (heartbeat).
use crate::log::log;
use crate::node::SingleNode;
use crate::rpc::RpcEvent;
use anyhow::{Result, bail};
use futures_util::future::{BoxFuture, join_all};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

#[derive(Debug)]
pub enum NodeEvent {
    Utxos(Value),
    Chain(Vec<String>),
    Connect,
    Disconnect,
    Failover { from: Option<String>, to: String },
}

pub type Opened = (Arc<SingleNode>, mpsc::UnboundedReceiver<RpcEvent>);
pub type OpenFn = Arc<dyn Fn(String) -> BoxFuture<'static, Result<Opened>> + Send + Sync>;
pub struct PickReq {
    pub failed_url: String,
    pub avoid_p2p_ids: Vec<String>,
    pub avoid_urls: Vec<String>,
}
pub type PickFn = Arc<dyn Fn(PickReq) -> BoxFuture<'static, Result<Option<String>>> + Send + Sync>;

struct Member {
    url: String,
    node: Option<Arc<SingleNode>>,
    up: bool,
    attempts: usize,
    scheduled: bool,
    timer: Option<JoinHandle<()>>,
    generation: u64,
}

struct State {
    members: Vec<Member>,
    active: Option<usize>,
    subs: Vec<String>,
    chain_sub: bool,
    closed: bool,
}

pub struct DualNode {
    st: Mutex<State>,
    events: mpsc::UnboundedSender<NodeEvent>,
    open: OpenFn,
    pick: Option<PickFn>,
    reconnect_ms: Vec<u64>,
    connect_timeout: Duration,
    replace_after: usize,
    heartbeat: Mutex<Option<JoinHandle<()>>>,
}

fn short(s: &str) -> String {
    crate::util::slice_chars(s, 80)
}

fn is_transport_error(msg: &str) -> bool {
    let m = msg.to_lowercase();
    m.contains("websocket")
        || m.contains("not connected")
        || m.contains("disconnected")
        || m.contains("connect_timeout")
}

impl DualNode {
    /// Start members in parallel; return after all initial attempts, requiring at least one usable member.
    pub async fn open(
        urls: Vec<String>,
        open: OpenFn,
        pick: Option<PickFn>,
    ) -> Result<(Arc<DualNode>, mpsc::UnboundedReceiver<NodeEvent>)> {
        if urls.is_empty() || urls.len() > 2 || (urls.len() == 2 && urls[0] == urls[1]) {
            bail!("DUAL_NODE_NEEDS_TWO_DISTINCT_URLS");
        }
        let (tx, rx) = mpsc::unbounded_channel();
        let members = urls
            .into_iter()
            .map(|url| Member {
                url,
                node: None,
                up: false,
                attempts: 0,
                scheduled: false,
                timer: None,
                generation: 0,
            })
            .collect();
        let d = Arc::new(DualNode {
            st: Mutex::new(State {
                members,
                active: None,
                subs: vec![],
                chain_sub: false,
                closed: false,
            }),
            events: tx,
            open,
            pick,
            reconnect_ms: vec![1000, 3000, 5000, 10000],
            connect_timeout: Duration::from_millis(10_000),
            replace_after: 3,
            heartbeat: Mutex::new(None),
        });
        let n = d.st.lock().unwrap().members.len();
        let first = join_all((0..n).map(|i| d.clone().connect(i))).await;
        for (i, ok) in first.iter().enumerate() {
            if !ok {
                d.schedule(i);
            }
        }
        if d.st.lock().unwrap().active.is_none() {
            d.close().await;
            bail!("NO_NODE_AVAILABLE");
        }
        let hb = d.clone();
        *d.heartbeat.lock().unwrap() = Some(tokio::spawn(async move {
            let mut iv = tokio::time::interval(Duration::from_millis(20_000));
            iv.tick().await;
            loop {
                iv.tick().await;
                let live: Vec<(usize, Arc<SingleNode>)> = {
                    let st = hb.st.lock().unwrap();
                    st.members
                        .iter()
                        .enumerate()
                        .filter(|(_, m)| m.up)
                        .filter_map(|(i, m)| m.node.clone().map(|n| (i, n)))
                        .collect()
                };
                for (i, node) in live {
                    let me = hb.clone();
                    tokio::spawn(async move {
                        let r = tokio::time::timeout(
                            me.connect_timeout,
                            node.call("getSinkBlueScore", json!({})),
                        )
                        .await;
                        let err = match r {
                            Err(_) => Some("HEARTBEAT_TIMEOUT".to_string()),
                            Ok(Err(e)) => Some(e.to_string()),
                            Ok(Ok(_)) => None,
                        };
                        if let Some(e) = err
                            && me.member_node_is(i, node.id)
                        {
                            me.mark_down(i, &format!("HEARTBEAT:{}", short(&e)));
                        }
                    });
                }
            }
        }));
        Ok((d, rx))
    }

    fn member_node_is(&self, i: usize, id: u64) -> bool {
        self.st
            .lock()
            .unwrap()
            .members
            .get(i)
            .and_then(|m| m.node.as_ref())
            .map(|n| n.id)
            == Some(id)
    }

    fn emit(&self, e: NodeEvent) {
        let _ = self.events.send(e);
    }

    fn pick_active(&self, st: &mut State, reason: &str) {
        let prev = st.active;
        let next = prev
            .filter(|p| st.members[*p].up)
            .or_else(|| st.members.iter().position(|m| m.up));
        st.active = next;
        if next != prev {
            match (prev, next) {
                (Some(p), Some(n)) => {
                    let (from, to) = (st.members[p].url.clone(), st.members[n].url.clone());
                    log(
                        "FAILOVER",
                        json!({"from": from, "to": to, "reason": reason}),
                    );
                    self.emit(NodeEvent::Failover {
                        from: Some(from),
                        to,
                    });
                }
                (None, Some(n)) => {
                    log(
                        "NODE_ACTIVE",
                        json!({"url": st.members[n].url, "reason": reason}),
                    );
                    self.emit(NodeEvent::Connect);
                }
                (_, None) => {
                    log("ALL_NODES_DOWN", json!({"reason": reason}));
                    self.emit(NodeEvent::Disconnect);
                }
            }
        }
    }

    fn mark_down(self: &Arc<Self>, i: usize, reason: &str) {
        {
            let mut st = self.st.lock().unwrap();
            let m = &mut st.members[i];
            if !m.up && m.node.is_none() {
                return;
            }
            m.up = false;
            let old = m.node.take();
            m.generation += 1;
            if let Some(o) = old {
                o.close();
            }
            log("NODE_DOWN", json!({"url": m.url, "reason": reason}));
            self.pick_active(&mut st, reason);
        }
        self.schedule(i);
    }

    fn connect(self: Arc<Self>, i: usize) -> BoxFuture<'static, bool> {
        Box::pin(async move {
            let (generation, url, attempt) = {
                let mut st = self.st.lock().unwrap();
                if st.closed {
                    return false;
                }
                let m = &mut st.members[i];
                m.generation += 1;
                (m.generation, m.url.clone(), m.attempts.max(1))
            };
            let failed = |msg: String| {
                log(
                    "NODE_CONNECT_FAILED",
                    json!({"url": url, "attempt": attempt, "message": msg}),
                );
                false
            };
            let (node, rx) =
                match tokio::time::timeout(self.connect_timeout, (self.open)(url.clone())).await {
                    Err(_) => return failed("CONNECT_TIMEOUT".into()),
                    Ok(Err(e)) => return failed(e.to_string()),
                    Ok(Ok(x)) => x,
                };
            {
                let mut st = self.st.lock().unwrap();
                if st.closed || st.members[i].generation != generation {
                    node.close();
                    return false;
                }
                let same = st
                    .members
                    .iter()
                    .enumerate()
                    .find(|(j, x)| *j != i && x.up)
                    .and_then(|(_, o)| {
                        let (a, b) = (node.p2p_id(), o.node.as_ref().and_then(|n| n.p2p_id()));
                        if a.is_some() && a == b {
                            Some((o.url.clone(), a.unwrap()))
                        } else {
                            None
                        }
                    });
                if let Some((as_url, p2p)) = same {
                    node.close();
                    log(
                        "NODE_SAME_BACKEND",
                        json!({"url": url, "as": as_url, "p2pId": p2p}),
                    );
                    let ra = self.replace_after;
                    let m = &mut st.members[i];
                    m.attempts = m.attempts.max(ra); // not a standby at all: replace immediately (pool mode)
                    return false;
                }
            }
            self.clone().spawn_forwarder(i, node.clone(), rx);
            let (subs, chain) = {
                let st = self.st.lock().unwrap();
                (st.subs.clone(), st.chain_sub)
            };
            if !subs.is_empty() {
                let r = tokio::time::timeout(
                    self.connect_timeout,
                    node.call("subscribe", json!({"UtxosChanged": {"addresses": subs}})),
                )
                .await;
                let err = match r {
                    Err(_) => Some("SUBSCRIBE_TIMEOUT".to_string()),
                    Ok(Err(e)) => Some(e.to_string()),
                    Ok(Ok(_)) => None,
                };
                if let Some(e) = err {
                    node.close();
                    return failed(e);
                }
            }
            if chain {
                let r = tokio::time::timeout(
                    self.connect_timeout,
                    node.call(
                        "subscribe",
                        json!({"VirtualChainChanged": {"include_accepted_transaction_ids": false}}),
                    ),
                )
                .await;
                match r {
                    Err(_) => log(
                        "CHAIN_SUBSCRIBE_FAILED",
                        json!({"url": url, "message": "SUBSCRIBE_CHAIN_TIMEOUT"}),
                    ),
                    Ok(Err(e)) => log(
                        "CHAIN_SUBSCRIBE_FAILED",
                        json!({"url": url, "message": e.to_string()}),
                    ),
                    Ok(Ok(_)) => {}
                }
            }
            {
                let mut st = self.st.lock().unwrap();
                if st.closed || st.members[i].generation != generation {
                    node.close();
                    return false;
                }
                // Parallel connects can both pass the earlier backend check before subscriptions finish.
                if node.p2p_id().is_some()
                    && st.members.iter().enumerate().any(|(j, m)| {
                        j != i && m.up && m.node.as_ref().and_then(|n| n.p2p_id()) == node.p2p_id()
                    })
                {
                    node.close();
                    st.members[i].attempts = self.replace_after;
                    return false;
                }
                let m = &mut st.members[i];
                m.node = Some(node.clone());
                m.up = true;
                m.attempts = 0;
                log(
                    "NODE_UP",
                    json!({"url": m.url, "identity": node.identity(), "subscriptions": subs.len()}),
                );
                self.pick_active(&mut st, "NODE_UP");
            }
            // A socket that died during the subscribe phase never reaches the forwarder's ownership check.
            if !node.rpc.is_connected() {
                self.mark_down(i, "SOCKET_DISCONNECTED");
            }
            true
        })
    }

    fn spawn_forwarder(
        self: Arc<Self>,
        i: usize,
        node: Arc<SingleNode>,
        mut rx: mpsc::UnboundedReceiver<RpcEvent>,
    ) {
        tokio::spawn(async move {
            while let Some(ev) = rx.recv().await {
                match ev {
                    RpcEvent::Utxos(data) => {
                        if self.member_node_is(i, node.id) {
                            self.emit(NodeEvent::Utxos(data));
                        }
                    }
                    RpcEvent::Chain(data) => {
                        let removed: Vec<String> = data
                            .get("removedChainBlockHashes")
                            .and_then(|r| r.as_array())
                            .map(|a| a.iter().map(|h| crate::util::js_string(Some(h))).collect())
                            .unwrap_or_default();
                        if removed.is_empty() || removed.len() > 10_000 {
                            continue;
                        }
                        // Reorg hints from the ACTIVE member only (the standby may sit on a slightly different view).
                        let ok = {
                            let st = self.st.lock().unwrap();
                            st.members
                                .get(i)
                                .and_then(|m| m.node.as_ref())
                                .map(|n| n.id)
                                == Some(node.id)
                                && st.active == Some(i)
                        };
                        if ok {
                            self.emit(NodeEvent::Chain(removed));
                        }
                    }
                    RpcEvent::Disconnected => {
                        if self.member_node_is(i, node.id) {
                            self.mark_down(i, "SOCKET_DISCONNECTED");
                        }
                        break;
                    }
                }
            }
        });
    }

    fn schedule(self: &Arc<Self>, i: usize) {
        let ms = {
            let mut st = self.st.lock().unwrap();
            if st.closed {
                return;
            }
            let rm = self.reconnect_ms.clone();
            let m = &mut st.members[i];
            if m.scheduled || m.up {
                return;
            }
            let ms = rm[m.attempts.min(rm.len() - 1)];
            m.attempts += 1;
            m.scheduled = true;
            ms
        };
        let me = self.clone();
        let h = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(ms)).await;
            let (attempts, failed_url, other_up_p2p, other_url) = {
                let mut st = me.st.lock().unwrap();
                st.members[i].scheduled = false;
                let other = st
                    .members
                    .iter()
                    .enumerate()
                    .find(|(j, _)| *j != i)
                    .map(|(_, o)| o);
                let p2p = other
                    .filter(|o| o.up)
                    .and_then(|o| o.node.as_ref())
                    .and_then(|n| n.p2p_id());
                let ou = other.map(|o| o.url.clone());
                (st.members[i].attempts, st.members[i].url.clone(), p2p, ou)
            };
            if let Some(pick) = me.pick.clone()
                && attempts > me.replace_after
            {
                let req = PickReq {
                    failed_url: failed_url.clone(),
                    avoid_p2p_ids: other_up_p2p.into_iter().collect(),
                    avoid_urls: std::iter::once(failed_url.clone())
                        .chain(other_url.clone())
                        .collect(),
                };
                let next = match pick(req).await {
                    Ok(n) => n,
                    Err(e) => {
                        log(
                            "NODE_REPLACEMENT_FAILED",
                            json!({"url": failed_url, "message": e.to_string()}),
                        );
                        None
                    }
                };
                let mut st = me.st.lock().unwrap();
                if st.closed {
                    return;
                }
                if let Some(n) = next
                    && n != failed_url
                    && Some(&n) != other_url.as_ref()
                {
                    log("NODE_REPLACED", json!({"from": failed_url, "to": n}));
                    st.members[i].url = n;
                    st.members[i].attempts = 1;
                }
            }
            if !me.clone().connect(i).await {
                me.schedule(i);
            }
        });
        let mut st = self.st.lock().unwrap();
        if let Some(old) = st.members[i].timer.replace(h) {
            drop(old);
        }
    }

    /// Run on the active member; on a transport error mark it down and retry once on the other member.
    async fn on_active<F, Fut>(self: &Arc<Self>, name: &str, f: F) -> Result<Value>
    where
        F: Fn(Arc<SingleNode>) -> Fut,
        Fut: std::future::Future<Output = Result<Value>>,
    {
        for _pass in 0..2 {
            let (i, node) = {
                let st = self.st.lock().unwrap();
                match st
                    .active
                    .and_then(|i| st.members[i].node.clone().map(|n| (i, n)))
                {
                    Some(x) => x,
                    None => bail!("NO_NODE_AVAILABLE"),
                }
            };
            match f(node.clone()).await {
                Ok(v) => return Ok(v),
                Err(e) => {
                    let msg = e.to_string();
                    if msg.to_lowercase().contains("cannot find header") {
                        // The active member may be slightly behind (catching up): try the other live member first.
                        let other = {
                            let st = self.st.lock().unwrap();
                            st.members
                                .iter()
                                .enumerate()
                                .find(|(j, x)| *j != i && x.up && x.node.is_some())
                                .and_then(|(_, x)| x.node.clone())
                        };
                        if let Some(o) = other
                            && let Ok(v) = f(o).await
                        {
                            return Ok(v);
                        }
                    }
                    if !is_transport_error(&msg) {
                        return Err(e); // RPC-level errors are answers
                    }
                    if self.member_node_is(i, node.id) {
                        self.mark_down(i, &format!("{name}:{}", short(&msg)));
                    }
                }
            }
        }
        bail!("NO_NODE_AVAILABLE")
    }

    pub async fn query(self: &Arc<Self>, method: &'static str, params: Value) -> Result<Value> {
        self.on_active(method, |n| {
            let p = params.clone();
            async move { n.call(method, p).await }
        })
        .await
    }

    pub async fn verify(self: &Arc<Self>) -> Result<Value> {
        self.on_active("verify", |n| async move { n.verify().await })
            .await
    }

    async fn both(self: &Arc<Self>, what: &str, payload: Value) -> Result<()> {
        let live: Vec<(usize, Arc<SingleNode>)> = {
            let st = self.st.lock().unwrap();
            st.members
                .iter()
                .enumerate()
                .filter(|(_, m)| m.up)
                .filter_map(|(i, m)| m.node.clone().map(|n| (i, n)))
                .collect()
        };
        let res = join_all(live.iter().map(|(_, n)| {
            let p = payload.clone();
            let n = n.clone();
            async move {
                n.call(
                    if what.starts_with("unsubscribe") {
                        "unsubscribe"
                    } else {
                        "subscribe"
                    },
                    p,
                )
                .await
            }
        }))
        .await;
        let mut any = false;
        for ((i, n), r) in live.iter().zip(res) {
            match r {
                Ok(_) => any = true,
                Err(e) => {
                    if self.member_node_is(*i, n.id) {
                        self.mark_down(*i, &format!("{what}:{}", short(&e.to_string())));
                    }
                }
            }
        }
        if !any {
            bail!("NO_NODE_AVAILABLE");
        }
        Ok(())
    }

    pub async fn subscribe_utxos(self: &Arc<Self>, addrs: &[String]) -> Result<()> {
        {
            let mut st = self.st.lock().unwrap();
            for a in addrs {
                if !st.subs.contains(a) {
                    st.subs.push(a.clone());
                }
            }
        }
        self.both(
            "subscribeUtxosChanged",
            json!({"UtxosChanged": {"addresses": addrs}}),
        )
        .await
    }

    pub async fn unsubscribe_utxos(self: &Arc<Self>, addrs: &[String]) -> Result<()> {
        {
            let mut st = self.st.lock().unwrap();
            st.subs.retain(|s| !addrs.contains(s));
        }
        self.both(
            "unsubscribeUtxosChanged",
            json!({"UtxosChanged": {"addresses": addrs}}),
        )
        .await
    }

    pub async fn subscribe_chain(self: &Arc<Self>) -> Result<()> {
        self.st.lock().unwrap().chain_sub = true;
        self.both(
            "subscribeVirtualChainChanged",
            json!({"VirtualChainChanged": {"include_accepted_transaction_ids": false}}),
        )
        .await
    }

    pub fn identity(&self) -> Value {
        let st = self.st.lock().unwrap();
        st.active
            .and_then(|i| st.members[i].node.as_ref().map(|n| n.identity()))
            .unwrap_or(Value::Null)
    }

    pub fn url(&self) -> Option<String> {
        let st = self.st.lock().unwrap();
        st.active.map(|i| st.members[i].url.clone())
    }

    pub fn status(&self) -> Vec<Value> {
        let st = self.st.lock().unwrap();
        st.members
            .iter()
            .enumerate()
            .map(|(i, m)| {
                let id = m.node.as_ref().map(|n| n.identity()).unwrap_or(Value::Null);
                json!({"url": m.url, "up": m.up, "active": st.active == Some(i), "attempts": m.attempts,
                    "nodeId": id.get("nodeId").cloned().unwrap_or(Value::Null), "p2pId": id.get("p2pId").cloned().unwrap_or(Value::Null),
                    "serverVersion": id.get("serverVersion").cloned().unwrap_or(Value::Null), "isSynced": id.get("isSynced").cloned().unwrap_or(Value::Null),
                    "hasUtxoIndex": id.get("hasUtxoIndex").cloned().unwrap_or(Value::Null)})
            })
            .collect()
    }

    pub fn member_count(&self) -> usize {
        self.st.lock().unwrap().members.len()
    }

    /// Operator switch: replace member URLs (primary first) and reconnect changed members.
    pub async fn switch_urls(self: &Arc<Self>, new_urls: Vec<String>) -> Result<Vec<Value>> {
        let n = self.member_count();
        if new_urls.len() != 2 || n != 2 || new_urls[0] == new_urls[1] {
            bail!("DUAL_NODE_NEEDS_TWO_DISTINCT_URLS");
        }
        {
            let mut st = self.st.lock().unwrap();
            for (i, u) in new_urls.iter().enumerate() {
                let m = &mut st.members[i];
                if &m.url != u {
                    if let Some(t) = m.timer.take() {
                        t.abort();
                    }
                    m.scheduled = false;
                    if let Some(old) = m.node.take() {
                        old.close();
                    }
                    m.up = false;
                    m.generation += 1;
                    m.url = u.clone();
                    m.attempts = 0;
                }
            }
            // A member that was active but is now down must not stay "active".
            let a = st.active;
            if let Some(a) = a
                && !st.members[a].up
            {
                st.active = None;
                let next = st.members.iter().position(|m| m.up);
                st.active = next;
            }
        }
        let ups: Vec<bool> = self
            .st
            .lock()
            .unwrap()
            .members
            .iter()
            .map(|m| m.up)
            .collect();
        let results = join_all((0..n).map(|i| {
            let me = self.clone();
            let up = ups[i];
            async move { if up { true } else { me.connect(i).await } }
        }))
        .await;
        for (i, ok) in results.iter().enumerate() {
            if !ok {
                self.schedule(i);
            }
        }
        let ev = {
            let mut st = self.st.lock().unwrap();
            if st.members[0].up && st.active != Some(0) {
                let prev = st.active.map(|p| st.members[p].url.clone());
                st.active = Some(0);
                let to = st.members[0].url.clone();
                log(
                    "FAILOVER",
                    json!({"from": prev, "to": to, "reason": "OPERATOR_SWITCH"}),
                );
                Some(NodeEvent::Failover { from: prev, to })
            } else {
                None
            }
        };
        if let Some(e) = ev {
            self.emit(e);
        }
        Ok(self.status())
    }

    pub async fn promote(self: &Arc<Self>, url: &str) -> Result<Vec<Value>> {
        let lower = url.to_lowercase();
        if !(lower.starts_with("ws://") || lower.starts_with("wss://")) {
            bail!("INVALID_URL");
        }
        let (m0, m1, active_url, active_is_0, m0_up, m1_up) = {
            let st = self.st.lock().unwrap();
            if st.members.len() != 2 {
                bail!("DUAL_NODE_NOT_SUPPORTED");
            }
            (
                st.members[0].url.clone(),
                st.members[1].url.clone(),
                st.active.map(|i| st.members[i].url.clone()),
                st.active == Some(0),
                st.members[0].up,
                st.members[1].up,
            )
        };
        if m0 == url && m0_up && active_is_0 {
            return Ok(self.status());
        }
        if m1 == url && m1_up {
            return self.switch_urls(vec![m1, m0]).await;
        }
        let standby = match active_url.filter(|a| a != url) {
            Some(a) => Some(a),
            None => [m0, m1].into_iter().find(|u| u != url),
        };
        let Some(standby) = standby.filter(|s| s != url) else {
            bail!("CANNOT_DETERMINE_STANDBY")
        };
        self.switch_urls(vec![url.to_string(), standby]).await
    }

    pub async fn close(&self) {
        let mut st = self.st.lock().unwrap();
        st.closed = true;
        if let Some(h) = self.heartbeat.lock().unwrap().take() {
            h.abort();
        }
        for m in st.members.iter_mut() {
            if let Some(t) = m.timer.take() {
                t.abort();
            }
            m.generation += 1;
            if let Some(n) = m.node.take() {
                n.close();
            }
            m.up = false;
        }
        st.active = None;
    }
}
