# Phase 7 — Public `ClaudeClient` (Interactive, Multi-Turn)

**Objective**: the stateful counterpart of `query()`: connect once,
send many prompts, stream responses, interrupt, change permission
mode/model mid-session.

**Upstream source of truth**:
`reference/.../src/claude_agent_sdk/client.py` (`ClaudeSDKClient`) —
mirror its lifecycle and method set, translated to Rust naming.

## Naming decision (fixed)

Public type is `ClaudeClient` (not `ClaudeSDKClient` — "SDK" in a type
name inside the SDK is noise; the parity audit maps the two names).

## Deliverable — `src/client.rs`

```rust
//! Interactive multi-turn client.

use futures::Stream;

use crate::error::{Error, Result};
use crate::types::message::{Message, UserContent};
use crate::types::options::ClaudeAgentOptions;
use crate::types::permission::PermissionMode;

/// Default session id used when the caller does not name sessions.
const DEFAULT_SESSION_ID: &str = "default";

/// A stateful, bidirectional session with Claude Code.
///
/// Lifecycle: [`ClaudeClient::connect`] → [`send`](Self::send) /
/// [`receive_response`](Self::receive_response) (repeat) →
/// [`disconnect`](Self::disconnect).
pub struct ClaudeClient {
    query: Query,          // the Phase 5 actor
    connected: bool,
}

impl ClaudeClient {
    /// Spawns the CLI in streaming mode and performs the
    /// `initialize` control handshake.
    ///
    /// # Errors
    ///
    /// [`Error::CliNotFound`], [`Error::CliConnection`], or
    /// [`Error::ControlProtocol`] when the handshake fails.
    pub async fn connect(options: ClaudeAgentOptions) -> Result<Self> {
        // 1. SubprocessTransport with PromptInput::Streaming
        // 2. transport.connect().await?
        // 3. Query::start(transport, handlers built from options
        //    (hooks/can_use_tool arrive in Phase 8 — pass empty now))
        // 4. query.initialize(None).await?   (hooks payload in Phase 8)
        todo!()
    }

    /// Sends a user prompt into the session.
    ///
    /// # Errors
    ///
    /// [`Error::CliConnection`] when the session is closed.
    pub async fn send(&self, prompt: impl Into<String>) -> Result<()> {
        // guard: connected; then query.send_user_message(
        //   &UserContent::Text(prompt.into()), DEFAULT_SESSION_ID)
        todo!()
    }

    /// Sends structured content (text or blocks) into the session.
    ///
    /// Covers upstream `client.query()` with dict/block payloads.
    ///
    /// # Errors
    ///
    /// [`Error::CliConnection`] when the session is closed.
    pub async fn send_content(&self, content: UserContent) -> Result<()> { todo!() }

    /// Feeds a whole async stream of messages into the session.
    ///
    /// Covers upstream `client.query()` with an `AsyncIterable`
    /// argument (⚠️ VERIFY its exact semantics in `client.py` —
    /// notably the `session_id` handling per item). Unlike
    /// `query_stream()`, this does NOT close stdin afterwards; the
    /// session stays open for further sends.
    ///
    /// # Errors
    ///
    /// [`Error::CliConnection`] on a broken session mid-feed.
    pub async fn send_stream(
        &self,
        prompts: impl Stream<Item = UserContent> + Send,
    ) -> Result<()> { todo!() }

    /// Streams messages until (and including) the next
    /// [`Message::Result`] — i.e. one complete response.
    pub fn receive_response(
        &mut self,
    ) -> impl Stream<Item = Result<Message>> + Send + '_ {
        // Adapter over receive_messages() that ends AFTER yielding
        // a Message::Result (inclusive), mirroring upstream
        // receive_response().
        todo!()
    }

    /// Streams every message as it arrives (does not stop at results).
    pub fn receive_messages(
        &mut self,
    ) -> impl Stream<Item = Result<Message>> + Send + '_ {
        // Map query.next_message() Values through parse_message().
        todo!()
    }

    /// Sends an interrupt control request.
    ///
    /// # Errors
    ///
    /// [`Error::ControlProtocol`] if the CLI rejects or times out.
    pub async fn interrupt(&self) -> Result<()> { todo!() }

    /// Changes the permission mode mid-session.
    ///
    /// # Errors
    ///
    /// [`Error::ControlProtocol`] on rejection/timeout.
    pub async fn set_permission_mode(&self, mode: PermissionMode) -> Result<()> { todo!() }

    /// Changes the model mid-session.
    ///
    /// # Errors
    ///
    /// [`Error::ControlProtocol`] on rejection/timeout.
    pub async fn set_model(&self, model: &str) -> Result<()> { todo!() }

    /// Ends input, terminates the CLI, and releases resources.
    ///
    /// # Errors
    ///
    /// [`Error::CliConnection`] if teardown fails; safe to call twice.
    pub async fn disconnect(&mut self) -> Result<()> { todo!() }
}
```

Additional upstream methods — port EVERY public method of
`ClaudeSDKClient` (walk `client.py` top to bottom; ~100% parity is the
requirement, a missing method must have a written justification in
`PARITY.md`). Known list to expect (⚠️ VERIFY names/signatures):

- `get_server_info()` — returns the initialize response data
  (commands, output styles); in Rust: `server_info(&self) -> Option<&Value>`
  from the cached initialize result.
- any `rewind`/session-manipulation methods present in the pinned
  upstream version.

For each, the recipe is identical: a
`control_request(ControlRequestBody::X)` wrapper (or cached-state
getter). Add any missing `ControlRequestBody` variants discovered
during this walk, with serde unit tests for their wire shape.

Drop behavior: implement `Drop` only as best-effort `start_kill` via
the transport's `kill_on_drop(true)` (already set in Phase 4) — do NOT
block in `Drop`. Document that `disconnect()` is the correct teardown.

Register in `lib.rs`: `mod client; pub use client::ClaudeClient;`

## Tests (`tests/client_test.rs`, write FIRST — fake CLI with the `responding` harness; scripts answer `initialize` with a canned success using deterministic request ids as established in Phase 5)

1. `connect_performs_initialize_handshake` — recording+responding fake;
   after `connect`, the recording contains a `control_request` line with
   subtype `initialize`.
2. `connect_fails_when_initialize_rejected` — fake answers error →
   `connect` returns `Err(Error::ControlProtocol)`.
3. `send_writes_stream_json_user_message` — recorded line equals the
   canonical user-message shape (Value equality).
4. `receive_response_stops_after_result_inclusive` — script emits
   assistant, assistant, result, assistant(extra) → stream yields
   exactly 3 items, last is `Message::Result`.
5. `receive_messages_continues_past_result` — same script → 4 items.
6. `interrupt_sends_control_request_and_resolves` — fake answers
   success → `Ok(())`; recording contains subtype `interrupt`.
7. `set_permission_mode_sends_wire_string` — recording contains
   `"mode":"acceptEdits"`.
8. `set_model_sends_model_name` — recording contains the name.
9. `send_after_disconnect_returns_connection_error`.
10. `disconnect_twice_is_ok`.
11. `send_content_blocks_writes_block_json` — `UserContent::Blocks`
    with one tool_result block → recorded line carries the array form.
12. `send_stream_forwards_all_items_and_keeps_session_open` — feed 2
    items via `stream::iter`, then a regular `send()` → recording has
    3 user-message lines; no stdin close between them.
13. `server_info_available_after_connect` — fake answers initialize
    with `{"commands":["x"]}` → `server_info()` exposes it.

## Acceptance Gate

```bash
cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test && cargo doc --no-deps
```

## Commits

1. `phase-7: client tests (red)`
2. `phase-7: ClaudeClient (green)`
3. `phase-7: port remaining upstream client methods` (one commit per
   method group, after the verify walk)
