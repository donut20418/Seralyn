pub mod parser;
pub mod protocol;

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use chrono::Utc;
use serde_json::json;
use tokio::sync::{mpsc, Mutex, RwLock};

use crate::app::conversation::context::format_context_for_prompt;
use crate::app::error::Result;
use crate::app::events::{NormalizedEvent, ProviderKind};
use crate::app::process::json_rpc::{JsonRpcNotification, JsonRpcServerRequest, JsonRpcTransport};
use crate::app::process::{detect_executable, spawn, SpawnConfig};
use crate::app::providers::{
    AuthStatus, InstallationInfo, Provider, ProviderCapabilities,
    ProviderMessage, ProviderSession, SessionConfig, SessionMetadata,
};
use parser::{codex_notification_to_normalized, codex_server_request_to_normalized};
use protocol::{JsonRpcNotification as ProtoNotification, JsonRpcRequest as ProtoRequest};

pub struct CodexProvider;

impl CodexProvider {
    pub fn new() -> Self {
        Self
    }
}

impl Default for CodexProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for CodexProvider {
    fn kind(&self) -> ProviderKind {
        ProviderKind::Codex
    }

    async fn detect_installation(&self) -> Result<InstallationInfo> {
        match detect_executable("codex").await {
            Ok(info) => Ok(InstallationInfo {
                installed: true,
                executable_path: Some(info.path),
                version: info.version,
            }),
            Err(_) => Ok(InstallationInfo {
                installed: false,
                executable_path: None,
                version: None,
            }),
        }
    }

    async fn check_authentication(&self) -> Result<AuthStatus> {
        Ok(AuthStatus::Unknown)
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            text: true,
            image: false,
            file: false,
            tools: true,
            skills: false,
            mcp: false,
            resume: true,
            native_compact: false,
            token_usage: true,
            context_window: false,
            reasoning: false,
            streaming: true,
        }
    }

    async fn create_session(&self, config: SessionConfig) -> Result<Box<dyn ProviderSession>> {
        let spawn_config = SpawnConfig {
            executable: "codex".to_string(),
            args: vec!["app-server".to_string(), "--listen".to_string(), "stdio://".to_string()],
            working_dir: config.working_dir.clone(),
            env: config.env.clone(),
            startup_timeout: Duration::from_secs(10),
        };

        let process = Arc::new(spawn(spawn_config).await?);
        let (transport, notif_rx, server_req_rx) = JsonRpcTransport::new(process);

        // Strict handshake:
        // 1. initialize request -> await response
        let _ = transport.request("initialize", json!({
            "clientInfo": {
                "name": "Seralyn",
                "version": "0.1.0"
            }
        })).await?;

        // 2. initialized notification
        transport.notify("initialized", json!({})).await?;

        // 3. thread/start request -> await response containing threadId
        let thread_resp = transport.request(
            "thread/start",
            json!({
                "instructions": config.system_prompt.unwrap_or_default(),
                "model": config.model.unwrap_or_default(),
            }),
        ).await?;

        let thread_id = thread_resp
            .get("thread")
            .and_then(|t| t.get("id"))
            .or_else(|| thread_resp.get("threadId"))
            .or_else(|| thread_resp.get("id"))
            .and_then(|v| v.as_str())
            .map(ToString::to_string);

        let session = CodexSession::new(
            transport,
            thread_id,
            config.conversation_id,
            config.event_sender,
            notif_rx,
            server_req_rx,
        );

        session.start_dispatcher();

        Ok(Box::new(session))
    }

    async fn resume_session(
        &self,
        native_session_id: &str,
        config: SessionConfig,
    ) -> Result<Box<dyn ProviderSession>> {
        let spawn_config = SpawnConfig {
            executable: "codex".to_string(),
            args: vec!["app-server".to_string(), "--listen".to_string(), "stdio://".to_string()],
            working_dir: config.working_dir.clone(),
            env: config.env.clone(),
            startup_timeout: Duration::from_secs(10),
        };

        let process = Arc::new(spawn(spawn_config).await?);
        let (transport, notif_rx, server_req_rx) = JsonRpcTransport::new(process);

        // 1. initialize request -> await response
        let _ = transport.request("initialize", json!({
            "clientInfo": {
                "name": "Seralyn",
                "version": "0.1.0"
            }
        })).await?;

        // 2. initialized notification
        transport.notify("initialized", json!({})).await?;

        // 3. thread/resume request -> await response
        let _ = transport.request(
            "thread/resume",
            json!({
                "threadId": native_session_id,
            }),
        ).await?;

        let session = CodexSession::new(
            transport,
            Some(native_session_id.to_string()),
            config.conversation_id,
            config.event_sender,
            notif_rx,
            server_req_rx,
        );

        session.start_dispatcher();

        Ok(Box::new(session))
    }
}

pub struct CodexSession {
    transport: Arc<JsonRpcTransport>,
    thread_id: Arc<RwLock<Option<String>>>,
    active_turn_id: Arc<RwLock<Option<String>>>,
    conversation_id: String,
    event_sender: mpsc::Sender<NormalizedEvent>,
    created_at: String,
    notif_rx: Arc<Mutex<Option<mpsc::Receiver<JsonRpcNotification>>>>,
    server_req_rx: Arc<Mutex<Option<mpsc::Receiver<JsonRpcServerRequest>>>>,
}

impl CodexSession {
    pub fn new(
        transport: Arc<JsonRpcTransport>,
        thread_id: Option<String>,
        conversation_id: String,
        event_sender: mpsc::Sender<NormalizedEvent>,
        notif_rx: mpsc::Receiver<JsonRpcNotification>,
        server_req_rx: mpsc::Receiver<JsonRpcServerRequest>,
    ) -> Self {
        Self {
            transport,
            thread_id: Arc::new(RwLock::new(thread_id)),
            active_turn_id: Arc::new(RwLock::new(None)),
            conversation_id,
            event_sender,
            created_at: Utc::now().to_rfc3339(),
            notif_rx: Arc::new(Mutex::new(Some(notif_rx))),
            server_req_rx: Arc::new(Mutex::new(Some(server_req_rx))),
        }
    }

    pub fn start_dispatcher(&self) {
        let notif_rx_opt = {
            let mut guard = self.notif_rx.try_lock().ok();
            guard.as_mut().and_then(|g| g.take())
        };
        let server_req_rx_opt = {
            let mut guard = self.server_req_rx.try_lock().ok();
            guard.as_mut().and_then(|g| g.take())
        };

        let event_sender = self.event_sender.clone();
        let conv_id = self.conversation_id.clone();
        let thread_id_holder = self.thread_id.clone();
        let active_turn_id_holder = self.active_turn_id.clone();

        tokio::spawn(async move {
            let mut notif_rx = match notif_rx_opt {
                Some(rx) => rx,
                None => return,
            };
            let mut server_req_rx = match server_req_rx_opt {
                Some(rx) => rx,
                None => return,
            };

            loop {
                tokio::select! {
                    Some(notif) = notif_rx.recv() => {
                        let proto_notif = ProtoNotification {
                            jsonrpc: notif.jsonrpc.unwrap_or_else(|| "2.0".to_string()),
                            method: notif.method,
                            params: notif.params,
                        };

                        // Extract turnId if available
                        if let Some(params) = &proto_notif.params {
                            if let Some(turn_id) = params.get("turnId").or_else(|| params.get("turn").and_then(|t| t.get("id"))).and_then(|v| v.as_str()) {
                                let mut tid_guard = active_turn_id_holder.write().await;
                                *tid_guard = Some(turn_id.to_string());
                            }
                        }

                        let current_tid = thread_id_holder.read().await.clone();
                        if let Some(event) = codex_notification_to_normalized(
                            &proto_notif,
                            &conv_id,
                            current_tid.as_deref(),
                        ) {
                            let _ = event_sender.send(event).await;
                        }
                    }
                    Some(server_req) = server_req_rx.recv() => {
                        let proto_req = ProtoRequest {
                            jsonrpc: server_req.jsonrpc.unwrap_or_else(|| "2.0".to_string()),
                            id: server_req.id,
                            method: server_req.method,
                            params: server_req.params,
                        };

                        let current_tid = thread_id_holder.read().await.clone();
                        if let Some(event) = codex_server_request_to_normalized(
                            &proto_req,
                            &conv_id,
                            current_tid.as_deref(),
                        ) {
                            let _ = event_sender.send(event).await;
                        }
                    }
                    else => break,
                }
            }
        });
    }
}

#[async_trait]
impl ProviderSession for CodexSession {
    async fn send(&mut self, message: ProviderMessage) -> Result<()> {
        let tid = self.thread_id.read().await.clone().unwrap_or_default();

        // Cross-provider context injection:
        let prompt_text = format_context_for_prompt(&message.context, &message.content);

        let turn_resp = self.transport.request(
            "turn/start",
            json!({
                "threadId": tid,
                "input": [{
                    "type": "text",
                    "text": prompt_text,
                }],
            }),
        ).await?;

        if let Some(turn_id) = turn_resp
            .get("turn")
            .and_then(|t| t.get("id"))
            .or_else(|| turn_resp.get("turnId"))
            .and_then(|v| v.as_str())
        {
            let mut guard = self.active_turn_id.write().await;
            *guard = Some(turn_id.to_string());
        }

        Ok(())
    }

    async fn interrupt(&mut self) -> Result<()> {
        let tid = self.thread_id.read().await.clone().unwrap_or_default();
        let active_turn = self.active_turn_id.read().await.clone();

        if let Some(turn_id) = active_turn {
            let _ = self.transport.request(
                "turn/interrupt",
                json!({
                    "threadId": tid,
                    "turnId": turn_id,
                }),
            ).await;
        } else {
            let _ = self.transport.process().kill().await;
        }
        Ok(())
    }

    async fn cancel(&mut self) -> Result<()> {
        self.interrupt().await
    }

    async fn close(&mut self) -> Result<()> {
        self.transport.process().shutdown(Duration::from_secs(5)).await?;
        Ok(())
    }

    fn native_session_id(&self) -> Option<String> {
        self.thread_id.try_read().ok().and_then(|g| g.clone())
    }

    async fn respond_to_approval(&mut self, request_id: &str, approved: bool) -> Result<()> {
        let id_val = match request_id.parse::<u64>() {
            Ok(n) => json!(n),
            Err(_) => json!(request_id),
        };
        let decision = if approved { "accept" } else { "decline" };
        self.transport.respond_success(id_val, json!({ "decision": decision })).await
    }

    fn metadata(&self) -> SessionMetadata {
        SessionMetadata {
            provider: ProviderKind::Codex,
            native_session_id: self.native_session_id(),
            model: None,
            created_at: self.created_at.clone(),
        }
    }

    fn is_active(&self) -> bool {
        self.transport.is_active()
    }
}
