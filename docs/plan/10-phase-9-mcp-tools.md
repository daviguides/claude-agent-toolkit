# Phase 9 — In-Process MCP Tools

**Objective**: let users define custom tools as native Rust async
functions and expose them to Claude without any external MCP
subprocess — the Rust equivalent of upstream's `tool()` decorator +
`create_sdk_mcp_server()`.

**Upstream sources of truth**:
- `reference/.../src/claude_agent_sdk/__init__.py` (`tool`,
  `create_sdk_mcp_server` — public shape)
- `reference/.../src/claude_agent_sdk/_internal/query.py` — the
  `mcp_message` control-request routing and which JSON-RPC methods the
  in-process server must answer
- Upstream serializes sdk servers into `--mcp-config` as
  `{"type":"sdk","name":...}` while keeping the instance SDK-side —
  ⚠️ VERIFY in `subprocess_cli.py`.

## How it works (mechanism recap)

The CLI treats an `"sdk"`-type MCP server as reachable through the SDK:
it sends `control_request{subtype:"mcp_message", server_name, message}`
where `message` is a standard MCP JSON-RPC request. The SDK dispatches
to the in-process server and returns the JSON-RPC response inside the
control response. Only a minimal method set is needed
(⚠️ VERIFY the exact set upstream answers; expected):

- `initialize` — capabilities/serverInfo handshake
- `notifications/initialized` — no-op ack (notification: NO response —
  ⚠️ VERIFY how upstream signals "no reply" through the control channel)
- `tools/list` — tool names, descriptions, input schemas
- `tools/call` — run the tool, return content blocks

## Deliverable A — tool + server types (extend `src/types/mcp.rs` or new `src/mcp_server.rs`; fixed: `src/mcp_server.rs`)

```rust
//! In-process MCP server: define tools as Rust closures.

use std::collections::HashMap;
use std::sync::Arc;

use serde_json::{Value, json};

use crate::error::Result;
use crate::types::hook::BoxFuture; // move BoxFuture to a common module if needed

/// Result content returned by a tool invocation.
#[derive(Debug, Clone)]
pub struct ToolResult {
    /// Content blocks (MCP shape: [{"type":"text","text":...}]).
    pub content: Vec<Value>,
    /// Marks the result as an error.
    pub is_error: bool,
}

impl ToolResult {
    /// Convenience: a single text block, non-error.
    #[must_use]
    pub fn text(text: impl Into<String>) -> Self { /* ... */ }

    /// Convenience: a single text block flagged as an error.
    #[must_use]
    pub fn error(text: impl Into<String>) -> Self { /* ... */ }
}

/// Handler signature for a tool.
pub type ToolHandler =
    Arc<dyn Fn(Value) -> BoxFuture<ToolResult> + Send + Sync>;

/// A tool exposed to Claude from Rust code.
#[derive(Clone)]
pub struct SdkTool {
    /// Tool name (what the model calls).
    pub name: String,
    /// One-line description shown to the model.
    pub description: String,
    /// JSON Schema of the input object.
    pub input_schema: Value,
    /// The async handler.
    pub handler: ToolHandler,
}

/// Builder-style constructor mirroring upstream `tool()`.
///
/// `input_schema` accepts raw JSON Schema via `serde_json::json!`.
pub fn tool<F, Fut>(
    name: impl Into<String>,
    description: impl Into<String>,
    input_schema: Value,
    handler: F,
) -> SdkTool
where
    F: Fn(Value) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = ToolResult> + Send + 'static,
{ /* wrap into Arc + Box::pin */ }

/// An in-process MCP server (collection of tools).
#[derive(Clone)]
pub struct SdkMcpServer {
    /// Server name (key used by the CLI to route).
    pub name: String,
    /// Server version reported in the MCP handshake.
    pub version: String,
    tools: HashMap<String, SdkTool>,
}

/// Mirrors upstream `create_sdk_mcp_server(name, version, tools)`.
#[must_use]
pub fn create_sdk_mcp_server(
    name: impl Into<String>,
    version: impl Into<String>,
    tools: Vec<SdkTool>,
) -> SdkMcpServer { /* ... */ }

impl SdkMcpServer {
    /// Handles one MCP JSON-RPC message and returns the response
    /// value, or `None` for notifications.
    ///
    /// # Errors
    ///
    /// Never returns `Err` for tool failures — those become JSON-RPC
    /// error results per the MCP spec. `Err` is reserved for internal
    /// invariant violations.
    pub(crate) async fn handle_message(&self, message: Value) -> Result<Option<Value>> {
        // match message["method"].as_str():
        //   "initialize" => respond protocolVersion/capabilities/serverInfo
        //       (echo back the requested protocolVersion — ⚠️ VERIFY
        //        upstream's exact initialize result shape)
        //   "notifications/initialized" => Ok(None)
        //   "tools/list" => {"tools":[{name, description, inputSchema}...]}
        //   "tools/call" => run handler(params.arguments);
        //       result {"content":[...], "isError":bool}
        //   unknown method => JSON-RPC error -32601
        // Always wrap in {"jsonrpc":"2.0","id":<echo>,"result"/"error":...}
        todo!()
    }
}
```

## Deliverable B — wiring

1. `McpServerConfig` gains a variant `Sdk(SdkMcpServer)` — NOT serde-
   serializable as-is; therefore `--mcp-config` construction must
   special-case it: serialize `{"type":"sdk","name":<name>}` and stash
   the server instance into `QueryHandlers.sdk_mcp_servers` keyed by
   name (⚠️ VERIFY the serialized stub shape upstream).
   Since Phase 3 derived `Serialize` for `McpServerConfig`, replace the
   derive with a manual `Serialize` impl now (or serialize via a
   dedicated `fn to_cli_config_json(&McpServers) -> Value`). Fixed
   choice: dedicated function, keeping types clean.
2. Query actor: on `mcp_message` control request → look up server by
   `server_name` → `handle_message` → success control response with the
   JSON-RPC response payload (⚠️ VERIFY envelope key, likely
   `{"mcp_response": ...}` or the raw response — read `_internal/query.py`).
   Unknown server → error control response.

## Tests (write FIRST — unit tests in `mcp_server.rs` cover JSON-RPC; integration in `tests/mcp_test.rs` covers routing)

Unit (call `handle_message` directly):
1. `initialize_returns_server_info` — name/version echoed.
2. `initialized_notification_returns_none`.
3. `tools_list_returns_all_tools_with_schemas` — two tools; names,
   descriptions, `inputSchema` all present.
4. `tools_call_runs_handler_and_returns_content` — calculator add tool
   `{a:2,b:3}` → content `[{"type":"text","text":"5"}]`, `isError:false`.
5. `tools_call_unknown_tool_returns_jsonrpc_error`.
6. `tools_call_handler_error_flag_maps_to_is_error_true` — handler
   returns `ToolResult::error(...)`.
7. `unknown_method_returns_method_not_found` — code `-32601`.
8. `response_echoes_request_id` — string and numeric ids both.

Integration:
9. `mcp_config_serializes_sdk_server_as_stub` — options with one sdk
   server → `--mcp-config` JSON has `{"type":"sdk","name":"calc"}` and
   no handler leakage.
10. `mcp_message_request_routes_to_server` — responding fake sends an
    `mcp_message` (tools/list) control request; recorded control
    response contains the tool name.
11. `unknown_server_name_yields_error_response`.

## Acceptance Gate

```bash
cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test && cargo doc --no-deps
```

## Commits

1. `phase-9: sdk tool + server types with jsonrpc unit tests (red/green)`
2. `phase-9: mcp-config stub serialization + actor routing (green)`
