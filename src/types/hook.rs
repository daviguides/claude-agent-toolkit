//! Hook types: lifecycle callbacks fired by the CLI during a session.

use std::future::Future;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::types::session_store::BoxFuture;

/// Lifecycle events that can be hooked. Wire strings are `PascalCase`
/// (e.g. `"PreToolUse"`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum HookEvent {
    /// Before a tool call executes.
    PreToolUse,
    /// After a tool call executes successfully.
    PostToolUse,
    /// After a tool call fails.
    PostToolUseFailure,
    /// When the user submits a prompt.
    UserPromptSubmit,
    /// When the main agent stops.
    Stop,
    /// When a sub-agent stops.
    SubagentStop,
    /// Before context compaction.
    PreCompact,
    /// A CLI notification.
    Notification,
    /// When a sub-agent starts.
    SubagentStart,
    /// A permission request is being evaluated.
    PermissionRequest,
}

/// Every [`HookEvent`] variant, in a fixed order. Used to assign
/// `hook_{i}` callback ids deterministically (`HashMap` iteration
/// order is unspecified — see `DEVIATIONS.md`).
pub const ALL_HOOK_EVENTS: &[HookEvent] = &[
    HookEvent::PreToolUse,
    HookEvent::PostToolUse,
    HookEvent::PostToolUseFailure,
    HookEvent::UserPromptSubmit,
    HookEvent::Stop,
    HookEvent::SubagentStop,
    HookEvent::PreCompact,
    HookEvent::Notification,
    HookEvent::SubagentStart,
    HookEvent::PermissionRequest,
];

impl HookEvent {
    /// Wire string, e.g. `"PreToolUse"`.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PreToolUse => "PreToolUse",
            Self::PostToolUse => "PostToolUse",
            Self::PostToolUseFailure => "PostToolUseFailure",
            Self::UserPromptSubmit => "UserPromptSubmit",
            Self::Stop => "Stop",
            Self::SubagentStop => "SubagentStop",
            Self::PreCompact => "PreCompact",
            Self::Notification => "Notification",
            Self::SubagentStart => "SubagentStart",
            Self::PermissionRequest => "PermissionRequest",
        }
    }
}

/// Context passed to every hook callback alongside its input. `signal`
/// is a placeholder for future abort-signal support (upstream always
/// sends `{"signal": None}` today).
#[derive(Debug, Clone, Copy, Default)]
pub struct HookContext {
    _private: (),
}

/// Output of a hook callback, serialized into the control response.
///
/// Mirrors upstream `HookJSONOutput`'s synchronous-output shape.
/// `extra` keeps forward-compat with fields not modeled here (and, in
/// Rust, needs no `async_`/`continue_` keyword-avoidance renaming the
/// way upstream Python does — callers use the CLI's real field names
/// directly, e.g. `extra.insert("continue".into(), json!(false))`).
#[derive(Debug, Clone, Default, Serialize)]
pub struct HookOutput {
    /// `"block"` to block the action; `None` for no opinion.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decision: Option<String>,
    /// Message injected into the conversation as a system message.
    #[serde(rename = "systemMessage", skip_serializing_if = "Option::is_none")]
    pub system_message: Option<String>,
    /// Event-specific structured output (e.g. `PreToolUse` permission
    /// decision). Kept as raw JSON — its shape varies per event.
    #[serde(rename = "hookSpecificOutput", skip_serializing_if = "Option::is_none")]
    pub hook_specific_output: Option<Value>,
    /// Any additional upstream fields (flattened into the response).
    #[serde(flatten)]
    pub extra: serde_json::Map<String, Value>,
}

/// Hook callback: receives the raw event payload, the tool use id (if
/// any), and a context, and returns structured output.
pub type HookCallback =
    Arc<dyn Fn(Value, Option<String>, HookContext) -> BoxFuture<'static, HookOutput> + Send + Sync>;

/// Wraps a plain async closure into a [`HookCallback`].
#[must_use]
pub fn hook_callback<F, Fut>(callback: F) -> HookCallback
where
    F: Fn(Value, Option<String>, HookContext) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = HookOutput> + Send + 'static,
{
    Arc::new(move |payload, tool_use_id, context| Box::pin(callback(payload, tool_use_id, context)))
}

/// One hook registration: optional tool-name matcher + callbacks.
#[derive(Clone)]
pub struct HookMatcher {
    /// Matcher expression (tool name / pattern), when applicable.
    pub matcher: Option<String>,
    /// Callbacks fired for this matcher. Upstream dispatches all
    /// matchers registered on the same event concurrently, not
    /// sequentially — design each callback independently.
    pub hooks: Vec<HookCallback>,
    /// Per-matcher timeout in seconds, when set.
    pub timeout: Option<f64>,
}

impl std::fmt::Debug for HookMatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HookMatcher")
            .field("matcher", &self.matcher)
            .field("hooks", &format!("<{} callback(s)>", self.hooks.len()))
            .field("timeout", &self.timeout)
            .finish()
    }
}

impl HookMatcher {
    /// Creates a matcher with no callbacks yet.
    #[must_use]
    pub fn new(matcher: Option<impl Into<String>>) -> Self {
        Self {
            matcher: matcher.map(Into::into),
            hooks: Vec::new(),
            timeout: None,
        }
    }

    /// Adds a callback to this matcher.
    #[must_use]
    pub fn with_hook(mut self, callback: HookCallback) -> Self {
        self.hooks.push(callback);
        self
    }

    /// Sets the per-matcher timeout in seconds.
    #[must_use]
    pub fn with_timeout(mut self, seconds: f64) -> Self {
        self.timeout = Some(seconds);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hook_event_wire_strings() {
        assert_eq!(HookEvent::PreToolUse.as_str(), "PreToolUse");
        assert_eq!(HookEvent::PostToolUseFailure.as_str(), "PostToolUseFailure");
        assert_eq!(HookEvent::Notification.as_str(), "Notification");
        assert_eq!(HookEvent::SubagentStart.as_str(), "SubagentStart");
        assert_eq!(HookEvent::PermissionRequest.as_str(), "PermissionRequest");
    }

    #[test]
    fn all_hook_events_has_ten_entries() {
        assert_eq!(ALL_HOOK_EVENTS.len(), 10);
    }

    #[test]
    fn hook_output_default_serializes_to_empty_object() {
        let output = HookOutput::default();
        let json = serde_json::to_value(&output).expect("serializes");
        assert_eq!(json, serde_json::json!({}));
    }

    #[test]
    fn hook_output_serializes_typed_fields() {
        let output = HookOutput {
            decision: Some("block".to_string()),
            system_message: Some("blocked".to_string()),
            hook_specific_output: Some(serde_json::json!({"permissionDecision": "deny"})),
            extra: serde_json::Map::new(),
        };
        let json = serde_json::to_value(&output).expect("serializes");
        assert_eq!(
            json,
            serde_json::json!({
                "decision": "block",
                "systemMessage": "blocked",
                "hookSpecificOutput": {"permissionDecision": "deny"}
            })
        );
    }

    #[test]
    fn hook_output_extra_fields_flatten() {
        let mut extra = serde_json::Map::new();
        extra.insert("continue".to_string(), serde_json::json!(false));
        let output = HookOutput {
            extra,
            ..Default::default()
        };
        let json = serde_json::to_value(&output).expect("serializes");
        assert_eq!(json["continue"], serde_json::json!(false));
    }

    fn assert_send_sync<T: Send + Sync>() {}

    #[test]
    fn callbacks_are_send_sync() {
        assert_send_sync::<HookCallback>();
        assert_send_sync::<HookMatcher>();
    }
}
