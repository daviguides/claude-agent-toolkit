//! Control-protocol wire types: pure serde shapes for the
//! bidirectional request/response envelopes exchanged with the CLI.

use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Envelope for an SDK-initiated control request (SDK → CLI).
#[derive(Debug, Clone, Serialize)]
pub(crate) struct OutboundControlRequest {
    #[serde(rename = "type")]
    pub kind: String,
    pub request_id: String,
    pub request: ControlRequestBody,
}

impl OutboundControlRequest {
    pub fn new(request_id: impl Into<String>, request: ControlRequestBody) -> Self {
        Self {
            kind: "control_request".to_string(),
            request_id: request_id.into(),
            request,
        }
    }
}

/// Body of an SDK-initiated control request. Upstream implements 9
/// subtypes (`query.py`), not just the 4 the original phase-5 sketch
/// mentioned — see `DEVIATIONS.md`.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "subtype", rename_all = "snake_case")]
pub(crate) enum ControlRequestBody {
    /// Handshake: registers hooks/agents/skills for the session.
    Initialize {
        /// Hook matcher configuration, keyed by event name.
        #[serde(skip_serializing_if = "Option::is_none")]
        hooks: Option<Value>,
        /// Agent definitions, keyed by name.
        #[serde(skip_serializing_if = "Option::is_none")]
        agents: Option<Value>,
        /// Preset-prompt dynamic-section stripping flag.
        #[serde(
            rename = "excludeDynamicSections",
            skip_serializing_if = "Option::is_none"
        )]
        exclude_dynamic_sections: Option<bool>,
        /// Explicit skill allowlist (only sent when it's a concrete list).
        #[serde(skip_serializing_if = "Option::is_none")]
        skills: Option<Vec<String>>,
    },
    /// Interrupts the current turn.
    Interrupt,
    /// Changes the permission mode mid-session.
    SetPermissionMode {
        /// Wire string of the new [`crate::PermissionMode`].
        mode: String,
    },
    /// Changes the model mid-session.
    SetModel {
        /// New model id, or `None` to reset to the CLI default.
        model: Option<String>,
    },
    /// Rewinds tracked files to a prior user message (requires
    /// `enable_file_checkpointing`).
    RewindFiles {
        /// UUID of the user message to rewind to.
        user_message_id: String,
    },
    /// Reconnects a disconnected or failed MCP server.
    McpReconnect {
        /// Server name. Wire key is `serverName` (camelCase).
        #[serde(rename = "serverName")]
        server_name: String,
    },
    /// Enables or disables an MCP server.
    McpToggle {
        /// Server name. Wire key is `serverName` (camelCase).
        #[serde(rename = "serverName")]
        server_name: String,
        /// Whether the server should be enabled.
        enabled: bool,
    },
    /// Stops a running background task.
    StopTask {
        /// Task id from a `task_notification`/`task_updated` message.
        task_id: String,
    },
    /// Requests current MCP server connection status.
    McpStatus,
    /// Requests a breakdown of current context window usage.
    GetContextUsage,
}

/// One CLI-initiated control request the SDK must answer.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct InboundControlRequest {
    /// Correlates the eventual response.
    pub request_id: String,
    /// The request body.
    pub request: InboundControlRequestBody,
}

/// Body of a CLI-initiated control request. Exactly 3 subtypes are
/// dispatched by upstream's `_handle_control_request` — confirmed
/// exhaustive, see `DEVIATIONS.md`.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "subtype", rename_all = "snake_case")]
pub(crate) enum InboundControlRequestBody {
    /// A tool permission check the SDK's `can_use_tool` callback must
    /// answer.
    CanUseTool {
        /// Tool being invoked.
        tool_name: String,
        /// Tool input.
        input: Value,
        /// Permission suggestions from the CLI's own rule evaluation.
        #[serde(default)]
        permission_suggestions: Option<Value>,
        /// File path that triggered the check, if applicable.
        #[serde(default)]
        blocked_path: Option<String>,
        /// Why this check was triggered (e.g. a `PreToolUse` hook
        /// returning `"ask"`).
        #[serde(default)]
        decision_reason: Option<String>,
        /// Full permission-prompt sentence, when present.
        #[serde(default)]
        title: Option<String>,
        /// Short noun phrase for the tool action.
        #[serde(default)]
        display_name: Option<String>,
        /// Human-readable subtitle for the permission UI.
        #[serde(default)]
        description: Option<String>,
        /// Id of this specific tool call within its assistant message.
        #[serde(default)]
        tool_use_id: Option<String>,
        /// Sub-agent id, if running inside one.
        #[serde(default)]
        agent_id: Option<String>,
    },
    /// A registered hook callback the SDK must invoke and answer.
    HookCallback {
        /// Id assigned when the hook was registered during `initialize`.
        callback_id: String,
        /// Hook input payload.
        input: Value,
        /// Tool use id, when the hook fired for a tool lifecycle event.
        #[serde(default)]
        tool_use_id: Option<String>,
    },
    /// A JSON-RPC message for an in-process (SDK) MCP server.
    McpMessage {
        /// Target server name.
        server_name: String,
        /// Raw JSON-RPC message.
        message: Value,
    },
}

/// Envelope for a control response, either direction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ControlResponseEnvelope {
    #[serde(rename = "type")]
    pub kind: String,
    pub response: ControlResponseBody,
}

impl ControlResponseEnvelope {
    pub fn new(response: ControlResponseBody) -> Self {
        Self {
            kind: "control_response".to_string(),
            response,
        }
    }
}

/// Body of a control response.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "subtype", rename_all = "snake_case")]
pub(crate) enum ControlResponseBody {
    /// The paired request succeeded.
    Success {
        /// Id of the request this answers.
        request_id: String,
        /// Response payload (empty object when there is none).
        response: Value,
    },
    /// The paired request failed.
    Error {
        /// Id of the request this answers.
        request_id: String,
        /// Failure description.
        error: String,
    },
}

/// Generates unique-enough `request_id` values for correlating control
/// requests with their responses.
///
/// Not cryptographically random — ids only need to be unique within a
/// process for correlation and log readability, so no `rand`
/// dependency is pulled in (see `DEVIATIONS.md`). The suffix is
/// injectable so tests can pin deterministic ids (`req_1_test`,
/// `req_2_test`, ...).
pub(crate) struct RequestIdGenerator {
    counter: AtomicU64,
    suffix: String,
}

impl RequestIdGenerator {
    /// Creates a generator with a fixed suffix (e.g. `"test"` for
    /// deterministic tests).
    pub fn with_suffix(suffix: impl Into<String>) -> Self {
        Self {
            counter: AtomicU64::new(0),
            suffix: suffix.into(),
        }
    }

    /// Creates a generator with a process-unique-enough random suffix.
    #[must_use]
    pub fn new() -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.subsec_nanos())
            .unwrap_or_default();
        Self::with_suffix(format!("{nanos:x}"))
    }

    /// Returns the next `req_{counter}_{suffix}` id.
    pub fn next(&self) -> String {
        let n = self.counter.fetch_add(1, Ordering::SeqCst) + 1;
        format!("req_{n}_{}", self.suffix)
    }
}

impl Default for RequestIdGenerator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initialize_serializes_expected_shape_with_hooks() {
        let body = ControlRequestBody::Initialize {
            hooks: Some(serde_json::json!({
                "PreToolUse": [{"matcher": "Bash", "hookCallbackIds": ["hook_0"]}]
            })),
            agents: None,
            exclude_dynamic_sections: None,
            skills: None,
        };
        let envelope = OutboundControlRequest::new("req_0_test", body);
        let json = serde_json::to_value(&envelope).expect("serializes");
        assert_eq!(
            json,
            serde_json::json!({
                "type": "control_request",
                "request_id": "req_0_test",
                "request": {
                    "subtype": "initialize",
                    "hooks": {"PreToolUse": [{"matcher": "Bash", "hookCallbackIds": ["hook_0"]}]}
                }
            })
        );
    }

    #[test]
    fn interrupt_serializes_expected_shape() {
        let envelope = OutboundControlRequest::new("req_1_test", ControlRequestBody::Interrupt);
        let json = serde_json::to_value(&envelope).expect("serializes");
        assert_eq!(
            json,
            serde_json::json!({
                "type": "control_request",
                "request_id": "req_1_test",
                "request": {"subtype": "interrupt"}
            })
        );
    }

    #[test]
    fn set_permission_mode_serializes_expected_shape() {
        let envelope = OutboundControlRequest::new(
            "req_2_test",
            ControlRequestBody::SetPermissionMode {
                mode: "acceptEdits".to_string(),
            },
        );
        let json = serde_json::to_value(&envelope).expect("serializes");
        assert_eq!(
            json,
            serde_json::json!({
                "type": "control_request",
                "request_id": "req_2_test",
                "request": {"subtype": "set_permission_mode", "mode": "acceptEdits"}
            })
        );
    }

    #[test]
    fn set_model_serializes_expected_shape() {
        let envelope = OutboundControlRequest::new(
            "req_3_test",
            ControlRequestBody::SetModel {
                model: Some("claude-opus-4-8".to_string()),
            },
        );
        let json = serde_json::to_value(&envelope).expect("serializes");
        assert_eq!(
            json,
            serde_json::json!({
                "type": "control_request",
                "request_id": "req_3_test",
                "request": {"subtype": "set_model", "model": "claude-opus-4-8"}
            })
        );
    }

    #[test]
    fn mcp_reconnect_uses_camel_case_server_name_key() {
        let envelope = OutboundControlRequest::new(
            "req_4_test",
            ControlRequestBody::McpReconnect {
                server_name: "calc".to_string(),
            },
        );
        let json = serde_json::to_value(&envelope).expect("serializes");
        assert_eq!(
            json["request"],
            serde_json::json!({"subtype": "mcp_reconnect", "serverName": "calc"})
        );
    }

    #[test]
    fn mcp_toggle_uses_camel_case_server_name_key() {
        let envelope = OutboundControlRequest::new(
            "req_5_test",
            ControlRequestBody::McpToggle {
                server_name: "calc".to_string(),
                enabled: false,
            },
        );
        let json = serde_json::to_value(&envelope).expect("serializes");
        assert_eq!(
            json["request"],
            serde_json::json!({"subtype": "mcp_toggle", "serverName": "calc", "enabled": false})
        );
    }

    #[test]
    fn stop_task_serializes_expected_shape() {
        let envelope = OutboundControlRequest::new(
            "req_6_test",
            ControlRequestBody::StopTask {
                task_id: "task-1".to_string(),
            },
        );
        let json = serde_json::to_value(&envelope).expect("serializes");
        assert_eq!(
            json["request"],
            serde_json::json!({"subtype": "stop_task", "task_id": "task-1"})
        );
    }

    #[test]
    fn mcp_status_and_get_context_usage_have_no_extra_fields() {
        let status = OutboundControlRequest::new("req_7_test", ControlRequestBody::McpStatus);
        let usage = OutboundControlRequest::new("req_8_test", ControlRequestBody::GetContextUsage);
        assert_eq!(
            serde_json::to_value(&status).unwrap()["request"],
            serde_json::json!({"subtype": "mcp_status"})
        );
        assert_eq!(
            serde_json::to_value(&usage).unwrap()["request"],
            serde_json::json!({"subtype": "get_context_usage"})
        );
    }

    #[test]
    fn deserializes_can_use_tool_inbound_request() {
        let raw = serde_json::json!({
            "request_id": "cli_req_1",
            "request": {
                "subtype": "can_use_tool",
                "tool_name": "Bash",
                "input": {"command": "ls"},
                "permission_suggestions": null
            }
        });
        let request: InboundControlRequest = serde_json::from_value(raw).expect("deserializes");
        assert_eq!(request.request_id, "cli_req_1");
        let InboundControlRequestBody::CanUseTool {
            tool_name, input, ..
        } = request.request
        else {
            panic!("expected CanUseTool");
        };
        assert_eq!(tool_name, "Bash");
        assert_eq!(input["command"], "ls");
    }

    #[test]
    fn deserializes_hook_callback_inbound_request() {
        let raw = serde_json::json!({
            "request_id": "cli_req_2",
            "request": {
                "subtype": "hook_callback",
                "callback_id": "hook_0",
                "input": {"hook_event_name": "PreToolUse", "tool_name": "Bash", "tool_input": {"command": "ls"}}
            }
        });
        let request: InboundControlRequest = serde_json::from_value(raw).expect("deserializes");
        let InboundControlRequestBody::HookCallback { callback_id, .. } = request.request else {
            panic!("expected HookCallback");
        };
        assert_eq!(callback_id, "hook_0");
    }

    #[test]
    fn deserializes_mcp_message_inbound_request() {
        let raw = serde_json::json!({
            "request_id": "cli_req_3",
            "request": {
                "subtype": "mcp_message",
                "server_name": "calc",
                "message": {"jsonrpc": "2.0", "id": 1, "method": "tools/list", "params": {}}
            }
        });
        let request: InboundControlRequest = serde_json::from_value(raw).expect("deserializes");
        let InboundControlRequestBody::McpMessage {
            server_name,
            message,
        } = request.request
        else {
            panic!("expected McpMessage");
        };
        assert_eq!(server_name, "calc");
        assert_eq!(message["method"], "tools/list");
    }

    #[test]
    fn success_response_round_trips() {
        let envelope = ControlResponseEnvelope::new(ControlResponseBody::Success {
            request_id: "req_1_test".to_string(),
            response: serde_json::json!({}),
        });
        let json = serde_json::to_value(&envelope).expect("serializes");
        assert_eq!(
            json,
            serde_json::json!({
                "type": "control_response",
                "response": {"subtype": "success", "request_id": "req_1_test", "response": {}}
            })
        );
        let parsed: ControlResponseEnvelope = serde_json::from_value(json).expect("deserializes");
        assert!(matches!(
            parsed.response,
            ControlResponseBody::Success { .. }
        ));
    }

    #[test]
    fn error_response_round_trips() {
        let envelope = ControlResponseEnvelope::new(ControlResponseBody::Error {
            request_id: "req_1_test".to_string(),
            error: "interrupt not available".to_string(),
        });
        let json = serde_json::to_value(&envelope).expect("serializes");
        assert_eq!(
            json,
            serde_json::json!({
                "type": "control_response",
                "response": {"subtype": "error", "request_id": "req_1_test", "error": "interrupt not available"}
            })
        );
        let parsed: ControlResponseEnvelope = serde_json::from_value(json).expect("deserializes");
        assert!(matches!(parsed.response, ControlResponseBody::Error { .. }));
    }

    #[test]
    fn request_id_generator_produces_deterministic_sequence_with_fixed_suffix() {
        let generator = RequestIdGenerator::with_suffix("test");
        assert_eq!(generator.next(), "req_1_test");
        assert_eq!(generator.next(), "req_2_test");
        assert_eq!(generator.next(), "req_3_test");
    }

    #[test]
    fn request_id_generator_default_produces_unique_ids() {
        let generator = RequestIdGenerator::new();
        let first = generator.next();
        let second = generator.next();
        assert_ne!(first, second);
        assert!(first.starts_with("req_1_"));
        assert!(second.starts_with("req_2_"));
    }
}
