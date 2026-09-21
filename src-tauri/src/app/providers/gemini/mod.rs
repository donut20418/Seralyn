pub mod parser;
pub mod protocol;

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use chrono::Utc;
use serde_json::{json, Value};
use tokio::sync::{mpsc, Mutex, Notify, RwLock};

use crate::app::conversation::context::format_context_for_prompt;
use crate::app::error::Result;
use crate::app::events::{NormalizedEvent, ProviderKind};
use crate::app::process::json_rpc::{JsonRpcNotification, JsonRpcServerRequest, JsonRpcTransport};
use crate::app::process::{detect_executable, spawn, SpawnConfig};
use crate::app::providers::{
    resolve_profile_dir, AttachmentKind, AttachmentRef, AuthStatus, InstallationInfo, PermissionMode, Provider, ProviderCapabilities,
    ProviderMessage, ProviderSession, SessionConfig, SessionMetadata,
};
use parser::{gemini_notification_to_normalized, gemini_server_request_to_normalized};
use protocol::{AcpNotification, AcpRequest};

pub struct GeminiProvider;

impl GeminiProvider {
    pub fn new() -> Self {
        Self
    }
}

impl Default for GeminiProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for GeminiProvider {
    fn kind(&self) -> ProviderKind {
        ProviderKind::Gemini
    }

    async fn detect_installation(&self) -> Result<InstallationInfo> {
        match detect_executable("gemini").await {
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
        if std::env::var("GEMINI_API_KEY").is_ok()
            || std::env::var("GOOGLE_GENAI_USE_VERTEXAI").is_ok()
            || std::env::var("GOOGLE_GENAI_USE_GCA").is_ok()
        {
            return Ok(AuthStatus::Authenticated);
        }
        if let Some(home) = dirs::home_dir() {
            let creds = home.join(".gemini").join("oauth_creds.json");
            if creds.exists() {
                return Ok(AuthStatus::Authenticated);
            }
        }
        Ok(AuthStatus::Unknown)
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            text: true,
            image: true,
            file: false,
            tools: true,
            skills: false,
            mcp: false,
            resume: true,
            native_compact: false,
            token_usage: false,
            context_window: true,
            reasoning: false,
            streaming: true,
        }
    }

    async fn create_session(&self, config: SessionConfig) -> Result<Box<dyn ProviderSession>> {
        let approval_mode = match config.permission_mode {
            PermissionMode::Safe => "default",
            PermissionMode::Workspace => "auto_edit",
            PermissionMode::FullAccess => "yolo",
        };

        let cwd_str = resolve_canonical_cwd(config.working_dir.as_deref());

        let mut args = vec![
            "--acp".to_string(),
            "--skip-trust".to_string(),
            "--approval-mode".to_string(),
            approval_mode.to_string(),
        ];

        if let Some(m) = &config.model {
            args.push("--model".to_string());
            args.push(m.clone());
        }

        let mut env = config.env.clone();
        if let Some(profile_dir) = resolve_profile_dir(ProviderKind::Gemini, config.account.as_deref()) {
            env.insert("GEMINI_CLI_HOME".to_string(), profile_dir.to_string_lossy().to_string());
        }

        let spawn_config = SpawnConfig {
            executable: "gemini".to_string(),
            args,
            working_dir: config.working_dir.clone(),
            env,
            startup_timeout: Duration::from_secs(10),
        };

        let process = Arc::new(spawn(spawn_config).await?);
        let (transport, notif_rx, server_req_rx) = JsonRpcTransport::new(process);

        // Step 1: ACP v1 initialize request -> await response
        let init_resp = transport.request(
            "initialize",
            json!({
                "protocolVersion": 1,
                "clientCapabilities": {}
            }),
        ).await?;

        let server_capabilities = init_resp
            .get("serverCapabilities")
            .or_else(|| init_resp.get("capabilities"))
            .cloned();

        // Optional authenticate if API key is explicitly provided in environment
        if let Ok(key) = std::env::var("GEMINI_API_KEY") {
            let _ = transport.request(
                "authenticate",
                json!({
                    "methodId": "gemini-api-key",
                    "_meta": {
                        "api-key": key
                    }
                }),
            ).await;
        }

        // Step 2: session/new request -> await response to extract real sessionId
        let new_resp = transport.request(
            "session/new",
            json!({
                "cwd": cwd_str,
                "mcpServers": []
            }),
        ).await?;

        let session_id = new_resp
            .get("sessionId")
            .or_else(|| new_resp.get("id"))
            .and_then(|v| v.as_str())
            .map(ToString::to_string);

        let session = GeminiSession::new(
            transport,
            session_id,
            server_capabilities,
            config.conversation_id,
            config.model,
            config.event_sender,
            notif_rx,
            server_req_rx,
            false,
        );

        session.start_dispatcher();

        Ok(Box::new(session))
    }

    async fn resume_session(
        &self,
        native_session_id: &str,
        config: SessionConfig,
    ) -> Result<Box<dyn ProviderSession>> {
        let approval_mode = match config.permission_mode {
            PermissionMode::Safe => "default",
            PermissionMode::Workspace => "auto_edit",
            PermissionMode::FullAccess => "yolo",
        };

        let cwd_str = resolve_canonical_cwd(config.working_dir.as_deref());

        let mut args = vec![
            "--acp".to_string(),
            "--skip-trust".to_string(),
            "--approval-mode".to_string(),
            approval_mode.to_string(),
        ];

        if let Some(m) = &config.model {
            args.push("--model".to_string());
            args.push(m.clone());
        }

        let mut env = config.env.clone();
        if let Some(profile_dir) = resolve_profile_dir(ProviderKind::Gemini, config.account.as_deref()) {
            env.insert("GEMINI_CLI_HOME".to_string(), profile_dir.to_string_lossy().to_string());
        }

        let spawn_config = SpawnConfig {
            executable: "gemini".to_string(),
            args,
            working_dir: config.working_dir.clone(),
            env,
            startup_timeout: Duration::from_secs(10),
        };

        let process = Arc::new(spawn(spawn_config).await?);
        let (transport, notif_rx, server_req_rx) = JsonRpcTransport::new(process);

        // Step 1: ACP v1 initialize -> await response
        let init_resp = transport.request(
            "initialize",
            json!({
                "protocolVersion": 1,
                "clientCapabilities": {}
            }),
        ).await?;

        let server_capabilities = init_resp
            .get("serverCapabilities")
            .or_else(|| init_resp.get("capabilities"))
            .cloned();

        // Optional authenticate if API key is explicitly provided in environment
        if let Ok(key) = std::env::var("GEMINI_API_KEY") {
            let _ = transport.request(
                "authenticate",
                json!({
                    "methodId": "gemini-api-key",
                    "_meta": {
                        "api-key": key
                    }
                }),
            ).await;
        }

        // Step 2: session/load request -> await response
        let _ = transport.request(
            "session/load",
            json!({
                "sessionId": native_session_id,
                "cwd": cwd_str,
                "mcpServers": []
            }),
        ).await?;

        let session = GeminiSession::new(
            transport,
            Some(native_session_id.to_string()),
            server_capabilities,
            config.conversation_id,
            config.model,
            config.event_sender,
            notif_rx,
            server_req_rx,
            true,
        );

        session.start_dispatcher();
        session.wait_replay_quiescence(Duration::from_millis(1000)).await;

        Ok(Box::new(session))
    }
}

pub struct GeminiSession {
    transport: Arc<JsonRpcTransport>,
    session_id: Arc<RwLock<Option<String>>>,
    server_capabilities: Arc<RwLock<Option<Value>>>,
    conversation_id: String,
    model: Option<String>,
    event_sender: mpsc::Sender<NormalizedEvent>,
    created_at: String,
    notif_rx: Arc<Mutex<Option<mpsc::Receiver<JsonRpcNotification>>>>,
    server_req_rx: Arc<Mutex<Option<mpsc::Receiver<JsonRpcServerRequest>>>>,
    pending_permissions: Arc<Mutex<HashMap<String, Vec<Value>>>>,
    replay_complete: Arc<AtomicBool>,
    replay_ready: Arc<Notify>,
    is_resume: bool,
}

impl GeminiSession {
    pub fn new(
        transport: Arc<JsonRpcTransport>,
        session_id: Option<String>,
        server_capabilities: Option<Value>,
        conversation_id: String,
        model: Option<String>,
        event_sender: mpsc::Sender<NormalizedEvent>,
        notif_rx: mpsc::Receiver<JsonRpcNotification>,
        server_req_rx: mpsc::Receiver<JsonRpcServerRequest>,
        is_resume: bool,
    ) -> Self {
        Self {
            transport,
            session_id: Arc::new(RwLock::new(session_id)),
            server_capabilities: Arc::new(RwLock::new(server_capabilities)),
            conversation_id,
            model,
            event_sender,
            created_at: Utc::now().to_rfc3339(),
            notif_rx: Arc::new(Mutex::new(Some(notif_rx))),
            server_req_rx: Arc::new(Mutex::new(Some(server_req_rx))),
            pending_permissions: Arc::new(Mutex::new(HashMap::new())),
            replay_complete: Arc::new(AtomicBool::new(!is_resume)),
            replay_ready: Arc::new(Notify::new()),
            is_resume,
        }
    }

    pub async fn supports_images(&self) -> bool {
        let caps = self.server_capabilities.read().await;
        if let Some(ref c) = *caps {
            check_acp_image_support(c)
        } else {
            false
        }
    }

    pub async fn wait_replay_quiescence(&self, timeout: Duration) {
        if !self.replay_complete.load(Ordering::SeqCst) {
            let notified = self.replay_ready.notified();
            if !self.replay_complete.load(Ordering::SeqCst) {
                let _ = tokio::time::timeout(timeout, notified).await;
            }
        }
    }

    pub fn mark_replay_complete(&self) {
        self.replay_complete.store(true, Ordering::SeqCst);
        self.replay_ready.notify_waiters();
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
        let session_id_holder = self.session_id.clone();
        let pending_permissions = self.pending_permissions.clone();
        let replay_complete = self.replay_complete.clone();
        let replay_ready = self.replay_ready.clone();
        let is_resume = self.is_resume;

        let (activity_tx, mut activity_rx) = mpsc::channel::<()>(100);

        if is_resume {
            let replay_complete_clone = replay_complete.clone();
            let replay_ready_clone = replay_ready.clone();

            tokio::spawn(async move {
                let min_duration = Duration::from_millis(750);
                let quiet_duration = Duration::from_millis(250);
                let max_duration = Duration::from_millis(2500);
                let start = tokio::time::Instant::now();
                let mut last_activity = start;
                let mut seen_replay_activity = false;

                loop {
                    if start.elapsed() >= max_duration {
                        break;
                    }

                    tokio::select! {
                        act = activity_rx.recv() => {
                            if act.is_none() {
                                break;
                            }
                            seen_replay_activity = true;
                            last_activity = tokio::time::Instant::now();
                        }
                        _ = tokio::time::sleep(Duration::from_millis(50)) => {
                            if seen_replay_activity
                                && start.elapsed() >= min_duration
                                && last_activity.elapsed() >= quiet_duration
                            {
                                break;
                            }
                        }
                    }
                }

                replay_complete_clone.store(true, Ordering::SeqCst);
                replay_ready_clone.notify_waiters();
            });
        }

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
                        let proto_notif = AcpNotification {
                            jsonrpc: notif.jsonrpc.unwrap_or_else(|| "2.0".to_string()),
                            method: notif.method,
                            params: notif.params,
                        };

                        let current_sid = session_id_holder.read().await.clone();
                        if let Some(event) = gemini_notification_to_normalized(
                            &proto_notif,
                            &conv_id,
                            current_sid.as_deref(),
                        ) {
                            if !replay_complete.load(Ordering::SeqCst) {
                                match event.event_type {
                                    crate::app::events::EventType::TextDelta |
                                    crate::app::events::EventType::ThinkingDelta |
                                    crate::app::events::EventType::ToolStarted |
                                    crate::app::events::EventType::ToolProgress |
                                    crate::app::events::EventType::ToolResult => {
                                        let _ = activity_tx.try_send(());
                                        continue;
                                    }
                                    _ => {}
                                }
                            }
                            let _ = event_sender.send(event).await;
                        }
                    }
                    Some(server_req) = server_req_rx.recv() => {
                        let proto_req = AcpRequest {
                            jsonrpc: server_req.jsonrpc.unwrap_or_else(|| "2.0".to_string()),
                            id: server_req.id.clone(),
                            method: server_req.method.clone(),
                            params: server_req.params.clone(),
                        };

                        // Store options for ACP permission outcome response
                        let id_str = match &server_req.id {
                            Value::Number(n) => n.to_string(),
                            Value::String(s) => s.clone(),
                            other => other.to_string(),
                        };
                        let options = server_req
                            .params
                            .as_ref()
                            .and_then(|p| p.get("options"))
                            .and_then(|v| v.as_array())
                            .cloned()
                            .unwrap_or_default();

                        {
                            let mut pp = pending_permissions.lock().await;
                            pp.insert(id_str, options);
                        }

                        let current_sid = session_id_holder.read().await.clone();
                        if let Some(event) = gemini_server_request_to_normalized(
                            &proto_req,
                            &conv_id,
                            current_sid.as_deref(),
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
impl ProviderSession for GeminiSession {
    async fn send(&self, message: ProviderMessage) -> Result<()> {
        let sid = self.session_id.read().await.clone().unwrap_or_default();

        let has_images = message.attachments.iter().any(|a| a.kind == AttachmentKind::Image);
        if has_images && !self.supports_images().await {
            return Err(crate::app::error::AppError::InvalidInput(
                "Gemini ACP instance does not support image attachments".to_string(),
            ));
        }

        // Resume replay barrier: ensure historical replay is complete before sending prompt
        if !self.replay_complete.load(Ordering::SeqCst) {
            let notified = self.replay_ready.notified();
            if !self.replay_complete.load(Ordering::SeqCst) {
                let _ = tokio::time::timeout(Duration::from_millis(3000), notified).await;
            }
        }

        // Cross-provider context injection:
        let prompt_text = format_context_for_prompt(&message.context, &message.content);
        let prompt_params = build_gemini_prompt_params(&sid, &prompt_text, &message.attachments)?;

        let _ = self.transport.request_with_timeout(
            "session/prompt",
            prompt_params,
            Some(std::time::Duration::from_secs(1800)), // 30 mins for thinking, tool execution, and user approvals
        ).await?;

        // In ACP v1, response to session/prompt signifies that turn execution has completed
        let finished = NormalizedEvent::session_finished(ProviderKind::Gemini, self.conversation_id.clone());
        let _ = self.event_sender.send(finished).await;

        Ok(())
    }

    async fn interrupt(&self) -> Result<()> {
        self.cancel().await
    }

    async fn cancel(&self) -> Result<()> {
        let sid = self.session_id.read().await.clone().unwrap_or_default();
        let _ = self.transport.notify("session/cancel", json!({ "sessionId": sid })).await;
        Ok(())
    }

    async fn close(&self) -> Result<()> {
        self.cancel().await?;
        self.transport.process().shutdown(Duration::from_secs(5)).await?;
        Ok(())
    }

    fn native_session_id(&self) -> Option<String> {
        self.session_id.try_read().ok().and_then(|g| g.clone())
    }

    async fn respond_to_approval(&self, request_id: &str, approved: bool) -> Result<()> {
        let id_val = match request_id.parse::<u64>() {
            Ok(n) => json!(n),
            Err(_) => json!(request_id),
        };

        let options = {
            let mut pp = self.pending_permissions.lock().await;
            pp.remove(request_id).unwrap_or_default()
        };

        let result_payload = if approved {
            // Find option representing allow (allow_once, allow_always, etc.)
            let allow_opt_id = options.iter().find_map(|opt| {
                let option_id = opt.get("optionId").and_then(|v| v.as_str())?;
                let kind = opt.get("kind").and_then(|v| v.as_str()).unwrap_or("");
                if kind.eq_ignore_ascii_case("allow_once") || kind.eq_ignore_ascii_case("allow_always") {
                    Some(option_id.to_string())
                } else {
                    None
                }
            });

            match allow_opt_id {
                Some(option_id) => json!({
                    "outcome": {
                        "outcome": "selected",
                        "optionId": option_id
                    }
                }),
                None => {
                    tracing::warn!("No valid allow option found in Gemini ACP request; cancelling");
                    json!({
                        "outcome": {
                            "outcome": "cancelled"
                        }
                    })
                }
            }
        } else {
            // Find explicit reject option or default to cancelled outcome
            let reject_opt_id = options.iter().find_map(|opt| {
                let option_id = opt.get("optionId").and_then(|v| v.as_str())?;
                let kind = opt.get("kind").and_then(|v| v.as_str()).unwrap_or("");
                if kind.eq_ignore_ascii_case("reject_once") || kind.eq_ignore_ascii_case("reject_always") {
                    Some(option_id.to_string())
                } else {
                    None
                }
            });

            match reject_opt_id {
                Some(option_id) => json!({
                    "outcome": {
                        "outcome": "selected",
                        "optionId": option_id
                    }
                }),
                None => json!({
                    "outcome": {
                        "outcome": "cancelled"
                    }
                }),
            }
        };

        self.transport.respond_success(id_val, result_payload).await
    }

    fn metadata(&self) -> SessionMetadata {
        SessionMetadata {
            provider: ProviderKind::Gemini,
            native_session_id: self.native_session_id(),
            model: self.model.clone(),
            created_at: self.created_at.clone(),
        }
    }

    fn is_active(&self) -> bool {
        self.transport.is_active()
    }

    async fn supports_images(&self) -> bool {
        let caps = self.server_capabilities.read().await;
        if let Some(ref c) = *caps {
            check_acp_image_support(c)
        } else {
            false
        }
    }
}

pub fn resolve_canonical_cwd(working_dir: Option<&std::path::Path>) -> String {
    let resolved = match working_dir {
        Some(path) => {
            if path.is_absolute() {
                path.to_path_buf()
            } else if let Ok(current) = std::env::current_dir() {
                current.join(path)
            } else {
                path.to_path_buf()
            }
        }
        None => std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")),
    };

    let canonical = std::fs::canonicalize(&resolved).unwrap_or(resolved);
    let s = canonical.to_string_lossy().to_string();
    if let Some(stripped) = s.strip_prefix(r"\\?\") {
        stripped.to_string()
    } else {
        s
    }
}

pub fn check_acp_image_support(capabilities: &Value) -> bool {
    let caps = capabilities
        .get("serverCapabilities")
        .or_else(|| capabilities.get("capabilities"))
        .unwrap_or(capabilities);

    caps.get("promptCapabilities")
        .and_then(|p| p.get("image"))
        .or_else(|| caps.get("image"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

pub fn build_gemini_prompt_params(
    session_id: &str,
    prompt_text: &str,
    attachments: &[AttachmentRef],
) -> Result<Value> {
    let mut prompt_blocks = Vec::new();
    let mut non_image_header = String::new();

    for att in attachments {
        if att.kind == AttachmentKind::Image {
            let data = std::fs::read(&att.path)
                .map_err(|e| crate::app::error::AppError::Io(e.to_string()))?;
            let b64 = crate::app::attachments::base64_encode(&data);
            prompt_blocks.push(json!({
                "type": "image",
                "data": b64,
                "mimeType": att.mime_type,
            }));
        } else {
            let line = format!("[Attached File: {} ({}, {:.1} KB)]", att.path.to_string_lossy(), att.name, att.size_bytes as f64 / 1024.0);
            if !non_image_header.is_empty() {
                non_image_header.push('\n');
            }
            non_image_header.push_str(&line);
        }
    }

    let final_prompt = if !non_image_header.is_empty() {
        format!("{}\n\n{}", non_image_header, prompt_text)
    } else {
        prompt_text.to_string()
    };

    prompt_blocks.push(json!({
        "type": "text",
        "text": final_prompt,
    }));

    Ok(json!({
        "sessionId": session_id,
        "prompt": prompt_blocks,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn test_resolve_canonical_cwd() {
        let cwd_none = resolve_canonical_cwd(None);
        assert!(!cwd_none.is_empty());
        assert!(!cwd_none.starts_with(r"\\?\"));
        assert!(Path::new(&cwd_none).is_absolute());

        let cwd_cur = resolve_canonical_cwd(Some(Path::new(".")));
        assert!(!cwd_cur.is_empty());
        assert!(!cwd_cur.starts_with(r"\\?\"));
        assert!(Path::new(&cwd_cur).is_absolute());
    }
}
