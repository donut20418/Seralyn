use myai_lib::app::db::conversations::{self, Conversation};
use myai_lib::app::db::messages;
use myai_lib::app::db::Database;
use myai_lib::app::events::{EventType, NormalizedEvent, ProviderKind, EventPayload};
use myai_lib::app::providers::claude::parser::{claude_event_to_normalized, parse_claude_line};
use myai_lib::app::providers::codex::parser::{codex_notification_to_normalized, parse_codex_line, CodexMessage};
use myai_lib::app::providers::gemini::parser::{gemini_notification_to_normalized, parse_gemini_line, AcpMessage};

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
