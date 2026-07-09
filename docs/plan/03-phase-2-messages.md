# Phase 2 — Message Types and Parser

**Objective**: the complete typed message hierarchy the CLI emits, plus
`parse_message()` converting a raw JSON line into a typed `Message`.

**Upstream sources of truth**:
- `reference/.../src/claude_agent_sdk/types.py` (dataclass shapes)
- `reference/.../src/claude_agent_sdk/_internal/message_parser.py`
  (exact parsing rules, required vs optional fields, error cases)

**Strategy**: define serde structs that mirror the WIRE shape exactly
(nested `"message"` objects and all), then expose clean public types.
Where the wire and public shapes coincide, one struct serves both.
Unknown *fields* are tolerated (no `deny_unknown_fields`) because the
protocol adds fields over time; unknown *message types* and unknown
*content block types* are errors, matching upstream behavior
(⚠️ VERIFY in `message_parser.py` — if upstream silently skips unknown
block types instead of erroring, do what upstream does).

## Wire shapes (sketch — ⚠️ VERIFY each against `message_parser.py`)

Every stdout line is one JSON object with a top-level `"type"`:

```jsonc
// type: "assistant"
{"type":"assistant","message":{"model":"claude-sonnet-5","content":[
    {"type":"text","text":"Hello"},
    {"type":"thinking","thinking":"...","signature":"sig"},
    {"type":"tool_use","id":"toolu_1","name":"Read","input":{"file_path":"/x"}},
    {"type":"tool_result","tool_use_id":"toolu_1","content":"ok","is_error":false}
  ]},"parent_tool_use_id":null,"session_id":"abc"}

// type: "user"
{"type":"user","message":{"role":"user","content":"hi"},"parent_tool_use_id":null,"session_id":"abc"}
// user content may ALSO be a list of content blocks — support both.

// type: "system"
{"type":"system","subtype":"init","session_id":"abc", ...arbitrary fields...}

// type: "result"
{"type":"result","subtype":"success","duration_ms":1200,"duration_api_ms":800,
 "is_error":false,"num_turns":2,"session_id":"abc","total_cost_usd":0.003,
 "usage":{...},"result":"final text"}

// type: "stream_event"  (only when include_partial_messages is on)
{"type":"stream_event","uuid":"u1","session_id":"abc","event":{...raw api event...},
 "parent_tool_use_id":null}
```

## Deliverable A — `src/types/message.rs`

Create `src/types.rs` (module declarations) and `src/types/message.rs`.

```rust
// src/types.rs
//! Public type definitions: messages, options, permissions, hooks, MCP.

pub mod message;
```

```rust
// src/types/message.rs
//! Typed messages emitted by the Claude Code CLI.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{Error, Result};

/// Any message the CLI can emit on stdout.
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
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
}

/// One content block inside a message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    /// Plain text.
    Text {
        /// The text content.
        text: String,
    },
    /// Extended thinking block.
    Thinking {
        /// Thinking content.
        thinking: String,
        /// Integrity signature.
        signature: String,
    },
    /// A tool invocation requested by the model.
    ToolUse {
        /// Unique id correlating with the matching tool result.
        id: String,
        /// Tool name.
        name: String,
        /// Tool input (arbitrary JSON).
        input: Value,
    },
    /// The result of a tool invocation.
    ToolResult {
        /// Id of the tool_use this answers.
        tool_use_id: String,
        /// Result content (string, blocks, or absent). Kept raw.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        content: Option<Value>,
        /// Whether the tool errored.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        is_error: Option<bool>,
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
    /// Total cost in USD, when reported.
    #[serde(default)]
    pub total_cost_usd: Option<f64>,
    /// Raw usage statistics, when reported.
    #[serde(default)]
    pub usage: Option<Value>,
    /// Final result text, when reported.
    #[serde(default)]
    pub result: Option<String>,
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

/// Parses one JSON line from the CLI into a typed [`Message`].
///
/// # Errors
///
/// Returns [`Error::MessageParse`] when `data` lacks a known `type`
/// or a required field, mirroring the upstream parser.
pub fn parse_message(data: Value) -> Result<Message> {
    // Implementation outline (write tests first):
    // 1. let Some(msg_type) = data.get("type").and_then(Value::as_str) else {
    //        return Err(Error::MessageParse { message: "missing 'type'".., data });
    //    };
    // 2. match msg_type {
    //      "user" | "assistant" => deserialize nested wire struct (below),
    //      "system" => read subtype + keep full Value,
    //      "result" => serde_json::from_value::<ResultMessage>,
    //      "stream_event" => serde_json::from_value::<StreamEvent>,
    //      other => Err(Error::MessageParse { message: format!("unknown message type: {other}"), data }),
    //    }
    // Map serde errors into Error::MessageParse with the original data.
    todo!()
}
```

Private wire structs (same file, not `pub`) used inside `parse_message`:

```rust
#[derive(Deserialize)]
struct AssistantWire {
    message: AssistantInner,
    #[serde(default)]
    parent_tool_use_id: Option<String>,
}

#[derive(Deserialize)]
struct AssistantInner {
    model: String,
    content: Vec<ContentBlock>,
}

#[derive(Deserialize)]
struct UserWire {
    message: UserInner,
    #[serde(default)]
    parent_tool_use_id: Option<String>,
}

#[derive(Deserialize)]
struct UserInner {
    content: UserContent,
}
```

Register in `src/lib.rs`:

```rust
mod error;
pub mod types;

pub use error::{Error, Result};
pub use types::message::{
    AssistantMessage, ContentBlock, Message, ResultMessage, StreamEvent,
    SystemMessage, UserContent, UserMessage,
};
```

## Deliverable B — fixtures

Create `tests/fixtures/` with one `.json` file per wire sample listed in
`appendix-a-wire-protocol.md` (files named `assistant_text.json`,
`assistant_tool_use.json`, `user_text.json`, `user_blocks.json`,
`system_init.json`, `result_success.json`, `stream_event.json`).
**Before committing fixtures**, cross-check each against the upstream
test suite (`reference/.../tests/`) and the parser — upstream tests
contain real captured shapes; prefer those verbatim.

## Tests (write FIRST — unit tests in `message.rs`, using `include_str!` on fixtures)

1. `parses_assistant_text_message` — fixture → `Message::Assistant`,
   one `ContentBlock::Text`, model non-empty.
2. `parses_assistant_tool_use_message` — asserts id/name/input of the
   `ToolUse` block.
3. `parses_thinking_block` — thinking + signature fields.
4. `parses_tool_result_block_with_error_flag` — `is_error == Some(true)`.
5. `parses_user_string_content` — `UserContent::Text`.
6. `parses_user_block_content` — `UserContent::Blocks`.
7. `parses_system_message_keeps_raw_data` — subtype extracted, `data`
   retains all original fields.
8. `parses_result_message_full` — every explicit field checked.
9. `parses_result_message_without_optional_fields` — missing
   `total_cost_usd`/`usage`/`result` → `None`, no error.
10. `parses_stream_event` — uuid/session_id/event present.
11. `rejects_unknown_message_type` — `{"type":"bogus"}` →
    `Err(Error::MessageParse { .. })` and the error's `data` echoes input.
12. `rejects_message_without_type` — `{}` → `MessageParse`.
13. `rejects_assistant_missing_content` — malformed nested message →
    `MessageParse` (not a panic).
14. `tolerates_unknown_extra_fields` — assistant fixture with an extra
    `"future_field": 1` still parses.
15. `content_block_roundtrip` — serialize each `ContentBlock` variant
    and deserialize back; equal (`rstest` parametrized over variants).

## Acceptance Gate

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo doc --no-deps
```

## Commits

1. `phase-2: wire fixtures from upstream samples`
2. `phase-2: message type tests (red)`
3. `phase-2: message types + parser (green)`
