pub mod parser;
pub mod protocol;

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde_json::json;
use tokio::sync::{mpsc, RwLock};
use tracing::{error, info};

use crate::app::conversation::context::format_context_for_prompt;
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

        let process = Arc::new(spawn(spawn_config).await?);
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

        let session = CodexSession::new(
            process,
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

        let process = Arc::new(spawn(spawn_config).await?);
        let request_id_counter = Arc::new(AtomicU64::new(1));

        let id = request_id_counter.fetch_add(1, Ordering::SeqCst);
        let init_req = make_request(id, "initialize", json!({}));
        process.send_line(&init_req).await?;

        // Use thread/resume according to OpenAI Codex app-server protocol
        let id = request_id_counter.fetch_add(1, Ordering::SeqCst);
        let thread_req = make_request(
            id,
            "thread/resume",
            json!({
                "threadId": native_session_id,
            }),
        );
        process.send_line(&thread_req).await?;

        let session = CodexSession::new(
            process,
            request_id_counter,
            config.conversation_id,
            config.event_sender,
        );
        {
            let mut tid = session.thread_id.write().await;
            *tid = Some(native_session_id.to_string());
        }

        session.start_reader_task();

        Ok(Box::new(session))
    }
}

pub struct CodexSession {
    pub process: Arc<ManagedProcess>,
    pub request_id_counter: Arc<AtomicU64>,
    pub thread_id: Arc<RwLock<Option<String>>>,
    pub event_sender: mpsc::Sender<NormalizedEvent>,
    pub conversation_id: String,
    pub created_at: String,
    pub is_active: Arc<RwLock<bool>>,
}

impl CodexSession {
    pub fn new(
        process: Arc<ManagedProcess>,
        request_id_counter: Arc<AtomicU64>,
        conversation_id: String,
        event_sender: mpsc::Sender<NormalizedEvent>,
    ) -> Self {
        Self {
            process,
            request_id_counter,
            thread_id: Arc::new(RwLock::new(None)),
            event_sender,
            conversation_id,
            created_at: chrono::Utc::now().to_rfc3339(),
            is_active: Arc::new(RwLock::new(true)),
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
        let thread_id_holder = self.thread_id.clone();

        tokio::spawn(async move {
            while let Ok(Some(line)) = process.recv_stdout_line().await {
                match parse_codex_line(&line) {
                    Ok(Some(CodexMessage::Notification(not))) => {
                        let current_tid = thread_id_holder.read().await.clone();
                        if let Some(event) = codex_notification_to_normalized(
                            &not,
                            &conversation_id,
                            current_tid.as_deref(),
                        ) {
                            let _ = event_sender.send(event).await;
                        }
                    }
                    Ok(Some(CodexMessage::Response(res))) => {
                        // Extract thread ID from thread/start response
                        if let Some(result_obj) = &res.result {
                            if let Some(thread_obj) = result_obj.get("thread") {
                                if let Some(id) = thread_obj.get("id").and_then(|v| v.as_str()) {
                                    let mut tid = thread_id_holder.write().await;
                                    *tid = Some(id.to_string());
                                }
                            }
                        }
                    }
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
        let tid = self.thread_id.read().await.clone();

        // Cross-provider context injection:
        // If thread has no native history yet, inject prior cross-provider turns!
        let prompt_text = if tid.is_some() {
            message.content
        } else {
            format_context_for_prompt(&message.context, &message.content)
        };

        let id = self.next_id();
        // Updated TurnStartParams according to OpenAI Codex app-server schema:
        // turn/start requires { threadId, input: [{ type: "text", text }] }
        let req = make_request(
            id,
            "turn/start",
            json!({
                "threadId": tid,
                "input": [{
                    "type": "text",
                    "text": prompt_text,
                }]
            }),
        );
        // Non-blocking lock-free stdin send:
        self.process.send_line(&req).await?;
        Ok(())
    }

    async fn interrupt(&mut self) -> Result<()> {
        let tid = self.thread_id.read().await.clone();
        let id = self.next_id();
        let req = make_request(
            id,
            "turn/cancel",
            json!({
                "threadId": tid,
            }),
        );
        self.process.send_line(&req).await?;
        Ok(())
    }

    async fn cancel(&mut self) -> Result<()> {
        self.interrupt().await
    }

    async fn close(&mut self) -> Result<()> {
        self.process.shutdown(Duration::from_secs(5)).await?;
        let mut active = self.is_active.write().await;
        *active = false;
        Ok(())
    }

    fn native_session_id(&self) -> Option<&str> {
        // Safe synchronous inspect if initialized
        None
    }

    fn metadata(&self) -> SessionMetadata {
        SessionMetadata {
            provider: ProviderKind::Codex,
            native_session_id: self.thread_id.try_read().ok().and_then(|g| g.clone()),
            model: None,
            created_at: self.created_at.clone(),
        }
    }

    fn is_active(&self) -> bool {
        self.is_active.try_read().map(|g| *g).unwrap_or(true)
    }
}
