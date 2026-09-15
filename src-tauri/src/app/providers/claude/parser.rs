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
        "init" => {
            let session_id = value.get("session_id").and_then(|v| v.as_str()).map(|s| s.to_string());
            let model = value.get("model").and_then(|v| v.as_str()).map(|s| s.to_string());
            Ok(Some(ClaudeEvent::Init { session_id, model }))
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
            let usage = if let Some(u) = value.get("usage") {
                serde_json::from_value(u.clone()).ok()
            } else {
                None
            };
            Ok(Some(ClaudeEvent::Result { session_id, usage }))
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
        ClaudeEvent::Result { session_id: new_sid, usage } => {
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
    fn test_parse_init() {
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
