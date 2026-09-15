pub mod parser;
pub mod protocol;

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde_json::json;
use tokio::sync::{mpsc, Mutex};
use tracing::{error, info};

use crate::app::error::{AppError, Result};
use crate::app::events::{NormalizedEvent, ProviderKind};
use crate::app::process::{detect_executable, spawn, ManagedProcess, SpawnConfig};
use crate::app::providers::{
    AuthStatus, InstallationInfo, Provider, ProviderCapabilities, ProviderMessage, ProviderSession,
    SessionConfig, SessionMetadata,
};
use parser::{codex_notification_to_normalized, parse_codex_line, CodexMessage};
use protocol::make_request;

pub struct CodexProvider;

impl CodexProvider {
    pub fn new() -> Self {
        Self
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

        let mut process = spawn(spawn_config).await?;
        let request_id_counter = Arc::new(AtomicU64::new(1));

        let id = request_id_counter.fetch_add(1, Ordering::SeqCst);
        let init_req = make_request(id, "initialize", json!({}));
        process.send_line(&init_req).await?;

        let id = request_id_counter.fetch_add(1, Ordering::SeqCst);
        let thread_req = make_request(
            id,
            "thread/start",
            json!({
                "instructions": config.system_prompt.unwrap_or_default(),
                "model": config.model.unwrap_or_default(),
            }),
        );
        process.send_line(&thread_req).await?;

        let process_arc = Arc::new(Mutex::new(process));
        let session = CodexSession::new(
            process_arc,
            request_id_counter,
            config.conversation_id,
            config.event_sender,
        );

        session.start_reader_task();

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

        let mut process = spawn(spawn_config).await?;
        let request_id_counter = Arc::new(AtomicU64::new(1));

        let id = request_id_counter.fetch_add(1, Ordering::SeqCst);
        let init_req = make_request(id, "initialize", json!({}));
        process.send_line(&init_req).await?;

        let id = request_id_counter.fetch_add(1, Ordering::SeqCst);
        let thread_req = make_request(
            id,
            "thread/start",
            json!({
                "threadId": native_session_id,
            }),
        );
        process.send_line(&thread_req).await?;

        let process_arc = Arc::new(Mutex::new(process));
        let mut session = CodexSession::new(
            process_arc,
            request_id_counter,
            config.conversation_id,
            config.event_sender,
        );
        session.thread_id = Some(native_session_id.to_string());

        session.start_reader_task();

        Ok(Box::new(session))
    }
}

pub struct CodexSession {
    pub process: Arc<Mutex<ManagedProcess>>,
    pub request_id_counter: Arc<AtomicU64>,
    pub thread_id: Option<String>,
    pub event_sender: mpsc::Sender<NormalizedEvent>,
    pub conversation_id: String,
    pub created_at: String,
    pub is_active: Arc<tokio::sync::RwLock<bool>>,
}

impl CodexSession {
    pub fn new(
        process: Arc<Mutex<ManagedProcess>>,
        request_id_counter: Arc<AtomicU64>,
        conversation_id: String,
        event_sender: mpsc::Sender<NormalizedEvent>,
    ) -> Self {
        Self {
            process,
            request_id_counter,
            thread_id: None,
            event_sender,
            conversation_id,
            created_at: chrono::Utc::now().to_rfc3339(),
            is_active: Arc::new(tokio::sync::RwLock::new(true)),
        }
    }

    pub fn next_id(&self) -> u64 {
        self.request_id_counter.fetch_add(1, Ordering::SeqCst)
    }

    pub fn start_reader_task(&self) {
        let process = self.process.clone();
        let event_sender = self.event_sender.clone();
        let conversation_id = self.conversation_id.clone();
        let is_active = self.is_active.clone();

        tokio::spawn(async move {
            loop {
                let line_opt = {
                    let mut guard = process.lock().await;
                    guard.recv_stdout_line().await.ok().flatten()
                };

                let Some(line) = line_opt else {
                    break;
                };

                match parse_codex_line(&line) {
                    Ok(Some(CodexMessage::Notification(not))) => {
                        if let Some(event) = codex_notification_to_normalized(
                            &not,
                            &conversation_id,
                            None,
                        ) {
                            let _ = event_sender.send(event).await;
                        }
                    }
                    Ok(Some(CodexMessage::Response(_))) => {}
                    Ok(_) => {}
                    Err(e) => {
                        error!("Failed to parse codex line: {}", e);
                    }
                }
            }

            let mut active = is_active.write().await;
            *active = false;
        });
    }
}

#[async_trait]
impl ProviderSession for CodexSession {
    async fn send(&mut self, message: ProviderMessage) -> Result<()> {
        let id = self.next_id();
        let req = make_request(
            id,
            "turn/start",
            json!({
                "threadId": self.thread_id,
                "content": [{
                    "type": "input_text",
                    "text": message.content,
                }]
            }),
        );
        let mut guard = self.process.lock().await;
        guard.send_line(&req).await?;
        Ok(())
    }

    async fn interrupt(&mut self) -> Result<()> {
        let id = self.next_id();
        let req = make_request(
            id,
            "turn/cancel",
            json!({
                "threadId": self.thread_id,
            }),
        );
        let mut guard = self.process.lock().await;
        guard.send_line(&req).await?;
        Ok(())
    }

    async fn cancel(&mut self) -> Result<()> {
        self.interrupt().await
    }

    async fn close(&mut self) -> Result<()> {
        let mut guard = self.process.lock().await;
        let _ = guard.shutdown(Duration::from_secs(5)).await;
        let mut active = self.is_active.write().await;
        *active = false;
        Ok(())
    }

    fn native_session_id(&self) -> Option<&str> {
        self.thread_id.as_deref()
    }

    fn metadata(&self) -> SessionMetadata {
        SessionMetadata {
            provider: ProviderKind::Codex,
            native_session_id: self.thread_id.clone(),
            model: None,
            created_at: self.created_at.clone(),
        }
    }

    fn is_active(&self) -> bool {
        // Safe sync check via try_read or default true
        self.is_active.try_read().map(|g| *g).unwrap_or(true)
    }
}
