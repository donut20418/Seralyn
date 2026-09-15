pub mod parser;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use chrono::Utc;
use tokio::sync::{Mutex, RwLock};

use crate::app::conversation::context::format_context_for_prompt;
use crate::app::error::Result;
use crate::app::events::{NormalizedEvent, ProviderKind};
use crate::app::process::{detect_executable, spawn, ManagedProcess, SpawnConfig};
use crate::app::providers::{
    AuthStatus, InstallationInfo, PermissionMode, Provider, ProviderCapabilities,
    ProviderMessage, ProviderSession, SessionConfig, SessionMetadata,
};
use parser::{claude_event_to_normalized, parse_claude_line, ClaudeEvent};

pub struct ClaudeProvider;

impl ClaudeProvider {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ClaudeProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for ClaudeProvider {
    fn kind(&self) -> ProviderKind {
        ProviderKind::Claude
    }

    async fn detect_installation(&self) -> Result<InstallationInfo> {
        match detect_executable("claude").await {
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
        let spawn_config = SpawnConfig {
            executable: "claude".to_string(),
            args: vec!["doctor".to_string()],
            working_dir: None,
            env: std::collections::HashMap::new(),
            startup_timeout: Duration::from_secs(5),
        };

        match spawn(spawn_config).await {
            Ok(_) => Ok(AuthStatus::NotAuthenticated),
            Err(_) => Ok(AuthStatus::Unknown),
        }
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
            reasoning: true,
            streaming: true,
        }
    }

    async fn create_session(
        &self,
        config: SessionConfig,
    ) -> Result<Box<dyn ProviderSession>> {
        Ok(Box::new(ClaudeSession::new(config, None)))
    }

    async fn resume_session(
        &self,
        native_session_id: &str,
        config: SessionConfig,
    ) -> Result<Box<dyn ProviderSession>> {
        Ok(Box::new(ClaudeSession::new(
            config,
            Some(native_session_id.to_string()),
        )))
    }
}

pub struct ClaudeSession {
    config: SessionConfig,
    native_session_id: Arc<RwLock<Option<String>>>,
    model: Option<String>,
    active: Arc<AtomicBool>,
    created_at: String,
    current_process: Arc<Mutex<Option<Arc<ManagedProcess>>>>,
}

impl ClaudeSession {
    pub fn new(config: SessionConfig, native_session_id: Option<String>) -> Self {
        Self {
            model: config.model.clone(),
            config,
            native_session_id: Arc::new(RwLock::new(native_session_id)),
            active: Arc::new(AtomicBool::new(true)),
            created_at: Utc::now().to_rfc3339(),
            current_process: Arc::new(Mutex::new(None)),
        }
    }
}

#[async_trait]
impl ProviderSession for ClaudeSession {
    async fn send(&self, message: ProviderMessage) -> Result<()> {
        let sid_opt = self.native_session_id.read().await.clone();

        // Cross-provider context injection:
        let prompt_text = format_context_for_prompt(&message.context, &message.content);

        let mut args = vec![
            "-p".to_string(),
            prompt_text,
            "--output-format".to_string(),
            "stream-json".to_string(),
        ];

        // Resume flag semantics
        if let Some(sid) = &sid_opt {
            args.push("--resume".to_string());
            args.push(sid.clone());
        }

        // Enforce permission mode
        if self.config.permission_mode == PermissionMode::FullAccess {
            args.push("--dangerously-skip-permissions".to_string());
        }

        let spawn_config = SpawnConfig {
            executable: "claude".to_string(),
            args,
            working_dir: self.config.working_dir.clone(),
            env: self.config.env.clone(),
            startup_timeout: Duration::from_secs(10),
        };

        let process = spawn(spawn_config).await?;
        let process_arc = Arc::new(process);
        {
            let mut cp = self.current_process.lock().await;
            *cp = Some(process_arc.clone());
        }

        let event_sender = self.config.event_sender.clone();
        let provider = ProviderKind::Claude;
        let conv_id = self.config.conversation_id.clone();
        let current_process_holder = self.current_process.clone();
        let sid_holder = self.native_session_id.clone();
        let process_for_task = process_arc.clone();

        tokio::spawn(async move {
            let mut finished_sent = false;
            while let Ok(Some(line)) = process_for_task.recv_stdout_line().await {
                if let Ok(Some(event)) = parse_claude_line(&line) {
                    if let ClaudeEvent::Result { session_id, .. } = &event {
                        if let Some(sid) = session_id {
                            let mut guard = sid_holder.write().await;
                            *guard = Some(sid.clone());
                        }
                    }

                    let current_sid = sid_holder.read().await.clone();

                    if let Some(normalized) = claude_event_to_normalized(
                        event,
                        provider.clone(),
                        &conv_id,
                        current_sid.as_deref(),
                    ) {
                        if normalized.event_type == crate::app::events::EventType::SessionFinished {
                            finished_sent = true;
                        }
                        let _ = event_sender.send(normalized).await;
                    }
                }
            }

            if !finished_sent {
                let finish_event = NormalizedEvent::session_finished(provider.clone(), conv_id.clone());
                let _ = event_sender.send(finish_event).await;
            }

            let _ = process_for_task.shutdown(Duration::from_secs(5)).await;
            let mut cp = current_process_holder.lock().await;
            *cp = None;
        });

        Ok(())
    }

    async fn interrupt(&self) -> Result<()> {
        self.cancel().await
    }

    async fn cancel(&self) -> Result<()> {
        let cp = self.current_process.lock().await;
        if let Some(p) = cp.as_ref() {
            let _ = p.kill().await;
        }
        Ok(())
    }

    async fn close(&self) -> Result<()> {
        self.cancel().await?;
        self.active.store(false, Ordering::SeqCst);
        Ok(())
    }

    fn native_session_id(&self) -> Option<String> {
        self.native_session_id.try_read().ok().and_then(|g| g.clone())
    }

    async fn respond_to_approval(&self, _request_id: &str, _approved: bool) -> Result<()> {
        Ok(())
    }

    fn metadata(&self) -> SessionMetadata {
        SessionMetadata {
            provider: ProviderKind::Claude,
            native_session_id: self.native_session_id(),
            model: self.model.clone(),
            created_at: self.created_at.clone(),
        }
    }

    fn is_active(&self) -> bool {
        self.active.load(Ordering::SeqCst)
    }
}
