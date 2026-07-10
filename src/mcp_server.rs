//! In-process MCP server: define tools as Rust closures, exposed to
//! Claude without spawning an external MCP subprocess. Rust port of
//! upstream's `tool()` decorator + `create_sdk_mcp_server()`.

use std::future::Future;
use std::panic::AssertUnwindSafe;
use std::sync::Arc;

use futures::FutureExt;
use serde_json::{Value, json};

use crate::types::session_store::BoxFuture;

/// MCP protocol version this in-process server speaks. Hardcoded to
/// match upstream's own `_handle_sdk_mcp_request`, which does not echo
/// the request's own `protocolVersion` — see `DEVIATIONS.md`.
const MCP_PROTOCOL_VERSION: &str = "2024-11-05";

/// JSON-RPC 2.0 "Method not found" error code.
const JSONRPC_METHOD_NOT_FOUND: i64 = -32601;
/// JSON-RPC 2.0 "Invalid params" error code — used here for an unknown
/// tool name in `tools/call` (see `DEVIATIONS.md`).
const JSONRPC_INVALID_PARAMS: i64 = -32602;
/// JSON-RPC 2.0 "Internal error" error code — used when a tool
/// handler panics.
const JSONRPC_INTERNAL_ERROR: i64 = -32603;

/// Result content returned by a tool invocation.
#[derive(Debug, Clone)]
pub struct ToolResult {
    /// Content blocks (MCP shape: `[{"type":"text","text":...}]`).
    pub content: Vec<Value>,
    /// Marks the result as an error.
    pub is_error: bool,
}

impl ToolResult {
    /// Convenience: a single text block, non-error.
    #[must_use]
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            content: vec![json!({"type": "text", "text": text.into()})],
            is_error: false,
        }
    }

    /// Convenience: a single text block flagged as an error.
    #[must_use]
    pub fn error(text: impl Into<String>) -> Self {
        Self {
            content: vec![json!({"type": "text", "text": text.into()})],
            is_error: true,
        }
    }
}

/// Handler signature for a tool.
pub type ToolHandler = Arc<dyn Fn(Value) -> BoxFuture<'static, ToolResult> + Send + Sync>;

/// A tool exposed to Claude from Rust code.
#[derive(Clone)]
pub struct SdkTool {
    /// Tool name (what the model calls).
    pub name: String,
    /// One-line description shown to the model.
    pub description: String,
    /// JSON Schema of the input object.
    pub input_schema: Value,
    /// Advisory hints (upstream `ToolAnnotations`), forwarded verbatim
    /// to `tools/list`. Kept as raw JSON — see `DEVIATIONS.md`.
    pub annotations: Option<Value>,
    /// The async handler.
    pub handler: ToolHandler,
}

impl std::fmt::Debug for SdkTool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SdkTool")
            .field("name", &self.name)
            .field("description", &self.description)
            .field("input_schema", &self.input_schema)
            .field("annotations", &self.annotations)
            .field("handler", &"<fn>")
            .finish()
    }
}

impl SdkTool {
    /// Attaches advisory `annotations` to this tool.
    #[must_use]
    pub fn with_annotations(mut self, annotations: Value) -> Self {
        self.annotations = Some(annotations);
        self
    }
}

/// Defines a tool. Mirrors upstream's `tool()` decorator.
///
/// `input_schema` is a raw JSON Schema value — Rust has no runtime
/// type reflection to support upstream's Python-dict-of-types
/// shorthand, so this port only accepts the JSON Schema form already
/// supported upstream (see `DEVIATIONS.md`).
#[must_use]
pub fn tool<F, Fut>(
    name: impl Into<String>,
    description: impl Into<String>,
    input_schema: Value,
    handler: F,
) -> SdkTool
where
    F: Fn(Value) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = ToolResult> + Send + 'static,
{
    SdkTool {
        name: name.into(),
        description: description.into(),
        input_schema,
        annotations: None,
        handler: Arc::new(move |input| Box::pin(handler(input))),
    }
}

/// An in-process MCP server: a named, versioned collection of tools.
///
/// Tools are stored in registration order (not a `HashMap`) so
/// `tools/list` responses are deterministic across runs, matching
/// upstream's own cached, insertion-ordered tool list — see
/// `DEVIATIONS.md`.
#[derive(Clone)]
pub struct SdkMcpServer {
    /// Server name — the CLI's routing key for `mcp_message` control
    /// requests, and the `name` in its `--mcp-config` stub entry.
    pub name: String,
    /// Server version reported in the MCP `initialize` handshake.
    pub version: String,
    tools: Vec<SdkTool>,
}

impl std::fmt::Debug for SdkMcpServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SdkMcpServer")
            .field("name", &self.name)
            .field("version", &self.version)
            .field(
                "tools",
                &self.tools.iter().map(|tool| &tool.name).collect::<Vec<_>>(),
            )
            .finish()
    }
}

/// Creates an in-process MCP server. Mirrors upstream
/// `create_sdk_mcp_server(name, version, tools)`.
#[must_use]
pub fn create_sdk_mcp_server(
    name: impl Into<String>,
    version: impl Into<String>,
    tools: Vec<SdkTool>,
) -> SdkMcpServer {
    SdkMcpServer {
        name: name.into(),
        version: version.into(),
        tools,
    }
}

impl SdkMcpServer {
    /// Handles one MCP JSON-RPC message and returns the JSON-RPC
    /// response value.
    ///
    /// Always returns a concrete `Value` — unknown methods, unknown
    /// tool names, and handler panics all become JSON-RPC error
    /// objects rather than being surfaced any other way, matching
    /// upstream's own infallible `_handle_sdk_mcp_request` (see
    /// `DEVIATIONS.md`).
    pub(crate) async fn handle_message(&self, message: &Value) -> Value {
        let id = message.get("id").unwrap_or(&Value::Null);
        match message.get("method").and_then(Value::as_str) {
            Some("initialize") => self.handle_initialize(id),
            Some("notifications/initialized") => json!({"jsonrpc": "2.0", "result": {}}),
            Some("tools/list") => self.handle_tools_list(id),
            Some("tools/call") => self.handle_tools_call(id, message.get("params")).await,
            Some(other) => jsonrpc_error(
                id,
                JSONRPC_METHOD_NOT_FOUND,
                &format!("Method '{other}' not found"),
            ),
            None => jsonrpc_error(id, JSONRPC_METHOD_NOT_FOUND, "Missing method"),
        }
    }

    fn handle_initialize(&self, id: &Value) -> Value {
        json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {
                "protocolVersion": MCP_PROTOCOL_VERSION,
                "capabilities": {"tools": {}},
                "serverInfo": {"name": self.name, "version": self.version},
            },
        })
    }

    fn handle_tools_list(&self, id: &Value) -> Value {
        let tools: Vec<Value> = self
            .tools
            .iter()
            .map(|tool| {
                let mut entry = json!({
                    "name": tool.name,
                    "description": tool.description,
                    "inputSchema": tool.input_schema,
                });
                if let Some(annotations) = &tool.annotations {
                    entry["annotations"] = annotations.clone();
                }
                entry
            })
            .collect();
        json!({"jsonrpc": "2.0", "id": id, "result": {"tools": tools}})
    }

    async fn handle_tools_call(&self, id: &Value, params: Option<&Value>) -> Value {
        let name = params
            .and_then(|params| params.get("name"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        let Some(tool) = self.tools.iter().find(|tool| tool.name == name) else {
            return jsonrpc_error(
                id,
                JSONRPC_INVALID_PARAMS,
                &format!("Tool '{name}' not found"),
            );
        };
        let arguments = params
            .and_then(|params| params.get("arguments"))
            .cloned()
            .unwrap_or_else(|| json!({}));

        let handler = Arc::clone(&tool.handler);
        let outcome = AssertUnwindSafe(handler(arguments)).catch_unwind().await;
        let Ok(result) = outcome else {
            return jsonrpc_error(id, JSONRPC_INTERNAL_ERROR, "tool handler panicked");
        };

        let mut response = json!({"content": result.content});
        if result.is_error {
            response["isError"] = json!(true);
        }
        json!({"jsonrpc": "2.0", "id": id, "result": response})
    }
}

fn jsonrpc_error(id: &Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}

#[cfg(test)]
mod tests {
    use super::*;

    fn calculator_server() -> SdkMcpServer {
        let add = tool(
            "add",
            "Add two numbers",
            json!({"type": "object", "properties": {"a": {"type": "number"}, "b": {"type": "number"}}}),
            |input: Value| async move {
                let a = input["a"].as_f64().unwrap_or_default();
                let b = input["b"].as_f64().unwrap_or_default();
                ToolResult::text((a + b).to_string())
            },
        );
        let fail = tool(
            "fail",
            "Always fails",
            json!({"type": "object", "properties": {}}),
            |_input: Value| async move { ToolResult::error("boom") },
        );
        let panics = tool(
            "panics",
            "Always panics",
            json!({"type": "object", "properties": {}}),
            |_input: Value| async move {
                panic!("tool handler exploded");
                #[allow(unreachable_code)]
                ToolResult::text("unreachable")
            },
        );
        create_sdk_mcp_server("calc", "1.0.0", vec![add, fail, panics])
    }

    #[tokio::test]
    async fn initialize_returns_server_info() {
        let server = calculator_server();
        let response = server
            .handle_message(&json!({"jsonrpc": "2.0", "id": 1, "method": "initialize"}))
            .await;
        assert_eq!(response["result"]["serverInfo"]["name"], "calc");
        assert_eq!(response["result"]["serverInfo"]["version"], "1.0.0");
        assert_eq!(response["result"]["protocolVersion"], "2024-11-05");
    }

    #[tokio::test]
    async fn initialized_notification_returns_a_result_object() {
        let server = calculator_server();
        let response = server
            .handle_message(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"}))
            .await;
        assert_eq!(response, json!({"jsonrpc": "2.0", "result": {}}));
    }

    #[tokio::test]
    async fn tools_list_returns_all_tools_with_schemas() {
        let server = calculator_server();
        let response = server
            .handle_message(&json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}))
            .await;
        let tools = response["result"]["tools"].as_array().expect("tools array");
        assert_eq!(tools.len(), 3);
        assert_eq!(tools[0]["name"], "add");
        assert_eq!(tools[0]["description"], "Add two numbers");
        assert_eq!(tools[0]["inputSchema"]["type"], "object");
    }

    #[tokio::test]
    async fn tools_list_includes_annotations_when_set() {
        let annotated = tool(
            "noop",
            "Does nothing",
            json!({"type": "object"}),
            |_input: Value| async move { ToolResult::text("ok") },
        )
        .with_annotations(json!({"readOnlyHint": true}));
        let server = create_sdk_mcp_server("srv", "1.0.0", vec![annotated]);

        let response = server
            .handle_message(&json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}))
            .await;
        assert_eq!(
            response["result"]["tools"][0]["annotations"],
            json!({"readOnlyHint": true})
        );
    }

    #[tokio::test]
    async fn tools_call_runs_handler_and_returns_content() {
        let server = calculator_server();
        let response = server
            .handle_message(&json!({
                "jsonrpc": "2.0", "id": 3, "method": "tools/call",
                "params": {"name": "add", "arguments": {"a": 2, "b": 3}},
            }))
            .await;
        assert_eq!(
            response["result"]["content"],
            json!([{"type": "text", "text": "5"}])
        );
        assert!(response["result"].get("isError").is_none());
    }

    #[tokio::test]
    async fn tools_call_unknown_tool_returns_jsonrpc_error() {
        let server = calculator_server();
        let response = server
            .handle_message(&json!({
                "jsonrpc": "2.0", "id": 4, "method": "tools/call",
                "params": {"name": "missing", "arguments": {}},
            }))
            .await;
        assert_eq!(response["error"]["code"], -32602);
        assert_eq!(response["id"], 4);
    }

    #[tokio::test]
    async fn tools_call_handler_error_flag_maps_to_is_error_true() {
        let server = calculator_server();
        let response = server
            .handle_message(&json!({
                "jsonrpc": "2.0", "id": 5, "method": "tools/call",
                "params": {"name": "fail", "arguments": {}},
            }))
            .await;
        assert_eq!(response["result"]["isError"], true);
        assert_eq!(
            response["result"]["content"],
            json!([{"type": "text", "text": "boom"}])
        );
    }

    #[tokio::test]
    async fn tools_call_handler_panic_returns_internal_error() {
        let server = calculator_server();
        let response = server
            .handle_message(&json!({
                "jsonrpc": "2.0", "id": 6, "method": "tools/call",
                "params": {"name": "panics", "arguments": {}},
            }))
            .await;
        assert_eq!(response["error"]["code"], -32603);
    }

    #[tokio::test]
    async fn unknown_method_returns_method_not_found() {
        let server = calculator_server();
        let response = server
            .handle_message(&json!({"jsonrpc": "2.0", "id": 7, "method": "resources/list"}))
            .await;
        assert_eq!(response["error"]["code"], -32601);
    }

    #[tokio::test]
    async fn response_echoes_request_id() {
        let server = calculator_server();
        let string_id = server
            .handle_message(&json!({"jsonrpc": "2.0", "id": "req-1", "method": "tools/list"}))
            .await;
        assert_eq!(string_id["id"], "req-1");

        let numeric_id = server
            .handle_message(&json!({"jsonrpc": "2.0", "id": 42, "method": "tools/list"}))
            .await;
        assert_eq!(numeric_id["id"], 42);
    }
}
