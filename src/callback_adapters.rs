//! Adapts public `can_use_tool`/hook callback types (from
//! [`crate::types::permission`] / [`crate::types::hook`]) into the
//! low-level handler shapes [`crate::protocol::query::QueryHandlers`]
//! dispatches, and the upstream validation/warning logic around
//! `can_use_tool`.

use std::collections::HashMap;
use std::panic::AssertUnwindSafe;
use std::sync::Arc;

use futures::FutureExt;
use serde_json::Value;

use crate::error::{Error, Result};
use crate::mcp_server::SdkMcpServer;
use crate::protocol::query::{CanUseToolHandler, HookHandler, McpServerHandle, QueryHandlers};
use crate::types::hook::{ALL_HOOK_EVENTS, HookContext, HookEvent, HookMatcher};
use crate::types::mcp::{McpServerConfig, McpServersOption};
use crate::types::options::ClaudeAgentOptions;
use crate::types::permission::{PermissionUpdate, ToolPermissionRequest};

/// Result of registering `options.hooks`: the low-level handler table
/// plus the `hooks` object to embed in the `initialize` request.
pub(crate) struct HookRegistration {
    pub handlers: HashMap<String, HookHandler>,
    pub payload: Option<Value>,
}

/// Builds [`QueryHandlers`] and the `initialize` hooks payload from
/// `options`. Hook ids are assigned by walking [`ALL_HOOK_EVENTS`] in a
/// fixed order (never `HashMap` iteration order — see `DEVIATIONS.md`).
pub(crate) fn build_query_handlers(options: &ClaudeAgentOptions) -> (QueryHandlers, Option<Value>) {
    let can_use_tool = options
        .can_use_tool
        .clone()
        .map(adapt_can_use_tool_callback);
    let registration = build_hook_registration(options.hooks.as_ref());
    let sdk_mcp_servers = build_sdk_mcp_handlers(&options.mcp_servers);

    (
        QueryHandlers {
            can_use_tool,
            hook_callbacks: registration.handlers,
            sdk_mcp_servers,
        },
        registration.payload,
    )
}

/// Extracts every in-process (`Sdk`) server from `mcp_servers` into the
/// low-level handler table `Query` dispatches `mcp_message` control
/// requests against. External server configs (stdio/sse/http) and the
/// file-path form of `mcp_servers` never produce entries here — the
/// CLI itself connects to those directly.
fn build_sdk_mcp_handlers(mcp_servers: &McpServersOption) -> HashMap<String, McpServerHandle> {
    let McpServersOption::Servers(servers) = mcp_servers else {
        return HashMap::new();
    };
    servers
        .iter()
        .filter_map(|(name, config)| match config {
            McpServerConfig::Sdk(server) => Some((name.clone(), adapt_sdk_server(server.clone()))),
            McpServerConfig::Stdio { .. }
            | McpServerConfig::Sse { .. }
            | McpServerConfig::Http { .. } => None,
        })
        .collect()
}

fn adapt_sdk_server(server: SdkMcpServer) -> McpServerHandle {
    let server = Arc::new(server);
    Arc::new(move |message: Value| {
        let server = Arc::clone(&server);
        Box::pin(async move { Ok(server.handle_message(&message).await) })
    })
}

fn build_hook_registration(
    hooks: Option<&HashMap<HookEvent, Vec<HookMatcher>>>,
) -> HookRegistration {
    let mut handlers = HashMap::new();
    let mut payload_map = serde_json::Map::new();
    let mut next_id: u64 = 0;

    if let Some(hooks) = hooks {
        for event in ALL_HOOK_EVENTS {
            let Some(matchers) = hooks.get(event) else {
                continue;
            };
            if matchers.is_empty() {
                continue;
            }

            let mut matcher_configs = Vec::with_capacity(matchers.len());
            for matcher in matchers {
                let mut callback_ids = Vec::with_capacity(matcher.hooks.len());
                for callback in &matcher.hooks {
                    let id = format!("hook_{next_id}");
                    next_id += 1;
                    handlers.insert(id.clone(), adapt_hook_callback(Arc::clone(callback)));
                    callback_ids.push(Value::String(id));
                }

                let mut config = serde_json::Map::new();
                config.insert(
                    "matcher".to_string(),
                    matcher.matcher.clone().map_or(Value::Null, Value::String),
                );
                config.insert("hookCallbackIds".to_string(), Value::Array(callback_ids));
                if let Some(timeout) = matcher.timeout {
                    config.insert("timeout".to_string(), serde_json::json!(timeout));
                }
                matcher_configs.push(Value::Object(config));
            }
            payload_map.insert(event.as_str().to_string(), Value::Array(matcher_configs));
        }
    }

    HookRegistration {
        handlers,
        payload: if payload_map.is_empty() {
            None
        } else {
            Some(Value::Object(payload_map))
        },
    }
}

fn adapt_hook_callback(callback: crate::types::hook::HookCallback) -> HookHandler {
    Arc::new(move |input: Value, tool_use_id: Option<String>| {
        let callback = Arc::clone(&callback);
        Box::pin(async move {
            let outcome = AssertUnwindSafe(callback(input, tool_use_id, HookContext::default()))
                .catch_unwind()
                .await
                .map_err(|_| Error::ControlProtocol {
                    message: "hook callback panicked".to_string(),
                })?;
            serde_json::to_value(outcome).map_err(|source| Error::JsonDecode {
                line: String::new(),
                source,
            })
        })
    })
}

fn adapt_can_use_tool_callback(
    callback: crate::types::permission::CanUseToolCallback,
) -> CanUseToolHandler {
    Arc::new(move |tool_name: String, input: Value, context: Value| {
        let callback = Arc::clone(&callback);
        Box::pin(async move {
            let suggestions: Vec<PermissionUpdate> = context
                .get("permission_suggestions")
                .and_then(|value| serde_json::from_value(value.clone()).ok())
                .unwrap_or_default();
            let request = ToolPermissionRequest {
                tool_name,
                input: input.clone(),
                suggestions,
                tool_use_id: str_field(&context, "tool_use_id"),
                agent_id: str_field(&context, "agent_id"),
                blocked_path: str_field(&context, "blocked_path"),
                decision_reason: str_field(&context, "decision_reason"),
                title: str_field(&context, "title"),
                display_name: str_field(&context, "display_name"),
                description: str_field(&context, "description"),
            };

            let outcome = AssertUnwindSafe(callback(request))
                .catch_unwind()
                .await
                .map_err(|_| Error::ControlProtocol {
                    message: "can_use_tool callback panicked".to_string(),
                })?;
            Ok(outcome.to_wire(&input))
        })
    })
}

fn str_field(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(str::to_string)
}

/// Validates `can_use_tool`'s upstream mutual-exclusivity rules and
/// returns the effective `permission_prompt_tool_name` (auto-set to
/// `"stdio"` when `can_use_tool` is set and the caller didn't already
/// pick one).
///
/// # Errors
///
/// Returns [`Error::ControlProtocol`] when `can_use_tool` is combined
/// with a plain-string prompt, or with an explicit
/// `permission_prompt_tool_name`.
pub(crate) fn validate_can_use_tool(
    options: &ClaudeAgentOptions,
    prompt_is_string: bool,
) -> Result<Option<String>> {
    if options.can_use_tool.is_none() {
        return Ok(options.permission_prompt_tool_name.clone());
    }

    if prompt_is_string {
        return Err(Error::ControlProtocol {
            message: "can_use_tool callback requires streaming mode. Please provide the prompt \
                      as a streaming input instead of a string."
                .to_string(),
        });
    }

    if let Some(existing) = &options.permission_prompt_tool_name {
        return Err(Error::ControlProtocol {
            message: format!(
                "can_use_tool callback cannot be used with permission_prompt_tool_name \
                 ({existing}). Please use one or the other."
            ),
        });
    }

    warn_if_can_use_tool_shadowed(options);
    Ok(Some("stdio".to_string()))
}

/// Advisory-only: warns (via `tracing::warn!`) when other options would
/// auto-approve a tool call before `can_use_tool` is ever consulted.
/// Mirrors upstream's `_warn_if_can_use_tool_shadowed` (a Python
/// `warnings.warn`, which this crate has no equivalent registry for).
fn warn_if_can_use_tool_shadowed(options: &ClaudeAgentOptions) {
    use crate::types::options::SkillsOption;
    use crate::types::permission::PermissionMode;

    if options.permission_mode == Some(PermissionMode::BypassPermissions) {
        tracing::warn!(
            "can_use_tool will not be invoked: permission_mode 'bypassPermissions' \
             auto-approves every tool call (except explicit deny rules) before the \
             callback is consulted. To gate every tool call, use a PreToolUse hook instead."
        );
        return;
    }

    let mut allowed_tools = options.allowed_tools.clone();
    if matches!(&options.skills, Some(SkillsOption::All))
        && !allowed_tools.iter().any(|tool| tool == "Skill")
    {
        allowed_tools.push("Skill".to_string());
    }

    let mut shadowed = Vec::new();
    for entry in &allowed_tools {
        if let Some(tool) = whole_tool_allowed(entry)
            && !shadowed.contains(&tool)
        {
            shadowed.push(tool);
        }
    }

    if shadowed.is_empty() {
        return;
    }

    tracing::warn!(
        tools = %shadowed.join(", "),
        "can_use_tool will not be invoked for these tools: an allowed_tools entry that \
         allows a whole tool auto-approves it before the callback is consulted. To gate \
         every tool call, use a PreToolUse hook; or narrow the entry so calls fall through \
         to can_use_tool. Allow rules from settings files can also shadow the callback but \
         are not visible here."
    );
}

/// Returns the tool an `allowed_tools` entry allows outright, else
/// `None`. Mirrors upstream's `_whole_tool_allowed`: an entry allows a
/// whole tool when it has no `(...)` specifier (`"Read"`), or when the
/// specifier is empty or a lone wildcard (`"Read()"`, `"Read(*)"`).
fn whole_tool_allowed(entry: &str) -> Option<String> {
    if entry.trim().is_empty() {
        return None;
    }
    let Some(open_index) = entry.find('(') else {
        return Some(entry.to_string());
    };
    if open_index == 0 || !entry.ends_with(')') {
        return None;
    }
    let specifier = &entry[open_index + 1..entry.len() - 1];
    if specifier.is_empty() || specifier == "*" {
        Some(entry[..open_index].to_string())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::hook::{HookOutput, hook_callback};
    use crate::types::options::ToolsOption;
    use crate::types::permission::PermissionMode;

    #[test]
    fn hook_ids_are_assigned_in_registration_order() {
        let mut hooks: HashMap<HookEvent, Vec<HookMatcher>> = HashMap::new();
        hooks.insert(
            HookEvent::PreToolUse,
            vec![
                HookMatcher::new(Some("Bash"))
                    .with_hook(hook_callback(|_, _, _| async { HookOutput::default() })),
                HookMatcher::new(None::<String>)
                    .with_hook(hook_callback(|_, _, _| async { HookOutput::default() }))
                    .with_hook(hook_callback(|_, _, _| async { HookOutput::default() })),
            ],
        );

        let registration = build_hook_registration(Some(&hooks));
        assert_eq!(registration.handlers.len(), 3);
        assert!(registration.handlers.contains_key("hook_0"));
        assert!(registration.handlers.contains_key("hook_1"));
        assert!(registration.handlers.contains_key("hook_2"));

        let payload = registration.payload.expect("has payload");
        assert_eq!(
            payload["PreToolUse"][0],
            serde_json::json!({"matcher": "Bash", "hookCallbackIds": ["hook_0"]})
        );
        assert_eq!(
            payload["PreToolUse"][1],
            serde_json::json!({"matcher": null, "hookCallbackIds": ["hook_1", "hook_2"]})
        );
    }

    #[test]
    fn no_hooks_produce_no_payload() {
        let registration = build_hook_registration(None);
        assert!(registration.payload.is_none());
        assert!(registration.handlers.is_empty());
    }

    #[test]
    fn matcher_timeout_is_included_when_set() {
        let mut hooks: HashMap<HookEvent, Vec<HookMatcher>> = HashMap::new();
        hooks.insert(
            HookEvent::Stop,
            vec![
                HookMatcher::new(None::<String>)
                    .with_hook(hook_callback(|_, _, _| async { HookOutput::default() }))
                    .with_timeout(30.0),
            ],
        );
        let registration = build_hook_registration(Some(&hooks));
        assert_eq!(registration.payload.unwrap()["Stop"][0]["timeout"], 30.0);
    }

    #[test]
    fn validate_can_use_tool_none_passes_through_existing_tool_name() {
        let options = ClaudeAgentOptions::builder()
            .permission_prompt_tool_name("mcp__perms__ask")
            .build();
        let resolved = validate_can_use_tool(&options, true).expect("no can_use_tool set");
        assert_eq!(resolved.as_deref(), Some("mcp__perms__ask"));
    }

    #[test]
    fn validate_can_use_tool_rejects_string_prompt() {
        let options = ClaudeAgentOptions::builder()
            .can_use_tool(|_req| async {
                crate::types::permission::PermissionResult::Allow {
                    updated_input: None,
                    updated_permissions: None,
                }
            })
            .build();
        let err = validate_can_use_tool(&options, true).expect_err("must reject string prompt");
        assert!(matches!(err, Error::ControlProtocol { .. }));
    }

    #[test]
    fn validate_can_use_tool_rejects_conflicting_permission_prompt_tool_name() {
        let options = ClaudeAgentOptions::builder()
            .can_use_tool(|_req| async {
                crate::types::permission::PermissionResult::Allow {
                    updated_input: None,
                    updated_permissions: None,
                }
            })
            .permission_prompt_tool_name("mcp__perms__ask")
            .build();
        let err = validate_can_use_tool(&options, false).expect_err("must reject conflict");
        assert!(matches!(err, Error::ControlProtocol { .. }));
    }

    #[test]
    fn validate_can_use_tool_auto_sets_stdio() {
        let options = ClaudeAgentOptions::builder()
            .can_use_tool(|_req| async {
                crate::types::permission::PermissionResult::Allow {
                    updated_input: None,
                    updated_permissions: None,
                }
            })
            .build();
        let resolved = validate_can_use_tool(&options, false).expect("valid combination");
        assert_eq!(resolved.as_deref(), Some("stdio"));
    }

    #[test]
    fn whole_tool_allowed_recognizes_bare_and_wildcard_entries() {
        assert_eq!(whole_tool_allowed("Read"), Some("Read".to_string()));
        assert_eq!(whole_tool_allowed("Read()"), Some("Read".to_string()));
        assert_eq!(whole_tool_allowed("Read(*)"), Some("Read".to_string()));
        assert_eq!(whole_tool_allowed("Bash(ls:*)"), None);
        assert_eq!(whole_tool_allowed(""), None);
    }

    #[test]
    fn warn_if_can_use_tool_shadowed_does_not_panic_on_bypass_mode() {
        // Smoke test: just confirm this code path runs without
        // panicking; tracing output isn't asserted here.
        let options = ClaudeAgentOptions::builder()
            .permission_mode(PermissionMode::BypassPermissions)
            .can_use_tool(|_req| async {
                crate::types::permission::PermissionResult::Allow {
                    updated_input: None,
                    updated_permissions: None,
                }
            })
            .build();
        warn_if_can_use_tool_shadowed(&options);
    }

    #[test]
    fn build_query_handlers_wires_can_use_tool_and_hooks() {
        let mut hooks: HashMap<HookEvent, Vec<HookMatcher>> = HashMap::new();
        hooks.insert(
            HookEvent::PreToolUse,
            vec![
                HookMatcher::new(None::<String>)
                    .with_hook(hook_callback(|_, _, _| async { HookOutput::default() })),
            ],
        );
        let options = ClaudeAgentOptions {
            tools: None::<ToolsOption>,
            hooks: Some(hooks),
            can_use_tool: Some(crate::types::permission::can_use_tool_callback(
                |_req| async {
                    crate::types::permission::PermissionResult::Deny {
                        message: "no".to_string(),
                        interrupt: false,
                    }
                },
            )),
            ..ClaudeAgentOptions::default()
        };
        let (handlers, payload) = build_query_handlers(&options);
        assert!(handlers.can_use_tool.is_some());
        assert_eq!(handlers.hook_callbacks.len(), 1);
        assert!(payload.is_some());
    }
}
