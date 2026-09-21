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
    resolve_profile_dir, AuthStatus, InstallationInfo, PermissionMode, Provider,
    ProviderCapabilities, ProviderMessage, ProviderSession, SessionConfig, SessionMetadata,
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
        // 1. Check API key in environment
        if let Ok(key) = std::env::var("ANTHROPIC_API_KEY") {
            if !key.trim().is_empty() {
                return Ok(AuthStatus::Authenticated);
            }
        }

        // 2. Check claude auth status
        let spawn_config = SpawnConfig {
            executable: "claude".to_string(),
            args: vec!["auth".to_string(), "status".to_string()],
            working_dir: None,
            env: std::collections::HashMap::new(),
            startup_timeout: Duration::from_secs(5),
        };

        let process = match spawn(spawn_config).await {
            Ok(p) => p,
            Err(_) => return Ok(AuthStatus::Unknown),
        };

        let mut output = String::new();
        while let Ok(Some(line)) = process.recv_stdout_line().await {
            output.push_str(&line);
            output.push('\n');
        }

        if let Ok(val) = serde_json::from_str::<serde_json::Value>(&output) {
            if val.get("loggedIn").and_then(|v| v.as_bool()) == Some(true) {
                return Ok(AuthStatus::Authenticated);
            } else if val.get("loggedIn").and_then(|v| v.as_bool()) == Some(false) {
                return Ok(AuthStatus::NotAuthenticated);
            }
        }

        if output.contains("\"loggedIn\": true") || output.contains("\"loggedIn\":true") {
            Ok(AuthStatus::Authenticated)
        } else if output.contains("\"loggedIn\": false") || output.contains("\"loggedIn\":false") {
            Ok(AuthStatus::NotAuthenticated)
        } else {
            Ok(AuthStatus::Unknown)
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
            context_window: true,
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
        if message.attachments.iter().any(|a| a.kind == AttachmentKind::Image) {
            return Err(crate::app::error::AppError::InvalidInput(
                "Claude does not support image attachments".to_string(),
            ));
        }

        let sid_opt = self.native_session_id.read().await.clone();

        // Cross-provider context injection:
        let prompt_text = format_context_for_prompt(&message.context, &message.content);

        let final_prompt = if !message.attachments.is_empty() {
            let att_header = message.attachments
                .iter()
                .map(|a| format!("[Attached File: {} ({}, {:.1} KB)]", a.path.to_string_lossy(), a.name, a.size_bytes as f64 / 1024.0))
                .collect::<Vec<_>>()
                .join("\n");
            format!("{}\n\n{}", att_header, prompt_text)
        } else {
            prompt_text
        };

        let args = build_claude_args(
            &final_prompt,
            self.model.as_deref(),
            self.config.effort.as_deref(),
            sid_opt.as_deref(),
            self.config.permission_mode,
        );

        // Account profile isolation via dedicated config directory
        let mut env = self.config.env.clone();
        if let Some(profile_dir) = resolve_profile_dir(ProviderKind::Claude, self.config.account.as_deref()) {
            env.insert("CLAUDE_CONFIG_DIR".to_string(), profile_dir.to_string_lossy().to_string());
        }

        let spawn_config = SpawnConfig {
            executable: "claude".to_string(),
            args,
            working_dir: self.config.working_dir.clone(),
            env,
            startup_timeout: Duration::from_secs(10),
        };

        let process = spawn(spawn_config).await?;
        let _ = process.send_line("").await;
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
            let mut streamed_deltas = false;

            while let Ok(Some(line)) = process_for_task.recv_stdout_line().await {
                if let Ok(Some(event)) = parse_claude_line(&line) {
                    // 1. Capture session ID as early as possible (Init or Result)
                    match &event {
                        ClaudeEvent::Init { session_id: Some(sid), .. } => {
                            let mut guard = sid_holder.write().await;
                            if guard.is_none() {
                                *guard = Some(sid.clone());
                            }
                        }
                        ClaudeEvent::Result { session_id: Some(sid), .. } => {
                            let mut guard = sid_holder.write().await;
                            if guard.is_none() {
                                *guard = Some(sid.clone());
                            }
                        }
                        _ => {}
                    }

                    // 2. Track whether token deltas have been streamed
                    if let ClaudeEvent::ContentBlockDelta { delta_type, .. } = &event {
                        if delta_type == "text_delta" {
                            streamed_deltas = true;
                        }
                    }

                    // 3. Suppress duplicate complete text if deltas have already streamed
                    if streamed_deltas && matches!(&event, ClaudeEvent::AssistantMessage { .. }) {
                        continue;
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

    async fn supports_images(&self) -> bool {
        false
    }
}

pub fn build_claude_args(
    prompt_text: &str,
    model: Option<&str>,
    effort: Option<&str>,
    resume_sid: Option<&str>,
    permission_mode: PermissionMode,
) -> Vec<String> {
    let mut args = vec![
        "-p".to_string(),
        prompt_text.to_string(),
        "--output-format".to_string(),
        "stream-json".to_string(),
        "--verbose".to_string(),
        "--include-partial-messages".to_string(),
    ];

    if let Some(m) = model {
        args.push("--model".to_string());
        args.push(m.to_string());
    }

    if let Some(e) = effort {
        args.push("--effort".to_string());
        args.push(e.to_string());
    }

    if let Some(sid) = resume_sid {
        args.push("--resume".to_string());
        args.push(sid.to_string());
    }

    if permission_mode == PermissionMode::FullAccess {
        args.push("--dangerously-skip-permissions".to_string());
    }

    args
}
