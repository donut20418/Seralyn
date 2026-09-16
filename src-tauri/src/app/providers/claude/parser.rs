use serde::{Deserialize, Serialize};
use serde_json::Value;
use tracing::{debug, warn};

use crate::app::error::Result;
use crate::app::events::{EventPayload, EventType, NormalizedEvent, ProviderKind, TokenConfidence};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: Option<u64>,
    pub cache_creation_tokens: Option<u64>,
}

#[derive(Debug, Clone)]
pub enum ClaudeEvent {
    Init {
        session_id: Option<String>,
        model: Option<String>,
    },
    ContentBlockDelta {
        delta_type: String,
        text: Option<String>,
        thinking: Option<String>,
    },
    AssistantMessage {
        content: String,
    },
    ToolUse {
        tool_id: String,
        tool_name: String,
        input: Value,
    },
    ToolResult {
        tool_id: String,
        output: Value,
    },
    Result {
        session_id: Option<String>,
        result: Option<String>,
        usage: Option<ClaudeUsage>,
    },
    Error {
        code: Option<String>,
        message: String,
    },
    Unknown {
        raw: Value,
    },
}

pub fn parse_claude_line(line: &str) -> Result<Option<ClaudeEvent>> {
    let line = line.trim();
    if line.is_empty() {
        return Ok(None);
    }

    let value: Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(e) => {
            warn!("Failed to parse Claude NDJSON line: {} - {}", e, line);
            return Ok(None);
        }
    };

    let Some(type_str) = value.get("type").and_then(|v| v.as_str()) else {
        return Ok(Some(ClaudeEvent::Unknown { raw: value }));
    };

    match type_str {
        "system" => {
            let subtype = value.get("subtype").and_then(|v| v.as_str());
            if subtype == Some("init") {
                let session_id = value.get("session_id").and_then(|v| v.as_str()).map(|s| s.to_string());
                let model = value.get("model").and_then(|v| v.as_str()).map(|s| s.to_string());
                Ok(Some(ClaudeEvent::Init { session_id, model }))
            } else {
                Ok(Some(ClaudeEvent::Unknown { raw: value }))
            }
        }
        "init" => {
            let session_id = value.get("session_id").and_then(|v| v.as_str()).map(|s| s.to_string());
            let model = value.get("model").and_then(|v| v.as_str()).map(|s| s.to_string());
            Ok(Some(ClaudeEvent::Init { session_id, model }))
        }
        "assistant" => {
            let mut full_text = String::new();
            if let Some(msg) = value.get("message") {
                if let Some(content_arr) = msg.get("content").and_then(|v| v.as_array()) {
                    for item in content_arr {
                        if item.get("type").and_then(|v| v.as_str()) == Some("text") {
                            if let Some(t) = item.get("text").and_then(|v| v.as_str()) {
                                full_text.push_str(t);
                            }
                        }
                    }
                }
            }
            if !full_text.is_empty() {
                Ok(Some(ClaudeEvent::AssistantMessage { content: full_text }))
            } else {
                Ok(Some(ClaudeEvent::Unknown { raw: value }))
            }
        }
        "stream_event" => {
            let Some(event) = value.get("event") else {
                return Ok(Some(ClaudeEvent::Unknown { raw: value }));
            };
            let Some(ev_type) = event.get("type").and_then(|v| v.as_str()) else {
                return Ok(Some(ClaudeEvent::Unknown { raw: value }));
            };
            match ev_type {
                "content_block_delta" => {
                    let Some(delta) = event.get("delta") else {
                        return Ok(Some(ClaudeEvent::Unknown { raw: value }));
                    };
                    let delta_type = delta.get("type").and_then(|v| v.as_str()).unwrap_or("").to_string();
                    let text = delta.get("text").and_then(|v| v.as_str()).map(|s| s.to_string());
                    let thinking = delta.get("thinking").and_then(|v| v.as_str()).map(|s| s.to_string());
                    Ok(Some(ClaudeEvent::ContentBlockDelta { delta_type, text, thinking }))
                }
                "content_block_start" => {
                    if let Some(cb) = event.get("content_block") {
                        if cb.get("type").and_then(|v| v.as_str()) == Some("tool_use") {
                            let tool_id = cb.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                            let tool_name = cb.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
                            let input = cb.get("input").cloned().unwrap_or(Value::Null);
                            Ok(Some(ClaudeEvent::ToolUse { tool_id, tool_name, input }))
                        } else {
                            Ok(Some(ClaudeEvent::Unknown { raw: value }))
                        }
                    } else {
                        Ok(Some(ClaudeEvent::Unknown { raw: value }))
                    }
                }
                "tool_use" => {
                    let tool_id = event.get("tool_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                    let tool_name = event.get("tool_name").and_then(|v| v.as_str()).unwrap_or("").to_string();
                    let input = event.get("input").cloned().unwrap_or(Value::Null);
                    Ok(Some(ClaudeEvent::ToolUse { tool_id, tool_name, input }))
                }
                "tool_result" => {
                    let tool_id = event.get("tool_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                    let output = event.get("output").cloned().unwrap_or(Value::Null);
                    Ok(Some(ClaudeEvent::ToolResult { tool_id, output }))
                }
                _ => Ok(Some(ClaudeEvent::Unknown { raw: value }))
            }
        }
        "result" => {
            let session_id = value.get("session_id").and_then(|v| v.as_str()).map(|s| s.to_string());
            let result = value.get("result").and_then(|v| v.as_str()).map(|s| s.to_string());
            let usage = if let Some(u) = value.get("usage") {
                serde_json::from_value(u.clone()).ok()
            } else {
                None
            };
            Ok(Some(ClaudeEvent::Result { session_id, result, usage }))
        }
        "error" => {
            let (code, message) = if let Some(err_obj) = value.get("error") {
                let code = err_obj.get("type").and_then(|v| v.as_str()).map(|s| s.to_string());
                let message = err_obj.get("message").and_then(|v| v.as_str()).unwrap_or("Unknown error").to_string();
                (code, message)
            } else {
                (None, value.get("message").and_then(|v| v.as_str()).unwrap_or("Unknown error").to_string())
            };
            Ok(Some(ClaudeEvent::Error { code, message }))
        }
        _ => Ok(Some(ClaudeEvent::Unknown { raw: value }))
    }
}

pub fn claude_event_to_normalized(
    event: ClaudeEvent,
    provider: ProviderKind,
    conversation_id: &str,
    session_id: Option<&str>,
) -> Option<NormalizedEvent> {
    let mut normalized = NormalizedEvent::new(
        EventType::Error, // Placeholder, updated below
        provider.clone(),
        conversation_id.to_string(),
        EventPayload::Empty,
    );
    normalized.provider_session_id = session_id.map(|s| s.to_string());

    match event {
        ClaudeEvent::Init { session_id: new_sid, model } => {
            normalized.event_type = EventType::SessionStarted;
            normalized.payload = EventPayload::Session {
                session_id: new_sid.clone().or_else(|| session_id.map(|s| s.to_string())),
                model,
            };
            if let Some(sid) = new_sid {
                normalized.provider_session_id = Some(sid);
            }
        }
        ClaudeEvent::ContentBlockDelta { delta_type, text, thinking } => {
            if delta_type == "text_delta" {
                if let Some(t) = text {
                    normalized.event_type = EventType::TextDelta;
                    normalized.payload = EventPayload::Text { content: t };
                } else {
                    return None;
                }
            } else if delta_type == "thinking_delta" {
                if let Some(t) = thinking {
                    normalized.event_type = EventType::ThinkingDelta;
                    normalized.payload = EventPayload::Thinking { content: t };
                } else {
                    return None;
                }
            } else {
                return None;
            }
        }
        ClaudeEvent::AssistantMessage { content } => {
            normalized.event_type = EventType::TextDelta;
            normalized.payload = EventPayload::Text { content };
        }
        ClaudeEvent::ToolUse { tool_id, tool_name, input } => {
            normalized.event_type = EventType::ToolStarted;
            normalized.payload = EventPayload::Tool {
                tool_id,
                tool_name,
                input: Some(input),
                output: None,
                status: None,
            };
        }
        ClaudeEvent::ToolResult { tool_id, output } => {
            normalized.event_type = EventType::ToolResult;
            normalized.payload = EventPayload::Tool {
                tool_id,
                tool_name: String::new(),
                input: None,
                output: Some(output),
                status: None,
            };
        }
        ClaudeEvent::Result { session_id: new_sid, usage, .. } => {
            if let Some(sid) = new_sid {
                normalized.provider_session_id = Some(sid);
            }
            if let Some(u) = usage {
                normalized.event_type = EventType::UsageUpdated;
                normalized.payload = EventPayload::Usage {
                    input_tokens: Some(u.input_tokens),
                    output_tokens: Some(u.output_tokens),
                    cache_read_tokens: u.cache_read_tokens,
                    reasoning_tokens: None,
                    context_tokens: u.cache_creation_tokens, // Approximate
                    context_window: None,
                    confidence: TokenConfidence::Exact,
                };
            } else {
                normalized.event_type = EventType::SessionFinished;
                normalized.payload = EventPayload::Empty;
            }
        }
        ClaudeEvent::Error { code, message } => {
            normalized.event_type = EventType::Error;
            normalized.payload = EventPayload::Error { code, message };
        }
        ClaudeEvent::Unknown { raw } => {
            debug!("Unknown Claude event: {:?}", raw);
            return None;
        }
    }

    Some(normalized)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_legacy_init() {
        let line = r#"{"type": "init", "session_id": "sid-123", "model": "claude-3-5-sonnet-20240620"}"#;
        let event = parse_claude_line(line).unwrap().unwrap();
        match event {
            ClaudeEvent::Init { session_id, model } => {
                assert_eq!(session_id, Some("sid-123".to_string()));
                assert_eq!(model, Some("claude-3-5-sonnet-20240620".to_string()));
            }
            _ => panic!("Expected Init event"),
        }
    }

    #[test]
    fn test_parse_system_init() {
        let line = r#"{"type":"system","subtype":"init","cwd":"/path/to","session_id":"sess-sys-999","model":"claude-opus-5"}"#;
        let event = parse_claude_line(line).unwrap().unwrap();
        match &event {
            ClaudeEvent::Init { session_id, model } => {
                assert_eq!(session_id.as_deref(), Some("sess-sys-999"));
                assert_eq!(model.as_deref(), Some("claude-opus-5"));
            }
            _ => panic!("Expected Init event"),
        }

        let norm = claude_event_to_normalized(event, ProviderKind::Claude, "conv-1", None).unwrap();
        assert_eq!(norm.event_type, EventType::SessionStarted);
        assert_eq!(norm.provider_session_id, Some("sess-sys-999".to_string()));
    }

    #[test]
    fn test_parse_assistant_message() {
        let line = r#"{"type":"assistant","message":{"model":"claude-opus-5","id":"msg_1","type":"message","role":"assistant","content":[{"type":"text","text":"Hello world from Claude!"}]}}"#;
        let event = parse_claude_line(line).unwrap().unwrap();
        match &event {
            ClaudeEvent::AssistantMessage { content } => {
                assert_eq!(content, "Hello world from Claude!");
            }
            _ => panic!("Expected AssistantMessage event"),
        }

        let norm = claude_event_to_normalized(event, ProviderKind::Claude, "conv-1", Some("sess-1")).unwrap();
        assert_eq!(norm.event_type, EventType::TextDelta);
        if let EventPayload::Text { content } = norm.payload {
            assert_eq!(content, "Hello world from Claude!");
        } else {
            panic!("Expected Text payload");
        }
    }

    #[test]
    fn test_parse_text_delta() {
        let line = r#"{"type": "stream_event", "event": {"type": "content_block_delta", "delta": {"type": "text_delta", "text": "Hello"}}}"#;
        let event = parse_claude_line(line).unwrap().unwrap();
        match event {
            ClaudeEvent::ContentBlockDelta { delta_type, text, thinking } => {
                assert_eq!(delta_type, "text_delta");
                assert_eq!(text, Some("Hello".to_string()));
                assert_eq!(thinking, None);
            }
            _ => panic!("Expected ContentBlockDelta"),
        }
    }
}
