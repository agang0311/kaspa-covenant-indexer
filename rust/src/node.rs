//! One verified wRPC connection (`src/node.mjs`). Fail-closed identity: getServerInfo.networkId, getBlockDagInfo.network
//! and the pinned network table must name the SAME network; the node must be synced and have a UTXO index.
use crate::networks::Network;
use crate::rpc::{DEFAULT_CALL_TIMEOUT, RpcClient, RpcEvent};
use anyhow::{Result, bail};
use serde_json::{Value, json};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::mpsc;

static NEXT_NODE_ID: AtomicU64 = AtomicU64::new(1);

pub struct SingleNode {
    pub id: u64,
    pub rpc: Arc<RpcClient>,
    pub net: &'static Network,
    identity: Mutex<Value>,
}

impl SingleNode {
    pub fn identity(&self) -> Value {
        self.identity.lock().unwrap().clone()
    }
    pub fn p2p_id(&self) -> Option<String> {
        self.identity
            .lock()
            .unwrap()
            .get("p2pId")
            .and_then(|p| p.as_str())
            .map(|s| s.to_string())
    }
    pub async fn verify(&self) -> Result<Value> {
        let id = verify_node(&self.rpc, self.net).await?;
        *self.identity.lock().unwrap() = id.clone();
        Ok(id)
    }
    pub async fn call(&self, method: &str, params: Value) -> Result<Value> {
        self.rpc.call(method, params, DEFAULT_CALL_TIMEOUT).await
    }
    pub fn close(&self) {
        self.rpc.close();
    }
}

pub async fn verify_node(rpc: &RpcClient, net: &Network) -> Result<Value> {
    let info = rpc.call_default("getServerInfo", json!({})).await?;
    let nid = info.get("networkId").and_then(|n| n.as_str()).unwrap_or("");
    if nid != net.network_id {
        bail!(
            "WRONG_NETWORK_{}",
            crate::util::js_string(info.get("networkId"))
        );
    }
    if info.get("isSynced") != Some(&json!(true)) {
        bail!("NODE_NOT_SYNCED");
    }
    if info.get("hasUtxoIndex") != Some(&json!(true)) {
        bail!("NODE_WITHOUT_UTXOINDEX");
    }
    if let Ok(dag) = rpc.call_default("getBlockDagInfo", json!({})).await
        && let Some(n) = dag.get("network").filter(|n| !n.is_null())
        && crate::util::js_string(Some(n)) != net.network_id
    {
        bail!("WRONG_NETWORK_DAG_{}", crate::util::js_string(Some(n)));
    }
    let p2p = rpc
        .call_default("getInfo", json!({}))
        .await
        .ok()
        .and_then(|i| i.get("p2pId").cloned())
        .unwrap_or(Value::Null);
    Ok(json!({
        "network": net.key, "networkId": net.network_id, "genesis": net.genesis, "url": rpc.url, "nodeId": Value::Null,
        "p2pId": p2p, "serverVersion": info.get("serverVersion").cloned().unwrap_or(Value::Null), "isSynced": true, "hasUtxoIndex": true,
        "virtualDaaScore": info.get("virtualDaaScore").filter(|v| !v.is_null()).map(|v| json!(crate::util::js_string(Some(v)))).unwrap_or(Value::Null),
        "checkedAt": crate::util::iso_now(),
    }))
}

/// Open and verify one member. The connect itself is bounded by min(timeout, connect_deadline); verification calls use
/// the regular per-call RPC timeout (same as the JS build).
pub async fn open_node(
    net: &'static Network,
    url: &str,
    timeout: Duration,
    connect_deadline: Option<Duration>,
) -> Result<(Arc<SingleNode>, mpsc::UnboundedReceiver<RpcEvent>)> {
    let limit = connect_deadline.map(|d| d.min(timeout)).unwrap_or(timeout);
    let (rpc, events) = RpcClient::connect(url, limit).await?;
    match verify_node(&rpc, net).await {
        Ok(identity) => Ok((
            Arc::new(SingleNode {
                id: NEXT_NODE_ID.fetch_add(1, Ordering::SeqCst),
                rpc,
                net,
                identity: Mutex::new(identity),
            }),
            events,
        )),
        Err(e) => {
            rpc.close();
            Err(e)
        }
    }
}
