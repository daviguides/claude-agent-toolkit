//! Typed messages emitted by the Claude Code CLI.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{Error, Result};

/// Any message the CLI can emit on stdout.
///
/// Upstream models several of these (task lifecycle, hook events,
/// mirror errors) as `SystemMessage` subclasses so `isinstance(x,
/// SystemMessage)` keeps working for old call sites; Rust has no
/// inheritance, so this port gives each its own `Message` variant
/// instead — no information is lost, callers just match the specific
/// variant rather than relying on a subtype check.
#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    /// Echo of user input (also carries tool results in the loop).
    User(UserMessage),
    /// Assistant output: text, thinking, and tool-use blocks.
    Assistant(AssistantMessage),
    /// CLI lifecycle/system information not covered by a more specific
    /// variant below (includes genuinely unrecognized `system` subtypes,
    /// forward-compatibly, with the full raw payload in `data`).
    System(SystemMessage),
    /// A background task started.
    TaskStarted(TaskStartedMessage),
    /// A background task reported progress.
    TaskProgress(TaskProgressMessage),
    /// A background task completed, failed, or was stopped.
    TaskNotification(TaskNotificationMessage),
    /// A background task's lifecycle state changed.
    TaskUpdated(TaskUpdatedMessage),
    /// An SDK-synthesized session-store mirroring failure.
    MirrorError(MirrorErrorMessage),
    /// A hook lifecycle event (only emitted when `include_hook_events`
    /// is enabled).
    HookEvent(HookEventMessage),
    /// Terminal message of a turn, with cost and usage stats.
    Result(ResultMessage),
    /// Raw partial-message stream event (opt-in).
    StreamEvent(StreamEvent),
    /// Rate limit status changed.
    RateLimitEvent(RateLimitEvent),
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

/// Task lifecycle statuses considered terminal (the task has finished).
///
/// `task_notification` reports the mapped `"stopped"` form; `task_updated`
/// reports the raw `"killed"` — both are terminal.
pub const TERMINAL_TASK_STATUSES: &[&str] = &["completed", "failed", "stopped", "killed"];

/// Usage statistics reported in task progress/notification messages.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct TaskUsage {
    /// Total tokens consumed by the task so far.
    pub total_tokens: u64,
    /// Number of tool calls made by the task so far.
    pub tool_uses: u64,
    /// Task duration so far, in milliseconds.
    pub duration_ms: u64,
}

/// Emitted when a background task starts.
#[derive(Debug, Clone, PartialEq)]
pub struct TaskStartedMessage {
    /// Always `"task_started"`.
    pub subtype: String,
    /// Full raw payload.
    pub data: Value,
    /// Task identifier.
    pub task_id: String,
    /// Human-readable task description.
    pub description: String,
    /// Unique message identifier.
    pub uuid: String,
    /// Session identifier.
    pub session_id: String,
    /// Tool use id that spawned this task, if any.
    pub tool_use_id: Option<String>,
    /// Task kind, e.g. `"background"`.
    pub task_type: Option<String>,
}

/// Emitted while a background task is in progress.
#[derive(Debug, Clone, PartialEq)]
pub struct TaskProgressMessage {
    /// Always `"task_progress"`.
    pub subtype: String,
    /// Full raw payload.
    pub data: Value,
    /// Task identifier.
    pub task_id: String,
    /// Human-readable task description.
    pub description: String,
    /// Cumulative usage statistics.
    pub usage: TaskUsage,
    /// Unique message identifier.
    pub uuid: String,
    /// Session identifier.
    pub session_id: String,
    /// Tool use id that spawned this task, if any.
    pub tool_use_id: Option<String>,
    /// Name of the most recently used tool, if any.
    pub last_tool_name: Option<String>,
}

/// Emitted when a background task completes, fails, or is stopped.
///
/// Not every terminal task emits this — some report completion only
/// via a [`TaskUpdatedMessage`] whose `status` is terminal.
#[derive(Debug, Clone, PartialEq)]
pub struct TaskNotificationMessage {
    /// Always `"task_notification"`.
    pub subtype: String,
    /// Full raw payload.
    pub data: Value,
    /// Task identifier.
    pub task_id: String,
    /// Terminal status: `"completed"`, `"failed"`, or `"stopped"`.
    pub status: String,
    /// Path to the task's output file.
    pub output_file: String,
    /// Human-readable summary of the outcome.
    pub summary: String,
    /// Unique message identifier.
    pub uuid: String,
    /// Session identifier.
    pub session_id: String,
    /// Tool use id that spawned this task, if any.
    pub tool_use_id: Option<String>,
    /// Final usage statistics, when reported.
    pub usage: Option<TaskUsage>,
}

/// Emitted when a background task's lifecycle state changes.
///
/// `patch` carries the changed fields; when `patch.status` is one of
/// [`TERMINAL_TASK_STATUSES`] the task has finished. A task stopped via
/// cancellation may report its terminal state ONLY here (no matching
/// [`TaskNotificationMessage`]), so callers tracking active task ids
/// should treat either message's terminal status as clearing the id.
#[derive(Debug, Clone, PartialEq)]
pub struct TaskUpdatedMessage {
    /// Always `"task_updated"`.
    pub subtype: String,
    /// Full raw payload.
    pub data: Value,
    /// Task identifier.
    pub task_id: String,
    /// Changed fields; empty object if the CLI sent no patch or a
    /// non-object patch — parsing a lifecycle event never fails.
    pub patch: Value,
    /// `patch.status`, when present.
    pub status: Option<String>,
    /// Session identifier, when present.
    pub session_id: Option<String>,
    /// Unique message identifier, when present.
    pub uuid: Option<String>,
}

/// Emitted when mirroring a session transcript to an external store
/// fails. SDK-synthesized — never emitted by the CLI subprocess itself.
/// Non-fatal: the local-disk transcript is already durable.
#[derive(Debug, Clone, PartialEq)]
pub struct MirrorErrorMessage {
    /// Always `"mirror_error"`.
    pub subtype: String,
    /// Full raw payload.
    pub data: Value,
    /// The session-store key that failed to mirror, kept as raw JSON
    /// (the session-store subsystem is not otherwise modeled by this
    /// crate).
    pub key: Option<Value>,
    /// Failure description.
    pub error: String,
}

/// A hook lifecycle event, emitted only when `include_hook_events` is
/// enabled on the session options.
#[derive(Debug, Clone, PartialEq)]
pub struct HookEventMessage {
    /// `"hook_started"` when a hook begins executing, `"hook_response"`
    /// when it completes.
    pub subtype: String,
    /// Full raw payload, including event-specific fields (e.g. `output`,
    /// `exit_code`, `outcome` on `"hook_response"`).
    pub data: Value,
    /// Name of the hook event (e.g. `"PreToolUse"`).
    pub hook_event_name: String,
    /// Session identifier, when present.
    pub session_id: Option<String>,
    /// Unique message identifier, when present.
    pub uuid: Option<String>,
}

/// Rate limit status reported by the CLI.
#[derive(Debug, Clone, PartialEq)]
pub struct RateLimitInfo {
    /// Current status: `"allowed"`, `"allowed_warning"`, or `"rejected"`.
    pub status: String,
    /// Unix timestamp when the rate limit window resets.
    pub resets_at: Option<i64>,
    /// Which rate limit window applies (e.g. `"five_hour"`).
    pub rate_limit_type: Option<String>,
    /// Fraction of the rate limit consumed (0.0 - 1.0).
    pub utilization: Option<f64>,
    /// Status of overage/pay-as-you-go usage, if applicable.
    pub overage_status: Option<String>,
    /// Unix timestamp when the overage window resets.
    pub overage_resets_at: Option<i64>,
    /// Why overage is unavailable, when `overage_status` is `"rejected"`.
    pub overage_disabled_reason: Option<String>,
    /// Full raw payload, including fields not modeled above.
    pub raw: Value,
}

/// Emitted when the CLI's rate limit status changes.
#[derive(Debug, Clone, PartialEq)]
pub struct RateLimitEvent {
    /// The new rate limit status.
    pub rate_limit_info: RateLimitInfo,
    /// Unique message identifier.
    pub uuid: String,
    /// Session identifier.
    pub session_id: String,
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
        "rate_limit_event" => parse_rate_limit_event(&data).map(Some),
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

    match subtype {
        "task_started" => parse_task_started(data, subtype),
        "task_progress" => parse_task_progress(data, subtype),
        "task_notification" => parse_task_notification(data, subtype),
        "task_updated" => Ok(parse_task_updated(data, subtype)),
        "mirror_error" => Ok(parse_mirror_error(data, subtype)),
        "hook_started" | "hook_response" => Ok(parse_hook_event(data, subtype)),
        _ => Ok(Message::System(SystemMessage {
            subtype: subtype.to_string(),
            data: data.clone(),
        })),
    }
}

fn parse_task_started(data: &Value, subtype: &str) -> Result<Message> {
    let missing = |field: &str| missing_field(data, "system", field);
    Ok(Message::TaskStarted(TaskStartedMessage {
        subtype: subtype.to_string(),
        data: data.clone(),
        task_id: str_field(data, "task_id").ok_or_else(|| missing("task_id"))?,
        description: str_field(data, "description").ok_or_else(|| missing("description"))?,
        uuid: str_field(data, "uuid").ok_or_else(|| missing("uuid"))?,
        session_id: str_field(data, "session_id").ok_or_else(|| missing("session_id"))?,
        tool_use_id: str_field(data, "tool_use_id"),
        task_type: str_field(data, "task_type"),
    }))
}

fn parse_task_progress(data: &Value, subtype: &str) -> Result<Message> {
    let missing = |field: &str| missing_field(data, "system", field);
    let raw_usage = data.get("usage").ok_or_else(|| missing("usage"))?;
    let usage =
        serde_json::from_value(raw_usage.clone()).map_err(|source| Error::MessageParse {
            message: format!("invalid task usage: {source}"),
            data: data.clone(),
        })?;
    Ok(Message::TaskProgress(TaskProgressMessage {
        subtype: subtype.to_string(),
        data: data.clone(),
        task_id: str_field(data, "task_id").ok_or_else(|| missing("task_id"))?,
        description: str_field(data, "description").ok_or_else(|| missing("description"))?,
        usage,
        uuid: str_field(data, "uuid").ok_or_else(|| missing("uuid"))?,
        session_id: str_field(data, "session_id").ok_or_else(|| missing("session_id"))?,
        tool_use_id: str_field(data, "tool_use_id"),
        last_tool_name: str_field(data, "last_tool_name"),
    }))
}

fn parse_task_notification(data: &Value, subtype: &str) -> Result<Message> {
    let missing = |field: &str| missing_field(data, "system", field);
    let usage = match data.get("usage") {
        Some(raw) => {
            Some(
                serde_json::from_value(raw.clone()).map_err(|source| Error::MessageParse {
                    message: format!("invalid task usage: {source}"),
                    data: data.clone(),
                })?,
            )
        }
        None => None,
    };
    Ok(Message::TaskNotification(TaskNotificationMessage {
        subtype: subtype.to_string(),
        data: data.clone(),
        task_id: str_field(data, "task_id").ok_or_else(|| missing("task_id"))?,
        status: str_field(data, "status").ok_or_else(|| missing("status"))?,
        output_file: str_field(data, "output_file").ok_or_else(|| missing("output_file"))?,
        summary: str_field(data, "summary").ok_or_else(|| missing("summary"))?,
        uuid: str_field(data, "uuid").ok_or_else(|| missing("uuid"))?,
        session_id: str_field(data, "session_id").ok_or_else(|| missing("session_id"))?,
        tool_use_id: str_field(data, "tool_use_id"),
        usage,
    }))
}

fn parse_task_updated(data: &Value, subtype: &str) -> Message {
    let patch = match data.get("patch") {
        Some(Value::Object(_)) => data["patch"].clone(),
        _ => Value::Object(serde_json::Map::new()),
    };
    let status = str_field(&patch, "status");
    Message::TaskUpdated(TaskUpdatedMessage {
        subtype: subtype.to_string(),
        data: data.clone(),
        task_id: str_field(data, "task_id").unwrap_or_default(),
        patch,
        status,
        session_id: str_field(data, "session_id"),
        uuid: str_field(data, "uuid"),
    })
}

fn parse_mirror_error(data: &Value, subtype: &str) -> Message {
    Message::MirrorError(MirrorErrorMessage {
        subtype: subtype.to_string(),
        data: data.clone(),
        key: data.get("key").cloned(),
        error: str_field(data, "error").unwrap_or_default(),
    })
}

fn parse_hook_event(data: &Value, subtype: &str) -> Message {
    let hook_event_name = ["hook_event", "hook_name", "hook_event_name"]
        .into_iter()
        .find_map(|key| str_field(data, key).filter(|value| !value.is_empty()))
        .unwrap_or_default();
    Message::HookEvent(HookEventMessage {
        subtype: subtype.to_string(),
        data: data.clone(),
        hook_event_name,
        session_id: str_field(data, "session_id"),
        uuid: str_field(data, "uuid"),
    })
}

fn parse_rate_limit_event(data: &Value) -> Result<Message> {
    let missing = |field: &str| missing_field(data, "rate_limit_event", field);
    let info = data
        .get("rate_limit_info")
        .ok_or_else(|| missing("rate_limit_info"))?;
    let status = str_field(info, "status").ok_or_else(|| missing("rate_limit_info.status"))?;
    Ok(Message::RateLimitEvent(RateLimitEvent {
        rate_limit_info: RateLimitInfo {
            status,
            resets_at: info.get("resetsAt").and_then(Value::as_i64),
            rate_limit_type: str_field(info, "rateLimitType"),
            utilization: info.get("utilization").and_then(Value::as_f64),
            overage_status: str_field(info, "overageStatus"),
            overage_resets_at: info.get("overageResetsAt").and_then(Value::as_i64),
            overage_disabled_reason: str_field(info, "overageDisabledReason"),
            raw: info.clone(),
        },
        uuid: str_field(data, "uuid").ok_or_else(|| missing("uuid"))?,
        session_id: str_field(data, "session_id").ok_or_else(|| missing("session_id"))?,
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
            "task_started" => include_str!("../../tests/fixtures/task_started.json"),
            "task_progress" => include_str!("../../tests/fixtures/task_progress.json"),
            "task_notification" => {
                include_str!("../../tests/fixtures/task_notification.json")
            }
            "task_updated_terminal" => {
                include_str!("../../tests/fixtures/task_updated_terminal.json")
            }
            "hook_started" => include_str!("../../tests/fixtures/hook_started.json"),
            "rate_limit_event" => include_str!("../../tests/fixtures/rate_limit_event.json"),
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
        let result = parse_message(serde_json::json!({"type": "some_future_message_type"}));
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

    #[test]
    fn parses_task_started_message() {
        let message = parse_message(fixture("task_started"))
            .expect("parses")
            .expect("is Some");
        let Message::TaskStarted(task) = message else {
            panic!("expected Message::TaskStarted");
        };
        assert_eq!(task.task_id, "task-abc");
        assert_eq!(task.description, "Reticulating splines");
        assert_eq!(task.uuid, "uuid-1");
        assert_eq!(task.session_id, "session-1");
        assert_eq!(task.tool_use_id.as_deref(), Some("toolu_01"));
        assert_eq!(task.task_type.as_deref(), Some("background"));
    }

    #[test]
    fn parses_task_started_message_optional_fields_absent() {
        let message = parse_message(serde_json::json!({
            "type": "system",
            "subtype": "task_started",
            "task_id": "task-abc",
            "description": "Working",
            "uuid": "uuid-1",
            "session_id": "session-1"
        }))
        .expect("parses")
        .expect("is Some");
        let Message::TaskStarted(task) = message else {
            panic!("expected Message::TaskStarted");
        };
        assert_eq!(task.tool_use_id, None);
        assert_eq!(task.task_type, None);
    }

    #[test]
    fn parses_task_progress_message() {
        let message = parse_message(fixture("task_progress"))
            .expect("parses")
            .expect("is Some");
        let Message::TaskProgress(task) = message else {
            panic!("expected Message::TaskProgress");
        };
        assert_eq!(task.task_id, "task-abc");
        assert_eq!(task.usage.total_tokens, 1234);
        assert_eq!(task.usage.tool_uses, 5);
        assert_eq!(task.usage.duration_ms, 9876);
        assert_eq!(task.last_tool_name.as_deref(), Some("Read"));
    }

    #[test]
    fn parses_task_notification_message() {
        let message = parse_message(fixture("task_notification"))
            .expect("parses")
            .expect("is Some");
        let Message::TaskNotification(task) = message else {
            panic!("expected Message::TaskNotification");
        };
        assert_eq!(task.status, "completed");
        assert_eq!(task.output_file, "/tmp/out.md");
        assert_eq!(task.summary, "All done");
        assert!(task.usage.is_some());
    }

    #[test]
    fn parses_task_notification_message_optional_fields_absent() {
        let message = parse_message(serde_json::json!({
            "type": "system",
            "subtype": "task_notification",
            "task_id": "task-abc",
            "status": "failed",
            "output_file": "/tmp/out.md",
            "summary": "Boom",
            "uuid": "uuid-3",
            "session_id": "session-1"
        }))
        .expect("parses")
        .expect("is Some");
        let Message::TaskNotification(task) = message else {
            panic!("expected Message::TaskNotification");
        };
        assert_eq!(task.status, "failed");
        assert_eq!(task.usage, None);
        assert_eq!(task.tool_use_id, None);
    }

    #[test]
    fn parses_task_updated_message_terminal() {
        let message = parse_message(fixture("task_updated_terminal"))
            .expect("parses")
            .expect("is Some");
        let Message::TaskUpdated(task) = message else {
            panic!("expected Message::TaskUpdated");
        };
        assert_eq!(task.status.as_deref(), Some("completed"));
        assert!(TERMINAL_TASK_STATUSES.contains(&task.status.as_deref().unwrap()));
    }

    #[test]
    fn parses_task_updated_message_minimal() {
        let message = parse_message(serde_json::json!({
            "type": "system",
            "subtype": "task_updated",
            "task_id": "b1m21w89v",
            "patch": {"status": "completed", "end_time": 1_780_405_729_183i64}
        }))
        .expect("parses")
        .expect("is Some");
        let Message::TaskUpdated(task) = message else {
            panic!("expected Message::TaskUpdated");
        };
        assert_eq!(task.task_id, "b1m21w89v");
        assert_eq!(task.status.as_deref(), Some("completed"));
        assert_eq!(task.uuid, None);
        assert_eq!(task.session_id, None);
    }

    #[rstest]
    #[case("pending")]
    #[case("running")]
    #[case("paused")]
    fn parses_task_updated_message_non_terminal_statuses(#[case] status: &str) {
        let message = parse_message(serde_json::json!({
            "type": "system",
            "subtype": "task_updated",
            "task_id": "task-abc",
            "patch": {"status": status}
        }))
        .expect("parses")
        .expect("is Some");
        let Message::TaskUpdated(task) = message else {
            panic!("expected Message::TaskUpdated");
        };
        assert_eq!(task.status.as_deref(), Some(status));
        assert!(!TERMINAL_TASK_STATUSES.contains(&status));
    }

    #[test]
    fn parses_task_updated_message_no_patch() {
        let message = parse_message(serde_json::json!({
            "type": "system",
            "subtype": "task_updated",
            "task_id": "task-abc"
        }))
        .expect("parses")
        .expect("is Some");
        let Message::TaskUpdated(task) = message else {
            panic!("expected Message::TaskUpdated");
        };
        assert_eq!(task.patch, serde_json::json!({}));
        assert_eq!(task.status, None);
    }

    #[rstest]
    #[case(serde_json::json!("completed"))]
    #[case(serde_json::json!(["completed"]))]
    #[case(serde_json::json!(42))]
    #[case(serde_json::Value::Null)]
    fn parses_task_updated_message_non_dict_patch(#[case] patch: Value) {
        let message = parse_message(serde_json::json!({
            "type": "system",
            "subtype": "task_updated",
            "task_id": "task-abc",
            "patch": patch
        }))
        .expect("parses")
        .expect("is Some");
        let Message::TaskUpdated(task) = message else {
            panic!("expected Message::TaskUpdated");
        };
        assert_eq!(task.patch, serde_json::json!({}));
        assert_eq!(task.status, None);
    }

    #[test]
    fn parses_task_updated_killed_is_terminal() {
        let message = parse_message(serde_json::json!({
            "type": "system",
            "subtype": "task_updated",
            "task_id": "bs2r8eew4",
            "patch": {"status": "killed", "end_time": 1_780_405_729_183i64}
        }))
        .expect("parses")
        .expect("is Some");
        let Message::TaskUpdated(task) = message else {
            panic!("expected Message::TaskUpdated");
        };
        assert_eq!(task.status.as_deref(), Some("killed"));
        assert!(TERMINAL_TASK_STATUSES.contains(&"killed"));
    }

    #[test]
    fn unknown_system_subtype_yields_generic_system_message() {
        let message = parse_message(serde_json::json!({
            "type": "system",
            "subtype": "some_future_subtype",
            "foo": "bar"
        }))
        .expect("parses")
        .expect("is Some");
        let Message::System(system) = message else {
            panic!("expected Message::System, not a typed subclass");
        };
        assert_eq!(system.subtype, "some_future_subtype");
        assert_eq!(system.data["foo"], "bar");
    }

    #[test]
    fn parses_mirror_error_message() {
        let message = parse_message(serde_json::json!({
            "type": "system",
            "subtype": "mirror_error",
            "key": {"project_key": "p1", "session_id": "s1"},
            "error": "connection refused"
        }))
        .expect("parses")
        .expect("is Some");
        let Message::MirrorError(mirror) = message else {
            panic!("expected Message::MirrorError");
        };
        assert_eq!(mirror.error, "connection refused");
        assert!(mirror.key.is_some());
    }

    #[test]
    fn parses_hook_event_message_started() {
        let message = parse_message(fixture("hook_started"))
            .expect("parses")
            .expect("is Some");
        let Message::HookEvent(hook) = message else {
            panic!("expected Message::HookEvent");
        };
        assert_eq!(hook.subtype, "hook_started");
        assert_eq!(hook.hook_event_name, "PreToolUse");
        assert_eq!(hook.session_id.as_deref(), Some("sess-123"));
        assert_eq!(hook.uuid.as_deref(), Some("uuid-456"));
    }

    #[test]
    fn parses_hook_event_message_response() {
        let message = parse_message(serde_json::json!({
            "type": "system",
            "subtype": "hook_response",
            "hook_event": "PostToolUse",
            "session_id": "sess-123",
            "uuid": "uuid-789",
            "output": "",
            "exit_code": 0,
            "outcome": "success"
        }))
        .expect("parses")
        .expect("is Some");
        let Message::HookEvent(hook) = message else {
            panic!("expected Message::HookEvent");
        };
        assert_eq!(hook.subtype, "hook_response");
        assert_eq!(hook.hook_event_name, "PostToolUse");
        assert_eq!(hook.data["outcome"], "success");
    }

    #[test]
    fn parses_hook_event_message_minimal() {
        let message = parse_message(serde_json::json!({
            "type": "system",
            "subtype": "hook_started",
            "hook_name": "Stop"
        }))
        .expect("parses")
        .expect("is Some");
        let Message::HookEvent(hook) = message else {
            panic!("expected Message::HookEvent");
        };
        assert_eq!(hook.hook_event_name, "Stop");
        assert_eq!(hook.session_id, None);
        assert_eq!(hook.uuid, None);
    }

    #[test]
    fn parses_rate_limit_event() {
        let message = parse_message(fixture("rate_limit_event"))
            .expect("parses")
            .expect("is Some");
        let Message::RateLimitEvent(event) = message else {
            panic!("expected Message::RateLimitEvent");
        };
        assert_eq!(event.uuid, "abc-123");
        assert_eq!(event.session_id, "session_xyz");
        assert_eq!(event.rate_limit_info.status, "allowed_warning");
        assert_eq!(event.rate_limit_info.resets_at, Some(1_700_000_000));
        assert_eq!(
            event.rate_limit_info.rate_limit_type.as_deref(),
            Some("five_hour")
        );
        assert_eq!(event.rate_limit_info.utilization, Some(0.91));
    }
}
