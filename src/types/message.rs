//! Typed messages emitted by the Claude Code CLI.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{Error, Result};

/// Any message the CLI can emit on stdout.
///
/// See `docs/plan/DEVIATIONS.md` for the set of upstream message kinds
/// (task lifecycle, hook events, rate-limit events) intentionally not
/// modeled as typed variants yet; they fall through to [`SystemMessage`]
/// or are skipped, never lost or misparsed.
#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    /// Echo of user input (also carries tool results in the loop).
    User(UserMessage),
    /// Assistant output: text, thinking, and tool-use blocks.
    Assistant(AssistantMessage),
    /// CLI lifecycle/system information.
    System(SystemMessage),
    /// Terminal message of a turn, with cost and usage stats.
    Result(ResultMessage),
    /// Raw partial-message stream event (opt-in).
    StreamEvent(StreamEvent),
}

/// Content of a user message: plain text or structured blocks.
#[derive(Debug, Clone, PartialEq)]
pub enum UserContent {
    /// Plain string content.
    Text(String),
    /// Structured content blocks.
    Blocks(Vec<ContentBlock>),
}

/// A user-role message.
#[derive(Debug, Clone, PartialEq)]
pub struct UserMessage {
    /// Message content (string or blocks).
    pub content: UserContent,
    /// Set when this message was produced inside a subagent tool call.
    pub parent_tool_use_id: Option<String>,
    /// Unique message identifier, used e.g. for file checkpointing.
    pub uuid: Option<String>,
    /// Metadata about a tool execution result (file edit details, etc.),
    /// kept as the raw CLI payload.
    pub tool_use_result: Option<Value>,
}

/// An assistant-role message.
#[derive(Debug, Clone, PartialEq)]
pub struct AssistantMessage {
    /// Ordered content blocks.
    pub content: Vec<ContentBlock>,
    /// Model that produced this message.
    pub model: String,
    /// Set when produced inside a subagent tool call.
    pub parent_tool_use_id: Option<String>,
    /// API-level error kind, when the turn failed (e.g.
    /// `"authentication_failed"`, `"rate_limit"`). Kept as a raw string
    /// to stay forward-compatible with new error kinds the CLI emits.
    pub error: Option<String>,
    /// Raw per-turn usage statistics, when reported.
    pub usage: Option<Value>,
    /// Anthropic API message id.
    pub message_id: Option<String>,
    /// Anthropic API stop reason (e.g. `"end_turn"`, `"tool_use"`).
    pub stop_reason: Option<String>,
    /// Session identifier.
    pub session_id: Option<String>,
    /// Unique message identifier.
    pub uuid: Option<String>,
}

/// One content block inside a message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ContentBlock {
    /// Plain text.
    #[serde(rename = "text")]
    Text {
        /// The text content.
        text: String,
    },
    /// Extended thinking block.
    #[serde(rename = "thinking")]
    Thinking {
        /// Thinking content.
        thinking: String,
        /// Integrity signature.
        signature: String,
    },
    /// A tool invocation requested by the model.
    #[serde(rename = "tool_use")]
    ToolUse {
        /// Unique id correlating with the matching tool result.
        id: String,
        /// Tool name.
        name: String,
        /// Tool input (arbitrary JSON).
        input: Value,
    },
    /// The result of a tool invocation.
    #[serde(rename = "tool_result")]
    ToolResult {
        /// Id of the `tool_use` this answers.
        tool_use_id: String,
        /// Result content (string, blocks, or absent). Kept raw.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        content: Option<Value>,
        /// Whether the tool errored.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        is_error: Option<bool>,
    },
    /// A server-executed tool invocation (e.g. advisor, `web_search`).
    #[serde(rename = "server_tool_use")]
    ServerToolUse {
        /// Unique id correlating with the matching server tool result.
        id: String,
        /// Server tool name.
        name: String,
        /// Tool input (arbitrary JSON).
        input: Value,
    },
    /// The result of a server-executed tool invocation. Wire tag is
    /// `"advisor_tool_result"` (asymmetric with `ServerToolUse`'s tag).
    #[serde(rename = "advisor_tool_result")]
    ServerToolResult {
        /// Id of the `ServerToolUse` this answers.
        tool_use_id: String,
        /// Raw, tool-specific result payload.
        content: Value,
    },
}

/// A system message (init banners, notices, etc.).
#[derive(Debug, Clone, PartialEq)]
pub struct SystemMessage {
    /// Discriminator, e.g. `"init"`.
    pub subtype: String,
    /// Full raw payload for fields not modeled explicitly.
    pub data: Value,
}

/// A tool use deferred by a `PreToolUse` hook returning `"defer"`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct DeferredToolUse {
    /// Tool use id.
    pub id: String,
    /// Tool name.
    pub name: String,
    /// Tool input (arbitrary JSON).
    pub input: Value,
}

/// Terminal message of a query turn.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ResultMessage {
    /// Result discriminator, e.g. `"success"`.
    pub subtype: String,
    /// Total wall-clock duration in milliseconds.
    pub duration_ms: u64,
    /// API-time duration in milliseconds.
    pub duration_api_ms: u64,
    /// Whether the turn ended in error.
    pub is_error: bool,
    /// Number of turns consumed.
    pub num_turns: u32,
    /// Session identifier (usable with resume).
    pub session_id: String,
    /// Anthropic API stop reason on the final assistant turn.
    #[serde(default)]
    pub stop_reason: Option<String>,
    /// Total cost in USD, when reported.
    #[serde(default)]
    pub total_cost_usd: Option<f64>,
    /// Raw usage statistics, when reported.
    #[serde(default)]
    pub usage: Option<Value>,
    /// Final result text, when reported.
    #[serde(default)]
    pub result: Option<String>,
    /// Structured output payload, when the session used one.
    #[serde(default)]
    pub structured_output: Option<Value>,
    /// Per-model usage/cost breakdown. Wire key is `modelUsage`.
    #[serde(default, rename = "modelUsage")]
    pub model_usage: Option<Value>,
    /// Permission denials recorded during the turn.
    #[serde(default)]
    pub permission_denials: Option<Vec<Value>>,
    /// A tool use deferred by a `PreToolUse` hook, when present.
    #[serde(default)]
    pub deferred_tool_use: Option<DeferredToolUse>,
    /// Error messages, present on error result subtypes.
    #[serde(default)]
    pub errors: Option<Vec<String>>,
    /// HTTP status code of the failing API call, when `is_error` is
    /// true and the failure originated at the API layer.
    #[serde(default)]
    pub api_error_status: Option<i64>,
    /// Unique message identifier.
    #[serde(default)]
    pub uuid: Option<String>,
}

/// Raw partial-message event (mirrors the Anthropic API stream event).
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct StreamEvent {
    /// Event uuid.
    pub uuid: String,
    /// Session identifier.
    pub session_id: String,
    /// The raw API event payload.
    pub event: Value,
    /// Set when produced inside a subagent tool call.
    #[serde(default)]
    pub parent_tool_use_id: Option<String>,
}

/// Content block `"type"` tags this parser recognizes. Any other tag is
/// skipped rather than treated as an error (forward compatibility).
const KNOWN_BLOCK_TYPES: &[&str] = &[
    "text",
    "thinking",
    "tool_use",
    "tool_result",
    "server_tool_use",
    "advisor_tool_result",
];

/// Parses one JSON line from the CLI into a typed [`Message`].
///
/// Returns `Ok(None)` for a `"type"` this parser does not recognize —
/// matching upstream's forward-compatible skip behavior, never an
/// error.
///
/// # Errors
///
/// Returns [`Error::MessageParse`] when `data` is not a JSON object, or
/// a recognized message type is missing a required field.
pub fn parse_message(data: Value) -> Result<Option<Message>> {
    if !data.is_object() {
        return Err(Error::MessageParse {
            message: format!(
                "invalid message data type (expected object, got {})",
                value_kind(&data)
            ),
            data,
        });
    }

    let Some(msg_type) = data.get("type").and_then(Value::as_str) else {
        return Err(Error::MessageParse {
            message: "message missing 'type' field".to_string(),
            data,
        });
    };

    match msg_type {
        "user" => parse_user_message(&data).map(Some),
        "assistant" => parse_assistant_message(&data).map(Some),
        "system" => parse_system_message(&data).map(Some),
        "result" => parse_result_message(&data).map(Some),
        "stream_event" => parse_stream_event(&data).map(Some),
        _ => Ok(None),
    }
}

fn parse_user_message(data: &Value) -> Result<Message> {
    let message = data
        .get("message")
        .ok_or_else(|| missing_field(data, "user", "message"))?;
    let raw_content = message
        .get("content")
        .ok_or_else(|| missing_field(data, "user", "content"))?;

    let content = match raw_content {
        Value::String(text) => UserContent::Text(text.clone()),
        Value::Array(blocks) => UserContent::Blocks(parse_content_blocks(data, blocks)?),
        other => {
            return Err(Error::MessageParse {
                message: format!(
                    "invalid user content (expected string or array, got {})",
                    value_kind(other)
                ),
                data: data.clone(),
            });
        }
    };

    Ok(Message::User(UserMessage {
        content,
        parent_tool_use_id: str_field(data, "parent_tool_use_id"),
        uuid: str_field(data, "uuid"),
        tool_use_result: data.get("tool_use_result").cloned(),
    }))
}

fn parse_assistant_message(data: &Value) -> Result<Message> {
    let message = data
        .get("message")
        .ok_or_else(|| missing_field(data, "assistant", "message"))?;
    let raw_content = message
        .get("content")
        .ok_or_else(|| missing_field(data, "assistant", "content"))?;
    let Value::Array(blocks) = raw_content else {
        return Err(Error::MessageParse {
            message: format!(
                "invalid assistant content (expected array, got {})",
                value_kind(raw_content)
            ),
            data: data.clone(),
        });
    };
    let content = parse_content_blocks(data, blocks)?;
    let model = message
        .get("model")
        .and_then(Value::as_str)
        .ok_or_else(|| missing_field(data, "assistant", "model"))?
        .to_string();

    Ok(Message::Assistant(AssistantMessage {
        content,
        model,
        parent_tool_use_id: str_field(data, "parent_tool_use_id"),
        error: str_field(data, "error"),
        usage: message.get("usage").cloned(),
        message_id: str_field(message, "id"),
        stop_reason: str_field(message, "stop_reason"),
        session_id: str_field(data, "session_id"),
        uuid: str_field(data, "uuid"),
    }))
}

fn parse_system_message(data: &Value) -> Result<Message> {
    let subtype = data
        .get("subtype")
        .and_then(Value::as_str)
        .ok_or_else(|| missing_field(data, "system", "subtype"))?;
    Ok(Message::System(SystemMessage {
        subtype: subtype.to_string(),
        data: data.clone(),
    }))
}

fn parse_result_message(data: &Value) -> Result<Message> {
    serde_json::from_value::<ResultMessage>(data.clone())
        .map(Message::Result)
        .map_err(|source| Error::MessageParse {
            message: format!("missing required field in result message: {source}"),
            data: data.clone(),
        })
}

fn parse_stream_event(data: &Value) -> Result<Message> {
    serde_json::from_value::<StreamEvent>(data.clone())
        .map(Message::StreamEvent)
        .map_err(|source| Error::MessageParse {
            message: format!("missing required field in stream_event message: {source}"),
            data: data.clone(),
        })
}

fn parse_content_blocks(outer: &Value, raw_blocks: &[Value]) -> Result<Vec<ContentBlock>> {
    let mut blocks = Vec::with_capacity(raw_blocks.len());
    for block in raw_blocks {
        if !block.is_object() {
            return Err(Error::MessageParse {
                message: format!(
                    "invalid content block (expected object, got {})",
                    value_kind(block)
                ),
                data: outer.clone(),
            });
        }
        if let Some(parsed) = parse_content_block(outer, block)? {
            blocks.push(parsed);
        }
    }
    Ok(blocks)
}

fn parse_content_block(outer: &Value, block: &Value) -> Result<Option<ContentBlock>> {
    let Some(tag) = block.get("type").and_then(Value::as_str) else {
        return Err(Error::MessageParse {
            message: "content block missing 'type'".to_string(),
            data: outer.clone(),
        });
    };
    if !KNOWN_BLOCK_TYPES.contains(&tag) {
        return Ok(None);
    }
    serde_json::from_value(block.clone())
        .map(Some)
        .map_err(|source| Error::MessageParse {
            message: format!("invalid '{tag}' content block: {source}"),
            data: outer.clone(),
        })
}

fn missing_field(data: &Value, message_kind: &str, field: &str) -> Error {
    Error::MessageParse {
        message: format!("missing required field in {message_kind} message: '{field}'"),
        data: data.clone(),
    }
}

fn str_field(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(str::to_string)
}

fn value_kind(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use serde_json::Value;

    use super::*;

    fn fixture(name: &str) -> Value {
        let raw = match name {
            "assistant_text" => include_str!("../../tests/fixtures/assistant_text.json"),
            "assistant_tool_use" => {
                include_str!("../../tests/fixtures/assistant_tool_use.json")
            }
            "assistant_thinking" => {
                include_str!("../../tests/fixtures/assistant_thinking.json")
            }
            "assistant_server_tool_use" => {
                include_str!("../../tests/fixtures/assistant_server_tool_use.json")
            }
            "assistant_server_tool_result" => {
                include_str!("../../tests/fixtures/assistant_server_tool_result.json")
            }
            "assistant_full_fields" => {
                include_str!("../../tests/fixtures/assistant_full_fields.json")
            }
            "user_text" => include_str!("../../tests/fixtures/user_text.json"),
            "user_blocks" => include_str!("../../tests/fixtures/user_blocks.json"),
            "user_with_uuid" => include_str!("../../tests/fixtures/user_with_uuid.json"),
            "system_init" => include_str!("../../tests/fixtures/system_init.json"),
            "result_success" => include_str!("../../tests/fixtures/result_success.json"),
            "result_minimal" => include_str!("../../tests/fixtures/result_minimal.json"),
            "result_full" => include_str!("../../tests/fixtures/result_full.json"),
            "stream_event" => include_str!("../../tests/fixtures/stream_event.json"),
            other => panic!("unknown fixture: {other}"),
        };
        serde_json::from_str(raw).expect("fixture is valid JSON")
    }

    #[test]
    fn parses_assistant_text_message() {
        let message = parse_message(fixture("assistant_text"))
            .expect("parses")
            .expect("is Some");
        let Message::Assistant(assistant) = message else {
            panic!("expected Message::Assistant");
        };
        assert!(!assistant.model.is_empty());
        assert_eq!(assistant.content.len(), 1);
        assert!(matches!(&assistant.content[0], ContentBlock::Text { text } if !text.is_empty()));
    }

    #[test]
    fn parses_assistant_tool_use_message() {
        let message = parse_message(fixture("assistant_tool_use"))
            .expect("parses")
            .expect("is Some");
        let Message::Assistant(assistant) = message else {
            panic!("expected Message::Assistant");
        };
        let ContentBlock::ToolUse { id, name, input } = &assistant.content[0] else {
            panic!("expected ContentBlock::ToolUse");
        };
        assert_eq!(id, "toolu_01");
        assert_eq!(name, "Read");
        assert_eq!(input["file_path"], "/tmp/x.txt");
    }

    #[test]
    fn parses_thinking_block() {
        let message = parse_message(fixture("assistant_thinking"))
            .expect("parses")
            .expect("is Some");
        let Message::Assistant(assistant) = message else {
            panic!("expected Message::Assistant");
        };
        let ContentBlock::Thinking {
            thinking,
            signature,
        } = &assistant.content[0]
        else {
            panic!("expected ContentBlock::Thinking");
        };
        assert_eq!(thinking, "Let me consider...");
        assert_eq!(signature, "EqQBCg==");
    }

    #[test]
    fn parses_assistant_server_tool_use_block() {
        let message = parse_message(fixture("assistant_server_tool_use"))
            .expect("parses")
            .expect("is Some");
        let Message::Assistant(assistant) = message else {
            panic!("expected Message::Assistant");
        };
        let ContentBlock::ServerToolUse { id, name, .. } = &assistant.content[0] else {
            panic!("expected ContentBlock::ServerToolUse");
        };
        assert_eq!(id, "srvtoolu_01ABC");
        assert_eq!(name, "advisor");
    }

    #[test]
    fn parses_assistant_server_tool_result_block() {
        let message = parse_message(fixture("assistant_server_tool_result"))
            .expect("parses")
            .expect("is Some");
        let Message::Assistant(assistant) = message else {
            panic!("expected Message::Assistant");
        };
        let ContentBlock::ServerToolResult {
            tool_use_id,
            content,
        } = &assistant.content[0]
        else {
            panic!("expected ContentBlock::ServerToolResult");
        };
        assert_eq!(tool_use_id, "srvtoolu_01ABC");
        assert_eq!(content["type"], "advisor_result");
    }

    #[test]
    fn parses_assistant_message_with_all_fields() {
        let message = parse_message(fixture("assistant_full_fields"))
            .expect("parses")
            .expect("is Some");
        let Message::Assistant(assistant) = message else {
            panic!("expected Message::Assistant");
        };
        assert_eq!(
            assistant.message_id.as_deref(),
            Some("msg_01HRq7YZE3apPqSHydvG77Ve")
        );
        assert_eq!(assistant.stop_reason.as_deref(), Some("end_turn"));
        assert_eq!(
            assistant.session_id.as_deref(),
            Some("fdf2d90a-fd9e-4736-ae35-806edd13643f")
        );
        assert_eq!(
            assistant.uuid.as_deref(),
            Some("0dbd2453-1209-4fe9-bd51-4102f64e33df")
        );
        assert!(assistant.usage.is_some());
        assert!(assistant.error.is_none());
    }

    #[test]
    fn parses_tool_result_block_with_error_flag() {
        let message = parse_message(fixture("user_blocks"))
            .expect("parses")
            .expect("is Some");
        let Message::User(user) = message else {
            panic!("expected Message::User");
        };
        let UserContent::Blocks(blocks) = user.content else {
            panic!("expected UserContent::Blocks");
        };
        let ContentBlock::ToolResult {
            tool_use_id,
            is_error,
            ..
        } = &blocks[0]
        else {
            panic!("expected ContentBlock::ToolResult");
        };
        assert_eq!(tool_use_id, "toolu_01");
        assert_eq!(*is_error, Some(false));
    }

    #[test]
    fn parses_user_string_content() {
        let message = parse_message(fixture("user_text"))
            .expect("parses")
            .expect("is Some");
        let Message::User(user) = message else {
            panic!("expected Message::User");
        };
        assert!(matches!(user.content, UserContent::Text(text) if text == "What is 2 + 2?"));
    }

    #[test]
    fn parses_user_block_content() {
        let message = parse_message(fixture("user_blocks"))
            .expect("parses")
            .expect("is Some");
        let Message::User(user) = message else {
            panic!("expected Message::User");
        };
        assert!(matches!(user.content, UserContent::Blocks(_)));
    }

    #[test]
    fn parses_user_message_with_uuid() {
        let message = parse_message(fixture("user_with_uuid"))
            .expect("parses")
            .expect("is Some");
        let Message::User(user) = message else {
            panic!("expected Message::User");
        };
        assert_eq!(user.uuid.as_deref(), Some("msg-abc123-def456"));
    }

    #[test]
    fn parses_system_message_keeps_raw_data() {
        let message = parse_message(fixture("system_init"))
            .expect("parses")
            .expect("is Some");
        let Message::System(system) = message else {
            panic!("expected Message::System");
        };
        assert_eq!(system.subtype, "init");
        assert_eq!(system.data["cwd"], "/home/user/project");
        assert_eq!(system.data["tools"][0], "Read");
    }

    #[test]
    fn parses_result_message_full() {
        let message = parse_message(fixture("result_success"))
            .expect("parses")
            .expect("is Some");
        let Message::Result(result) = message else {
            panic!("expected Message::Result");
        };
        assert_eq!(result.subtype, "success");
        assert_eq!(result.duration_ms, 2400);
        assert_eq!(result.duration_api_ms, 1800);
        assert!(!result.is_error);
        assert_eq!(result.num_turns, 1);
        assert_eq!(result.session_id, "sess_123");
        assert_eq!(result.total_cost_usd, Some(0.0031));
        assert!(result.usage.is_some());
        assert_eq!(result.result.as_deref(), Some("2 + 2 = 4."));
    }

    #[test]
    fn parses_result_message_without_optional_fields() {
        let message = parse_message(fixture("result_minimal"))
            .expect("parses")
            .expect("is Some");
        let Message::Result(result) = message else {
            panic!("expected Message::Result");
        };
        assert_eq!(result.total_cost_usd, None);
        assert_eq!(result.usage, None);
        assert_eq!(result.result, None);
        assert_eq!(result.model_usage, None);
        assert_eq!(result.deferred_tool_use, None);
    }

    #[test]
    fn parses_result_message_with_extended_fields() {
        let message = parse_message(fixture("result_full"))
            .expect("parses")
            .expect("is Some");
        let Message::Result(result) = message else {
            panic!("expected Message::Result");
        };
        let model_usage = result.model_usage.expect("model_usage present");
        assert_eq!(model_usage["claude-sonnet-4-5-20250929"]["costUSD"], 0.0106);
        assert_eq!(result.permission_denials, Some(Vec::new()));
        assert_eq!(
            result.uuid.as_deref(),
            Some("d379c496-f33a-4ea4-b920-3c5483baa6f7")
        );
    }

    #[test]
    fn parses_stream_event() {
        let message = parse_message(fixture("stream_event"))
            .expect("parses")
            .expect("is Some");
        let Message::StreamEvent(event) = message else {
            panic!("expected Message::StreamEvent");
        };
        assert_eq!(event.uuid, "evt_1");
        assert_eq!(event.session_id, "sess_123");
        assert_eq!(event.event["type"], "content_block_delta");
    }

    #[test]
    fn skips_unknown_message_type() {
        let result = parse_message(serde_json::json!({"type": "rate_limit_event"}));
        assert_eq!(result.expect("does not error"), None);
    }

    #[test]
    fn rejects_message_without_type() {
        let err = parse_message(serde_json::json!({})).expect_err("must error");
        assert!(matches!(err, Error::MessageParse { .. }));
    }

    #[test]
    fn rejects_non_object_data() {
        let err = parse_message(serde_json::json!("not an object")).expect_err("must error");
        assert!(matches!(err, Error::MessageParse { .. }));
    }

    #[test]
    fn rejects_assistant_missing_content() {
        let err = parse_message(serde_json::json!({"type": "assistant"})).expect_err("must error");
        assert!(matches!(err, Error::MessageParse { .. }));
    }

    #[test]
    fn rejects_user_missing_message() {
        let err = parse_message(serde_json::json!({"type": "user"})).expect_err("must error");
        assert!(matches!(err, Error::MessageParse { .. }));
    }

    #[test]
    fn rejects_assistant_string_content() {
        let err = parse_message(serde_json::json!({
            "type": "assistant",
            "message": {"model": "m", "content": "hi"}
        }))
        .expect_err("must error");
        assert!(matches!(err, Error::MessageParse { .. }));
    }

    #[test]
    fn rejects_non_object_content_block() {
        let err = parse_message(serde_json::json!({
            "type": "assistant",
            "message": {"model": "m", "content": ["oops"]}
        }))
        .expect_err("must error");
        assert!(matches!(err, Error::MessageParse { .. }));
    }

    #[test]
    fn skips_unknown_content_block_type() {
        let message = parse_message(serde_json::json!({
            "type": "assistant",
            "message": {
                "model": "m",
                "content": [
                    {"type": "text", "text": "kept"},
                    {"type": "future_block", "whatever": 1}
                ]
            }
        }))
        .expect("parses")
        .expect("is Some");
        let Message::Assistant(assistant) = message else {
            panic!("expected Message::Assistant");
        };
        assert_eq!(assistant.content.len(), 1);
    }

    #[test]
    fn rejects_known_block_type_missing_required_field() {
        let err = parse_message(serde_json::json!({
            "type": "assistant",
            "message": {
                "model": "m",
                "content": [{"type": "text"}]
            }
        }))
        .expect_err("must error");
        assert!(matches!(err, Error::MessageParse { .. }));
    }

    #[test]
    fn tolerates_unknown_extra_fields() {
        let mut raw = fixture("assistant_text");
        raw.as_object_mut()
            .expect("object")
            .insert("future_field".to_string(), serde_json::json!(1));
        let message = parse_message(raw).expect("parses").expect("is Some");
        assert!(matches!(message, Message::Assistant(_)));
    }

    #[rstest]
    #[case(ContentBlock::Text { text: "hi".to_string() })]
    #[case(ContentBlock::Thinking { thinking: "t".to_string(), signature: "s".to_string() })]
    #[case(ContentBlock::ToolUse { id: "1".to_string(), name: "Read".to_string(), input: serde_json::json!({}) })]
    #[case(ContentBlock::ToolResult { tool_use_id: "1".to_string(), content: Some(serde_json::json!("ok")), is_error: Some(false) })]
    #[case(ContentBlock::ServerToolUse { id: "1".to_string(), name: "advisor".to_string(), input: serde_json::json!({}) })]
    #[case(ContentBlock::ServerToolResult { tool_use_id: "1".to_string(), content: serde_json::json!({}) })]
    fn content_block_roundtrip(#[case] block: ContentBlock) {
        let json = serde_json::to_value(&block).expect("serializes");
        let parsed: ContentBlock = serde_json::from_value(json).expect("deserializes");
        assert_eq!(parsed, block);
    }
}
