# Phase 8 — Permission Callback (`can_use_tool`) and Hooks

**Objective**: user-supplied async callbacks for (a) tool permission
decisions and (b) lifecycle hooks, wired through the Phase 5 control
protocol.

**Upstream sources of truth**:
- `reference/.../src/claude_agent_sdk/types.py` — `CanUseTool`,
  `PermissionResult*`, `HookMatcher`, `HookContext`, hook event names
- `reference/.../src/claude_agent_sdk/_internal/query.py` — how
  `can_use_tool` / `hook_callback` requests are answered on the wire,
  and how the `initialize` payload registers hooks

## Callback representation in Rust (fixed design)

Async closures stored as boxed trait objects:

```rust
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

/// Boxed future returned by user callbacks.
pub type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send>>;

/// Tool-permission callback.
pub type CanUseToolCallback = Arc<
    dyn Fn(ToolPermissionRequest) -> BoxFuture<PermissionResult> + Send + Sync,
>;

/// Hook callback.
pub type HookCallback = Arc<
    dyn Fn(HookInput) -> BoxFuture<HookOutput> + Send + Sync,
>;
```

Ergonomics helper so users can pass plain `async fn`-like closures:

```rust
impl ClaudeAgentOptionsBuilder {
    /// Registers the tool-permission callback.
    pub fn can_use_tool<F, Fut>(mut self, callback: F) -> Self
    where
        F: Fn(ToolPermissionRequest) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = PermissionResult> + Send + 'static,
    {
        self.can_use_tool = Some(Arc::new(move |req| Box::pin(callback(req))));
        self
    }
}
```

(Note: adding callback fields makes `ClaudeAgentOptions` non-`PartialEq`
and its `Debug` must skip callbacks — implement `Debug` manually or
wrap callbacks in a newtype with a manual `Debug`. Fixed choice:
manual `Debug` printing `can_use_tool: <set|unset>`.)

## Deliverable A — permission types (extend `src/types/permission.rs`)

```rust
/// A tool invocation awaiting a permission decision.
#[derive(Debug, Clone)]
pub struct ToolPermissionRequest {
    /// Tool name, e.g. `"Bash"`.
    pub tool_name: String,
    /// Tool input as raw JSON.
    pub input: serde_json::Value,
    /// CLI-suggested permission updates, when present.
    pub suggestions: Option<serde_json::Value>,
}

/// Decision returned by a permission callback.
#[derive(Debug, Clone)]
pub enum PermissionResult {
    /// Allow the call, optionally rewriting its input.
    Allow {
        /// Replacement input; `None` keeps the original.
        updated_input: Option<serde_json::Value>,
        /// Permission-rule updates to apply (e.g. "always allow").
        /// Mirrors upstream `PermissionResultAllow.updated_permissions`.
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

/// A permission-rule update, as accepted by the CLI.
///
/// Upstream models this as the `PermissionUpdate` dataclass with a
/// `type` discriminator (`addRules`, `replaceRules`, `removeRules`,
/// `setMode`, `addDirectories`, `removeDirectories`) plus `rules`
/// (tool_name + rule_content pairs), `behavior`, `mode`,
/// `directories`, and `destination` (`userSettings`/`projectSettings`/
/// `localSettings`/`session`). ⚠️ VERIFY the exact variant set, field
/// names, and camelCase wire spellings in `types.py` (including its
/// `to_dict`-style serializer), then encode as:
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum PermissionUpdate {
    /// Add permission rules.
    #[serde(rename_all = "camelCase")]
    AddRules {
        rules: Vec<PermissionRuleValue>,
        behavior: String,
        destination: String,
    },
    /// Replace permission rules.
    #[serde(rename_all = "camelCase")]
    ReplaceRules {
        rules: Vec<PermissionRuleValue>,
        behavior: String,
        destination: String,
    },
    /// Remove permission rules.
    #[serde(rename_all = "camelCase")]
    RemoveRules {
        rules: Vec<PermissionRuleValue>,
        behavior: String,
        destination: String,
    },
    /// Change the permission mode.
    #[serde(rename_all = "camelCase")]
    SetMode { mode: String, destination: String },
    /// Grant directory access.
    #[serde(rename_all = "camelCase")]
    AddDirectories {
        directories: Vec<String>,
        destination: String,
    },
    /// Revoke directory access.
    #[serde(rename_all = "camelCase")]
    RemoveDirectories {
        directories: Vec<String>,
        destination: String,
    },
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
```

Also type the incoming `permission_suggestions` on
`ToolPermissionRequest` as `Option<Vec<PermissionUpdate>>` instead of
raw `Value` (they are the same shape — ⚠️ VERIFY).

Wire encoding of the decision inside the control response payload
(⚠️ VERIFY field names in `_internal/query.py`):

```json
{"behavior":"allow","updatedInput":{...}}
{"behavior":"deny","message":"...","interrupt":false}
```

Implement `PermissionResult::to_wire(&self, original_input: &Value) -> Value`
— note upstream sends `updatedInput` ALWAYS on allow (falling back to
the original input) — ⚠️ VERIFY and mirror.

## Deliverable B — hook types (`src/types/hook.rs`)

```rust
/// Lifecycle events that can be hooked.
/// (⚠️ VERIFY exact set + wire spellings against types.py; wire uses
/// PascalCase strings like "PreToolUse".)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HookEvent {
    PreToolUse,
    PostToolUse,
    UserPromptSubmit,
    Stop,
    SubagentStop,
    PreCompact,
}

impl HookEvent {
    /// Wire string, e.g. `"PreToolUse"`.
    #[must_use]
    pub fn as_str(self) -> &'static str { /* match */ }
}

/// One hook registration: optional tool-name matcher + callbacks.
#[derive(Clone)]
pub struct HookMatcher {
    /// Matcher expression (tool name / pattern), when applicable.
    pub matcher: Option<String>,
    /// Callbacks fired for this matcher.
    pub hooks: Vec<HookCallback>,
}

/// Input delivered to a hook callback (raw payload + context).
#[derive(Debug, Clone)]
pub struct HookInput {
    /// Raw hook payload from the CLI.
    pub payload: serde_json::Value,
}

/// Output of a hook callback, serialized into the control response.
///
/// TYPED, mirroring upstream `HookJSONOutput` (⚠️ VERIFY field set and
/// camelCase spellings in `types.py`). `extra` keeps forward-compat
/// with fields this struct does not model yet.
#[derive(Debug, Clone, Default, Serialize)]
pub struct HookOutput {
    /// `"block"` to block the action; `None` for no opinion.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decision: Option<String>,
    /// Message injected into the conversation as a system message.
    #[serde(rename = "systemMessage", skip_serializing_if = "Option::is_none")]
    pub system_message: Option<String>,
    /// Event-specific structured output (e.g. PreToolUse permission
    /// decision). Kept as raw JSON — its shape varies per event.
    #[serde(rename = "hookSpecificOutput", skip_serializing_if = "Option::is_none")]
    pub hook_specific_output: Option<serde_json::Value>,
    /// Any additional upstream fields (flattened into the response).
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}
```

Options gains: `pub hooks: HashMap<HookEvent, Vec<HookMatcher>>` (plus
builder method `hook(event, matcher)` that appends).

### Hook registration flow (implement exactly)

1. At `ClaudeClient::connect`, walk `options.hooks`; assign each
   callback a stable id `format!("hook_{i}")` in registration order;
   store `HashMap<String, HookCallback>` into `QueryHandlers.hook_callbacks`.
2. Build the initialize payload (⚠️ VERIFY shape in `_internal/query.py`):

```json
{"subtype":"initialize","hooks":{
  "PreToolUse":[{"matcher":"Bash","hookCallbackIds":["hook_0"]}]
}}
```

3. When a `hook_callback` control request arrives with `callback_id`,
   look up and run the callback; respond success with
   `HookOutput.payload` (or error response if the callback panics —
   catch with `tokio::task::spawn` + join-error mapping).

### can_use_tool flow

`can_use_tool` control request → run callback → respond success with
`PermissionResult::to_wire(...)`. When NO callback is registered but
the CLI asks (⚠️ VERIFY upstream): respond with an error control
response mirroring upstream's message.

Interaction with flags (⚠️ VERIFY in subprocess_cli/client): upstream
requires `can_use_tool` to be used with streaming mode and sets
`--permission-prompt-tool stdio` (or similar) automatically — read the
upstream code, replicate the flag injection and the validation errors
(e.g. conflict with a user-set `permission_prompt_tool_name`).

## Tests (extend `tests/client_test.rs` or new `tests/callbacks_test.rs`, write FIRST)

Permission:
1. `permission_allow_maps_to_behavior_allow` — unit: `to_wire` output
   JSON equality (with and without `updated_input`).
2. `permission_deny_includes_message_and_interrupt` — unit.
3. `can_use_tool_callback_answers_cli_request` — responding fake emits
   a `can_use_tool` request; callback allows; recorded control response
   has `"behavior":"allow"` and matching request id.
4. `deny_result_is_sent_when_callback_denies`.
5. `missing_callback_yields_error_response` — no callback registered →
   recorded error control response.

Hooks:
6. `hook_ids_are_assigned_in_registration_order` — unit on the
   registration builder: two matchers, three callbacks → `hook_0..2`
   and initialize payload structure matches the expected JSON exactly.
7. `initialize_payload_contains_hooks` — integration: recorded
   initialize line contains the `hooks` object.
8. `hook_callback_is_invoked_and_response_forwarded` — fake emits
   `hook_callback` with `callback_id":"hook_0"`; callback returns
   `{"decision":"block","reason":"nope"}`; recorded response carries it.
9. `unknown_callback_id_yields_error_response`.
10. `callbacks_are_send_sync` — compile-time assertion test.
11. `permission_update_serde_matches_wire` — rstest over all
    `PermissionUpdate` variants: serialized JSON uses the camelCase
    `type` tags and field names (compare against `serde_json::json!`
    literals taken from upstream).
12. `allow_with_updated_permissions_serializes_them` — `to_wire`
    output contains `updatedPermissions` array (⚠️ VERIFY key name).
13. `hook_output_serializes_typed_fields` — decision + systemMessage +
    hookSpecificOutput all present with camelCase keys; `Default`
    serializes to `{}`.
14. `hook_output_extra_fields_flatten` — an `extra` entry appears at
    the top level of the JSON.

## Acceptance Gate

```bash
cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test && cargo doc --no-deps
```

## Commits

1. `phase-8: permission types + wire mapping (tests first)`
2. `phase-8: hook types + registration (tests first)`
3. `phase-8: callback routing through query actor (green)`
