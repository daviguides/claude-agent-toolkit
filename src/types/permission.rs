//! Permission types.

use std::future::Future;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::types::session_store::BoxFuture;

/// Permission-prompt behavior for the session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PermissionMode {
    /// Prompt normally (CLI default).
    #[serde(rename = "default")]
    Default,
    /// Auto-accept file edits.
    #[serde(rename = "acceptEdits")]
    AcceptEdits,
    /// Planning mode: no tool execution.
    #[serde(rename = "plan")]
    Plan,
    /// Skip all permission checks.
    #[serde(rename = "bypassPermissions")]
    BypassPermissions,
    /// Don't prompt; deny anything not pre-approved.
    #[serde(rename = "dontAsk")]
    DontAsk,
    /// Automatic mode (CLI-defined heuristics).
    #[serde(rename = "auto")]
    Auto,
}

impl PermissionMode {
    /// Wire string used by the CLI flag and control protocol.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::AcceptEdits => "acceptEdits",
            Self::Plan => "plan",
            Self::BypassPermissions => "bypassPermissions",
            Self::DontAsk => "dontAsk",
            Self::Auto => "auto",
        }
    }
}

/// One tool permission rule.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionRuleValue {
    /// Tool the rule applies to.
    pub tool_name: String,
    /// Rule content (pattern), when applicable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rule_content: Option<String>,
}

/// A permission-rule update, as accepted by the CLI.
///
/// Every field beyond `type` is optional even within its "applicable"
/// variant — upstream's `PermissionUpdate.to_dict()` includes each one
/// only when set (`if self.rules is not None: ...`), so a value can
/// legitimately carry just a `type` and nothing else.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum PermissionUpdate {
    /// Add permission rules.
    #[serde(rename_all = "camelCase")]
    AddRules {
        /// Rules to add.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        rules: Option<Vec<PermissionRuleValue>>,
        /// Whether the rules allow, deny, or ask.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        behavior: Option<String>,
        /// Where the update is written.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        destination: Option<String>,
    },
    /// Replace permission rules.
    #[serde(rename_all = "camelCase")]
    ReplaceRules {
        /// Rules to replace with.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        rules: Option<Vec<PermissionRuleValue>>,
        /// Whether the rules allow, deny, or ask.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        behavior: Option<String>,
        /// Where the update is written.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        destination: Option<String>,
    },
    /// Remove permission rules.
    #[serde(rename_all = "camelCase")]
    RemoveRules {
        /// Rules to remove.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        rules: Option<Vec<PermissionRuleValue>>,
        /// Whether the rules allow, deny, or ask.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        behavior: Option<String>,
        /// Where the update is written.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        destination: Option<String>,
    },
    /// Change the permission mode.
    #[serde(rename_all = "camelCase")]
    SetMode {
        /// New permission mode wire string.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        mode: Option<String>,
        /// Where the update is written.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        destination: Option<String>,
    },
    /// Grant directory access.
    #[serde(rename_all = "camelCase")]
    AddDirectories {
        /// Directories to add.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        directories: Option<Vec<String>>,
        /// Where the update is written.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        destination: Option<String>,
    },
    /// Revoke directory access.
    #[serde(rename_all = "camelCase")]
    RemoveDirectories {
        /// Directories to remove.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        directories: Option<Vec<String>>,
        /// Where the update is written.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        destination: Option<String>,
    },
}

/// A tool invocation awaiting a permission decision.
///
/// Mirrors upstream's `(tool_name, input, ToolPermissionContext)`
/// callback signature, flattened into one struct. `signal` (a future
/// abort-signal placeholder, always `None` upstream today) is omitted.
#[derive(Debug, Clone)]
pub struct ToolPermissionRequest {
    /// Tool name, e.g. `"Bash"`.
    pub tool_name: String,
    /// Tool input as raw JSON.
    pub input: Value,
    /// CLI-suggested permission updates, when present.
    pub suggestions: Vec<PermissionUpdate>,
    /// Id of this specific tool call within its assistant message.
    pub tool_use_id: Option<String>,
    /// Sub-agent id, if running inside one.
    pub agent_id: Option<String>,
    /// File path that triggered the check, if applicable.
    pub blocked_path: Option<String>,
    /// Why this check was triggered (e.g. a `PreToolUse` hook
    /// returning `"ask"`).
    pub decision_reason: Option<String>,
    /// Full permission-prompt sentence, when present.
    pub title: Option<String>,
    /// Short noun phrase for the tool action.
    pub display_name: Option<String>,
    /// Human-readable subtitle for the permission UI.
    pub description: Option<String>,
}

/// Decision returned by a permission callback.
#[derive(Debug, Clone)]
pub enum PermissionResult {
    /// Allow the call, optionally rewriting its input.
    Allow {
        /// Replacement input; `None` keeps the original.
        updated_input: Option<Value>,
        /// Permission-rule updates to apply (e.g. "always allow").
        updated_permissions: Option<Vec<PermissionUpdate>>,
    },
    /// Deny the call.
    Deny {
        /// Reason shown to the model.
        message: String,
        /// Also interrupt the whole turn.
        interrupt: bool,
    },
}

impl PermissionResult {
    /// Encodes the decision into the control-response payload shape
    /// the CLI expects. Upstream always sends `updatedInput` on
    /// allow, falling back to `original_input` when the callback
    /// didn't rewrite it.
    #[must_use]
    pub fn to_wire(&self, original_input: &Value) -> Value {
        match self {
            Self::Allow {
                updated_input,
                updated_permissions,
            } => {
                let mut object = serde_json::Map::new();
                object.insert("behavior".to_string(), Value::String("allow".to_string()));
                object.insert(
                    "updatedInput".to_string(),
                    updated_input
                        .clone()
                        .unwrap_or_else(|| original_input.clone()),
                );
                if let Some(permissions) = updated_permissions {
                    let encoded = permissions
                        .iter()
                        .map(|permission| serde_json::to_value(permission).unwrap_or(Value::Null))
                        .collect();
                    object.insert("updatedPermissions".to_string(), Value::Array(encoded));
                }
                Value::Object(object)
            }
            Self::Deny { message, interrupt } => {
                let mut object = serde_json::Map::new();
                object.insert("behavior".to_string(), Value::String("deny".to_string()));
                object.insert("message".to_string(), Value::String(message.clone()));
                if *interrupt {
                    object.insert("interrupt".to_string(), Value::Bool(true));
                }
                Value::Object(object)
            }
        }
    }
}

/// Tool-permission callback: decides whether a tool call may proceed.
pub type CanUseToolCallback =
    Arc<dyn Fn(ToolPermissionRequest) -> BoxFuture<'static, PermissionResult> + Send + Sync>;

/// Wraps a plain async closure into a [`CanUseToolCallback`].
#[must_use]
pub fn can_use_tool_callback<F, Fut>(callback: F) -> CanUseToolCallback
where
    F: Fn(ToolPermissionRequest) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = PermissionResult> + Send + 'static,
{
    Arc::new(move |request| Box::pin(callback(request)))
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case(PermissionMode::Default, "default")]
    #[case(PermissionMode::AcceptEdits, "acceptEdits")]
    #[case(PermissionMode::Plan, "plan")]
    #[case(PermissionMode::BypassPermissions, "bypassPermissions")]
    #[case(PermissionMode::DontAsk, "dontAsk")]
    #[case(PermissionMode::Auto, "auto")]
    fn permission_mode_as_str(#[case] mode: PermissionMode, #[case] expected: &str) {
        assert_eq!(mode.as_str(), expected);
    }

    #[rstest]
    #[case(PermissionMode::Default, "\"default\"")]
    #[case(PermissionMode::AcceptEdits, "\"acceptEdits\"")]
    #[case(PermissionMode::Plan, "\"plan\"")]
    #[case(PermissionMode::BypassPermissions, "\"bypassPermissions\"")]
    #[case(PermissionMode::DontAsk, "\"dontAsk\"")]
    #[case(PermissionMode::Auto, "\"auto\"")]
    fn permission_mode_serde_roundtrip(#[case] mode: PermissionMode, #[case] wire: &str) {
        let json = serde_json::to_string(&mode).expect("serializes");
        assert_eq!(json, wire);
        let parsed: PermissionMode = serde_json::from_str(&json).expect("deserializes");
        assert_eq!(parsed, mode);
    }

    #[test]
    fn permission_allow_maps_to_behavior_allow_with_original_input() {
        let result = PermissionResult::Allow {
            updated_input: None,
            updated_permissions: None,
        };
        let wire = result.to_wire(&serde_json::json!({"command": "ls"}));
        assert_eq!(
            wire,
            serde_json::json!({"behavior": "allow", "updatedInput": {"command": "ls"}})
        );
    }

    #[test]
    fn permission_allow_uses_updated_input_when_given() {
        let result = PermissionResult::Allow {
            updated_input: Some(serde_json::json!({"command": "ls -la"})),
            updated_permissions: None,
        };
        let wire = result.to_wire(&serde_json::json!({"command": "ls"}));
        assert_eq!(
            wire["updatedInput"],
            serde_json::json!({"command": "ls -la"})
        );
    }

    #[test]
    fn permission_deny_includes_message_and_interrupt() {
        let result = PermissionResult::Deny {
            message: "not allowed".to_string(),
            interrupt: true,
        };
        let wire = result.to_wire(&Value::Null);
        assert_eq!(
            wire,
            serde_json::json!({"behavior": "deny", "message": "not allowed", "interrupt": true})
        );
    }

    #[test]
    fn permission_deny_omits_interrupt_when_false() {
        let result = PermissionResult::Deny {
            message: "no".to_string(),
            interrupt: false,
        };
        let wire = result.to_wire(&Value::Null);
        assert!(wire.get("interrupt").is_none());
    }

    #[test]
    fn allow_with_updated_permissions_serializes_them() {
        let result = PermissionResult::Allow {
            updated_input: None,
            updated_permissions: Some(vec![PermissionUpdate::SetMode {
                mode: Some("acceptEdits".to_string()),
                destination: Some("session".to_string()),
            }]),
        };
        let wire = result.to_wire(&Value::Null);
        assert_eq!(
            wire["updatedPermissions"],
            serde_json::json!([{"type": "setMode", "mode": "acceptEdits", "destination": "session"}])
        );
    }

    #[rstest]
    #[case(
        PermissionUpdate::AddRules {
            rules: Some(vec![PermissionRuleValue { tool_name: "Bash".to_string(), rule_content: Some("ls:*".to_string()) }]),
            behavior: Some("allow".to_string()),
            destination: Some("session".to_string()),
        },
        serde_json::json!({"type": "addRules", "rules": [{"toolName": "Bash", "ruleContent": "ls:*"}], "behavior": "allow", "destination": "session"})
    )]
    #[case(
        PermissionUpdate::ReplaceRules { rules: None, behavior: None, destination: Some("userSettings".to_string()) },
        serde_json::json!({"type": "replaceRules", "destination": "userSettings"})
    )]
    #[case(
        PermissionUpdate::RemoveRules {
            rules: Some(vec![PermissionRuleValue { tool_name: "Bash".to_string(), rule_content: None }]),
            behavior: Some("deny".to_string()),
            destination: None,
        },
        serde_json::json!({"type": "removeRules", "rules": [{"toolName": "Bash"}], "behavior": "deny"})
    )]
    #[case(
        PermissionUpdate::SetMode { mode: Some("plan".to_string()), destination: None },
        serde_json::json!({"type": "setMode", "mode": "plan"})
    )]
    #[case(
        PermissionUpdate::AddDirectories { directories: Some(vec!["/tmp".to_string()]), destination: Some("localSettings".to_string()) },
        serde_json::json!({"type": "addDirectories", "directories": ["/tmp"], "destination": "localSettings"})
    )]
    #[case(
        PermissionUpdate::RemoveDirectories { directories: Some(vec!["/tmp".to_string()]), destination: None },
        serde_json::json!({"type": "removeDirectories", "directories": ["/tmp"]})
    )]
    fn permission_update_serde_matches_wire(#[case] update: PermissionUpdate, #[case] wire: Value) {
        let json = serde_json::to_value(&update).expect("serializes");
        assert_eq!(json, wire);
        let parsed: PermissionUpdate = serde_json::from_value(json).expect("deserializes");
        assert_eq!(parsed, update);
    }

    #[test]
    fn permission_update_with_only_type_round_trips() {
        let update = PermissionUpdate::SetMode {
            mode: None,
            destination: None,
        };
        let json = serde_json::to_value(&update).expect("serializes");
        assert_eq!(json, serde_json::json!({"type": "setMode"}));
        let parsed: PermissionUpdate = serde_json::from_value(json).expect("deserializes");
        assert_eq!(parsed, update);
    }

    fn assert_send_sync<T: Send + Sync>() {}

    #[test]
    fn can_use_tool_callback_is_send_sync() {
        assert_send_sync::<CanUseToolCallback>();
    }
}
