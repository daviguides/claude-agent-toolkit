# Phase 5 — Control Protocol and the Query Actor

**Objective**: the internal `Query` actor that owns the transport,
routes normal messages to consumers, correlates SDK-initiated control
requests with their responses, and answers CLI-initiated control
requests. This is the heart of the SDK — everything public sits on top.

**Upstream source of truth**:
`reference/.../src/claude_agent_sdk/_internal/query.py` — read it fully.
Every wire shape below is a sketch to be diffed against that file.

## Wire shapes (⚠️ VERIFY all against `_internal/query.py`)

SDK → CLI control request:

```json
{"type":"control_request","request_id":"req_1_abc123","request":{"subtype":"interrupt"}}
```

Known SDK-initiated subtypes: `initialize` (carries hook registration),
`interrupt`, `set_permission_mode` (`{"mode": "..."}`),
`set_model` (`{"model": "..."}`).

CLI → SDK control response (answers an SDK request):

```json
{"type":"control_response","response":{"subtype":"success","request_id":"req_1_abc123","response":{}}}
{"type":"control_response","response":{"subtype":"error","request_id":"req_1_abc123","error":"why"}}
```

CLI → SDK control request (SDK must answer):

```json
{"type":"control_request","request_id":"cli_req_1","request":{"subtype":"can_use_tool","tool_name":"Bash","input":{...},"permission_suggestions":[...]}}
{"type":"control_request","request_id":"cli_req_2","request":{"subtype":"hook_callback","callback_id":"hook_0","input":{...}}}
{"type":"control_request","request_id":"cli_req_3","request":{"subtype":"mcp_message","server_name":"calc","message":{...jsonrpc...}}}
```

SDK → CLI control response:

```json
{"type":"control_response","response":{"subtype":"success","request_id":"cli_req_1","response":{...payload...}}}
{"type":"control_response","response":{"subtype":"error","request_id":"cli_req_1","error":"why"}}
```

Streaming user message (SDK → CLI, plain message, NOT control):

```json
{"type":"user","message":{"role":"user","content":"hello"},"parent_tool_use_id":null,"session_id":"default"}
```

## Deliverable A — `src/protocol/control.rs` (pure serde types)

Define serde structs/enums for exactly the shapes above:

- `OutboundControlRequest { request_id: String, request: ControlRequestBody }`
  with `#[serde(tag = "subtype", rename_all = "snake_case")]` enum body:
  `Initialize { hooks: Option<Value> }`, `Interrupt`,
  `SetPermissionMode { mode: String }`, `SetModel { model: String }`.
- `InboundControlRequest` — the CLI-initiated bodies:
  `CanUseTool { tool_name: String, input: Value, permission_suggestions: Option<Value> }`,
  `HookCallback { callback_id: String, input: Value }`,
  `McpMessage { server_name: String, message: Value }`.
- `ControlResponseEnvelope` / success + error variants for both
  directions.
- Request-id generation: `format!("req_{}_{:x}", counter, nanos)` where
  `counter` is an `AtomicU64` — deterministic prefix, unique suffix
  (⚠️ VERIFY upstream format; matching it aids log comparison but any
  unique string works — note the choice).

Unit tests (in-file, write first): serialize each outbound body and
assert exact JSON via `serde_json::json!` comparison; deserialize each
inbound sample from the appendix fixtures.

## Deliverable B — `src/protocol/query.rs` — the actor

### Shape

```rust
/// Handlers a Query needs to answer CLI-initiated requests.
/// Phase 5 wires the plumbing with these as simple placeholders;
/// Phases 8-9 supply real implementations.
pub(crate) struct QueryHandlers {
    pub can_use_tool: Option<CanUseToolHandler>,   // defined Phase 8
    pub hook_callbacks: HashMap<String, HookHandler>, // defined Phase 8
    pub sdk_mcp_servers: HashMap<String, McpServerHandle>, // Phase 9
}

/// Owns the transport; runs a background read loop.
pub(crate) struct Query {
    outbound: mpsc::Sender<String>,            // lines to write
    pending: Arc<Mutex<HashMap<String, oneshot::Sender<Result<Value>>>>>,
    messages: mpsc::Receiver<Result<Value>>,   // normal (non-control) messages
    read_task: tokio::task::JoinHandle<()>,
    write_task: tokio::task::JoinHandle<()>,
}
```

Concurrency model (fixed — do not redesign):

- **write task**: single owner of `transport.write_line`; consumes an
  `mpsc::Receiver<String>`. All writers (user messages, control
  responses, control requests) go through this channel. This serializes
  stdin access with zero locks around the transport.
- **read task**: consumes `transport.read_messages()`; for each `Value`:
  - `"control_response"` → look up `request_id` in `pending`, send the
    payload (or `Error::ControlProtocol` on error subtype) through the
    oneshot; drop unknown ids with a `tracing::warn!`.
  - `"control_request"` → spawn a handler future (so a slow permission
    callback cannot stall the read loop): run the matching handler,
    wrap its output in a success/error control response, push the line
    into the outbound channel.
  - `"control_cancel_request"` — ⚠️ VERIFY existence upstream; if
    present, implement cancellation of the spawned handler task.
  - anything else → forward to the `messages` channel untouched.
  - `Err(e)` from transport → forward the error to `messages` and stop.

### Methods

```rust
impl Query {
    /// Spawns read/write tasks over a connected transport.
    pub(crate) fn start(
        transport: impl Transport + 'static,
        handlers: QueryHandlers,
    ) -> Self;

    /// Sends `initialize` and waits for the response (streaming mode
    /// only). Carries hook registration data. 60s timeout →
    /// Error::ControlProtocol (⚠️ VERIFY timeout value upstream).
    pub(crate) async fn initialize(&self, hooks: Option<Value>) -> Result<Value>;

    /// Sends a control request and awaits its response.
    pub(crate) async fn control_request(
        &self,
        body: ControlRequestBody,
    ) -> Result<Value>;

    /// Writes a plain user message line.
    pub(crate) async fn send_user_message(&self, content: &UserContent, session_id: &str) -> Result<()>;

    /// Receives the next normal message (already routed).
    pub(crate) async fn next_message(&mut self) -> Option<Result<Value>>;

    /// Closes input, terminates tasks and transport.
    pub(crate) async fn close(&mut self) -> Result<()>;
}
```

`control_request` algorithm (spell out):

1. Generate `request_id`; create `oneshot`; insert into `pending`.
2. Serialize envelope; send through outbound channel.
3. `tokio::time::timeout(CONTROL_TIMEOUT, rx).await` — on timeout,
   remove from `pending`, return `Error::ControlProtocol` naming the
   subtype.

## Tests (`tests/protocol_test.rs` + in-file unit tests, write FIRST)

Use the fake CLI harness. For control-protocol round-trips the fake
script must ANSWER requests — extend `fake_cli.rs` with:

```rust
/// A fake CLI that reads stdin lines and responds per a simple rule
/// table baked into a shell script with a `while read line` loop:
/// any line containing "control_request" and "interrupt" triggers
/// printing the canned success response; other stdin lines are logged.
pub fn responding(rules: &[(&str, &str)], trailing: &[&str]) -> FakeCli
```

(`rules`: substring → response line. Shell `case "$line" in *substr*)`
is enough; no JSON parsing needed in the script.)

1. `routes_normal_messages_to_consumer` — scripted assistant+result
   lines → `next_message()` yields both, no control interference.
2. `control_request_resolves_on_success_response` — responding fake:
   substring `interrupt` → canned success with the SAME request id...
   Problem: the script cannot extract the id. Solution (fixed design):
   make request-id generation injectable — `Query::start` takes an
   optional `id_prefix` and the test constructs the Query with a
   deterministic counter so ids are `req_1_test`, `req_2_test`...; the
   canned response hardcodes `req_1_test`.
3. `control_request_error_response_maps_to_control_protocol_error` —
   canned error subtype → `Err(Error::ControlProtocol)` with the CLI's
   message inside.
4. `control_request_times_out` — fake never answers; use a 100ms
   timeout override (make `CONTROL_TIMEOUT` a field with default) →
   `Err(Error::ControlProtocol)` mentioning timeout.
5. `answers_hook_callback_request` — register a hook handler that
   returns `json!({"ok": true})`; scripted CLI emits a `hook_callback`
   control request; assert (via recording fake) that the SDK wrote a
   `control_response` success with `"ok":true` and matching request id.
6. `handler_error_produces_error_response` — handler returns `Err` →
   SDK writes error control response, read loop keeps going (subsequent
   scripted message still delivered).
7. `unknown_control_response_id_is_ignored` — canned response with
   bogus id → no panic, later messages still flow.
8. `user_message_line_shape` — `send_user_message` writes exactly
   `{"type":"user","message":{"role":"user","content":"hello"},"parent_tool_use_id":null,"session_id":"default"}`
   (assert with `serde_json::Value` equality on the recorded line, not
   string equality).

## Acceptance Gate

```bash
cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test && cargo doc --no-deps
```

## Commits

1. `phase-5: control wire types (tests first)`
2. `phase-5: responding fake CLI + query actor tests (red)`
3. `phase-5: query actor (green)`
