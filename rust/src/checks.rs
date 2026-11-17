//! Deterministic transport fault fixtures. These are offline tests, not chain acceptance evidence.
use crate::{
    dual_node::{DualNode, OpenFn},
    networks::MAINNET,
    node::open_node,
    rpc::RpcClient,
};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};
use tokio::{sync::watch, task::JoinHandle};
use tokio_tungstenite::tungstenite::Message;

pub struct FakeNode {
    pub url: String,
    stop: watch::Sender<bool>,
    task: JoinHandle<()>,
}
impl FakeNode {
    pub fn disconnect(&self) {
        let _ = self.stop.send(true);
    }
}
impl Drop for FakeNode {
    fn drop(&mut self) {
        self.disconnect();
        self.task.abort();
    }
}
pub async fn fake(
    p2p: &str,
    network: &str,
    handler: Arc<dyn Fn(&Value) -> Option<Value> + Send + Sync>,
) -> FakeNode {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("ws://{}", listener.local_addr().unwrap());
    let (stop, rx) = watch::channel(false);
    let p2p = p2p.to_string();
    let network = network.to_string();
    let task = tokio::spawn(async move {
        loop {
            let (tcp, _) = listener.accept().await.unwrap();
            let mut rx = rx.clone();
            let handler = handler.clone();
            let p2p = p2p.clone();
            let network = network.clone();
            tokio::spawn(async move {
                let Ok(mut ws) = tokio_tungstenite::accept_async(tcp).await else {
                    return;
                };
                loop {
                    tokio::select! {
                        _=rx.changed()=>{let _=ws.close(None).await;break;},
                        message=ws.next()=>{
                            let Some(Ok(Message::Text(text)))=message else {break;};
                            let req:Value=serde_json::from_str(&text).unwrap();
                            let mut result=handler(&req).unwrap_or_else(||match req["method"].as_str().unwrap_or("") {
                                "getServerInfo"=>json!({"params":{"networkId":network,"isSynced":true,"hasUtxoIndex":true,"serverVersion":"fixture","virtualDaaScore":"1000"}}),
                                "getInfo"=>json!({"params":{"p2pId":p2p}}),
                                "getBlockDagInfo"=>json!({"params":{"network":network,"sink":"aa".repeat(32),"virtualDaaScore":"1000"}}),
                                "getSinkBlueScore"=>json!({"params":{"blueScore":"1000"}}),
                                _=>json!({"params":{}}),
                            });
                            if result.is_null(){continue;}
                            result["id"]=req["id"].clone();result["method"]=req["method"].clone();
                            if ws.send(Message::Text(result.to_string().into())).await.is_err(){break;}
                        }
                    }
                }
            });
        }
    });
    FakeNode { url, stop, task }
}
pub fn opener() -> OpenFn {
    Arc::new(|url| {
        Box::pin(async move { open_node(&MAINNET, &url, Duration::from_secs(1), None).await })
    })
}
pub async fn engine_fixture(
    handler: Arc<dyn Fn(&Value) -> Option<Value> + Send + Sync>,
) -> (Arc<crate::engine::Engine>, FakeNode, tempfile::TempDir) {
    let fake = fake("engine", "mainnet", handler).await;
    let (node, _) = DualNode::open(vec![fake.url.clone()], opener(), None)
        .await
        .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(
        crate::store::Store::open(
            dir.path().join("test.sqlite").to_str().unwrap(),
            None,
            "mainnet",
        )
        .unwrap(),
    );
    let contracts = Arc::new(
        crate::contracts::load_contracts(
            None,
            &crate::contracts::LoadCtx {
                network: "mainnet",
                registry_addresses: None,
            },
        )
        .unwrap(),
    );
    (
        crate::engine::Engine::new(store, contracts, node, &MAINNET, Default::default()),
        fake,
        dir,
    )
}
#[tokio::test]
async fn wrong_network_is_rejected() {
    let fake = fake("wrong", "testnet-10", Arc::new(|_| None)).await;
    let result = open_node(&MAINNET, &fake.url, Duration::from_secs(1), None).await;
    assert!(
        result
            .err()
            .unwrap()
            .to_string()
            .starts_with("WRONG_NETWORK_")
    );
}
#[tokio::test]
async fn distinct_backend_failover() {
    let a = fake("a", "mainnet", Arc::new(|_| None)).await;
    let b = fake("b", "mainnet", Arc::new(|_| None)).await;
    let (node, mut events) = DualNode::open(vec![a.url.clone(), b.url.clone()], opener(), None)
        .await
        .unwrap();
    assert_eq!(node.status().iter().filter(|x| x["up"] == true).count(), 2);
    let before = node.url().unwrap();
    if before == a.url {
        a.disconnect();
    } else {
        b.disconnect();
    }
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if matches!(
                events.recv().await,
                Some(crate::dual_node::NodeEvent::Failover { .. })
            ) {
                break;
            }
        }
    })
    .await
    .unwrap();
    assert_ne!(node.url().unwrap(), before);
    assert_eq!(
        node.query("getSinkBlueScore", json!({})).await.unwrap()["blueScore"],
        "1000"
    );
    node.close().await;
}
#[tokio::test]
async fn aliases_of_same_backend_are_not_standby() {
    let a = fake("same", "mainnet", Arc::new(|_| None)).await;
    let b = fake("same", "mainnet", Arc::new(|_| None)).await;
    let (node, _) = DualNode::open(vec![a.url.clone(), b.url.clone()], opener(), None)
        .await
        .unwrap();
    assert_eq!(node.status().iter().filter(|x| x["up"] == true).count(), 1);
    node.close().await;
}
#[tokio::test]
async fn transport_blocks_broadcast_and_cleans_cancelled_calls() {
    let a = fake(
        "a",
        "mainnet",
        Arc::new(|req| {
            if req["method"] == "getBlock" {
                Some(Value::Null)
            } else {
                None
            }
        }),
    )
    .await;
    let (rpc, _) = RpcClient::connect(&a.url, Duration::from_secs(1))
        .await
        .unwrap();
    assert!(
        rpc.call_default("submitTransaction", json!({}))
            .await
            .unwrap_err()
            .to_string()
            .contains("RPC_METHOD_NOT_ALLOWED")
    );
    assert!(
        tokio::time::timeout(
            Duration::from_millis(20),
            rpc.call_default("getBlock", json!({}))
        )
        .await
        .is_err()
    );
    assert_eq!(rpc.pending_count(), 0);
    rpc.close();
}
