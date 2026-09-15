//! Generic JSON-RPC 2.0 Transport over stdio.
//!
//! Provides bidirectional request-response routing, notification streaming,
//! and server-request handling for CLI processes (Codex app-server, Gemini ACP).

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::{mpsc, oneshot, Mutex};

use crate::app::error::{AppError, Result};
use super::ManagedProcess;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcNotification {
    pub jsonrpc: Option<String>,
    pub method: String,
    pub params: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcServerRequest {
    pub jsonrpc: Option<String>,
    pub id: Value,
    pub method: String,
    pub params: Option<Value>,
}

pub struct JsonRpcTransport {
    process: Arc<ManagedProcess>,
    next_id: AtomicU64,
    pending_requests: Arc<Mutex<HashMap<u64, oneshot::Sender<Result<Value>>>>>,
    is_active: Arc<AtomicBool>,
}

impl JsonRpcTransport {
    /// Creates a new JsonRpcTransport over a ManagedProcess.
    /// Returns the transport instance and channels for server notifications and server requests.
    pub fn new(
        process: Arc<ManagedProcess>,
    ) -> (
        Arc<Self>,
        mpsc::Receiver<JsonRpcNotification>,
        mpsc::Receiver<JsonRpcServerRequest>,
    ) {
        let (notif_tx, notif_rx) = mpsc::channel(500);
        let (server_req_tx, server_req_rx) = mpsc::channel(100);
        let pending_requests = Arc::new(Mutex::new(HashMap::<u64, oneshot::Sender<Result<Value>>>::new()));
        let is_active = Arc::new(AtomicBool::new(true));

        let transport = Arc::new(Self {
            process: process.clone(),
            next_id: AtomicU64::new(1),
            pending_requests: pending_requests.clone(),
            is_active: is_active.clone(),
        });

        let process_reader = process.clone();
        let pending_clone = pending_requests.clone();
        let is_active_clone = is_active.clone();

        tokio::spawn(async move {
            while let Ok(Some(line)) = process_reader.recv_stdout_line().await {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }

                let val: Value = match serde_json::from_str(trimmed) {
                    Ok(v) => v,
                    Err(_) => {
                        // Not JSON or partial, skip
                        continue;
                    }
                };

                // Case 1: Response to a client-initiated request (no method, has id)
                if val.get("method").is_none() {
                    if let Some(id_val) = val.get("id") {
                        if let Some(id) = id_val.as_u64() {
                            let mut pending = pending_clone.lock().await;
                            if let Some(reply_tx) = pending.remove(&id) {
                                if let Some(err) = val.get("error") {
                                    let msg = err
                                        .get("message")
                                        .and_then(|m| m.as_str())
                                        .unwrap_or("JSON-RPC error");
                                    let _ = reply_tx.send(Err(AppError::Provider(msg.to_string())));
                                } else {
                                    let result = val.get("result").cloned().unwrap_or(Value::Null);
                                    let _ = reply_tx.send(Ok(result));
                                }
                                continue;
                            }
                        }
                    }
                }

                // Case 2: Server -> Client Request (has id AND method)
                if let (Some(id_val), Some(method_val)) = (val.get("id"), val.get("method").and_then(|m| m.as_str())) {
                    let server_req = JsonRpcServerRequest {
                        jsonrpc: val.get("jsonrpc").and_then(|v| v.as_str()).map(ToString::to_string),
                        id: id_val.clone(),
                        method: method_val.to_string(),
                        params: val.get("params").cloned(),
                    };
                    let _ = server_req_tx.send(server_req).await;
                    continue;
                }

                // Case 3: Server -> Client Notification (has method, no id or null id)
                if let Some(method_val) = val.get("method").and_then(|m| m.as_str()) {
                    let notif = JsonRpcNotification {
                        jsonrpc: val.get("jsonrpc").and_then(|v| v.as_str()).map(ToString::to_string),
                        method: method_val.to_string(),
                        params: val.get("params").cloned(),
                    };
                    let _ = notif_tx.send(notif).await;
                    continue;
                }
            }

            // When stdout ends, cancel any pending requests
            is_active_clone.store(false, Ordering::SeqCst);
            let mut pending = pending_clone.lock().await;
            for (_, sender) in pending.drain() {
                let _ = sender.send(Err(AppError::Process("Process stdout stream closed".to_string())));
            }
        });

        (transport, notif_rx, server_req_rx)
    }

    /// Sends a JSON-RPC request and synchronously awaits the matching response (default 30s timeout).
    pub async fn request(&self, method: &str, params: Value) -> Result<Value> {
        self.request_with_timeout(method, params, Some(std::time::Duration::from_secs(30))).await
    }

    /// Sends a JSON-RPC request with a configurable timeout.
    /// If timeout is None, awaits response without time limit (for long-running operations like agent prompts).
    pub async fn request_with_timeout(
        &self,
        method: &str,
        params: Value,
        timeout_opt: Option<std::time::Duration>,
    ) -> Result<Value> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let (reply_tx, reply_rx) = oneshot::channel();

        {
            let mut pending = self.pending_requests.lock().await;
            pending.insert(id, reply_tx);
        }

        let payload = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });

        let line = serde_json::to_string(&payload)?;
        if let Err(e) = self.process.send_line(&line).await {
            let mut pending = self.pending_requests.lock().await;
            pending.remove(&id);
            return Err(e);
        }

        if let Some(dur) = timeout_opt {
            match tokio::time::timeout(dur, reply_rx).await {
                Ok(Ok(res)) => res,
                Ok(Err(_)) => Err(AppError::Provider("JSON-RPC response channel dropped".to_string())),
                Err(_) => {
                    let mut pending = self.pending_requests.lock().await;
                    pending.remove(&id);
                    Err(AppError::Provider(format!("JSON-RPC request '{method}' timed out after {:?}", dur)))
                }
            }
        } else {
            match reply_rx.await {
                Ok(res) => res,
                Err(_) => Err(AppError::Provider("JSON-RPC response channel dropped".to_string())),
            }
        }
    }

    /// Sends a JSON-RPC notification (fire-and-forget).
    pub async fn notify(&self, method: &str, params: Value) -> Result<()> {
        let payload = serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        });
        let line = serde_json::to_string(&payload)?;
        self.process.send_line(&line).await
    }

    /// Sends a successful JSON-RPC response to a server request.
    pub async fn respond_success(&self, id: Value, result: Value) -> Result<()> {
        let payload = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": result,
        });
        let line = serde_json::to_string(&payload)?;
        self.process.send_line(&line).await
    }

    /// Sends an error JSON-RPC response to a server request.
    pub async fn respond_error(&self, id: Value, code: i64, message: &str) -> Result<()> {
        let payload = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": {
                "code": code,
                "message": message,
            },
        });
        let line = serde_json::to_string(&payload)?;
        self.process.send_line(&line).await
    }

    pub fn is_active(&self) -> bool {
        self.is_active.load(Ordering::SeqCst) && self.process.is_alive()
    }

    pub fn process(&self) -> &Arc<ManagedProcess> {
        &self.process
    }
}
