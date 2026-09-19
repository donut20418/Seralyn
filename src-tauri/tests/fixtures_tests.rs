use seralyn_lib::app::db::conversations;
use seralyn_lib::app::db::messages;
use seralyn_lib::app::db::Database;
use seralyn_lib::app::events::{EventType, NormalizedEvent, ProviderKind, EventPayload};
use seralyn_lib::app::providers::claude::parser::{claude_event_to_normalized, parse_claude_line};
use seralyn_lib::app::providers::codex::parser::{codex_notification_to_normalized, codex_server_request_to_normalized, parse_codex_line, CodexMessage};
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
    let mut approval_count = 0;

    for line in fixture.lines() {
        if let Ok(Some(msg)) = parse_codex_line(line) {
            match msg {
                CodexMessage::ServerRequest(req) => {
                    if let Some(norm) = codex_server_request_to_normalized(&req, "conv-1", None) {
                        assert_eq!(norm.event_type, EventType::ApprovalRequired);
                        approval_count += 1;
                    }
                }
                CodexMessage::Notification(notif) => {
                    if let Some(norm) = codex_notification_to_normalized(&notif, "conv-1", None) {
                        if norm.event_type == EventType::ApprovalRequired {
                            approval_count += 1;
                        }
                    }
                }
                _ => {}
            }
        }
    }

    assert!(approval_count >= 1, "Expected ApprovalRequired events from Codex approval fixture");
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
    assert_eq!(msgs[0].seq, 1);
    assert_eq!(msgs[1].role, "assistant");
    assert_eq!(msgs[1].seq, 2);
    assert_eq!(msgs[1].provider.as_deref(), Some("claude"));

    assert_eq!(msgs[2].role, "user");
    assert_eq!(msgs[2].seq, 3);
    assert_eq!(msgs[3].role, "assistant");
    assert_eq!(msgs[3].seq, 4);
    assert_eq!(msgs[3].provider.as_deref(), Some("codex"));

    assert_eq!(msgs[4].role, "user");
    assert_eq!(msgs[4].seq, 5);
    assert_eq!(msgs[5].role, "assistant");
    assert_eq!(msgs[5].seq, 6);
    assert_eq!(msgs[5].provider.as_deref(), Some("gemini"));
}

#[tokio::test]
async fn test_cross_provider_sync_cursor_full_cycle() {
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
        async fn send(&self, message: ProviderMessage) -> seralyn_lib::app::error::Result<()> {
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
        async fn interrupt(&self) -> seralyn_lib::app::error::Result<()> { Ok(()) }
        async fn cancel(&self) -> seralyn_lib::app::error::Result<()> { Ok(()) }
        async fn close(&self) -> seralyn_lib::app::error::Result<()> { Ok(()) }
        fn native_session_id(&self) -> Option<String> { Some("mock-sid".into()) }
        async fn respond_to_approval(&self, _id: &str, _app: bool) -> seralyn_lib::app::error::Result<()> { Ok(()) }
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

    let conv = manager.create_conversation(Some("Full Cycle Sync Cursor Test")).unwrap();

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

    // Turn 2: Switch to Codex -> must receive Turn 1 (Question 1 + Claude response)
    let (tx2, mut rx2) = tokio::sync::mpsc::channel(10);
    tokio::spawn(async move { while rx2.recv().await.is_some() {} });
    manager.send_message(&conv.id, "Question 2", ProviderKind::Codex, tx2).await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let codex_msgs = codex_history.lock().await;
    assert_eq!(codex_msgs.len(), 1);
    assert_eq!(codex_msgs[0].content, "Question 2");
    assert_eq!(codex_msgs[0].context.len(), 2, "Turn 2 must receive Turn 1 in context");
    assert_eq!(codex_msgs[0].context[0].content, "Question 1");
    assert_eq!(codex_msgs[0].context[1].content, "Response from claude");
    drop(codex_msgs);

    // Turn 3: Switch to Gemini -> must receive all 4 prior turns
    let (tx3, mut rx3) = tokio::sync::mpsc::channel(10);
    tokio::spawn(async move { while rx3.recv().await.is_some() {} });
    manager.send_message(&conv.id, "Question 3", ProviderKind::Gemini, tx3).await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let gemini_msgs = gemini_history.lock().await;
    assert_eq!(gemini_msgs.len(), 1);
    assert_eq!(gemini_msgs[0].content, "Question 3");
    assert_eq!(gemini_msgs[0].context.len(), 4, "Turn 3 must receive all 4 prior messages");
    drop(gemini_msgs);

    // Turn 4: SWITCH BACK TO CLAUDE! (A -> B -> C -> A)
    // Claude native session knows turns 1 & 2 (synced_through_seq = 2).
    // It is missing Turn 2 (Question 2 + Codex response) and Turn 3 (Question 3 + Gemini response).
    // Exactly 4 delta messages!
    let (tx4, mut rx4) = tokio::sync::mpsc::channel(10);
    tokio::spawn(async move { while rx4.recv().await.is_some() {} });
    manager.send_message(&conv.id, "Question 4", ProviderKind::Claude, tx4).await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let c_msgs = claude_history.lock().await;
    assert_eq!(c_msgs.len(), 2, "Claude should have received 2 turns now");
    assert_eq!(c_msgs[1].content, "Question 4");
    assert_eq!(c_msgs[1].context.len(), 4, "Claude must receive ONLY the 4 delta turns that occurred while away");
    assert_eq!(c_msgs[1].context[0].content, "Question 2");
    assert_eq!(c_msgs[1].context[1].content, "Response from codex");
    assert_eq!(c_msgs[1].context[2].content, "Question 3");
    assert_eq!(c_msgs[1].context[3].content, "Response from gemini");

    let formatted_c4 = format_context_for_prompt(&c_msgs[1].context, &c_msgs[1].content);
    assert!(formatted_c4.contains("Assistant (codex): Response from codex"));
    assert!(formatted_c4.contains("Assistant (gemini): Response from gemini"));
    assert!(formatted_c4.contains("Question 4"));
    drop(c_msgs);

    // Turn 5: User continues chatting with Claude (A -> A)
    // Claude is now synced through seq 8. Current user turn is seq 9.
    // Delta must be completely EMPTY!
    let (tx5, mut rx5) = tokio::sync::mpsc::channel(10);
    tokio::spawn(async move { while rx5.recv().await.is_some() {} });
    manager.send_message(&conv.id, "Question 5", ProviderKind::Claude, tx5).await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let c_msgs = claude_history.lock().await;
    assert_eq!(c_msgs.len(), 3);
    assert_eq!(c_msgs[2].content, "Question 5");
    assert!(c_msgs[2].context.is_empty(), "Turn 5 (continuing with same provider) must have empty delta context");

    let formatted_c5 = format_context_for_prompt(&c_msgs[2].context, &c_msgs[2].content);
    assert_eq!(formatted_c5, "Question 5", "Prompt should remain untouched when continuing with same provider");
    drop(c_msgs);
}

#[tokio::test]
async fn test_claude_full_lifecycle_with_restart_and_native_resume() {
    use std::sync::Arc;
    use seralyn_lib::app::conversation::ConversationManager;
    use seralyn_lib::app::providers::{Provider, ProviderSession, SessionConfig, ProviderMessage, ProviderCapabilities, SessionMetadata};
    use seralyn_lib::app::db::provider_sessions;

    struct ResumableClaudeSession {
        event_sender: tokio::sync::mpsc::Sender<NormalizedEvent>,
        conv_id: String,
        native_sid: String,
    }

    #[async_trait::async_trait]
    impl ProviderSession for ResumableClaudeSession {
        async fn send(&self, _msg: ProviderMessage) -> seralyn_lib::app::error::Result<()> {
            let sender = self.event_sender.clone();
            let conv_id = self.conv_id.clone();
            let sid = self.native_sid.clone();
            tokio::spawn(async move {
                // Simulate Claude sending system/init
                let init_line = format!(r#"{{"type":"system","subtype":"init","session_id":"{}","model":"claude-opus-5"}}"#, sid);
                let ev = parse_claude_line(&init_line).unwrap().unwrap();
                let norm = claude_event_to_normalized(ev, ProviderKind::Claude, &conv_id, None).unwrap();
                let _ = sender.send(norm).await;

                // Simulate token stream
                let delta1 = r#"{"type":"stream_event","event":{"type":"content_block_delta","delta":{"type":"text_delta","text":"Hello "}}}"#;
                let ev1 = parse_claude_line(delta1).unwrap().unwrap();
                let norm1 = claude_event_to_normalized(ev1, ProviderKind::Claude, &conv_id, Some(&sid)).unwrap();
                let _ = sender.send(norm1).await;

                let delta2 = r#"{"type":"stream_event","event":{"type":"content_block_delta","delta":{"type":"text_delta","text":"from Claude!"}}}"#;
                let ev2 = parse_claude_line(delta2).unwrap().unwrap();
                let norm2 = claude_event_to_normalized(ev2, ProviderKind::Claude, &conv_id, Some(&sid)).unwrap();
                let _ = sender.send(norm2).await;

                // Finish
                let finish = NormalizedEvent::session_finished(ProviderKind::Claude, conv_id);
                let _ = sender.send(finish).await;
            });
            Ok(())
        }
        async fn interrupt(&self) -> seralyn_lib::app::error::Result<()> { Ok(()) }
        async fn cancel(&self) -> seralyn_lib::app::error::Result<()> { Ok(()) }
        async fn close(&self) -> seralyn_lib::app::error::Result<()> { Ok(()) }
        fn native_session_id(&self) -> Option<String> { Some(self.native_sid.clone()) }
        fn metadata(&self) -> SessionMetadata {
            SessionMetadata {
                provider: ProviderKind::Claude,
                native_session_id: Some(self.native_sid.clone()),
                model: Some("claude-opus-5".to_string()),
                created_at: String::new(),
            }
        }
        fn is_active(&self) -> bool { true }
    }

    struct MockClaudeProvider {
        resumed_with: Arc<tokio::sync::Mutex<Vec<String>>>,
    }

    #[async_trait::async_trait]
    impl Provider for MockClaudeProvider {
        fn kind(&self) -> ProviderKind { ProviderKind::Claude }
        async fn detect_installation(&self) -> seralyn_lib::app::error::Result<seralyn_lib::app::providers::InstallationInfo> {
            Ok(seralyn_lib::app::providers::InstallationInfo { installed: true, executable_path: None, version: Some("2.1.270".to_string()) })
        }
        async fn check_authentication(&self) -> seralyn_lib::app::error::Result<seralyn_lib::app::providers::AuthStatus> {
            Ok(seralyn_lib::app::providers::AuthStatus::Authenticated)
        }
        fn capabilities(&self) -> ProviderCapabilities { Default::default() }
        async fn create_session(&self, config: SessionConfig) -> seralyn_lib::app::error::Result<Box<dyn ProviderSession>> {
            Ok(Box::new(ResumableClaudeSession {
                event_sender: config.event_sender,
                conv_id: config.conversation_id,
                native_sid: "claude-native-xyz-123".to_string(),
            }))
        }
        async fn resume_session(&self, native_session_id: &str, config: SessionConfig) -> seralyn_lib::app::error::Result<Box<dyn ProviderSession>> {
            self.resumed_with.lock().await.push(native_session_id.to_string());
            Ok(Box::new(ResumableClaudeSession {
                event_sender: config.event_sender,
                conv_id: config.conversation_id,
                native_sid: native_session_id.to_string(),
            }))
        }
    }

    let db = Arc::new(Database::new_in_memory().unwrap());
    db.run_migrations().unwrap();

    let resumed_log = Arc::new(tokio::sync::Mutex::new(Vec::new()));
    let provider = Arc::new(MockClaudeProvider { resumed_with: resumed_log.clone() });
    let mut provider_map = std::collections::HashMap::new();
    provider_map.insert(ProviderKind::Claude, provider.clone() as Arc<dyn Provider>);

    // Instance 1 of App / ConversationManager
    let manager1 = ConversationManager::new(db.clone(), Arc::new(seralyn_lib::app::providers::ProviderManager::with_providers(provider_map.clone())));
    let conv = manager1.create_conversation(Some("Claude Resume Test")).unwrap();

    // Turn 1 on manager1:
    let (tx1, mut rx1) = tokio::sync::mpsc::channel::<NormalizedEvent>(10);
    let mut text_acc1 = String::new();
    let collect_task1 = tokio::spawn(async move {
        while let Some(ev) = rx1.recv().await {
            if let EventPayload::Text { content } = &ev.payload {
                text_acc1.push_str(content.as_str());
            }
            if ev.event_type == EventType::SessionFinished {
                break;
            }
        }
        text_acc1
    });

    manager1.send_message(&conv.id, "Turn 1 Question", ProviderKind::Claude, tx1).await.unwrap();
    let streamed_result1 = collect_task1.await.unwrap();
    assert_eq!(streamed_result1, "Hello from Claude!");

    // Wait a brief moment for background event processing to commit DB writes
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;

    // Verify Turn 1 saved in SQLite
    let msgs = messages::get_messages(&db, &conv.id).unwrap();
    assert_eq!(msgs.len(), 2, "Turn 1: Should have 1 user message + 1 assistant message");
    assert_eq!(msgs[0].role, "user");
    assert_eq!(msgs[0].seq, 1);
    assert_eq!(msgs[1].role, "assistant");
    assert_eq!(msgs[1].content, "Hello from Claude!");
    assert_eq!(msgs[1].seq, 2);

    // Verify native session ID was persisted in SQLite provider_sessions table
    let active_sess = provider_sessions::get_active_session(&db, &conv.id, "claude").unwrap().unwrap();
    assert_eq!(active_sess.provider_session_id, Some("claude-native-xyz-123".to_string()));

    // Turn 2 on manager1 WITHOUT RESTART (testing dynamic event routing & persistent event bus!):
    let (tx2, mut rx2) = tokio::sync::mpsc::channel::<NormalizedEvent>(10);
    let mut text_acc2 = String::new();
    let collect_task2 = tokio::spawn(async move {
        while let Some(ev) = rx2.recv().await {
            if let EventPayload::Text { content } = &ev.payload {
                text_acc2.push_str(content.as_str());
            }
            if ev.event_type == EventType::SessionFinished {
                break;
            }
        }
        text_acc2
    });

    manager1.send_message(&conv.id, "Turn 2 in same session", ProviderKind::Claude, tx2).await.unwrap();
    let streamed_result2 = collect_task2.await.unwrap();
    assert_eq!(streamed_result2, "Hello from Claude!", "Turn 2 must route events to rx2 successfully");

    tokio::time::sleep(std::time::Duration::from_millis(150)).await;

    // Verify Turn 2 saved in SQLite with continuous sequence
    let msgs2 = messages::get_messages(&db, &conv.id).unwrap();
    assert_eq!(msgs2.len(), 4, "Turn 2: Should have 2 user messages + 2 assistant messages");
    assert_eq!(msgs2[2].role, "user");
    assert_eq!(msgs2[2].seq, 3);
    assert_eq!(msgs2[3].role, "assistant");
    assert_eq!(msgs2[3].seq, 4);

    // SIMULATE APP RESTART:
    // Drop manager1 entirely (simulates quitting the desktop application)
    drop(manager1);

    // Create manager2 from the SAME db (as when user relaunches desktop app)
    let manager2 = ConversationManager::new(db.clone(), Arc::new(seralyn_lib::app::providers::ProviderManager::with_providers(provider_map)));

    // Turn 3 in the resumed conversation after restart:
    let (tx3, mut rx3) = tokio::sync::mpsc::channel::<NormalizedEvent>(10);
    let mut text_acc3 = String::new();
    let collect_task3 = tokio::spawn(async move {
        while let Some(ev) = rx3.recv().await {
            if let EventPayload::Text { content } = &ev.payload {
                text_acc3.push_str(content.as_str());
            }
            if ev.event_type == EventType::SessionFinished {
                break;
            }
        }
        text_acc3
    });

    manager2.send_message(&conv.id, "Turn 3 after restart", ProviderKind::Claude, tx3).await.unwrap();
    let streamed_result3 = collect_task3.await.unwrap();
    assert_eq!(streamed_result3, "Hello from Claude!", "Turn 3 must route events to rx3");

    tokio::time::sleep(std::time::Duration::from_millis(150)).await;

    // Verify that manager2 invoked resume_session with the persisted native session ID!
    let resumed = resumed_log.lock().await;
    assert_eq!(resumed.len(), 1, "Must have called resume_session exactly once upon restart");
    assert_eq!(resumed[0], "claude-native-xyz-123", "Must resume with the exact native session ID persisted in Turn 1");

    // Verify all 6 messages exist in continuous SQLite sequence
    let msgs3 = messages::get_messages(&db, &conv.id).unwrap();
    assert_eq!(msgs3.len(), 6, "Turn 3: Total 6 messages (3 user + 3 assistant)");
    assert_eq!(msgs3[4].role, "user");
    assert_eq!(msgs3[4].seq, 5);
    assert_eq!(msgs3[5].role, "assistant");
    assert_eq!(msgs3[5].seq, 6);
}

#[tokio::test]
async fn test_gemini_resume_history_isolation() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;
    use tokio::sync::Notify;
    use seralyn_lib::app::conversation::ConversationManager;
    use seralyn_lib::app::providers::{Provider, ProviderSession, SessionConfig, ProviderMessage, ProviderCapabilities, SessionMetadata};
    use seralyn_lib::app::db::{Database, messages, provider_sessions};

    struct FixedGeminiSession {
        event_sender: tokio::sync::mpsc::Sender<NormalizedEvent>,
        conv_id: String,
        native_sid: String,
        replay_complete: Arc<AtomicBool>,
        replay_ready: Arc<Notify>,
    }

    #[async_trait::async_trait]
    impl ProviderSession for FixedGeminiSession {
        async fn send(&self, msg: ProviderMessage) -> seralyn_lib::app::error::Result<()> {
            // Replay barrier: send() waits until history replay quiescence/completion!
            if !self.replay_complete.load(Ordering::SeqCst) {
                let notified = self.replay_ready.notified();
                if !self.replay_complete.load(Ordering::SeqCst) {
                    let _ = tokio::time::timeout(Duration::from_millis(2000), notified).await;
                }
            }

            let sender = self.event_sender.clone();
            let conv_id = self.conv_id.clone();
            let response_text = format!("Turn 3 Response. Prompt was {}", msg.content);
            
            tokio::spawn(async move {
                let delta = NormalizedEvent::text_delta(ProviderKind::Gemini, conv_id.clone(), response_text);
                let _ = sender.send(delta).await;
                
                let finish = NormalizedEvent::session_finished(ProviderKind::Gemini, conv_id);
                let _ = sender.send(finish).await;
            });
            Ok(())
        }
        async fn interrupt(&self) -> seralyn_lib::app::error::Result<()> { Ok(()) }
        async fn cancel(&self) -> seralyn_lib::app::error::Result<()> { Ok(()) }
        async fn close(&self) -> seralyn_lib::app::error::Result<()> { Ok(()) }
        fn native_session_id(&self) -> Option<String> { Some(self.native_sid.clone()) }
        fn metadata(&self) -> SessionMetadata {
            SessionMetadata {
                provider: ProviderKind::Gemini,
                native_session_id: Some(self.native_sid.clone()),
                model: Some("gemini-3.1-flash-lite".to_string()),
                created_at: String::new(),
            }
        }
        fn is_active(&self) -> bool { true }
    }

    struct MockGeminiProvider {
        resumed_count: Arc<std::sync::atomic::AtomicUsize>,
        delay_ms: Option<u64>,
        max_duration_ms: u64,
    }

    #[async_trait::async_trait]
    impl Provider for MockGeminiProvider {
        fn kind(&self) -> ProviderKind { ProviderKind::Gemini }
        async fn detect_installation(&self) -> seralyn_lib::app::error::Result<seralyn_lib::app::providers::InstallationInfo> {
            Ok(seralyn_lib::app::providers::InstallationInfo { installed: true, executable_path: None, version: None })
        }
        async fn check_authentication(&self) -> seralyn_lib::app::error::Result<seralyn_lib::app::providers::AuthStatus> {
            Ok(seralyn_lib::app::providers::AuthStatus::Authenticated)
        }
        fn capabilities(&self) -> ProviderCapabilities { Default::default() }
        async fn create_session(&self, _config: SessionConfig) -> seralyn_lib::app::error::Result<Box<dyn ProviderSession>> {
            panic!("Test must invoke resume_session, not create_session!");
        }
        async fn resume_session(&self, native_session_id: &str, config: SessionConfig) -> seralyn_lib::app::error::Result<Box<dyn ProviderSession>> {
            self.resumed_count.fetch_add(1, Ordering::SeqCst);
            let replay_complete = Arc::new(AtomicBool::new(false));
            let replay_ready = Arc::new(Notify::new());

            let sender_clone = config.event_sender.clone();
            let conv_id_clone = config.conversation_id.clone();
            let (activity_tx, mut activity_rx) = tokio::sync::mpsc::channel::<()>(100);

            let rc = replay_complete.clone();
            let rr = replay_ready.clone();
            let max_duration = Duration::from_millis(self.max_duration_ms);
            let min_duration = Duration::from_millis(750.min(self.max_duration_ms));
            let quiet_duration = Duration::from_millis(250.min(self.max_duration_ms / 2));

            // Quiescence task enforcing seen_replay_activity, min_duration, and quiet_duration
            tokio::spawn(async move {
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
                        _ = tokio::time::sleep(Duration::from_millis(25)) => {
                            if seen_replay_activity
                                && start.elapsed() >= min_duration
                                && last_activity.elapsed() >= quiet_duration
                            {
                                break;
                            }
                        }
                    }
                }

                rc.store(true, Ordering::SeqCst);
                rr.notify_waiters();
            });

            // Delayed history generator (if delay_ms is configured)
            if let Some(delay) = self.delay_ms {
                let rc_filter = replay_complete.clone();
                tokio::spawn(async move {
                    tokio::time::sleep(Duration::from_millis(delay)).await;

                    let history_text = "Turn 1 and 2 old history text";
                    let event = NormalizedEvent::text_delta(ProviderKind::Gemini, conv_id_clone.clone(), history_text.to_string());

                    if !rc_filter.load(Ordering::SeqCst) {
                        let _ = activity_tx.try_send(());
                    } else {
                        // Leaked because barrier concluded too early!
                        let _ = sender_clone.send(event).await;
                    }
                });
            }

            Ok(Box::new(FixedGeminiSession {
                event_sender: config.event_sender,
                conv_id: config.conversation_id,
                native_sid: native_session_id.to_string(),
                replay_complete,
                replay_ready,
            }))
        }
    }

    async fn run_gemini_resume_scenario(delay_ms: Option<u64>, max_duration_ms: u64) {
        let db = Arc::new(Database::new_in_memory().unwrap());
        db.run_migrations().unwrap();

        let resumed_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let mut provider_map = std::collections::HashMap::new();
        provider_map.insert(
            ProviderKind::Gemini,
            Arc::new(MockGeminiProvider {
                resumed_count: resumed_count.clone(),
                delay_ms,
                max_duration_ms,
            }) as Arc<dyn Provider>,
        );

        let manager = ConversationManager::new(db.clone(), Arc::new(seralyn_lib::app::providers::ProviderManager::with_providers(provider_map)));
        
        // 1. Simulate pre-existing conversation in SQLite (Turns 1 & 2)
        let conv = manager.create_conversation(Some("History Isolation Test")).unwrap();
        messages::create_message(&db, &conv.id, None, "user", "Turn 1 Prompt", None, None, None, None).unwrap();
        messages::create_message(&db, &conv.id, None, "assistant", "Turn 1 Response", Some("gemini"), Some("gemini-3.1-flash-lite"), Some("gemini-old-sid"), None).unwrap();
        messages::create_message(&db, &conv.id, None, "user", "Turn 2 Prompt", None, None, None, None).unwrap();
        messages::create_message(&db, &conv.id, None, "assistant", "Turn 2 Response", Some("gemini"), Some("gemini-3.1-flash-lite"), Some("gemini-old-sid"), None).unwrap();

        // 2. Persist active provider session record with native session ID
        provider_sessions::create_provider_session(
            &db,
            &conv.id,
            "gemini",
            Some("gemini-old-sid"),
            Some("gemini-3.1-flash-lite"),
        ).unwrap();

        // 3. Trigger Turn 3 immediately without waiting
        let (tx, mut rx) = tokio::sync::mpsc::channel::<NormalizedEvent>(10);
        manager.send_message(&conv.id, "Turn 3 Prompt", ProviderKind::Gemini, tx).await.unwrap();

        let mut turn3_result = String::new();
        while let Some(ev) = rx.recv().await {
            if let EventPayload::Text { content } = ev.payload {
                turn3_result.push_str(&content);
            }
            if ev.event_type == EventType::SessionFinished {
                break;
            }
        }
        
        tokio::time::sleep(Duration::from_millis(200)).await;

        assert_eq!(resumed_count.load(Ordering::SeqCst), 1, "Must have called resume_session exactly once");

        let all_msgs = messages::get_messages(&db, &conv.id).unwrap();
        assert_eq!(all_msgs.len(), 6, "Should have exactly 6 messages (3 user, 3 assistant)");
        
        assert_eq!(all_msgs[0].seq, 1);
        assert_eq!(all_msgs[1].seq, 2);
        assert_eq!(all_msgs[1].content, "Turn 1 Response");
        assert_eq!(all_msgs[2].seq, 3);
        assert_eq!(all_msgs[3].seq, 4);
        assert_eq!(all_msgs[3].content, "Turn 2 Response");
        
        assert_eq!(all_msgs[4].role, "user");
        assert_eq!(all_msgs[4].seq, 5);
        assert_eq!(all_msgs[4].content, "Turn 3 Prompt");
        
        assert_eq!(all_msgs[5].role, "assistant");
        assert_eq!(all_msgs[5].seq, 6);
        
        assert_eq!(all_msgs[5].content, turn3_result);
        assert_eq!(all_msgs[5].content, "Turn 3 Response. Prompt was Turn 3 Prompt");
        assert!(!all_msgs[5].content.contains("old history text"), "Failed deduplication: Turn 3 contains leaked history!");
    }

    // Scenario 1: First history chunk arrives at 400ms (>250ms quiet window) -> properly suppressed!
    run_gemini_resume_scenario(Some(400), 2500).await;
}

#[tokio::test]
async fn test_gemini_resume_history_isolation_delayed_1000ms() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;
    use tokio::sync::Notify;
    use seralyn_lib::app::conversation::ConversationManager;
    use seralyn_lib::app::providers::{Provider, ProviderSession, SessionConfig, ProviderMessage, ProviderCapabilities, SessionMetadata};
    use seralyn_lib::app::db::{Database, messages, provider_sessions};

    struct FixedGeminiSession {
        event_sender: tokio::sync::mpsc::Sender<NormalizedEvent>,
        conv_id: String,
        native_sid: String,
        replay_complete: Arc<AtomicBool>,
        replay_ready: Arc<Notify>,
    }

    #[async_trait::async_trait]
    impl ProviderSession for FixedGeminiSession {
        async fn send(&self, msg: ProviderMessage) -> seralyn_lib::app::error::Result<()> {
            if !self.replay_complete.load(Ordering::SeqCst) {
                let notified = self.replay_ready.notified();
                if !self.replay_complete.load(Ordering::SeqCst) {
                    let _ = tokio::time::timeout(Duration::from_millis(3000), notified).await;
                }
            }

            let sender = self.event_sender.clone();
            let conv_id = self.conv_id.clone();
            let response_text = format!("Turn 3 Response. Prompt was {}", msg.content);
            
            tokio::spawn(async move {
                let delta = NormalizedEvent::text_delta(ProviderKind::Gemini, conv_id.clone(), response_text);
                let _ = sender.send(delta).await;
                
                let finish = NormalizedEvent::session_finished(ProviderKind::Gemini, conv_id);
                let _ = sender.send(finish).await;
            });
            Ok(())
        }
        async fn interrupt(&self) -> seralyn_lib::app::error::Result<()> { Ok(()) }
        async fn cancel(&self) -> seralyn_lib::app::error::Result<()> { Ok(()) }
        async fn close(&self) -> seralyn_lib::app::error::Result<()> { Ok(()) }
        fn native_session_id(&self) -> Option<String> { Some(self.native_sid.clone()) }
        fn metadata(&self) -> SessionMetadata {
            SessionMetadata {
                provider: ProviderKind::Gemini,
                native_session_id: Some(self.native_sid.clone()),
                model: Some("gemini-3.1-flash-lite".to_string()),
                created_at: String::new(),
            }
        }
        fn is_active(&self) -> bool { true }
    }

    struct MockGeminiProvider1000 {
        resumed_count: Arc<std::sync::atomic::AtomicUsize>,
    }

    #[async_trait::async_trait]
    impl Provider for MockGeminiProvider1000 {
        fn kind(&self) -> ProviderKind { ProviderKind::Gemini }
        async fn detect_installation(&self) -> seralyn_lib::app::error::Result<seralyn_lib::app::providers::InstallationInfo> {
            Ok(seralyn_lib::app::providers::InstallationInfo { installed: true, executable_path: None, version: None })
        }
        async fn check_authentication(&self) -> seralyn_lib::app::error::Result<seralyn_lib::app::providers::AuthStatus> {
            Ok(seralyn_lib::app::providers::AuthStatus::Authenticated)
        }
        fn capabilities(&self) -> ProviderCapabilities { Default::default() }
        async fn create_session(&self, _config: SessionConfig) -> seralyn_lib::app::error::Result<Box<dyn ProviderSession>> {
            panic!("Test must invoke resume_session");
        }
        async fn resume_session(&self, native_session_id: &str, config: SessionConfig) -> seralyn_lib::app::error::Result<Box<dyn ProviderSession>> {
            self.resumed_count.fetch_add(1, Ordering::SeqCst);
            let replay_complete = Arc::new(AtomicBool::new(false));
            let replay_ready = Arc::new(Notify::new());

            let sender_clone = config.event_sender.clone();
            let conv_id_clone = config.conversation_id.clone();
            let (activity_tx, mut activity_rx) = tokio::sync::mpsc::channel::<()>(100);

            let rc = replay_complete.clone();
            let rr = replay_ready.clone();

            // Quiescence task enforcing seen_replay_activity + min_duration (750ms) + quiet_duration (250ms)
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
                        _ = tokio::time::sleep(Duration::from_millis(25)) => {
                            if seen_replay_activity
                                && start.elapsed() >= min_duration
                                && last_activity.elapsed() >= quiet_duration
                            {
                                break;
                            }
                        }
                    }
                }

                rc.store(true, Ordering::SeqCst);
                rr.notify_waiters();
            });

            // Delayed history generator: chunk arrives at 1000ms (>750ms min_duration!)
            let rc_filter = replay_complete.clone();
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(1000)).await;

                let history_text = "Turn 1 and 2 old history text";
                let event = NormalizedEvent::text_delta(ProviderKind::Gemini, conv_id_clone.clone(), history_text.to_string());

                if !rc_filter.load(Ordering::SeqCst) {
                    let _ = activity_tx.try_send(());
                } else {
                    // If seen_replay_activity was missing, this would leak because 750ms elapsed before 1000ms!
                    let _ = sender_clone.send(event).await;
                }
            });

            Ok(Box::new(FixedGeminiSession {
                event_sender: config.event_sender,
                conv_id: config.conversation_id,
                native_sid: native_session_id.to_string(),
                replay_complete,
                replay_ready,
            }))
        }
    }

    let db = Arc::new(Database::new_in_memory().unwrap());
    db.run_migrations().unwrap();

    let resumed_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let mut provider_map = std::collections::HashMap::new();
    provider_map.insert(
        ProviderKind::Gemini,
        Arc::new(MockGeminiProvider1000 { resumed_count: resumed_count.clone() }) as Arc<dyn Provider>,
    );

    let manager = ConversationManager::new(db.clone(), Arc::new(seralyn_lib::app::providers::ProviderManager::with_providers(provider_map)));
    let conv = manager.create_conversation(Some("History Isolation 1000ms Test")).unwrap();
    messages::create_message(&db, &conv.id, None, "user", "Turn 1 Prompt", None, None, None, None).unwrap();
    messages::create_message(&db, &conv.id, None, "assistant", "Turn 1 Response", Some("gemini"), Some("gemini-3.1-flash-lite"), Some("gemini-old-sid"), None).unwrap();
    messages::create_message(&db, &conv.id, None, "user", "Turn 2 Prompt", None, None, None, None).unwrap();
    messages::create_message(&db, &conv.id, None, "assistant", "Turn 2 Response", Some("gemini"), Some("gemini-3.1-flash-lite"), Some("gemini-old-sid"), None).unwrap();

    provider_sessions::create_provider_session(
        &db,
        &conv.id,
        "gemini",
        Some("gemini-old-sid"),
        Some("gemini-3.1-flash-lite"),
    ).unwrap();

    let (tx, mut rx) = tokio::sync::mpsc::channel::<NormalizedEvent>(10);
    manager.send_message(&conv.id, "Turn 3 Prompt", ProviderKind::Gemini, tx).await.unwrap();

    let mut turn3_result = String::new();
    while let Some(ev) = rx.recv().await {
        if let EventPayload::Text { content } = ev.payload {
            turn3_result.push_str(&content);
        }
        if ev.event_type == EventType::SessionFinished {
            break;
        }
    }
    
    tokio::time::sleep(Duration::from_millis(200)).await;

    assert_eq!(resumed_count.load(Ordering::SeqCst), 1);
    let all_msgs = messages::get_messages(&db, &conv.id).unwrap();
    assert_eq!(all_msgs.len(), 6);
    assert_eq!(all_msgs[5].content, turn3_result);
    assert_eq!(all_msgs[5].content, "Turn 3 Response. Prompt was Turn 3 Prompt");
    assert!(!all_msgs[5].content.contains("old history text"), "Failed deduplication: 1000ms delayed chunk leaked!");
}

#[tokio::test]
async fn test_gemini_resume_no_history_completes() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;
    use tokio::sync::Notify;
    use seralyn_lib::app::conversation::ConversationManager;
    use seralyn_lib::app::providers::{Provider, ProviderSession, SessionConfig, ProviderMessage, ProviderCapabilities, SessionMetadata};
    use seralyn_lib::app::db::{Database, messages, provider_sessions};

    struct FixedGeminiSession {
        event_sender: tokio::sync::mpsc::Sender<NormalizedEvent>,
        conv_id: String,
        native_sid: String,
        replay_complete: Arc<AtomicBool>,
        replay_ready: Arc<Notify>,
    }

    #[async_trait::async_trait]
    impl ProviderSession for FixedGeminiSession {
        async fn send(&self, msg: ProviderMessage) -> seralyn_lib::app::error::Result<()> {
            if !self.replay_complete.load(Ordering::SeqCst) {
                let notified = self.replay_ready.notified();
                if !self.replay_complete.load(Ordering::SeqCst) {
                    let _ = tokio::time::timeout(Duration::from_millis(3000), notified).await;
                }
            }

            let sender = self.event_sender.clone();
            let conv_id = self.conv_id.clone();
            let response_text = format!("Turn 3 Response. Prompt was {}", msg.content);
            
            tokio::spawn(async move {
                let delta = NormalizedEvent::text_delta(ProviderKind::Gemini, conv_id.clone(), response_text);
                let _ = sender.send(delta).await;
                
                let finish = NormalizedEvent::session_finished(ProviderKind::Gemini, conv_id);
                let _ = sender.send(finish).await;
            });
            Ok(())
        }
        async fn interrupt(&self) -> seralyn_lib::app::error::Result<()> { Ok(()) }
        async fn cancel(&self) -> seralyn_lib::app::error::Result<()> { Ok(()) }
        async fn close(&self) -> seralyn_lib::app::error::Result<()> { Ok(()) }
        fn native_session_id(&self) -> Option<String> { Some(self.native_sid.clone()) }
        fn metadata(&self) -> SessionMetadata {
            SessionMetadata {
                provider: ProviderKind::Gemini,
                native_session_id: Some(self.native_sid.clone()),
                model: Some("gemini-3.1-flash-lite".to_string()),
                created_at: String::new(),
            }
        }
        fn is_active(&self) -> bool { true }
    }

    struct MockGeminiProviderNoHistory {
        resumed_count: Arc<std::sync::atomic::AtomicUsize>,
    }

    #[async_trait::async_trait]
    impl Provider for MockGeminiProviderNoHistory {
        fn kind(&self) -> ProviderKind { ProviderKind::Gemini }
        async fn detect_installation(&self) -> seralyn_lib::app::error::Result<seralyn_lib::app::providers::InstallationInfo> {
            Ok(seralyn_lib::app::providers::InstallationInfo { installed: true, executable_path: None, version: None })
        }
        async fn check_authentication(&self) -> seralyn_lib::app::error::Result<seralyn_lib::app::providers::AuthStatus> {
            Ok(seralyn_lib::app::providers::AuthStatus::Authenticated)
        }
        fn capabilities(&self) -> ProviderCapabilities { Default::default() }
        async fn create_session(&self, _config: SessionConfig) -> seralyn_lib::app::error::Result<Box<dyn ProviderSession>> {
            panic!("Test must invoke resume_session");
        }
        async fn resume_session(&self, native_session_id: &str, config: SessionConfig) -> seralyn_lib::app::error::Result<Box<dyn ProviderSession>> {
            self.resumed_count.fetch_add(1, Ordering::SeqCst);
            let replay_complete = Arc::new(AtomicBool::new(false));
            let replay_ready = Arc::new(Notify::new());

            let rc = replay_complete.clone();
            let rr = replay_ready.clone();

            // Quiescence task with NO history activity: must complete via max_duration
            tokio::spawn(async move {
                let max_duration = Duration::from_millis(350);
                let start = tokio::time::Instant::now();

                while start.elapsed() < max_duration {
                    tokio::time::sleep(Duration::from_millis(25)).await;
                }

                rc.store(true, Ordering::SeqCst);
                rr.notify_waiters();
            });

            Ok(Box::new(FixedGeminiSession {
                event_sender: config.event_sender,
                conv_id: config.conversation_id,
                native_sid: native_session_id.to_string(),
                replay_complete,
                replay_ready,
            }))
        }
    }

    let db = Arc::new(Database::new_in_memory().unwrap());
    db.run_migrations().unwrap();

    let resumed_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let mut provider_map = std::collections::HashMap::new();
    provider_map.insert(
        ProviderKind::Gemini,
        Arc::new(MockGeminiProviderNoHistory { resumed_count: resumed_count.clone() }) as Arc<dyn Provider>,
    );

    let manager = ConversationManager::new(db.clone(), Arc::new(seralyn_lib::app::providers::ProviderManager::with_providers(provider_map)));
    let conv = manager.create_conversation(Some("No History Resume Test")).unwrap();
    messages::create_message(&db, &conv.id, None, "user", "Turn 1 Prompt", None, None, None, None).unwrap();
    messages::create_message(&db, &conv.id, None, "assistant", "Turn 1 Response", Some("gemini"), Some("gemini-3.1-flash-lite"), Some("gemini-old-sid"), None).unwrap();

    provider_sessions::create_provider_session(
        &db,
        &conv.id,
        "gemini",
        Some("gemini-old-sid"),
        Some("gemini-3.1-flash-lite"),
    ).unwrap();

    let (tx, mut rx) = tokio::sync::mpsc::channel::<NormalizedEvent>(10);
    manager.send_message(&conv.id, "Turn 2 Prompt", ProviderKind::Gemini, tx).await.unwrap();

    let mut turn2_result = String::new();
    while let Some(ev) = rx.recv().await {
        if let EventPayload::Text { content } = ev.payload {
            turn2_result.push_str(&content);
        }
        if ev.event_type == EventType::SessionFinished {
            break;
        }
    }
    
    tokio::time::sleep(Duration::from_millis(150)).await;

    assert_eq!(resumed_count.load(Ordering::SeqCst), 1);
    let all_msgs = messages::get_messages(&db, &conv.id).unwrap();
    assert_eq!(all_msgs.len(), 4);
    assert_eq!(all_msgs[3].content, turn2_result);
    assert_eq!(all_msgs[3].content, "Turn 3 Response. Prompt was Turn 2 Prompt");
}

#[tokio::test]
async fn test_multi_account_same_provider_isolation_and_exact_routing() {
    use seralyn_lib::app::conversation::ConversationManager;
    use seralyn_lib::app::db::provider_sessions;
    use seralyn_lib::app::providers::{Provider, ProviderSession, SessionConfig, SessionMetadata, ProviderCapabilities};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    struct MultiAccountSession {
        account: String,
        interrupted: Arc<AtomicBool>,
        approval_responded: Arc<AtomicBool>,
        event_sender: tokio::sync::mpsc::Sender<NormalizedEvent>,
        conv_id: String,
    }

    #[async_trait::async_trait]
    impl ProviderSession for MultiAccountSession {
        async fn send(&self, _msg: seralyn_lib::app::providers::ProviderMessage) -> seralyn_lib::app::error::Result<()> {
            let _ = self.event_sender.send(NormalizedEvent::text_delta(ProviderKind::Claude, self.conv_id.clone(), format!("Reply from {}", self.account))).await;
            let _ = self.event_sender.send(NormalizedEvent::session_finished(ProviderKind::Claude, self.conv_id.clone())).await;
            Ok(())
        }
        async fn interrupt(&self) -> seralyn_lib::app::error::Result<()> {
            self.interrupted.store(true, Ordering::SeqCst);
            Ok(())
        }
        async fn respond_to_approval(&self, _id: &str, _approved: bool) -> seralyn_lib::app::error::Result<()> {
            self.approval_responded.store(true, Ordering::SeqCst);
            Ok(())
        }
        async fn cancel(&self) -> seralyn_lib::app::error::Result<()> {
            Ok(())
        }
        async fn close(&self) -> seralyn_lib::app::error::Result<()> {
            Ok(())
        }
        fn native_session_id(&self) -> Option<String> {
            Some(format!("native-{}", self.account))
        }
        fn is_active(&self) -> bool {
            true
        }
        fn metadata(&self) -> SessionMetadata {
            SessionMetadata {
                provider: ProviderKind::Claude,
                native_session_id: Some(format!("native-{}", self.account)),
                model: Some("claude-3-7-sonnet".to_string()),
                created_at: "now".to_string(),
            }
        }
    }

    struct MultiAccountProvider {
        work_interrupted: Arc<AtomicBool>,
        work_approval: Arc<AtomicBool>,
        personal_interrupted: Arc<AtomicBool>,
        personal_approval: Arc<AtomicBool>,
    }

    #[async_trait::async_trait]
    impl Provider for MultiAccountProvider {
        fn kind(&self) -> ProviderKind { ProviderKind::Claude }
        async fn detect_installation(&self) -> seralyn_lib::app::error::Result<seralyn_lib::app::providers::InstallationInfo> {
            Ok(seralyn_lib::app::providers::InstallationInfo { installed: true, executable_path: None, version: Some("2.1.270".to_string()) })
        }
        async fn check_authentication(&self) -> seralyn_lib::app::error::Result<seralyn_lib::app::providers::AuthStatus> {
            Ok(seralyn_lib::app::providers::AuthStatus::Authenticated)
        }
        fn capabilities(&self) -> ProviderCapabilities { Default::default() }
        async fn create_session(&self, config: SessionConfig) -> seralyn_lib::app::error::Result<Box<dyn ProviderSession>> {
            let acc = config.account.clone().unwrap_or_else(|| "default".to_string());
            let (interrupted, approval) = if acc == "work" {
                (self.work_interrupted.clone(), self.work_approval.clone())
            } else {
                (self.personal_interrupted.clone(), self.personal_approval.clone())
            };
            Ok(Box::new(MultiAccountSession {
                account: acc,
                interrupted,
                approval_responded: approval,
                event_sender: config.event_sender,
                conv_id: config.conversation_id,
            }))
        }
        async fn resume_session(&self, _native_id: &str, config: SessionConfig) -> seralyn_lib::app::error::Result<Box<dyn ProviderSession>> {
            self.create_session(config).await
        }
    }

    let db = Arc::new(Database::new_in_memory().unwrap());
    db.run_migrations().unwrap();

    let work_interrupted = Arc::new(AtomicBool::new(false));
    let work_approval = Arc::new(AtomicBool::new(false));
    let personal_interrupted = Arc::new(AtomicBool::new(false));
    let personal_approval = Arc::new(AtomicBool::new(false));

    let provider = Arc::new(MultiAccountProvider {
        work_interrupted: work_interrupted.clone(),
        work_approval: work_approval.clone(),
        personal_interrupted: personal_interrupted.clone(),
        personal_approval: personal_approval.clone(),
    });

    let mut provider_map = std::collections::HashMap::new();
    provider_map.insert(ProviderKind::Claude, provider as Arc<dyn Provider>);

    let manager = ConversationManager::new(db.clone(), Arc::new(seralyn_lib::app::providers::ProviderManager::with_providers(provider_map)));
    let conv = manager.create_conversation(Some("Multi-Account Isolation")).unwrap();

    // Turn 1: Send on "work" account
    let (tx1, mut rx1) = tokio::sync::mpsc::channel(10);
    manager.send_message_with_attachments(
        &conv.id,
        "Hello from work",
        ProviderKind::Claude,
        vec![],
        Some("claude-3-7-sonnet".to_string()),
        Some("work".to_string()),
        Some("high".to_string()),
        tx1,
    ).await.unwrap();
    while let Some(ev) = rx1.recv().await {
        if ev.event_type == EventType::SessionFinished { break; }
    }

    // Turn 2: Send on "personal" account
    let (tx2, mut rx2) = tokio::sync::mpsc::channel(10);
    manager.send_message_with_attachments(
        &conv.id,
        "Hello from personal",
        ProviderKind::Claude,
        vec![],
        Some("claude-3-7-sonnet".to_string()),
        Some("personal".to_string()),
        Some("low".to_string()),
        tx2,
    ).await.unwrap();
    while let Some(ev) = rx2.recv().await {
        if ev.event_type == EventType::SessionFinished { break; }
    }

    tokio::time::sleep(std::time::Duration::from_millis(150)).await;

    // Assert: Interrupt "work" account ONLY!
    manager.interrupt_turn(&conv.id, ProviderKind::Claude, Some("work")).await.unwrap();
    assert!(work_interrupted.load(Ordering::SeqCst), "Work session must be interrupted");
    assert!(!personal_interrupted.load(Ordering::SeqCst), "Personal session must NOT be interrupted!");

    // Assert: Respond to approval on "personal" account ONLY!
    manager.respond_to_approval(&conv.id, ProviderKind::Claude, Some("personal"), "app-1", true).await.unwrap();
    assert!(personal_approval.load(Ordering::SeqCst), "Personal session must receive approval response");
    assert!(!work_approval.load(Ordering::SeqCst), "Work session must NOT receive personal approval response!");

    // Assert: SQLite provider_sessions has separate scoped records
    let work_sess = provider_sessions::get_active_session_for_account(&db, &conv.id, "claude", Some("work")).unwrap().unwrap();
    let personal_sess = provider_sessions::get_active_session_for_account(&db, &conv.id, "claude", Some("personal")).unwrap().unwrap();
    assert_ne!(work_sess.id, personal_sess.id, "Work and Personal must have distinct SQLite session records");
    assert_eq!(work_sess.provider_session_id, Some("native-work".to_string()));
    assert_eq!(personal_sess.provider_session_id, Some("native-personal".to_string()));
}

#[test]
fn test_default_vs_named_account_isolation_in_db() {
    use seralyn_lib::app::db::provider_sessions;

    let db = Database::new_in_memory().unwrap();
    db.run_migrations().unwrap();

    let conv = conversations::create_conversation(&db, Some("Isolation Test")).unwrap();
    let conv_id = &conv.id;

    // Create work session
    let work_meta = serde_json::json!({ "account": "work" }).to_string();
    provider_sessions::create_provider_session_with_metadata(
        &db,
        conv_id,
        "claude",
        Some("native-work"),
        Some("sonnet"),
        Some(&work_meta),
    ).unwrap();

    // Create personal session
    let personal_meta = serde_json::json!({ "account": "personal" }).to_string();
    provider_sessions::create_provider_session_with_metadata(
        &db,
        conv_id,
        "claude",
        Some("native-personal"),
        Some("sonnet"),
        Some(&personal_meta),
    ).unwrap();

    // 1. Exact lookups
    let work = provider_sessions::get_active_session_for_account(&db, conv_id, "claude", Some("work")).unwrap();
    assert!(work.is_some());
    assert_eq!(work.unwrap().provider_session_id, Some("native-work".to_string()));

    let personal = provider_sessions::get_active_session_for_account(&db, conv_id, "claude", Some("personal")).unwrap();
    assert!(personal.is_some());
    assert_eq!(personal.unwrap().provider_session_id, Some("native-personal".to_string()));

    // 2. Default lookup MUST NOT return work or personal!
    let default_lookup = provider_sessions::get_active_session_for_account(&db, conv_id, "claude", Some("default")).unwrap();
    assert!(default_lookup.is_none(), "Default lookup must NOT fallback to named accounts like work or personal!");

    let none_lookup = provider_sessions::get_active_session_for_account(&db, conv_id, "claude", None).unwrap();
    assert!(none_lookup.is_none(), "None lookup must NOT fallback to named accounts!");

    // 3. If a legacy record (metadata_json is None) exists, default lookup returns it
    let conv_legacy = conversations::create_conversation(&db, Some("Legacy Test")).unwrap();
    let conv_id_legacy = &conv_legacy.id;

    provider_sessions::create_provider_session(
        &db,
        conv_id_legacy,
        "claude",
        Some("native-legacy"),
        Some("sonnet"),
    ).unwrap();

    let legacy_lookup = provider_sessions::get_active_session_for_account(&db, conv_id_legacy, "claude", Some("default")).unwrap();
    assert!(legacy_lookup.is_some());
    assert_eq!(legacy_lookup.unwrap().provider_session_id, Some("native-legacy".to_string()));
}

#[test]
fn test_adapter_profile_isolation_and_native_effort_contracts() {
    use seralyn_lib::app::providers::resolve_profile_dir;

    // 1. Profile directory resolution
    // Default or empty accounts must NOT isolate (fall back to user's standard CLI config)
    assert_eq!(resolve_profile_dir(ProviderKind::Claude, None), None);
    assert_eq!(resolve_profile_dir(ProviderKind::Claude, Some("default")), None);
    assert_eq!(resolve_profile_dir(ProviderKind::Codex, Some("")), None);

    // Named accounts must resolve to dedicated profile paths
    let claude_work = resolve_profile_dir(ProviderKind::Claude, Some("work")).unwrap();
    assert!(claude_work.to_string_lossy().contains("profiles"));
    assert!(claude_work.to_string_lossy().contains("claude"));
    assert!(claude_work.to_string_lossy().contains("work"));

    let codex_work = resolve_profile_dir(ProviderKind::Codex, Some("work-team")).unwrap();
    assert!(codex_work.to_string_lossy().contains("codex"));
    assert!(codex_work.to_string_lossy().contains("work-team"));

    let gemini_studio = resolve_profile_dir(ProviderKind::Gemini, Some("studio")).unwrap();
    assert!(gemini_studio.to_string_lossy().contains("gemini"));
    assert!(gemini_studio.to_string_lossy().contains("studio"));

    // 2. Real production Codex builder contracts:
    let turn_params = seralyn_lib::app::providers::codex::build_codex_turn_params("tid-123", "Hello", Some("o3-mini"), Some("high"));
    assert_eq!(turn_params.get("effort").and_then(|v| v.as_str()), Some("high"));
    assert!(turn_params.get("reasoningEffort").is_none(), "Codex turn/start must use 'effort', NOT 'reasoningEffort'");

    let start_params = seralyn_lib::app::providers::codex::build_codex_start_params(
        seralyn_lib::app::providers::PermissionMode::Safe,
        None,
        Some("o3-mini"),
        Some("high"),
    );
    assert_eq!(start_params.get("effort").and_then(|v| v.as_str()), Some("high"));
    assert!(start_params.get("reasoningEffort").is_none(), "Codex thread/start must use 'effort', NOT 'reasoningEffort'");

    // 3. Real production Gemini prompt builder contract:
    let prompt_params = seralyn_lib::app::providers::gemini::build_gemini_prompt_params("sid-123", "Hello");
    assert!(prompt_params.get("prompt").is_some());
    assert!(prompt_params.get("model").is_none(), "Gemini ACP session/prompt must NOT contain 'model'");

    // 4. Real production Claude args builder contract:
    let claude_args = seralyn_lib::app::providers::claude::build_claude_args(
        "Hello",
        Some("sonnet"),
        Some("high"),
        Some("sid-123"),
        seralyn_lib::app::providers::PermissionMode::Safe,
    );
    assert!(claude_args.contains(&"--effort".to_string()));
    assert!(!claude_args.contains(&"--profile".to_string()), "Claude must NOT use non-standard --profile");
    assert!(!claude_args.contains(&"--max-thinking-tokens".to_string()), "Claude must use --effort, NOT --max-thinking-tokens");
}

#[test]
fn test_adaptive_context_model_windows_and_budget_calculation() {
    use seralyn_lib::app::conversation::context::{calculate_input_budget, resolve_model_context_window};

    // 1. Claude model windows
    assert_eq!(resolve_model_context_window(ProviderKind::Claude, Some("opus")), 1_000_000);
    assert_eq!(resolve_model_context_window(ProviderKind::Claude, Some("claude-opus-5")), 1_000_000);
    assert_eq!(resolve_model_context_window(ProviderKind::Claude, Some("sonnet")), 1_000_000);
    assert_eq!(resolve_model_context_window(ProviderKind::Claude, Some("claude-sonnet-5")), 1_000_000);
    assert_eq!(resolve_model_context_window(ProviderKind::Claude, Some("haiku")), 200_000);
    assert_eq!(resolve_model_context_window(ProviderKind::Claude, Some("claude-haiku-4-5-20251001")), 200_000);

    // 2. Codex model windows
    assert_eq!(resolve_model_context_window(ProviderKind::Codex, Some("o3")), 200_000);
    assert_eq!(resolve_model_context_window(ProviderKind::Codex, Some("o3-mini")), 200_000);
    assert_eq!(resolve_model_context_window(ProviderKind::Codex, Some("o1")), 200_000);
    assert_eq!(resolve_model_context_window(ProviderKind::Codex, Some("gpt-4.5")), 128_000);
    assert_eq!(resolve_model_context_window(ProviderKind::Codex, Some("gpt-4o")), 128_000);

    // 3. Gemini model windows
    assert_eq!(resolve_model_context_window(ProviderKind::Gemini, Some("gemini-2.5-flash")), 1_000_000);
    assert_eq!(resolve_model_context_window(ProviderKind::Gemini, Some("gemini-2.5-pro")), 1_000_000);

    // 4. Input budget calculations:
    // 200K window -> 30K output reserve + 10K overhead + 10K safety = 50K total reserve -> 150K input budget
    assert_eq!(calculate_input_budget(200_000), 150_000);

    // 1M window -> clamped 64K output reserve + 10K overhead + 10K safety = 84K total reserve -> 916K input budget
    assert_eq!(calculate_input_budget(1_000_000), 916_000);

    // 128K window -> 19.2K reserve + 20K = ~39.2K reserve -> 88.8K budget
    assert_eq!(calculate_input_budget(128_000), 88_800);
}

#[test]
fn test_adaptive_context_no_compaction_when_within_budget() {
    use seralyn_lib::app::conversation::context::build_adaptive_context;

    let db = Database::new_in_memory().unwrap();
    db.run_migrations().unwrap();

    let conv = conversations::create_conversation(&db, Some("Adaptive Test - No Compaction")).unwrap();
    messages::create_message(&db, &conv.id, None, "user", "Hello Opus", None, None, None, None).unwrap();
    messages::create_message(&db, &conv.id, None, "assistant", "Hello! I am ready.", Some("claude"), None, None, None).unwrap();
    let current_turn = messages::create_message(&db, &conv.id, None, "user", "Next turn", None, None, None, None).unwrap();

    // Calling adaptive context for Opus 5 (1M window, 916K budget):
    let result = build_adaptive_context(
        &db,
        &conv.id,
        0,
        current_turn.seq,
        ProviderKind::Claude,
        Some("opus"),
        0,
        "",
    ).unwrap();

    assert!(!result.compacted, "Within 916K budget, no compaction must occur");
    assert_eq!(result.messages.len(), 2);
    assert_eq!(result.messages[0].content, "Hello Opus");
    assert_eq!(result.messages[1].content, "Hello! I am ready.");
    assert_eq!(result.working_tokens, result.canonical_tokens);
}

#[test]
fn test_adaptive_context_compaction_when_exceeding_budget() {
    use seralyn_lib::app::conversation::context::build_adaptive_context;

    let db = Database::new_in_memory().unwrap();
    db.run_migrations().unwrap();

    let conv = conversations::create_conversation(&db, Some("Adaptive Test - Compaction")).unwrap();

    // Create 12 large turns in canonical history to simulate token volume exceeding 150K
    let large_chunk = "Detailed technical analysis of complex architectural patterns and memory safety invariants. ".repeat(600);
    for i in 1..=12 {
        let role = if i % 2 == 1 { "user" } else { "assistant" };
        let prov = if role == "assistant" { Some("claude") } else { None };
        messages::create_message(
            &db,
            &conv.id,
            None,
            role,
            &format!("Turn {i}: {large_chunk}"),
            prov,
            None,
            None,
            None,
        ).unwrap();
    }
    let current_turn = messages::create_message(&db, &conv.id, None, "user", "Current turn prompt", None, None, None, None).unwrap();

    // Switching to Haiku (200K window, 150K budget):
    let result = build_adaptive_context(
        &db,
        &conv.id,
        0,
        current_turn.seq,
        ProviderKind::Claude,
        Some("haiku"),
        0,
        "",
    ).unwrap();

    assert!(result.compacted, "Exceeding 150K budget must trigger compaction");
    assert!(result.canonical_tokens > result.input_budget, "Canonical tokens should exceed budget");
    assert!(result.working_tokens <= result.input_budget, "Working tokens must fit within 150K budget! Actual: {}", result.working_tokens);

    // Assert that the first message is the handoff summary
    assert_eq!(result.messages[0].role, "assistant");
    assert_eq!(result.messages[0].provider.as_deref(), Some("handoff-manager"));
    assert!(result.messages[0].content.contains("[Context Hand-off:"));
    assert!(result.messages[0].content.contains("### Condensed Prior Context:"));

    // Assert that recent turns are preserved in raw fidelity
    let last_raw_msg = result.messages.last().unwrap();
    assert!(last_raw_msg.content.contains("Turn 12:"));
}

#[test]
fn test_canonical_db_fidelity_unmodified_by_compaction() {
    use seralyn_lib::app::conversation::context::build_adaptive_context;

    let db = Database::new_in_memory().unwrap();
    db.run_migrations().unwrap();

    let conv = conversations::create_conversation(&db, Some("Canonical DB Fidelity Test")).unwrap();

    let original_content_1 = "Original turn 1 content with precise invariant specifications";
    let original_content_2 = "Original assistant turn 2 response with verbatim code";
    messages::create_message(&db, &conv.id, None, "user", original_content_1, None, None, None, None).unwrap();
    messages::create_message(&db, &conv.id, None, "assistant", original_content_2, Some("claude"), None, None, None).unwrap();
    let current = messages::create_message(&db, &conv.id, None, "user", "Turn 3", None, None, None, None).unwrap();

    // Execute adaptive context with compaction target
    let _ = build_adaptive_context(
        &db,
        &conv.id,
        0,
        current.seq,
        ProviderKind::Claude,
        Some("haiku"),
        0,
        "",
    ).unwrap();

    // Query SQLite messages table directly: verify canonical records are 100% intact!
    let all_messages = messages::get_messages(&db, &conv.id).unwrap();
    assert_eq!(all_messages.len(), 3);
    assert_eq!(all_messages[0].content, original_content_1);
    assert_eq!(all_messages[1].content, original_content_2);
    assert_eq!(all_messages[2].content, "Turn 3");
}

#[test]
fn test_switch_back_to_larger_session_resumes_without_compaction_pollution() {
    use seralyn_lib::app::conversation::context::build_adaptive_context;

    let db = Database::new_in_memory().unwrap();
    db.run_migrations().unwrap();

    let conv = conversations::create_conversation(&db, Some("Switch Back Resumption Test")).unwrap();

    // 1. Opus session had run up to seq 4 (Opus sync cursor = 4)
    messages::create_message(&db, &conv.id, None, "user", "Opus Turn 1", None, None, None, None).unwrap();
    messages::create_message(&db, &conv.id, None, "assistant", "Opus Answer 1", Some("claude"), None, None, None).unwrap();
    messages::create_message(&db, &conv.id, None, "user", "Opus Turn 2", None, None, None, None).unwrap();
    messages::create_message(&db, &conv.id, None, "assistant", "Opus Answer 2", Some("claude"), None, None, None).unwrap();

    // 2. User switched to Haiku (Haiku sync cursor = 0):
    // Haiku ran turns 5 and 6
    messages::create_message(&db, &conv.id, None, "user", "Haiku Turn 3", None, None, None, None).unwrap();
    messages::create_message(&db, &conv.id, None, "assistant", "Haiku Answer 3", Some("claude"), None, None, None).unwrap();

    // 3. User sends turn 7 and switches BACK to Opus 5:
    let current_turn = messages::create_message(&db, &conv.id, None, "user", "Back to Opus Turn 4", None, None, None, None).unwrap();

    // For Opus 5, its sync cursor is 4!
    let opus_res = build_adaptive_context(
        &db,
        &conv.id,
        4, // Opus synced_through_seq!
        current_turn.seq,
        ProviderKind::Claude,
        Some("opus"),
        0,
        "",
    ).unwrap();

    // Assert: Opus ONLY receives turns 5 and 6 (the delta since its sync cursor)
    assert!(!opus_res.compacted, "Opus incremental delta is small and fits in 916K budget without compaction");
    assert_eq!(opus_res.messages.len(), 2);
    assert_eq!(opus_res.messages[0].content, "Haiku Turn 3");
    assert_eq!(opus_res.messages[1].content, "Haiku Answer 3");

    // ZERO pollution: Opus does NOT receive Haiku's compacted summary!
    assert!(!opus_res.messages.iter().any(|m| m.content.contains("Context Hand-off")));
}

#[test]
fn test_adaptive_context_thai_unicode_safety() {
    use seralyn_lib::app::conversation::context::build_adaptive_context;

    let db = Database::new_in_memory().unwrap();
    db.run_migrations().unwrap();

    let conv = conversations::create_conversation(&db, Some("Thai Unicode Safety Test")).unwrap();

    let thai_turn_1 = "ช่วยแก้ Maya tool ตรงนี้หน่อยครับ มีปัญหาเรื่อง unicode UTF-8 encoding และคำสั่ง python ใน Maya ที่ยาวมาก";
    let thai_turn_2 = "ได้เลยครับ สำหรับ Maya python script นั้นเราจำเป็นต้องกำหนด encoding utf-8 ให้ถูกต้อง และตรวจสอบ character boundary ให้แม่นยำเพื่อไม่ให้เกิด panic ในขณะที่มีการตัดคำภาษาไทยครับ";

    // Repeat to create a long string with multibyte characters (Thai characters are 3 bytes each)
    let thai_long = format!("{} {}", thai_turn_1, thai_turn_2).repeat(150);

    for i in 1..=6 {
        let role = if i % 2 == 1 { "user" } else { "assistant" };
        messages::create_message(
            &db,
            &conv.id,
            None,
            role,
            &format!("รอบที่ {i}: {thai_long}"),
            if role == "assistant" { Some("claude") } else { None },
            None,
            None,
            None,
        ).unwrap();
    }
    let current_turn = messages::create_message(&db, &conv.id, None, "user", "ข้อความภาษาไทยรอบปัจจุบัน", None, None, None, None).unwrap();

    // Call build_adaptive_context targeting Haiku (which forces compaction and character-safe slicing)
    let result = build_adaptive_context(
        &db,
        &conv.id,
        0,
        current_turn.seq,
        ProviderKind::Claude,
        Some("haiku"),
        0,
        "ข้อความภาษาไทยรอบปัจจุบัน",
    );

    assert!(result.is_ok(), "Adaptive context MUST not panic on Thai multibyte UTF-8 characters: {:?}", result.err());
    let res = result.unwrap();
    assert!(res.compacted, "Should trigger compaction");
    assert!(res.working_tokens <= res.input_budget, "Working tokens must fit within input budget");
    // Verify Thai text in summary does not contain broken UTF-8
    let summary = &res.messages[0].content;
    assert!(summary.contains("รอบที่ 1:"), "Summary must contain turns");
    assert!(std::str::from_utf8(summary.as_bytes()).is_ok(), "Must be valid UTF-8");
}

#[test]
fn test_adaptive_context_hard_budget_guarantee_single_large_message() {
    use seralyn_lib::app::conversation::context::build_adaptive_context;

    let db = Database::new_in_memory().unwrap();
    db.run_migrations().unwrap();

    let conv = conversations::create_conversation(&db, Some("Hard Budget Single Large Message Test")).unwrap();

    // Single message containing ~180K tokens (approx 720K characters)
    let huge_content = "X".repeat(720_000);
    messages::create_message(&db, &conv.id, None, "user", &huge_content, None, None, None, None).unwrap();
    let current_turn = messages::create_message(&db, &conv.id, None, "user", "Next turn", None, None, None, None).unwrap();

    // Budget for Haiku is 150K tokens. A single message of 180K tokens alone exceeds the budget!
    let result = build_adaptive_context(
        &db,
        &conv.id,
        0,
        current_turn.seq,
        ProviderKind::Claude,
        Some("haiku"),
        0,
        "Next turn",
    ).unwrap();

    // Hard budget guarantee must hold:
    assert!(
        result.working_tokens <= result.input_budget,
        "Working tokens ({}) MUST be <= input_budget ({}) even for single huge turn",
        result.working_tokens,
        result.input_budget
    );
}

#[test]
fn test_cross_model_switch_opus_to_haiku_and_back_full_lifecycle() {
    use seralyn_lib::app::conversation::context::build_adaptive_context;
    use seralyn_lib::app::db::provider_sessions::{create_provider_session_with_metadata, get_active_session_for_account_model, update_synced_seq};
    use seralyn_lib::app::db::usage_snapshots::{save_usage_snapshot, get_latest_usage_snapshot};

    let db = Database::new_in_memory().unwrap();
    db.run_migrations().unwrap();

    let conv = conversations::create_conversation(&db, Some("Full Lifecycle Opus-Haiku Switch")).unwrap();

    // 1. Initial Opus 5 session (1M window) runs on account "work", processes turns 1..=500
    for i in 1..=500 {
        let role = if i % 2 == 1 { "user" } else { "assistant" };
        messages::create_message(
            &db,
            &conv.id,
            None,
            role,
            &format!("Turn {i}: content data chunk for large conversation testing"),
            if role == "assistant" { Some("claude") } else { None },
            None,
            None,
            None,
        ).unwrap();
    }

    let opus_session = create_provider_session_with_metadata(
        &db,
        &conv.id,
        "claude",
        Some("opus-session-123"),
        Some("opus"),
        Some(r#"{"account":"work"}"#),
    ).unwrap();
    update_synced_seq(&db, &opus_session.id, 500).unwrap();

    // Opus has recorded native usage snapshot of 400K tokens
    save_usage_snapshot(&db, &conv.id, Some(&opus_session.id), Some(400_000), Some(10_000), None, None, None, Some(410_000), Some(1_000_000), "Exact").unwrap();

    // Verify Opus session lookup by (account, model)
    let found_opus = get_active_session_for_account_model(&db, &conv.id, "claude", Some("work"), Some("opus")).unwrap().unwrap();
    assert_eq!(found_opus.synced_through_seq, 500);
    assert_eq!(found_opus.provider_session_id.as_deref(), Some("opus-session-123"));

    // 2. User switches to Haiku 4.5 on account "work"
    // Haiku session does not exist yet for (work, haiku)
    let haiku_session_opt = get_active_session_for_account_model(&db, &conv.id, "claude", Some("work"), Some("haiku")).unwrap();
    assert!(haiku_session_opt.is_none(), "Haiku must not have an active session yet");

    // Haiku starts at cursor 0 and has 0 native tokens
    let haiku_active_cursor = 0;
    let haiku_context = build_adaptive_context(
        &db,
        &conv.id,
        haiku_active_cursor,
        500,
        ProviderKind::Claude,
        Some("haiku"),
        0,
        "Current prompt for Haiku",
    ).unwrap();

    // Haiku receives compacted history fitting within its 150K budget
    assert!(haiku_context.working_tokens <= haiku_context.input_budget);
    assert!(haiku_context.compacted);
    assert!(haiku_context.messages[0].content.contains("[Context Hand-off:"));

    // Haiku creates its own native session and syncs through turn 500
    let haiku_session = create_provider_session_with_metadata(
        &db,
        &conv.id,
        "claude",
        Some("haiku-session-456"),
        Some("haiku"),
        Some(r#"{"account":"work"}"#),
    ).unwrap();
    update_synced_seq(&db, &haiku_session.id, 500).unwrap();

    // Haiku executes turns 501..=504
    for i in 501..=504 {
        let role = if i % 2 == 1 { "user" } else { "assistant" };
        messages::create_message(
            &db,
            &conv.id,
            None,
            role,
            &format!("Turn {i}: haiku specific turn"),
            if role == "assistant" { Some("claude") } else { None },
            None,
            None,
            None,
        ).unwrap();
    }
    update_synced_seq(&db, &haiku_session.id, 504).unwrap();

    // 3. User sends turn 505 and switches BACK to Opus 5 on account "work"
    let turn_505 = messages::create_message(&db, &conv.id, None, "user", "Back to Opus prompt", None, None, None, None).unwrap();

    // Look up active session for (work, opus) -> found Opus session!
    let resumed_opus = get_active_session_for_account_model(&db, &conv.id, "claude", Some("work"), Some("opus")).unwrap().unwrap();
    assert_eq!(resumed_opus.provider_session_id.as_deref(), Some("opus-session-123"));
    assert_eq!(resumed_opus.synced_through_seq, 500, "Opus sync cursor remained at 500!");

    // Query usage snapshot for Opus:
    let opus_usage = get_latest_usage_snapshot(&db, &conv.id, Some("claude"), Some("work"), Some("opus")).unwrap().unwrap();
    let opus_existing_tokens = opus_usage.context_tokens.unwrap_or(0);
    assert_eq!(opus_existing_tokens, 410_000);

    // Build context for Opus: after_seq = 500, existing_native_tokens = 410_000
    let opus_resumed_context = build_adaptive_context(
        &db,
        &conv.id,
        resumed_opus.synced_through_seq,
        turn_505.seq,
        ProviderKind::Claude,
        Some("opus"),
        opus_existing_tokens,
        "Back to Opus prompt",
    ).unwrap();

    // Verification:
    // a) Opus ONLY receives delta messages since seq 500 (turns 501, 502, 503, 504)
    assert_eq!(opus_resumed_context.messages.len(), 4);
    assert_eq!(opus_resumed_context.messages[0].content, "Turn 501: haiku specific turn");
    assert_eq!(opus_resumed_context.messages[3].content, "Turn 504: haiku specific turn");

    // b) Zero compaction pollution: No handoff message
    assert!(!opus_resumed_context.compacted);
    assert!(!opus_resumed_context.messages.iter().any(|m| m.content.contains("Context Hand-off")));

    // c) Available budget dynamically accounts for existing native tokens (1M - 84K reserve - 410K native - prompt)
    assert!(opus_resumed_context.input_budget < 600_000, "Input budget must be reduced by 410K native context");
    assert!(opus_resumed_context.working_tokens <= opus_resumed_context.input_budget);
}


