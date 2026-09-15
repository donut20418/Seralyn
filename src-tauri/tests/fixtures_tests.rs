use seralyn_lib::app::db::conversations::{self, Conversation};
use seralyn_lib::app::db::messages;
use seralyn_lib::app::db::Database;
use seralyn_lib::app::events::{EventType, NormalizedEvent, ProviderKind, EventPayload};
use seralyn_lib::app::providers::claude::parser::{claude_event_to_normalized, parse_claude_line};
use seralyn_lib::app::providers::codex::parser::{codex_notification_to_normalized, parse_codex_line, CodexMessage};
use seralyn_lib::app::providers::gemini::parser::{gemini_notification_to_normalized, parse_gemini_line, AcpMessage};

#[test]
fn test_claude_stream_text_fixture() {
    let fixture = include_str!("../../tests/fixtures/claude/stream_text.jsonl");
    let mut events = Vec::new();

    for line in fixture.lines() {
        if let Ok(Some(parsed)) = parse_claude_line(line) {
            if let Some(norm) = claude_event_to_normalized(parsed, ProviderKind::Claude, "conv-1", None) {
                events.push(norm);
            }
        }
    }

    assert!(!events.is_empty());
    assert_eq!(events[0].event_type, EventType::SessionStarted);
    assert!(events.iter().any(|e| e.event_type == EventType::TextDelta));
    assert!(events.iter().any(|e| e.event_type == EventType::UsageUpdated || e.event_type == EventType::SessionFinished));
}

#[test]
fn test_claude_stream_tool_fixture() {
    let fixture = include_str!("../../tests/fixtures/claude/stream_tool.jsonl");
    let mut tool_started_found = false;

    for line in fixture.lines() {
        if let Ok(Some(parsed)) = parse_claude_line(line) {
            if let Some(norm) = claude_event_to_normalized(parsed, ProviderKind::Claude, "conv-1", None) {
                if norm.event_type == EventType::ToolStarted {
                    tool_started_found = true;
                    if let EventPayload::Tool { tool_name, .. } = norm.payload {
                        assert_eq!(tool_name, "Write");
                    }
                }
            }
        }
    }

    assert!(tool_started_found, "Expected ToolStarted event from Claude tool fixture");
}

#[test]
fn test_claude_error_fixture() {
    let fixture = include_str!("../../tests/fixtures/claude/error.jsonl");
    let mut error_found = false;

    for line in fixture.lines() {
        if let Ok(Some(parsed)) = parse_claude_line(line) {
            if let Some(norm) = claude_event_to_normalized(parsed, ProviderKind::Claude, "conv-1", None) {
                if norm.event_type == EventType::Error {
                    error_found = true;
                }
            }
        }
    }

    assert!(error_found, "Expected Error event from Claude error fixture");
}

#[test]
fn test_codex_init_handshake_fixture() {
    let fixture = include_str!("../../tests/fixtures/codex/init_handshake.jsonl");
    for line in fixture.lines() {
        if let Ok(Some(msg)) = parse_codex_line(line) {
            match msg {
                CodexMessage::Response(resp) => {
                    assert_eq!(resp.jsonrpc, "2.0");
                    assert!(resp.result.is_some());
                }
                _ => panic!("Expected Response message for init handshake"),
            }
        }
    }
}

#[test]
fn test_codex_turn_events_fixture() {
    let fixture = include_str!("../../tests/fixtures/codex/turn_events.jsonl");
    let mut event_types = Vec::new();

    for line in fixture.lines() {
        if let Ok(Some(CodexMessage::Notification(notif))) = parse_codex_line(line) {
            if let Some(norm) = codex_notification_to_normalized(&notif, "conv-1", Some("thread-1")) {
                event_types.push(norm.event_type);
            }
        }
    }

    assert!(event_types.contains(&EventType::TextDelta));
    assert!(event_types.contains(&EventType::ToolStarted));
    assert!(event_types.contains(&EventType::ToolResult));
}

#[test]
fn test_codex_approval_fixture() {
    let fixture = include_str!("../../tests/fixtures/codex/approval.jsonl");
    let mut approval_found = false;

    for line in fixture.lines() {
        if let Ok(Some(CodexMessage::Notification(notif))) = parse_codex_line(line) {
            if let Some(norm) = codex_notification_to_normalized(&notif, "conv-1", None) {
                if norm.event_type == EventType::ApprovalRequired {
                    approval_found = true;
                    if let EventPayload::Approval { tool_name, .. } = norm.payload {
                        assert_eq!(tool_name, "shell");
                    }
                }
            }
        }
    }

    assert!(approval_found, "Expected ApprovalRequired event from Codex approval fixture");
}

#[test]
fn test_gemini_acp_init_fixture() {
    let fixture = include_str!("../../tests/fixtures/gemini/acp_init.jsonl");
    for line in fixture.lines() {
        if let Ok(Some(msg)) = parse_gemini_line(line) {
            match msg {
                AcpMessage::Response(resp) => {
                    assert_eq!(resp.jsonrpc, "2.0");
                    assert!(resp.result.is_some());
                }
                _ => panic!("Expected Response message for Gemini init"),
            }
        }
    }
}

#[test]
fn test_gemini_stream_events_fixture() {
    let fixture = include_str!("../../tests/fixtures/gemini/stream_events.jsonl");
    let mut text_deltas = 0;
    let mut tool_call_found = false;
    let mut complete_found = false;

    for line in fixture.lines() {
        if let Ok(Some(AcpMessage::Notification(notif))) = parse_gemini_line(line) {
            if let Some(norm) = gemini_notification_to_normalized(&notif, "conv-1", Some("gemini-sess")) {
                match norm.event_type {
                    EventType::TextDelta => text_deltas += 1,
                    EventType::ToolStarted => tool_call_found = true,
                    EventType::SessionFinished => complete_found = true,
                    _ => {}
                }
            }
        }
    }

    assert!(text_deltas >= 2, "Expected multiple TextDelta events");
    assert!(tool_call_found, "Expected ToolStarted event");
    assert!(complete_found, "Expected SessionFinished event");
}

#[test]
fn test_cross_provider_conversation_persistence() {
    // Acceptance Test Simulation:
    // Start one conversation in SQLite.
    // Send Prompt A to Claude -> record assistant response.
    // Switch to Codex and send Prompt B -> record assistant response.
    // Switch to Gemini and send Prompt C -> record assistant response.
    // Verify all messages exist in exact order with correct provider attribution.
    let db = Database::new_in_memory().expect("Failed to create in-memory database");
    db.run_migrations().expect("Failed to run migrations");

    let conv = conversations::create_conversation(&db, Some("Cross-Provider Test"))
        .expect("Failed to create conversation");

    // Turn 1: Claude
    messages::create_message(&db, &conv.id, None, "user", "Prompt A: Write hello world in Rust", None, None, None, None)
        .expect("Failed to insert user message");
    messages::create_message(&db, &conv.id, None, "assistant", "fn main() { println!(\"Hello\"); }", Some("claude"), Some("claude-sonnet-4"), Some("claude-sess-1"), None)
        .expect("Failed to insert Claude response");

    // Turn 2: Switch to Codex
    messages::create_message(&db, &conv.id, None, "user", "Prompt B: Now add error handling", None, None, None, None)
        .expect("Failed to insert user message");
    messages::create_message(&db, &conv.id, None, "assistant", "use std::io::Result; fn main() -> Result<()> { ... }", Some("codex"), Some("gpt-4.1"), Some("codex-thread-1"), None)
        .expect("Failed to insert Codex response");

    // Turn 3: Switch to Gemini
    messages::create_message(&db, &conv.id, None, "user", "Prompt C: Add unit tests", None, None, None, None)
        .expect("Failed to insert user message");
    messages::create_message(&db, &conv.id, None, "assistant", "#[test] fn test_hello() { ... }", Some("gemini"), Some("gemini-2.5"), Some("gemini-sess-1"), None)
        .expect("Failed to insert Gemini response");

    // Query back all messages
    let msgs = messages::get_messages(&db, &conv.id).expect("Failed to fetch messages");
    assert_eq!(msgs.len(), 6);

    assert_eq!(msgs[0].role, "user");
    assert_eq!(msgs[1].role, "assistant");
    assert_eq!(msgs[1].provider.as_deref(), Some("claude"));

    assert_eq!(msgs[2].role, "user");
    assert_eq!(msgs[3].role, "assistant");
    assert_eq!(msgs[3].provider.as_deref(), Some("codex"));

    assert_eq!(msgs[4].role, "user");
    assert_eq!(msgs[5].role, "assistant");
    assert_eq!(msgs[5].provider.as_deref(), Some("gemini"));
}

#[tokio::test]
async fn test_cross_provider_context_handover_flow() {
    use std::sync::Arc;
    use tokio::sync::Mutex;
    use async_trait::async_trait;
    use seralyn_lib::app::conversation::ConversationManager;
    use seralyn_lib::app::conversation::context::format_context_for_prompt;
    use seralyn_lib::app::providers::{
        AuthStatus, InstallationInfo, Provider, ProviderCapabilities, ProviderMessage,
        ProviderSession, SessionConfig, SessionMetadata,
    };

    // A mock session that records all received messages and emits simulated responses
    struct MockSession {
        provider: ProviderKind,
        history: Arc<Mutex<Vec<ProviderMessage>>>,
        event_sender: tokio::sync::mpsc::Sender<NormalizedEvent>,
        conv_id: String,
    }

    #[async_trait]
    impl ProviderSession for MockSession {
        async fn send(&mut self, message: ProviderMessage) -> seralyn_lib::app::error::Result<()> {
            self.history.lock().await.push(message.clone());
            
            // Emit streaming response
            let started = NormalizedEvent::new(
                EventType::SessionStarted,
                self.provider,
                self.conv_id.clone(),
                EventPayload::Session { session_id: Some("mock-sid".into()), model: None },
            );
            let _ = self.event_sender.send(started).await;

            let delta = NormalizedEvent::new(
                EventType::TextDelta,
                self.provider,
                self.conv_id.clone(),
                EventPayload::Text { content: format!("Response from {}", self.provider) },
            );
            let _ = self.event_sender.send(delta).await;

            let finished = NormalizedEvent::new(
                EventType::SessionFinished,
                self.provider,
                self.conv_id.clone(),
                EventPayload::Empty,
            );
            let _ = self.event_sender.send(finished).await;
            Ok(())
        }
        async fn interrupt(&mut self) -> seralyn_lib::app::error::Result<()> { Ok(()) }
        async fn cancel(&mut self) -> seralyn_lib::app::error::Result<()> { Ok(()) }
        async fn close(&mut self) -> seralyn_lib::app::error::Result<()> { Ok(()) }
        fn native_session_id(&self) -> Option<&str> { Some("mock-sid") }
        fn metadata(&self) -> SessionMetadata {
            SessionMetadata {
                provider: self.provider,
                native_session_id: Some("mock-sid".into()),
                model: None,
                created_at: "".into(),
            }
        }
        fn is_active(&self) -> bool { true }
    }

    struct MockProvider {
        kind: ProviderKind,
        history: Arc<Mutex<Vec<ProviderMessage>>>,
    }

    #[async_trait]
    impl Provider for MockProvider {
        fn kind(&self) -> ProviderKind { self.kind }
        async fn detect_installation(&self) -> seralyn_lib::app::error::Result<InstallationInfo> {
            Ok(InstallationInfo { installed: true, executable_path: None, version: None })
        }
        async fn check_authentication(&self) -> seralyn_lib::app::error::Result<AuthStatus> {
            Ok(AuthStatus::Authenticated)
        }
        fn capabilities(&self) -> ProviderCapabilities { Default::default() }
        async fn create_session(&self, config: SessionConfig) -> seralyn_lib::app::error::Result<Box<dyn ProviderSession>> {
            Ok(Box::new(MockSession {
                provider: self.kind,
                history: self.history.clone(),
                event_sender: config.event_sender,
                conv_id: config.conversation_id,
            }))
        }
        async fn resume_session(&self, _id: &str, config: SessionConfig) -> seralyn_lib::app::error::Result<Box<dyn ProviderSession>> {
            self.create_session(config).await
        }
    }

    let db = Arc::new(Database::new_in_memory().unwrap());
    db.run_migrations().unwrap();

    let claude_history = Arc::new(Mutex::new(Vec::new()));
    let codex_history = Arc::new(Mutex::new(Vec::new()));
    let gemini_history = Arc::new(Mutex::new(Vec::new()));

    let mut provider_map = std::collections::HashMap::new();
    provider_map.insert(ProviderKind::Claude, Arc::new(MockProvider { kind: ProviderKind::Claude, history: claude_history.clone() }) as Arc<dyn Provider>);
    provider_map.insert(ProviderKind::Codex, Arc::new(MockProvider { kind: ProviderKind::Codex, history: codex_history.clone() }) as Arc<dyn Provider>);
    provider_map.insert(ProviderKind::Gemini, Arc::new(MockProvider { kind: ProviderKind::Gemini, history: gemini_history.clone() }) as Arc<dyn Provider>);

    let manager = ConversationManager::new(db.clone(), Arc::new(seralyn_lib::app::providers::ProviderManager::with_providers(provider_map)));

    let conv = manager.create_conversation(Some("Context Handover Test")).unwrap();

    // Turn 1: Claude
    let (tx1, mut rx1) = tokio::sync::mpsc::channel(10);
    tokio::spawn(async move { while rx1.recv().await.is_some() {} });
    manager.send_message(&conv.id, "Question 1", ProviderKind::Claude, tx1).await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let c_msgs = claude_history.lock().await;
    assert_eq!(c_msgs.len(), 1);
    assert_eq!(c_msgs[0].content, "Question 1");
    assert!(c_msgs[0].context.is_empty(), "Turn 1 must have no prior context");
    drop(c_msgs);

    // Turn 2: Switch to Codex -> must receive Question 1 and Claude response in context!
    let (tx2, mut rx2) = tokio::sync::mpsc::channel(10);
    tokio::spawn(async move { while rx2.recv().await.is_some() {} });
    manager.send_message(&conv.id, "Question 2", ProviderKind::Codex, tx2).await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let codex_msgs = codex_history.lock().await;
    assert_eq!(codex_msgs.len(), 1);
    assert_eq!(codex_msgs[0].content, "Question 2");
    assert_eq!(codex_msgs[0].context.len(), 2, "Turn 2 must receive Turn 1 (user + assistant) in context");
    assert_eq!(codex_msgs[0].context[0].content, "Question 1");
    assert_eq!(codex_msgs[0].context[1].content, "Response from claude");

    // Test prompt formatting
    let formatted = format_context_for_prompt(&codex_msgs[0].context, &codex_msgs[0].content);
    assert!(formatted.contains("Assistant (claude): Response from claude"));
    assert!(formatted.contains("Question 2"));
    drop(codex_msgs);

    // Turn 3: Switch to Gemini -> must receive all 4 prior turns in context!
    let (tx3, mut rx3) = tokio::sync::mpsc::channel(10);
    tokio::spawn(async move { while rx3.recv().await.is_some() {} });
    manager.send_message(&conv.id, "Question 3", ProviderKind::Gemini, tx3).await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let gemini_msgs = gemini_history.lock().await;
    assert_eq!(gemini_msgs.len(), 1);
    assert_eq!(gemini_msgs[0].content, "Question 3");
    assert_eq!(gemini_msgs[0].context.len(), 4, "Turn 3 must receive all 4 prior messages from Claude and Codex");
    assert_eq!(gemini_msgs[0].context[0].content, "Question 1");
    assert_eq!(gemini_msgs[0].context[1].content, "Response from claude");
    assert_eq!(gemini_msgs[0].context[2].content, "Question 2");
    assert_eq!(gemini_msgs[0].context[3].content, "Response from codex");
}
