pub mod parser;
pub mod protocol;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use chrono::Utc;
use serde_json::{json, Value};
use tokio::sync::{mpsc, Mutex, RwLock};

use crate::app::conversation::context::format_context_for_prompt;
use crate::app::error::Result;
use crate::app::events::{NormalizedEvent, ProviderKind};
use crate::app::process::json_rpc::{JsonRpcNotification, JsonRpcServerRequest, JsonRpcTransport};
use crate::app::process::{detect_executable, spawn, SpawnConfig};
use crate::app::providers::{
    resolve_profile_dir, AuthStatus, InstallationInfo, PermissionMode, Provider, ProviderCapabilities,
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
            context_window: true,
            reasoning: true,
            streaming: true,
        }
    }

    async fn create_session(&self, config: SessionConfig) -> Result<Box<dyn ProviderSession>> {
        let mut env = config.env.clone();
        if let Some(profile_dir) = resolve_profile_dir(ProviderKind::Codex, config.account.as_deref()) {
            env.insert("CODEX_HOME".to_string(), profile_dir.to_string_lossy().to_string());
        }

        let spawn_config = SpawnConfig {
            executable: "codex".to_string(),
            args: vec!["app-server".to_string(), "--listen".to_string(), "stdio://".to_string()],
            working_dir: config.working_dir.clone(),
            env,
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

        // 3. Map permissions to sandbox and approvalPolicy and build start_params
        let start_params = build_codex_start_params(
            config.permission_mode,
            config.system_prompt.as_deref(),
            config.model.as_deref(),
            config.effort.as_deref(),
        );

        // 4. thread/start request -> await response containing threadId
        let thread_resp = transport.request("thread/start", start_params).await?;

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
            config.model,
            config.effort,
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
        let mut env = config.env.clone();
        if let Some(profile_dir) = resolve_profile_dir(ProviderKind::Codex, config.account.as_deref()) {
            env.insert("CODEX_HOME".to_string(), profile_dir.to_string_lossy().to_string());
        }

        let spawn_config = SpawnConfig {
            executable: "codex".to_string(),
            args: vec!["app-server".to_string(), "--listen".to_string(), "stdio://".to_string()],
            working_dir: config.working_dir.clone(),
            env,
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
            config.model,
            config.effort,
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
    model: Option<String>,
    effort: Option<String>,
    event_sender: mpsc::Sender<NormalizedEvent>,
    created_at: String,
    notif_rx: Arc<Mutex<Option<mpsc::Receiver<JsonRpcNotification>>>>,
    server_req_rx: Arc<Mutex<Option<mpsc::Receiver<JsonRpcServerRequest>>>>,
    pending_approvals: Arc<Mutex<HashMap<String, (String, Option<Value>)>>>,
}

impl CodexSession {
    pub fn new(
        transport: Arc<JsonRpcTransport>,
        thread_id: Option<String>,
        conversation_id: String,
        model: Option<String>,
        effort: Option<String>,
        event_sender: mpsc::Sender<NormalizedEvent>,
        notif_rx: mpsc::Receiver<JsonRpcNotification>,
        server_req_rx: mpsc::Receiver<JsonRpcServerRequest>,
    ) -> Self {
        Self {
            transport,
            thread_id: Arc::new(RwLock::new(thread_id)),
            active_turn_id: Arc::new(RwLock::new(None)),
            conversation_id,
            model,
            effort,
            event_sender,
            created_at: Utc::now().to_rfc3339(),
            notif_rx: Arc::new(Mutex::new(Some(notif_rx))),
            server_req_rx: Arc::new(Mutex::new(Some(server_req_rx))),
            pending_approvals: Arc::new(Mutex::new(HashMap::new())),
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
        let pending_approvals = self.pending_approvals.clone();

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
                            id: server_req.id.clone(),
                            method: server_req.method.clone(),
                            params: server_req.params.clone(),
                        };

                        // Store pending approval for response formatting
                        let id_str = match &server_req.id {
                            Value::Number(n) => n.to_string(),
                            Value::String(s) => s.clone(),
                            other => other.to_string(),
                        };
                        {
                            let mut pa = pending_approvals.lock().await;
                            pa.insert(id_str, (server_req.method.clone(), server_req.params.clone()));
                        }

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
    async fn send(&self, message: ProviderMessage) -> Result<()> {
        let tid = self.thread_id.read().await.clone().unwrap_or_default();

        // Cross-provider context injection:
        let prompt_text = format_context_for_prompt(&message.context, &message.content);
        let turn_params = build_codex_turn_params(&tid, &prompt_text, self.model.as_deref(), self.effort.as_deref());

        let turn_resp = self.transport.request("turn/start", turn_params).await?;

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

    async fn interrupt(&self) -> Result<()> {
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

    async fn cancel(&self) -> Result<()> {
        self.interrupt().await
    }

    async fn close(&self) -> Result<()> {
        self.transport.process().shutdown(Duration::from_secs(5)).await?;
        Ok(())
    }

    fn native_session_id(&self) -> Option<String> {
        self.thread_id.try_read().ok().and_then(|g| g.clone())
    }

    async fn respond_to_approval(&self, request_id: &str, approved: bool) -> Result<()> {
        let id_val = match request_id.parse::<u64>() {
            Ok(n) => json!(n),
            Err(_) => json!(request_id),
        };

        let req_info = {
            let mut pa = self.pending_approvals.lock().await;
            pa.remove(request_id)
        };

        let result_payload = match req_info {
            Some((ref method, Some(ref params))) if method == "item/permissions/requestApproval" => {
                if approved {
                    let permissions = params.get("permissions").cloned().unwrap_or_else(|| json!({}));
                    json!({
                        "permissions": permissions,
                        "scope": "turn"
                    })
                } else {
                    json!({
                        "permissions": {},
                        "scope": "turn"
                    })
                }
            }
            _ => {
                let decision = if approved { "accept" } else { "decline" };
                json!({ "decision": decision })
            }
        };

        self.transport.respond_success(id_val, result_payload).await
    }

    fn metadata(&self) -> SessionMetadata {
        SessionMetadata {
            provider: ProviderKind::Codex,
            native_session_id: self.native_session_id(),
            model: self.model.clone(),
            created_at: self.created_at.clone(),
        }
    }

    fn is_active(&self) -> bool {
        self.transport.is_active()
    }
}

pub fn build_codex_start_params(
    permission_mode: PermissionMode,
    system_prompt: Option<&str>,
    model: Option<&str>,
    effort: Option<&str>,
) -> Value {
    let (approval_policy, sandbox) = match permission_mode {
        PermissionMode::Safe => ("on-request", "read-only"),
        PermissionMode::Workspace => ("on-request", "workspace-write"),
        PermissionMode::FullAccess => ("never", "danger-full-access"),
    };

    let mut start_params = json!({
        "approvalPolicy": approval_policy,
        "sandbox": sandbox,
    });

    if let Some(prompt) = system_prompt.filter(|s| !s.is_empty()) {
        start_params["baseInstructions"] = json!(prompt);
    }
    if let Some(m) = model.filter(|m| !m.is_empty()) {
        start_params["model"] = json!(m);
    }
    if let Some(e) = effort.filter(|e| !e.is_empty()) {
        start_params["effort"] = json!(e);
    }
    start_params
}

pub fn build_codex_turn_params(
    thread_id: &str,
    prompt_text: &str,
    model: Option<&str>,
    effort: Option<&str>,
) -> Value {
    let mut turn_params = json!({
        "threadId": thread_id,
        "input": [{
            "type": "text",
            "text": prompt_text,
        }],
    });
    if let Some(m) = model {
        turn_params["model"] = json!(m);
    }
    if let Some(e) = effort {
        turn_params["effort"] = json!(e);
    }
    turn_params
}
