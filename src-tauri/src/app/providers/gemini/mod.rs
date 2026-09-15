use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use chrono::Utc;
use serde_json::json;
use tokio::sync::{mpsc, Mutex};
use uuid::Uuid;

use crate::app::error::{AppError, Result};
use crate::app::events::{NormalizedEvent, ProviderKind};
use crate::app::process::{detect_executable, spawn, ManagedProcess, SpawnConfig};
use crate::app::providers::{
    AuthStatus, InstallationInfo, Provider, ProviderCapabilities, ProviderMessage, ProviderSession,
    SessionConfig, SessionMetadata,
};

pub mod parser;
pub mod protocol;

use protocol::{make_acp_request, AcpMessage};

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
        // Authentication check relies on specific commands. Since the exact command
        // might vary and we can't definitively check via ACP without launching a full session,
        // we return Unknown as requested.
        Ok(AuthStatus::Unknown)
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            text: true,
            streaming: true,
            tools: true,
            resume: false,
            token_usage: false,
            reasoning: false,
            ..Default::default()
        }
    }

    async fn create_session(&self, config: SessionConfig) -> Result<Box<dyn ProviderSession>> {
        let spawn_config = SpawnConfig {
            executable: "gemini".to_string(),
            args: vec!["--acp".to_string()],
            working_dir: config.working_dir.clone(),
            env: config.env.clone(),
            startup_timeout: Duration::from_secs(10),
        };

        let mut process = spawn(spawn_config).await?;
        let session_id = Uuid::new_v4().to_string();

        let init_req = make_acp_request(
            1,
            "initialize",
            Some(json!({
                "clientInfo": {
                    "name": "Seralyn",
                    "version": "1.0.0"
                }
            })),
        );
        let init_str = serde_json::to_string(&init_req).unwrap();
        process.send_line(&init_str).await?;

        let (cancel_tx, mut cancel_rx) = mpsc::channel::<()>(1);
        let request_id_counter = Arc::new(AtomicU64::new(2));
        let process_arc = Arc::new(Mutex::new(process));

        let bg_process = process_arc.clone();
        let bg_event_sender = config.event_sender.clone();
        let bg_conv_id = config.conversation_id.clone();
        let bg_sess_id = session_id.clone();

        tokio::spawn(async move {
            let mut process_guard = bg_process.lock().await;

            // Wait for initialize response
            while let Ok(Some(line)) = process_guard.recv_stdout_line().await {
                if let Ok(Some(msg)) = parser::parse_gemini_line(&line) {
                    if let AcpMessage::Response(res) = msg {
                        if res.id == 1 {
                            tracing::info!("Gemini ACP initialized");
                            let init_notif = protocol::make_acp_notification("initialized", None);
                            let notif_str = serde_json::to_string(&init_notif).unwrap();
                            let _ = process_guard.send_line(&notif_str).await;
                            break;
                        }
                    }
                }
            }

            drop(process_guard);

            loop {
                if cancel_rx.try_recv().is_ok() {
                    let mut p = bg_process.lock().await;
                    let cancel_notif = protocol::make_acp_notification("agent/cancel", None);
                    if let Ok(s) = serde_json::to_string(&cancel_notif) {
                        let _ = p.send_line(&s).await;
                    }
                }

                let mut p = bg_process.lock().await;
                if !p.is_alive() {
                    break;
                }

                let line_res = tokio::time::timeout(Duration::from_millis(50), p.recv_stdout_line()).await;
                drop(p);

                if let Ok(Ok(Some(line))) = line_res {
                    if let Ok(Some(msg)) = parser::parse_gemini_line(&line) {
                        if let AcpMessage::Notification(notif) = msg {
                            if let Some(event) = parser::gemini_notification_to_normalized(
                                &notif,
                                &bg_conv_id,
                                Some(&bg_sess_id),
                            ) {
                                if bg_event_sender.send(event).await.is_err() {
                                    break;
                                }
                            }
                        }
                    }
                } else if let Ok(Ok(None)) = line_res {
                    break; // EOF
                }
            }
        });

        Ok(Box::new(GeminiSession {
            process: process_arc,
            session_id,
            conversation_id: config.conversation_id,
            model: config.model,
            created_at: Utc::now().to_rfc3339(),
            request_id_counter,
            cancel_tx,
        }))
    }

    async fn resume_session(
        &self,
        _native_session_id: &str,
        _config: SessionConfig,
    ) -> Result<Box<dyn ProviderSession>> {
        Err(AppError::InvalidInput(
            "Gemini ACP provider does not currently support session resumption".to_string(),
        ))
    }
}

pub struct GeminiSession {
    process: Arc<Mutex<ManagedProcess>>,
    session_id: String,
    conversation_id: String,
    model: Option<String>,
    created_at: String,
    request_id_counter: Arc<AtomicU64>,
    cancel_tx: mpsc::Sender<()>,
}

#[async_trait]
impl ProviderSession for GeminiSession {
    async fn send(&mut self, message: ProviderMessage) -> Result<()> {
        let req_id = self.request_id_counter.fetch_add(1, Ordering::SeqCst);
        let req = make_acp_request(
            req_id,
            "agent/execute",
            Some(json!({
                "message": message.content,
                "context": message.context,
            })),
        );
        let s = serde_json::to_string(&req).unwrap();
        let mut p = self.process.lock().await;
        p.send_line(&s).await?;
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
        let mut p = self.process.lock().await;
        let req_id = self.request_id_counter.fetch_add(1, Ordering::SeqCst);
        let shutdown = make_acp_request(req_id, "shutdown", None);
        if let Ok(s) = serde_json::to_string(&shutdown) {
            let _ = p.send_line(&s).await;
        }

        let exit = protocol::make_acp_notification("exit", None);
        if let Ok(s) = serde_json::to_string(&exit) {
            let _ = p.send_line(&s).await;
        }

        p.shutdown(Duration::from_secs(2)).await?;
        Ok(())
    }

    fn native_session_id(&self) -> Option<&str> {
        Some(&self.session_id)
    }

    fn metadata(&self) -> SessionMetadata {
        SessionMetadata {
            provider: ProviderKind::Gemini,
            native_session_id: Some(self.session_id.clone()),
            model: self.model.clone(),
            created_at: self.created_at.clone(),
        }
    }

    fn is_active(&self) -> bool {
        if let Ok(guard) = self.process.try_lock() {
            guard.is_alive()
        } else {
            true // Cannot acquire lock right now, assume active
        }
    }
}
