use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use chrono::Utc;
use serde_json::json;
use tokio::sync::{mpsc, RwLock};
use tracing::{error, info};

use crate::app::conversation::context::format_context_for_prompt;
use crate::app::error::{AppError, Result};
use crate::app::events::{NormalizedEvent, ProviderKind};
use crate::app::process::{detect_executable, spawn, ManagedProcess, SpawnConfig};
use crate::app::providers::{
    AuthStatus, InstallationInfo, PermissionMode, Provider, ProviderCapabilities, ProviderMessage,
    ProviderSession, SessionConfig, SessionMetadata,
};

pub mod parser;
pub mod protocol;

use protocol::{make_acp_notification, make_acp_request, AcpMessage};

pub struct GeminiProvider {}

impl GeminiProvider {
    pub fn new() -> Self {
        Self {}
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
            token_usage: false,
            context_window: false,
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

        let spawn_config = SpawnConfig {
            executable: "gemini".to_string(),
            args: vec![
                "--acp".to_string(),
                "--approval-mode".to_string(),
                approval_mode.to_string(),
            ],
            working_dir: config.working_dir.clone(),
            env: config.env.clone(),
            startup_timeout: Duration::from_secs(10),
        };

        let process = Arc::new(spawn(spawn_config).await?);
        let request_id_counter = Arc::new(AtomicU64::new(1));
        let (cancel_tx, cancel_rx) = mpsc::channel::<()>(1);
        let session_id_holder = Arc::new(RwLock::new(None));

        // Step 1: initialize
        let init_id = request_id_counter.fetch_add(1, Ordering::SeqCst);
        let init_req = make_acp_request(
            init_id,
            "initialize",
            Some(json!({
                "clientInfo": {
                    "name": "Seralyn",
                    "version": "0.1.0"
                }
            })),
        );
        process.send_line(&serde_json::to_string(&init_req).unwrap()).await?;

        // Step 2: session/new
        let new_sess_id = request_id_counter.fetch_add(1, Ordering::SeqCst);
        let new_sess_req = make_acp_request(
            new_sess_id,
            "session/new",
            Some(json!({
                "instructions": config.system_prompt.unwrap_or_default(),
            })),
        );
        process.send_line(&serde_json::to_string(&new_sess_req).unwrap()).await?;

        let session = GeminiSession::new(
            process,
            session_id_holder,
            config.conversation_id,
            config.model,
            request_id_counter,
            cancel_tx,
        );

        session.start_reader_task(config.event_sender, cancel_rx);

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

        let spawn_config = SpawnConfig {
            executable: "gemini".to_string(),
            args: vec![
                "--acp".to_string(),
                "--approval-mode".to_string(),
                approval_mode.to_string(),
            ],
            working_dir: config.working_dir.clone(),
            env: config.env.clone(),
            startup_timeout: Duration::from_secs(10),
        };

        let process = Arc::new(spawn(spawn_config).await?);
        let request_id_counter = Arc::new(AtomicU64::new(1));
        let (cancel_tx, cancel_rx) = mpsc::channel::<()>(1);
        let session_id_holder = Arc::new(RwLock::new(Some(native_session_id.to_string())));

        // Initialize ACP
        let init_id = request_id_counter.fetch_add(1, Ordering::SeqCst);
        let init_req = make_acp_request(
            init_id,
            "initialize",
            Some(json!({
                "clientInfo": {
                    "name": "Seralyn",
                    "version": "0.1.0"
                }
            })),
        );
        process.send_line(&serde_json::to_string(&init_req).unwrap()).await?;

        // Resume native session in ACP
        let resume_id = request_id_counter.fetch_add(1, Ordering::SeqCst);
        let resume_req = make_acp_request(
            resume_id,
            "session/resume",
            Some(json!({
                "sessionId": native_session_id,
            })),
        );
        process.send_line(&serde_json::to_string(&resume_req).unwrap()).await?;

        let session = GeminiSession::new(
            process,
            session_id_holder,
            config.conversation_id,
            config.model,
            request_id_counter,
            cancel_tx,
        );

        session.start_reader_task(config.event_sender, cancel_rx);

        Ok(Box::new(session))
    }
}

pub struct GeminiSession {
    process: Arc<ManagedProcess>,
    session_id: Arc<RwLock<Option<String>>>,
    conversation_id: String,
    model: Option<String>,
    created_at: String,
    request_id_counter: Arc<AtomicU64>,
    cancel_tx: mpsc::Sender<()>,
    is_active: Arc<RwLock<bool>>,
}

impl GeminiSession {
    pub fn new(
        process: Arc<ManagedProcess>,
        session_id: Arc<RwLock<Option<String>>>,
        conversation_id: String,
        model: Option<String>,
        request_id_counter: Arc<AtomicU64>,
        cancel_tx: mpsc::Sender<()>,
    ) -> Self {
        Self {
            process,
            session_id,
            conversation_id,
            model,
            created_at: Utc::now().to_rfc3339(),
            request_id_counter,
            cancel_tx,
            is_active: Arc::new(RwLock::new(true)),
        }
    }

    pub fn start_reader_task(
        &self,
        event_sender: mpsc::Sender<NormalizedEvent>,
        mut cancel_rx: mpsc::Receiver<()>,
    ) {
        let process = self.process.clone();
        let conv_id = self.conversation_id.clone();
        let session_id_holder = self.session_id.clone();
        let is_active = self.is_active.clone();

        tokio::spawn(async move {
            while let Ok(Some(line)) = process.recv_stdout_line().await {
                // Check for cancel request
                if cancel_rx.try_recv().is_ok() {
                    let cancel_notif = make_acp_notification("session/cancel", None);
                    let _ = process.send_line(&serde_json::to_string(&cancel_notif).unwrap()).await;
                }

                if let Ok(Some(msg)) = parser::parse_gemini_line(&line) {
                    match msg {
                        AcpMessage::Response(res) => {
                            // Handshake: initialized notification
                            if res.id == 1 {
                                let init_notif = make_acp_notification("initialized", None);
                                let _ = process.send_line(&serde_json::to_string(&init_notif).unwrap()).await;
                            }
                            // Extract real sessionId from session/new response
                            if let Some(result_obj) = &res.result {
                                if let Some(sid) = result_obj.get("sessionId").and_then(|v| v.as_str()) {
                                    let mut s = session_id_holder.write().await;
                                    *s = Some(sid.to_string());
                                }
                            }
                        }
                        AcpMessage::Notification(notif) => {
                            let cur_sid = session_id_holder.read().await.clone();
                            if let Some(event) = parser::gemini_notification_to_normalized(
                                &notif,
                                &conv_id,
                                cur_sid.as_deref(),
                            ) {
                                let _ = event_sender.send(event).await;
                            }
                        }
                        _ => {}
                    }
                }
            }

            let mut active = is_active.write().await;
            *active = false;
        });
    }
}

#[async_trait]
impl ProviderSession for GeminiSession {
    async fn send(&mut self, message: ProviderMessage) -> Result<()> {
        let sid = self.session_id.read().await.clone();

        // Cross-provider context injection:
        // If session has no prior native turn, inject cross-provider turns
        let prompt_text = if sid.is_some() {
            message.content
        } else {
            format_context_for_prompt(&message.context, &message.content)
        };

        let req_id = self.request_id_counter.fetch_add(1, Ordering::SeqCst);
        let req = make_acp_request(
            req_id,
            "session/prompt",
            Some(json!({
                "sessionId": sid,
                "prompt": [
                    { "type": "text", "text": prompt_text }
                ]
            })),
        );
        let s = serde_json::to_string(&req).unwrap();
        self.process.send_line(&s).await?;
        Ok(())
    }

    async fn interrupt(&mut self) -> Result<()> {
        self.cancel().await
    }

    async fn cancel(&mut self) -> Result<()> {
        let _ = self.cancel_tx.send(()).await;
        Ok(())
    }

    async fn close(&mut self) -> Result<()> {
        let req_id = self.request_id_counter.fetch_add(1, Ordering::SeqCst);
        let shutdown = make_acp_request(req_id, "shutdown", None);
        if let Ok(s) = serde_json::to_string(&shutdown) {
            let _ = self.process.send_line(&s).await;
        }

        let exit = make_acp_notification("exit", None);
        if let Ok(s) = serde_json::to_string(&exit) {
            let _ = self.process.send_line(&s).await;
        }

        self.process.shutdown(Duration::from_secs(2)).await?;
        let mut active = self.is_active.write().await;
        *active = false;
        Ok(())
    }

    fn native_session_id(&self) -> Option<&str> {
        None
    }

    fn metadata(&self) -> SessionMetadata {
        SessionMetadata {
            provider: ProviderKind::Gemini,
            native_session_id: self.session_id.try_read().ok().and_then(|g| g.clone()),
            model: self.model.clone(),
            created_at: self.created_at.clone(),
        }
    }

    fn is_active(&self) -> bool {
        self.is_active.try_read().map(|g| *g).unwrap_or(true)
    }
}
