//! Native WebSocket JSON wRPC client for Kaspa nodes (`/wrpc/json`). Request {id, method, params}; response
//! {id, method, params} or {id, method, error}; notifications {method: "<x>Notification", params: {<Kind>: {...}}}.
//! Measured 2026-10-22 on kaspad 2.1.0: an UNKNOWN method makes the server close the socket, so only known methods
//! are ever sent.
use anyhow::{Result, anyhow, bail};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;

pub const DEFAULT_CALL_TIMEOUT: Duration = Duration::from_millis(15_000);

#[derive(Debug)]
pub enum RpcEvent {
    Utxos(Value),
    Chain(Value),
    Disconnected,
}

type Pending = Arc<Mutex<HashMap<u64, oneshot::Sender<Result<Value, String>>>>>;

struct PendingGuard(Pending, u64);
impl Drop for PendingGuard {
    fn drop(&mut self) {
        self.0.lock().unwrap().remove(&self.1);
    }
}

pub struct RpcClient {
    pub url: String,
    out: mpsc::UnboundedSender<Message>,
    pending: Pending,
    next_id: AtomicU64,
    connected: Arc<AtomicBool>,
}

/// `/wrpc` -> `/wrpc/json`, `/wrpc/borsh` -> `/wrpc/json` (the client only speaks JSON).
pub fn normalize_url(url: &str) -> String {
    let mut u = url.trim().to_string();
    let lower = u.to_lowercase();
    if (lower.starts_with("ws://") || lower.starts_with("wss://"))
        && lower.ends_with("/wrpc")
        && lower.matches('/').count() == 3
    {
        u.push_str("/json");
    }
    let re = regex::Regex::new(r"(?i)/wrpc/borsh/?$").unwrap();
    re.replace(&u, "/wrpc/json").to_string()
}

impl RpcClient {
    pub async fn connect(
        url: &str,
        timeout: Duration,
    ) -> Result<(Arc<RpcClient>, mpsc::UnboundedReceiver<RpcEvent>)> {
        let url = normalize_url(url);
        if url.is_empty() {
            bail!("CONNECT_FAILED: no url");
        }
        let mut cfg = WebSocketConfig::default();
        cfg.max_message_size = Some(512 << 20);
        cfg.max_frame_size = Some(512 << 20);
        let conn = tokio::time::timeout(
            timeout,
            tokio_tungstenite::connect_async_with_config(url.as_str(), Some(cfg), false),
        )
        .await;
        let (ws, _) = match conn {
            Err(_) => bail!("CONNECT_TIMEOUT"),
            Ok(Err(e)) => bail!("CONNECT_FAILED: {}", e),
            Ok(Ok(x)) => x,
        };
        let (mut sink, mut stream) = ws.split();
        let (out_tx, mut out_rx) = mpsc::unbounded_channel::<Message>();
        let (ev_tx, ev_rx) = mpsc::unbounded_channel::<RpcEvent>();
        let pending: Pending = Arc::new(Mutex::new(HashMap::new()));
        let connected = Arc::new(AtomicBool::new(true));
        // Writer
        let conn_w = connected.clone();
        tokio::spawn(async move {
            while let Some(m) = out_rx.recv().await {
                let close = matches!(m, Message::Close(_));
                if sink.send(m).await.is_err() || close {
                    break;
                }
            }
            let _ = sink.close().await;
            conn_w.store(false, Ordering::SeqCst);
        });
        // Reader
        let (p, c) = (pending.clone(), connected.clone());
        let pong_tx = out_tx.clone();
        tokio::spawn(async move {
            while let Some(m) = stream.next().await {
                let text = match m {
                    Ok(Message::Text(t)) => t.to_string(),
                    Ok(Message::Binary(b)) => String::from_utf8_lossy(&b).to_string(),
                    Ok(Message::Ping(d)) => {
                        let _ = pong_tx.send(Message::Pong(d));
                        continue;
                    }
                    Ok(Message::Close(_)) | Err(_) => break,
                    Ok(_) => continue,
                };
                let Ok(msg) = serde_json::from_str::<Value>(&text) else {
                    continue;
                };
                if let Some(id) = msg.get("id").and_then(|i| i.as_u64()) {
                    let tx = p.lock().unwrap().remove(&id);
                    if let Some(tx) = tx {
                        let r = match msg.get("error") {
                            Some(e) if !e.is_null() => Err(e
                                .get("message")
                                .and_then(|m| m.as_str())
                                .filter(|m| !m.is_empty())
                                .map(|m| m.to_string())
                                .unwrap_or_else(|| e.to_string())),
                            _ => Ok(msg
                                .get("params")
                                .cloned()
                                .filter(|v| !v.is_null())
                                .unwrap_or(json!({}))),
                        };
                        let _ = tx.send(r);
                        continue;
                    }
                }
                let method = msg.get("method").and_then(|m| m.as_str()).unwrap_or("");
                let params = msg.get("params").cloned().unwrap_or(json!({}));
                if method == "virtualChainChangedNotification"
                    || params.get("VirtualChainChanged").is_some()
                {
                    let data = params.get("VirtualChainChanged").cloned().unwrap_or(params);
                    let _ = ev_tx.send(RpcEvent::Chain(data));
                } else if method == "utxosChangedNotification"
                    || params.get("UtxosChanged").is_some()
                {
                    let data = params.get("UtxosChanged").cloned().unwrap_or(params);
                    let _ = ev_tx.send(RpcEvent::Utxos(data));
                }
            }
            c.store(false, Ordering::SeqCst);
            let _ = pong_tx.send(Message::Close(None));
            let drained: Vec<_> = p.lock().unwrap().drain().collect();
            for (_, tx) in drained {
                let _ = tx.send(Err("NODE_DISCONNECTED".into()));
            }
            let _ = ev_tx.send(RpcEvent::Disconnected);
        });
        Ok((
            Arc::new(RpcClient {
                url,
                out: out_tx,
                pending,
                next_id: AtomicU64::new(1),
                connected,
            }),
            ev_rx,
        ))
    }

    #[cfg(test)]
    pub fn pending_count(&self) -> usize {
        self.pending.lock().unwrap().len()
    }

    pub fn is_connected(&self) -> bool {
        self.connected.load(Ordering::SeqCst)
    }

    pub async fn call(&self, method: &str, params: Value, timeout: Duration) -> Result<Value> {
        // Enforce read-only RPC at the transport boundary, including future HTTP callers.
        if !matches!(
            method,
            "getServerInfo"
                | "getInfo"
                | "getBlockDagInfo"
                | "getSinkBlueScore"
                | "getBlock"
                | "getBlocks"
                | "getMempoolEntriesByAddresses"
                | "getUtxosByAddresses"
                | "getVirtualChainFromBlock"
                | "getVirtualChainFromBlockV2"
                | "subscribe"
                | "unsubscribe"
        ) {
            bail!("RPC_METHOD_NOT_ALLOWED: {method}");
        }
        if !self.is_connected() {
            bail!("NODE_DISCONNECTED: not connected");
        }
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = oneshot::channel();
        self.pending.lock().unwrap().insert(id, tx);
        let _pending_guard = PendingGuard(self.pending.clone(), id);
        let payload = json!({"id": id, "method": method, "params": params}).to_string();
        if self.out.send(Message::Text(payload.into())).is_err() {
            self.pending.lock().unwrap().remove(&id);
            bail!("WebSocket send failed: disconnected");
        }
        match tokio::time::timeout(timeout, rx).await {
            Err(_) => {
                self.pending.lock().unwrap().remove(&id);
                bail!("RPC_TIMEOUT: {method}")
            }
            Ok(Err(_)) => bail!("NODE_DISCONNECTED"),
            Ok(Ok(Err(m))) => Err(anyhow!(m)),
            Ok(Ok(Ok(v))) => Ok(v),
        }
    }

    pub async fn call_default(&self, method: &str, params: Value) -> Result<Value> {
        self.call(method, params, DEFAULT_CALL_TIMEOUT).await
    }

    pub fn close(&self) {
        self.connected.store(false, Ordering::SeqCst);
        let _ = self.out.send(Message::Close(None));
    }
}

impl Drop for RpcClient {
    fn drop(&mut self) {
        let _ = self.out.send(Message::Close(None));
    }
}
