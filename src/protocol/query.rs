//! The `Query` actor: owns the transport, routes normal messages to
//! consumers, correlates SDK-initiated control requests with their
//! responses, and dispatches CLI-initiated control requests to
//! registered handlers.

use std::collections::HashMap;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use futures::StreamExt;
use serde_json::Value;
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;

use crate::error::{Error, Result};
use crate::protocol::control::{
    ControlRequestBody, ControlResponseBody, ControlResponseEnvelope, InboundControlRequest,
    InboundControlRequestBody, OutboundControlRequest, RequestIdGenerator,
};
use crate::transport::Transport;
use crate::types::message::UserContent;
use crate::types::session_store::BoxFuture;

/// Default timeout for a control request awaiting its response.
const DEFAULT_CONTROL_TIMEOUT: Duration = Duration::from_secs(60);

/// Handler for a `can_use_tool` control request. Real implementation
/// wired in Phase 8; Phase 5 only defines the shape.
pub(crate) type CanUseToolHandler =
    Arc<dyn Fn(String, Value, Value) -> BoxFuture<'static, Result<Value>> + Send + Sync>;

/// Handler for one registered hook callback. Real implementation wired
/// in Phase 8.
pub(crate) type HookHandler =
    Arc<dyn Fn(Value, Option<String>) -> BoxFuture<'static, Result<Value>> + Send + Sync>;

/// Handler for one in-process (SDK) MCP server's JSON-RPC messages.
/// Real implementation wired in Phase 9.
pub(crate) type McpServerHandle =
    Arc<dyn Fn(Value) -> BoxFuture<'static, Result<Value>> + Send + Sync>;

/// Handlers a [`Query`] needs to answer CLI-initiated requests.
#[derive(Clone, Default)]
pub(crate) struct QueryHandlers {
    pub can_use_tool: Option<CanUseToolHandler>,
    pub hook_callbacks: HashMap<String, HookHandler>,
    pub sdk_mcp_servers: HashMap<String, McpServerHandle>,
}

/// Commands sent to the task that owns the transport. Modeled as an
/// enum (not raw strings) so `end_input`/`close` — which also need
/// `&mut Transport` — can be routed through the same single-writer
/// channel; see `DEVIATIONS.md`.
enum WriteCommand {
    Line(String),
    EndInput,
    Close(oneshot::Sender<Result<()>>),
}

type PendingMap = Arc<StdMutex<HashMap<String, oneshot::Sender<Result<Value>>>>>;
type InflightMap = Arc<StdMutex<HashMap<String, JoinHandle<()>>>>;

/// Owns the transport; runs a background read loop and a background
/// writer that serializes all stdin access.
///
/// `messages`/`driver_task` use `tokio::sync::Mutex` (not a design for
/// genuine multi-reader contention — exactly one task calls
/// `next_message`/`close` in practice) purely so those two methods can
/// take `&self` like every other method here. This lets a caller share
/// one `Query` between a reading loop and a concurrent input-feeding
/// task (needed by `query_stream()` in Phase 6, matching upstream's
/// own concurrent `stream_input` background task) without the whole
/// `Query` sitting behind one lock — a lock spanning both read and
/// write would serialize them, defeating the concurrency.
pub(crate) struct Query {
    outbound: mpsc::UnboundedSender<WriteCommand>,
    pending: PendingMap,
    messages: tokio::sync::Mutex<mpsc::UnboundedReceiver<Result<Value>>>,
    driver_task: tokio::sync::Mutex<Option<JoinHandle<()>>>,
    id_gen: RequestIdGenerator,
    control_timeout: Duration,
    initialize_timeout: Duration,
    /// Cached response from the last successful `initialize()` call —
    /// upstream's `get_server_info()` returns this cached value rather
    /// than re-fetching.
    initialization_result: tokio::sync::Mutex<Option<Value>>,
}

impl Query {
    /// Spawns the read/write driver over a connected transport.
    pub(crate) fn start(transport: impl Transport + 'static, handlers: QueryHandlers) -> Self {
        Self::start_with(
            transport,
            handlers,
            RequestIdGenerator::new(),
            DEFAULT_CONTROL_TIMEOUT,
        )
    }

    /// Like [`start`](Self::start), but with an injectable id
    /// generator and control-request timeout — used by tests.
    pub(crate) fn start_with(
        mut transport: impl Transport + 'static,
        handlers: QueryHandlers,
        id_gen: RequestIdGenerator,
        control_timeout: Duration,
    ) -> Self {
        let read_stream = transport.read_messages();

        let (outbound_tx, outbound_rx) = mpsc::unbounded_channel::<WriteCommand>();
        let (messages_tx, messages_rx) = mpsc::unbounded_channel::<Result<Value>>();
        let pending: PendingMap = Arc::new(StdMutex::new(HashMap::new()));
        let inflight: InflightMap = Arc::new(StdMutex::new(HashMap::new()));
        let handlers = Arc::new(handlers);
        let last_error_result_text: Arc<StdMutex<Option<String>>> = Arc::new(StdMutex::new(None));

        let driver_task = tokio::spawn(drive(
            transport,
            read_stream,
            outbound_rx,
            outbound_tx.clone(),
            pending.clone(),
            inflight,
            messages_tx,
            handlers,
            last_error_result_text,
        ));

        Self {
            outbound: outbound_tx,
            pending,
            messages: tokio::sync::Mutex::new(messages_rx),
            driver_task: tokio::sync::Mutex::new(Some(driver_task)),
            id_gen,
            control_timeout,
            initialize_timeout: DEFAULT_CONTROL_TIMEOUT,
            initialization_result: tokio::sync::Mutex::new(None),
        }
    }

    /// Overrides the initialize-specific timeout (upstream default:
    /// same 60s as [`DEFAULT_CONTROL_TIMEOUT`], independently
    /// configurable).
    pub(crate) fn set_initialize_timeout(&mut self, timeout: Duration) {
        self.initialize_timeout = timeout;
    }

    /// Sends `initialize` and waits for the response.
    pub(crate) async fn initialize(
        &self,
        hooks: Option<Value>,
        agents: Option<Value>,
        exclude_dynamic_sections: Option<bool>,
        skills: Option<Vec<String>>,
    ) -> Result<Value> {
        let result = self
            .control_request_with_timeout(
                ControlRequestBody::Initialize {
                    hooks,
                    agents,
                    exclude_dynamic_sections,
                    skills,
                },
                self.initialize_timeout,
            )
            .await?;
        *self.initialization_result.lock().await = Some(result.clone());
        Ok(result)
    }

    /// Cached response from the last successful `initialize()` call, or
    /// `None` if it hasn't run yet.
    pub(crate) async fn server_info(&self) -> Option<Value> {
        self.initialization_result.lock().await.clone()
    }

    /// Sends a control request and awaits its response, using the
    /// default control timeout.
    pub(crate) async fn control_request(&self, body: ControlRequestBody) -> Result<Value> {
        self.control_request_with_timeout(body, self.control_timeout)
            .await
    }

    async fn control_request_with_timeout(
        &self,
        body: ControlRequestBody,
        timeout: Duration,
    ) -> Result<Value> {
        let subtype = control_request_subtype(&body);
        let request_id = self.id_gen.next();
        let (tx, rx) = oneshot::channel();
        self.pending
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(request_id.clone(), tx);

        let envelope = OutboundControlRequest::new(request_id.clone(), body);
        let line = serde_json::to_string(&envelope).map_err(|source| Error::JsonDecode {
            line: String::new(),
            source,
        })?;

        if self.outbound.send(WriteCommand::Line(line)).is_err() {
            self.remove_pending(&request_id);
            return Err(Error::ControlProtocol {
                message: format!("transport closed before sending '{subtype}' request"),
            });
        }

        match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => {
                self.remove_pending(&request_id);
                Err(Error::ControlProtocol {
                    message: format!("transport closed while awaiting '{subtype}' response"),
                })
            }
            Err(_) => {
                self.remove_pending(&request_id);
                Err(Error::ControlProtocol {
                    message: format!("control request timeout: {subtype}"),
                })
            }
        }
    }

    fn remove_pending(&self, request_id: &str) {
        self.pending
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(request_id);
    }

    /// Interrupts the current turn.
    pub(crate) async fn interrupt(&self) -> Result<()> {
        self.control_request(ControlRequestBody::Interrupt)
            .await
            .map(|_| ())
    }

    /// Changes the permission mode mid-session.
    pub(crate) async fn set_permission_mode(&self, mode: &str) -> Result<()> {
        self.control_request(ControlRequestBody::SetPermissionMode {
            mode: mode.to_string(),
        })
        .await
        .map(|_| ())
    }

    /// Changes the model mid-session.
    pub(crate) async fn set_model(&self, model: Option<String>) -> Result<()> {
        self.control_request(ControlRequestBody::SetModel { model })
            .await
            .map(|_| ())
    }

    /// Rewinds tracked files to a prior user message.
    pub(crate) async fn rewind_files(&self, user_message_id: &str) -> Result<()> {
        self.control_request(ControlRequestBody::RewindFiles {
            user_message_id: user_message_id.to_string(),
        })
        .await
        .map(|_| ())
    }

    /// Reconnects a disconnected or failed MCP server.
    pub(crate) async fn reconnect_mcp_server(&self, server_name: &str) -> Result<()> {
        self.control_request(ControlRequestBody::McpReconnect {
            server_name: server_name.to_string(),
        })
        .await
        .map(|_| ())
    }

    /// Enables or disables an MCP server.
    pub(crate) async fn toggle_mcp_server(&self, server_name: &str, enabled: bool) -> Result<()> {
        self.control_request(ControlRequestBody::McpToggle {
            server_name: server_name.to_string(),
            enabled,
        })
        .await
        .map(|_| ())
    }

    /// Stops a running background task.
    pub(crate) async fn stop_task(&self, task_id: &str) -> Result<()> {
        self.control_request(ControlRequestBody::StopTask {
            task_id: task_id.to_string(),
        })
        .await
        .map(|_| ())
    }

    /// Gets current MCP server connection status.
    pub(crate) async fn get_mcp_status(&self) -> Result<Value> {
        self.control_request(ControlRequestBody::McpStatus).await
    }

    /// Gets a breakdown of current context window usage.
    pub(crate) async fn get_context_usage(&self) -> Result<Value> {
        self.control_request(ControlRequestBody::GetContextUsage)
            .await
    }

    /// Writes a plain user message line.
    ///
    /// `async` (despite no `.await` today) for API consistency with
    /// every other `Query` writer method — callers uniformly `.await`
    /// this alongside `control_request`/`initialize` without needing
    /// to remember which ones are "really" async.
    #[allow(clippy::unused_async)]
    pub(crate) async fn send_user_message(
        &self,
        content: &UserContent,
        session_id: &str,
    ) -> Result<()> {
        let content_value = match content {
            UserContent::Text(text) => Value::String(text.clone()),
            UserContent::Blocks(blocks) => {
                serde_json::to_value(blocks).map_err(|source| Error::JsonDecode {
                    line: String::new(),
                    source,
                })?
            }
        };
        let payload = serde_json::json!({
            "type": "user",
            "message": {"role": "user", "content": content_value},
            "parent_tool_use_id": Value::Null,
            "session_id": session_id,
        });
        let line = serde_json::to_string(&payload).map_err(|source| Error::JsonDecode {
            line: String::new(),
            source,
        })?;
        self.outbound
            .send(WriteCommand::Line(line))
            .map_err(|_| Error::ControlProtocol {
                message: "transport closed before sending user message".to_string(),
            })
    }

    /// Signals end of input (closes stdin).
    ///
    /// `async` for the same API-consistency reason as
    /// [`send_user_message`](Self::send_user_message).
    #[allow(clippy::unused_async)]
    pub(crate) async fn end_input(&self) -> Result<()> {
        self.outbound
            .send(WriteCommand::EndInput)
            .map_err(|_| Error::ControlProtocol {
                message: "transport closed before end_input".to_string(),
            })
    }

    /// Receives the next normal (non-control) message.
    pub(crate) async fn next_message(&self) -> Option<Result<Value>> {
        self.messages.lock().await.recv().await
    }

    /// Closes input, terminates the driver task and transport.
    pub(crate) async fn close(&self) -> Result<()> {
        let (tx, rx) = oneshot::channel();
        if self.outbound.send(WriteCommand::Close(tx)).is_ok() {
            let _ = rx.await;
        }
        if let Some(task) = self.driver_task.lock().await.take() {
            let _ = task.await;
        }
        Ok(())
    }
}

impl Drop for Query {
    /// Best-effort cleanup for a `Query` dropped without an explicit
    /// `.close().await` (e.g. a message stream abandoned mid-iteration).
    /// `Drop` can't `.await`, so this only *signals* the driver task to
    /// close the transport — fire-and-forget, not a guarantee the
    /// child has exited by the time `drop` returns. Harmless if
    /// `close()` already ran: the outbound channel is closed by then
    /// and this send silently no-ops.
    fn drop(&mut self) {
        let (tx, _rx) = oneshot::channel();
        let _ = self.outbound.send(WriteCommand::Close(tx));
    }
}

fn control_request_subtype(body: &ControlRequestBody) -> &'static str {
    match body {
        ControlRequestBody::Initialize { .. } => "initialize",
        ControlRequestBody::Interrupt => "interrupt",
        ControlRequestBody::SetPermissionMode { .. } => "set_permission_mode",
        ControlRequestBody::SetModel { .. } => "set_model",
        ControlRequestBody::RewindFiles { .. } => "rewind_files",
        ControlRequestBody::McpReconnect { .. } => "mcp_reconnect",
        ControlRequestBody::McpToggle { .. } => "mcp_toggle",
        ControlRequestBody::StopTask { .. } => "stop_task",
        ControlRequestBody::McpStatus => "mcp_status",
        ControlRequestBody::GetContextUsage => "get_context_usage",
    }
}

#[allow(clippy::too_many_arguments)]
async fn drive(
    mut transport: impl Transport + 'static,
    mut read_stream: futures::stream::BoxStream<'static, Result<Value>>,
    mut outbound_rx: mpsc::UnboundedReceiver<WriteCommand>,
    outbound_tx: mpsc::UnboundedSender<WriteCommand>,
    pending: PendingMap,
    inflight: InflightMap,
    messages_tx: mpsc::UnboundedSender<Result<Value>>,
    handlers: Arc<QueryHandlers>,
    last_error_result_text: Arc<StdMutex<Option<String>>>,
) {
    loop {
        tokio::select! {
            command = outbound_rx.recv() => {
                match command {
                    Some(WriteCommand::Line(line)) => {
                        let _ = transport.write_line(&line).await;
                    }
                    Some(WriteCommand::EndInput) => {
                        let _ = transport.end_input().await;
                    }
                    Some(WriteCommand::Close(ack)) => {
                        for task in inflight.lock().unwrap_or_else(std::sync::PoisonError::into_inner).drain() {
                            task.1.abort();
                        }
                        let result = transport.close().await;
                        let _ = ack.send(result);
                        return;
                    }
                    None => return,
                }
            }
            next = read_stream.next() => {
                match next {
                    Some(Ok(value)) => {
                        route_message(
                            value,
                            &pending,
                            &inflight,
                            &messages_tx,
                            &outbound_tx,
                            &handlers,
                            &last_error_result_text,
                        );
                    }
                    Some(Err(error)) => {
                        fail_all_pending(&pending, &error);
                        let enriched = enrich_process_error(error, &last_error_result_text);
                        let _ = messages_tx.send(Err(enriched));
                        // A read error doesn't imply the child already
                        // exited (unlike a clean EOF, where
                        // `read_messages()` has already reaped it) —
                        // close explicitly so an error mid-stream never
                        // leaks the subprocess.
                        let _ = transport.close().await;
                        return;
                    }
                    None => {
                        // Clean EOF: `read_messages()` already reaped
                        // the child before yielding it, but `close()`
                        // is still idempotent and cheap — call it so
                        // stdin/stdout handles are released deterministically
                        // rather than whenever `transport` happens to drop.
                        let _ = transport.close().await;
                        return;
                    }
                }
            }
        }
    }
}

fn fail_all_pending(pending: &PendingMap, error: &Error) {
    let mut guard = pending
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    for (_, sender) in guard.drain() {
        let _ = sender.send(Err(Error::ControlProtocol {
            message: format!("transport failed while awaiting response: {error}"),
        }));
    }
}

fn enrich_process_error(
    error: Error,
    last_error_result_text: &Arc<StdMutex<Option<String>>>,
) -> Error {
    let Error::Process { exit_code, .. } = &error else {
        return error;
    };
    let guard = last_error_result_text
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(text) = guard.as_ref() else {
        return error;
    };
    Error::Process {
        exit_code: *exit_code,
        stderr: format!("Claude Code returned an error result: {text}"),
    }
}

#[allow(clippy::too_many_arguments)]
fn route_message(
    value: Value,
    pending: &PendingMap,
    inflight: &InflightMap,
    messages_tx: &mpsc::UnboundedSender<Result<Value>>,
    outbound_tx: &mpsc::UnboundedSender<WriteCommand>,
    handlers: &Arc<QueryHandlers>,
    last_error_result_text: &Arc<StdMutex<Option<String>>>,
) {
    let msg_type = value
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default();

    match msg_type {
        "control_response" => route_control_response(&value, pending),
        "control_request" => {
            spawn_control_request_handler(
                value,
                Arc::clone(handlers),
                outbound_tx.clone(),
                inflight,
            );
        }
        "control_cancel_request" => {
            if let Some(cancel_id) = value.get("request_id").and_then(Value::as_str) {
                let mut guard = inflight
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if let Some(task) = guard.remove(cancel_id) {
                    task.abort();
                }
            }
        }
        "transcript_mirror" => {
            // Session-store live write path — deferred, see DEVIATIONS.md.
            // Recognized and dropped, never forwarded to consumers.
        }
        _ => {
            track_last_error_result_text(&value, msg_type, last_error_result_text);
            let _ = messages_tx.send(Ok(value));
        }
    }
}

fn track_last_error_result_text(
    value: &Value,
    msg_type: &str,
    last_error_result_text: &Arc<StdMutex<Option<String>>>,
) {
    let mut guard = last_error_result_text
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);

    if msg_type == "result" {
        if value.get("is_error").and_then(Value::as_bool) == Some(true) {
            let errors: Vec<String> = value
                .get("errors")
                .and_then(Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|item| item.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
            *guard = Some(if errors.is_empty() {
                value
                    .get("subtype")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown error")
                    .to_string()
            } else {
                errors.join("; ")
            });
        } else {
            *guard = None;
        }
    } else if !(msg_type == "system"
        && value.get("subtype").and_then(Value::as_str) == Some("session_state_changed"))
    {
        *guard = None;
    }
}

fn route_control_response(value: &Value, pending: &PendingMap) {
    let Some(response) = value.get("response") else {
        return;
    };
    let Some(request_id) = response.get("request_id").and_then(Value::as_str) else {
        return;
    };

    let sender = pending
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .remove(request_id);

    let Some(sender) = sender else {
        tracing::warn!(request_id, "unknown control_response request_id; dropping");
        return;
    };

    let result = if response.get("subtype").and_then(Value::as_str) == Some("error") {
        let message = response
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("unknown error")
            .to_string();
        Err(Error::ControlProtocol { message })
    } else {
        Ok(response
            .get("response")
            .cloned()
            .unwrap_or(Value::Object(serde_json::Map::new())))
    };

    let _ = sender.send(result);
}

fn spawn_control_request_handler(
    value: Value,
    handlers: Arc<QueryHandlers>,
    outbound_tx: mpsc::UnboundedSender<WriteCommand>,
    inflight: &InflightMap,
) {
    let Ok(request) = serde_json::from_value::<InboundControlRequest>(value) else {
        return;
    };
    let request_id = request.request_id.clone();

    let task = tokio::spawn(async move {
        let response_body = match handle_inbound_control_request(request.request, &handlers).await {
            Ok(response) => ControlResponseBody::Success {
                request_id: request.request_id.clone(),
                response,
            },
            Err(error) => ControlResponseBody::Error {
                request_id: request.request_id.clone(),
                error: error.to_string(),
            },
        };
        let envelope = ControlResponseEnvelope::new(response_body);
        if let Ok(line) = serde_json::to_string(&envelope) {
            let _ = outbound_tx.send(WriteCommand::Line(line));
        }
    });

    inflight
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert(request_id, task);
}

async fn handle_inbound_control_request(
    body: InboundControlRequestBody,
    handlers: &QueryHandlers,
) -> Result<Value> {
    match body {
        InboundControlRequestBody::CanUseTool {
            tool_name,
            input,
            permission_suggestions,
            blocked_path,
            decision_reason,
            title,
            display_name,
            description,
            tool_use_id,
            agent_id,
        } => {
            let Some(handler) = handlers.can_use_tool.as_ref() else {
                return Err(Error::ControlProtocol {
                    message: "canUseTool callback is not provided".to_string(),
                });
            };
            let mut context = serde_json::Map::new();
            if let Some(suggestions) = permission_suggestions {
                context.insert("permission_suggestions".to_string(), suggestions);
            }
            insert_opt_string(&mut context, "blocked_path", blocked_path);
            insert_opt_string(&mut context, "decision_reason", decision_reason);
            insert_opt_string(&mut context, "title", title);
            insert_opt_string(&mut context, "display_name", display_name);
            insert_opt_string(&mut context, "description", description);
            insert_opt_string(&mut context, "tool_use_id", tool_use_id);
            insert_opt_string(&mut context, "agent_id", agent_id);
            handler(tool_name, input, Value::Object(context)).await
        }
        InboundControlRequestBody::HookCallback {
            callback_id,
            input,
            tool_use_id,
        } => {
            let Some(handler) = handlers.hook_callbacks.get(&callback_id) else {
                return Err(Error::ControlProtocol {
                    message: format!("no hook callback found for id: {callback_id}"),
                });
            };
            handler(input, tool_use_id).await
        }
        InboundControlRequestBody::McpMessage {
            server_name,
            message,
        } => {
            // Upstream: an unrecognized server name is a JSON-RPC error
            // *inside* a successful control response, not a
            // control-protocol-level error — confirmed against
            // `_internal/query.py`'s `_handle_sdk_mcp_request`, which
            // never raises for this case (see `DEVIATIONS.md`).
            let mcp_response = if let Some(handler) = handlers.sdk_mcp_servers.get(&server_name) {
                handler(message).await?
            } else {
                let id = message.get("id").cloned().unwrap_or(Value::Null);
                serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "error": {
                        "code": -32601,
                        "message": format!("Server '{server_name}' not found"),
                    },
                })
            };
            Ok(serde_json::json!({ "mcp_response": mcp_response }))
        }
    }
}

/// Inserts `value` into `map` under `key` when present; a no-op on `None`.
fn insert_opt_string(map: &mut serde_json::Map<String, Value>, key: &str, value: Option<String>) {
    if let Some(value) = value {
        map.insert(key.to_string(), Value::String(value));
    }
}

#[cfg(test)]
#[path = "../../tests/fake_cli.rs"]
mod fake_cli;

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::fake_cli;
    use super::*;
    use crate::transport::subprocess::SubprocessTransport;
    use crate::types::options::ClaudeAgentOptions;

    fn transport_for(fake: &fake_cli::FakeCli) -> SubprocessTransport {
        let options = ClaudeAgentOptions::builder()
            .cli_path(fake.path.clone())
            .build();
        SubprocessTransport::new(options)
    }

    async fn wait_for_recording(path: &std::path::Path) -> String {
        let mut waited = Duration::ZERO;
        loop {
            let recorded = std::fs::read_to_string(path).unwrap_or_default();
            if !recorded.trim().is_empty() {
                return recorded;
            }
            assert!(
                waited <= Duration::from_secs(2),
                "SDK never wrote a response within 2s"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
            waited += Duration::from_millis(20);
        }
    }

    #[tokio::test]
    async fn routes_normal_messages_to_consumer() {
        let fake = fake_cli::scripted(
            &[
                r#"{"type":"assistant","message":{"model":"m","content":[]}}"#,
                r#"{"type":"result","subtype":"success","duration_ms":1,"duration_api_ms":1,"is_error":false,"num_turns":1,"session_id":"s"}"#,
            ],
            0,
        );
        let mut transport = transport_for(&fake);
        transport.connect().await.expect("connects");
        let query = Query::start(transport, QueryHandlers::default());

        let first = query
            .next_message()
            .await
            .expect("has message")
            .expect("ok");
        assert_eq!(first["type"], "assistant");
        let second = query
            .next_message()
            .await
            .expect("has message")
            .expect("ok");
        assert_eq!(second["type"], "result");

        query.close().await.expect("closes");
    }

    #[tokio::test]
    async fn control_request_resolves_on_success_response() {
        let fake = fake_cli::responding(
            &[(
                "interrupt",
                r#"{"type":"control_response","response":{"subtype":"success","request_id":"req_1_test","response":{}}}"#,
            )],
            &[],
        );
        let mut transport = transport_for(&fake);
        transport.connect().await.expect("connects");
        let query = Query::start_with(
            transport,
            QueryHandlers::default(),
            RequestIdGenerator::with_suffix("test"),
            Duration::from_secs(5),
        );

        query.interrupt().await.expect("interrupt succeeds");
    }

    #[tokio::test]
    async fn control_request_error_response_maps_to_control_protocol_error() {
        let fake = fake_cli::responding(
            &[(
                "interrupt",
                r#"{"type":"control_response","response":{"subtype":"error","request_id":"req_1_test","error":"interrupt not available"}}"#,
            )],
            &[],
        );
        let mut transport = transport_for(&fake);
        transport.connect().await.expect("connects");
        let query = Query::start_with(
            transport,
            QueryHandlers::default(),
            RequestIdGenerator::with_suffix("test"),
            Duration::from_secs(5),
        );

        let err = query.interrupt().await.expect_err("must error");
        assert!(matches!(err, Error::ControlProtocol { .. }));
        assert!(err.to_string().contains("interrupt not available"));
    }

    #[tokio::test]
    async fn control_request_times_out() {
        // Empty rule table: the fake reads stdin forever without ever
        // matching or responding, staying alive past the short timeout.
        let fake = fake_cli::responding(&[], &[]);
        let mut transport = transport_for(&fake);
        transport.connect().await.expect("connects");
        let query = Query::start_with(
            transport,
            QueryHandlers::default(),
            RequestIdGenerator::with_suffix("test"),
            Duration::from_millis(100),
        );

        let err = query.interrupt().await.expect_err("must time out");
        match &err {
            Error::ControlProtocol { message } => assert!(message.contains("timeout")),
            other => panic!("expected ControlProtocol timeout error, got {other:?}"),
        }

        query.close().await.expect("closes");
    }

    #[tokio::test]
    async fn answers_hook_callback_request() {
        let fake = fake_cli::scripted_and_recording(
            &[
                r#"{"type":"control_request","request_id":"cli_req_1","request":{"subtype":"hook_callback","callback_id":"hook_0","input":{"hook_event_name":"PreToolUse"}}}"#,
            ],
            0,
        );
        let mut hook_callbacks = HashMap::new();
        let handler: HookHandler = Arc::new(|_input, _tool_use_id| {
            Box::pin(async { Ok(serde_json::json!({"ok": true})) })
        });
        hook_callbacks.insert("hook_0".to_string(), handler);
        let handlers = QueryHandlers {
            can_use_tool: None,
            hook_callbacks,
            sdk_mcp_servers: HashMap::new(),
        };

        let mut transport = transport_for(&fake);
        transport.connect().await.expect("connects");
        let query = Query::start(transport, handlers);

        let recorded = wait_for_recording(&fake.stdin_recording_path).await;
        let line = recorded.lines().next().expect("SDK wrote a response line");
        let value: Value = serde_json::from_str(line).expect("valid json");
        assert_eq!(value["type"], "control_response");
        assert_eq!(value["response"]["subtype"], "success");
        assert_eq!(value["response"]["request_id"], "cli_req_1");
        assert_eq!(value["response"]["response"]["ok"], true);

        query.close().await.expect("closes");
    }

    #[tokio::test]
    async fn handler_error_produces_error_response() {
        let fake = fake_cli::scripted_and_recording(
            &[
                r#"{"type":"control_request","request_id":"cli_req_1","request":{"subtype":"hook_callback","callback_id":"missing_hook","input":{}}}"#,
                r#"{"type":"system","subtype":"init"}"#,
            ],
            0,
        );
        let mut transport = transport_for(&fake);
        transport.connect().await.expect("connects");
        let query = Query::start(transport, QueryHandlers::default());

        // Subsequent scripted message still flows despite the handler error.
        let next = query
            .next_message()
            .await
            .expect("has message")
            .expect("ok");
        assert_eq!(next["type"], "system");

        let recorded = wait_for_recording(&fake.stdin_recording_path).await;
        let line = recorded.lines().next().expect("SDK wrote a response line");
        let value: Value = serde_json::from_str(line).expect("valid json");
        assert_eq!(value["response"]["subtype"], "error");
        assert_eq!(value["response"]["request_id"], "cli_req_1");
        assert!(
            value["response"]["error"]
                .as_str()
                .unwrap()
                .contains("missing_hook")
        );

        query.close().await.expect("closes");
    }

    #[tokio::test]
    async fn unknown_control_response_id_is_ignored() {
        let fake = fake_cli::scripted(
            &[
                r#"{"type":"control_response","response":{"subtype":"success","request_id":"bogus","response":{}}}"#,
                r#"{"type":"system","subtype":"init"}"#,
            ],
            0,
        );
        let mut transport = transport_for(&fake);
        transport.connect().await.expect("connects");
        let query = Query::start(transport, QueryHandlers::default());

        let next = query
            .next_message()
            .await
            .expect("has message")
            .expect("ok");
        assert_eq!(next["type"], "system");

        query.close().await.expect("closes");
    }

    #[tokio::test]
    async fn user_message_line_shape() {
        let fake = fake_cli::recording(&[], 0);
        let mut transport = transport_for(&fake);
        transport.connect().await.expect("connects");
        let query = Query::start(transport, QueryHandlers::default());

        query
            .send_user_message(&UserContent::Text("hello".to_string()), "default")
            .await
            .expect("writes");
        query.end_input().await.expect("ends input");
        query.close().await.expect("closes");

        let recorded =
            std::fs::read_to_string(&fake.stdin_recording_path).expect("reads recording");
        let line = recorded.lines().next().expect("has line");
        let value: Value = serde_json::from_str(line).expect("valid json");
        assert_eq!(
            value,
            serde_json::json!({
                "type": "user",
                "message": {"role": "user", "content": "hello"},
                "parent_tool_use_id": null,
                "session_id": "default"
            })
        );
    }
}
