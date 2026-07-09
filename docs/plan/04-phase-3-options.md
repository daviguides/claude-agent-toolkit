# Phase 3 — Options and CLI Argument Builder

**Objective**: `ClaudeAgentOptions` (the unified configuration object)
plus a pure, unit-testable function that turns options into CLI args.

**Upstream sources of truth**:
- `reference/.../src/claude_agent_sdk/types.py` — `ClaudeAgentOptions`
  field list and defaults
- `reference/.../src/claude_agent_sdk/_internal/transport/subprocess_cli.py`
  — `_build_command()` flag mapping (THE authority for flag spellings)

## Field → flag mapping (sketch — ⚠️ VERIFY every row against `_build_command()`)

| Rust field | Type | CLI flag | Notes |
|---|---|---|---|
| `system_prompt` | `Option<SystemPrompt>` | `--system-prompt` / `--append-system-prompt` | enum: `Custom(String)` or `Preset { preset, append: Option<String> }` — check upstream shape |
| `allowed_tools` | `Vec<String>` | `--allowedTools a,b` | comma-joined; camelCase flag is upstream's, keep it |
| `disallowed_tools` | `Vec<String>` | `--disallowedTools a,b` | comma-joined |
| `max_turns` | `Option<u32>` | `--max-turns N` | |
| `model` | `Option<String>` | `--model` | |
| `permission_mode` | `Option<PermissionMode>` | `--permission-mode` | wire strings: `default`, `acceptEdits`, `plan`, `bypassPermissions` |
| `permission_prompt_tool_name` | `Option<String>` | `--permission-prompt-tool` | |
| `continue_conversation` | `bool` | `--continue` | flag only when true |
| `resume` | `Option<String>` | `--resume <session_id>` | |
| `settings` | `Option<String>` | `--settings` | |
| `add_dirs` | `Vec<PathBuf>` | `--add-dir <p>` repeated | one flag per dir |
| `mcp_servers` | `McpServers` | `--mcp-config <json>` | JSON `{"mcpServers": {...}}`; SDK (in-process) servers serialize as `{"type": "sdk", "name": ...}` — ⚠️ VERIFY |
| `include_partial_messages` | `bool` | `--include-partial-messages` | flag only when true |
| `fork_session` | `bool` | `--fork-session` | flag only when true |
| `agents` | `Option<HashMap<String, AgentDefinition>>` | `--agents <json>` | TYPED (see Deliverable C) — JSON object keyed by agent name; ⚠️ VERIFY field set in `types.py` `AgentDefinition` |
| `setting_sources` | `Option<Vec<String>>` | `--setting-sources a,b` | ⚠️ VERIFY spelling & join rule |
| `user` | `Option<String>` | ⚠️ VERIFY flag in `_build_command()` | exists in upstream options |
| `cwd` | `Option<PathBuf>` | (not a flag — subprocess working dir) | |
| `env` | `HashMap<String, String>` | (not a flag — subprocess env) | |
| `extra_args` | `HashMap<String, Option<String>>` | `--<key> [value]` | escape hatch for new flags |
| `max_buffer_size` | `Option<usize>` | (not a flag — reader limit) | default 1 MiB (⚠️ VERIFY constant) |
| `plugins` | `Vec<PluginConfig>` | `--plugins <json>`-style (⚠️ VERIFY exact flag name and JSON shape in `_build_command()`) | **REQUIRED for reference use cases** (foreman passes `plugins=[{...}]`); upstream type is `SdkPluginConfig` in `types.py` — mirror its fields exactly |
| `stderr` | `Option<StderrCallback>` | (not a flag — consumed by the Phase 4 transport, invoked once per stderr line) | **REQUIRED for reference use cases** (refiner and foreman capture stderr for error diagnostics) |
| `can_use_tool` | callback (deferred to Phase 8) | — | field exists but is added in Phase 8 |
| `hooks` | callback map (deferred to Phase 8) | — | added in Phase 8 |

### Reference use cases (do not regress these)

Three real projects define the minimum bar for this options struct —
their wrappers must be portable 1:1:

- `continuum/tools/orch/refiner` → `refiner/core/sdk_wrapper.py`
- `continuum/tools/orch/foreman` → `foreman/core/sdk_wrapper.py`
- `prisma/backend` → `src/prisma/agents/claude_runner.py`

Fields they exercise: `cwd`, `model`, `max_turns`, `permission_mode`,
`settings` (as a path AND as an inline JSON string — both are just the
string value, no special handling), `allowed_tools`, `add_dirs`,
`resume`, `include_partial_messages`, `system_prompt` (preset+append
and plain string), `plugins`, `stderr`.

Upstream fields not listed here that exist in `types.py` (e.g. `user`)
MUST be added too — walk the dataclass field by field and tick each off.

Always-present base args (⚠️ VERIFY): `--output-format stream-json --verbose`,
plus `--input-format stream-json` in streaming mode or `--print <prompt>`
in one-shot string mode. The base/mode args belong to the TRANSPORT
(Phase 4); this phase builds only the options-derived portion.

## Deliverable A — `src/types/permission.rs` (only the mode enum now)

```rust
//! Permission types.

use serde::{Deserialize, Serialize};

/// Permission-prompt behavior for the session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PermissionMode {
    /// Prompt normally (CLI default).
    #[serde(rename = "default")]
    Default,
    /// Auto-accept file edits.
    #[serde(rename = "acceptEdits")]
    AcceptEdits,
    /// Plan mode: no mutations.
    #[serde(rename = "plan")]
    Plan,
    /// Skip all permission checks.
    #[serde(rename = "bypassPermissions")]
    BypassPermissions,
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
        }
    }
}
```

## Deliverable B — `src/types/mcp.rs` (config variants only; SDK server body comes in Phase 9)

```rust
//! MCP server configuration.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// External MCP server configurations, keyed by server name.
pub type McpServers = HashMap<String, McpServerConfig>;

/// One MCP server entry in the configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum McpServerConfig {
    /// Subprocess (stdio) MCP server.
    Stdio {
        /// Executable to launch.
        command: String,
        /// Arguments.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        args: Vec<String>,
        /// Environment variables.
        #[serde(default, skip_serializing_if = "HashMap::is_empty")]
        env: HashMap<String, String>,
    },
    /// Server-sent-events MCP server.
    Sse {
        /// Endpoint URL.
        url: String,
        /// Extra headers.
        #[serde(default, skip_serializing_if = "HashMap::is_empty")]
        headers: HashMap<String, String>,
    },
    /// Streamable HTTP MCP server.
    Http {
        /// Endpoint URL.
        url: String,
        /// Extra headers.
        #[serde(default, skip_serializing_if = "HashMap::is_empty")]
        headers: HashMap<String, String>,
    },
    // NOTE: the in-process "sdk" variant is added in Phase 9; its
    // serialized form for --mcp-config is {"type":"sdk","name":...}.
    // ⚠️ VERIFY exact serialized shape in subprocess_cli.py.
}
```

(⚠️ VERIFY the exact tag values `stdio`/`sse`/`http` and whether the
`stdio` variant's `type` field is optional upstream — Python accepts
configs without `"type"` for stdio. If so, implement a custom
`Deserialize` that defaults missing `type` to stdio, and note it.)

## Deliverable C — `src/types/options.rs`

```rust
//! Unified configuration for queries and clients.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::types::mcp::McpServers;
use crate::types::permission::PermissionMode;

/// Default stdout line-buffer limit in bytes (⚠️ VERIFY vs upstream).
pub const DEFAULT_MAX_BUFFER_SIZE: usize = 1024 * 1024;

/// Callback invoked once per CLI stderr line.
///
/// Mirrors upstream `ClaudeAgentOptions.stderr`. Used by callers to
/// capture diagnostics (e.g. keep the last N lines for error reports).
pub type StderrCallback = Arc<dyn Fn(&str) + Send + Sync>;

/// A programmatic subagent definition.
///
/// Mirrors upstream `AgentDefinition` in `types.py` (⚠️ VERIFY the
/// exact field set and which are optional; expected as below).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentDefinition {
    /// When to use this agent (shown to the orchestrator).
    pub description: String,
    /// The agent's system prompt.
    pub prompt: String,
    /// Tools available to the agent; `None` inherits all.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<String>>,
    /// Model override for the agent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

/// A Claude Code plugin made available to the session.
///
/// Mirrors upstream `SdkPluginConfig` (⚠️ VERIFY field names and the
/// `type` tag values in `types.py`; expected: `{"type":"local","path":...}`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum PluginConfig {
    /// Plugin loaded from a local directory.
    Local {
        /// Path to the plugin directory.
        path: PathBuf,
    },
}

/// System prompt configuration.
#[derive(Debug, Clone, PartialEq)]
pub enum SystemPrompt {
    /// Replace the system prompt entirely.
    Custom(String),
    /// Use a named preset, optionally appending text.
    Preset {
        /// Preset name (e.g. `"claude_code"`).
        preset: String,
        /// Text appended after the preset.
        append: Option<String>,
    },
}

/// Configuration for [`query`](crate::query) and `ClaudeClient`.
///
/// Construct with [`ClaudeAgentOptions::builder()`]; `Default` gives
/// upstream-equivalent defaults.
#[derive(Clone, Default)]
#[non_exhaustive]
pub struct ClaudeAgentOptions {
    pub system_prompt: Option<SystemPrompt>,
    pub allowed_tools: Vec<String>,
    pub disallowed_tools: Vec<String>,
    pub max_turns: Option<u32>,
    pub model: Option<String>,
    pub permission_mode: Option<PermissionMode>,
    pub permission_prompt_tool_name: Option<String>,
    pub continue_conversation: bool,
    pub resume: Option<String>,
    pub settings: Option<String>,
    pub add_dirs: Vec<PathBuf>,
    pub mcp_servers: McpServers,
    pub include_partial_messages: bool,
    pub fork_session: bool,
    pub agents: Option<HashMap<String, AgentDefinition>>,
    pub setting_sources: Option<Vec<String>>,
    pub cwd: Option<PathBuf>,
    pub env: HashMap<String, String>,
    pub extra_args: HashMap<String, Option<String>>,
    pub max_buffer_size: Option<usize>,
    pub plugins: Vec<PluginConfig>,
    pub stderr: Option<StderrCallback>,
    pub user: Option<String>,
}
```

Every public field gets a `///` doc line (omitted above for brevity —
the executor MUST write them; `missing_docs = "warn"` + clippy gate
will catch omissions).

**Debug impl**: because `stderr` (and, from Phase 8, the other
callbacks) is a closure, `#[derive(Debug)]` is impossible — implement
`Debug` MANUALLY from this phase on, printing `stderr: <set|unset>`
for callback fields and normal values for the rest. `PartialEq` is NOT
derived (closures are not comparable); the `builder_sets_every_field`
test asserts field-by-field instead of whole-struct equality
(callbacks asserted via `is_some()`).

Builder gains, alongside the plain setters:

```rust
impl ClaudeAgentOptionsBuilder {
    /// Registers a callback invoked once per CLI stderr line.
    pub fn stderr<F>(mut self, callback: F) -> Self
    where
        F: Fn(&str) + Send + Sync + 'static,
    {
        self.stderr = Some(Arc::new(callback));
        self
    }

    /// Adds a plugin.
    pub fn plugin(mut self, plugin: PluginConfig) -> Self {
        self.plugins.push(plugin);
        self
    }
}
```

Builder: implement `ClaudeAgentOptions::builder()` returning
`ClaudeAgentOptionsBuilder` with one fluent method per field
(`fn model(mut self, model: impl Into<String>) -> Self`, etc.) and a
final infallible `build()`. Hand-write it (no derive-builder dep) —
mechanical, ~1 line per method.

## Deliverable D — arg builder (in `src/transport/subprocess.rs` later, but the FUNCTION is written and tested now in `options.rs`)

```rust
/// Converts options into the CLI flags they imply.
///
/// Base flags (`--output-format` etc.) and mode flags are appended by
/// the transport, not here.
#[must_use]
pub fn build_cli_args(options: &ClaudeAgentOptions) -> Vec<String> {
    // Straight-line implementation of the mapping table; guard-clause
    // style: for each Option/non-empty Vec, push flag(s).
    todo!()
}
```

## Tests (write FIRST)

In `options.rs` tests module — one test per mapping row, plus:

1. `default_options_produce_no_flags` — `build_cli_args(&Default::default())`
   is empty.
2. `allowed_tools_are_comma_joined` — `["Read","Bash"]` →
   contains `["--allowedTools", "Read,Bash"]` adjacent pair.
3. `permission_mode_uses_wire_string` — `AcceptEdits` →
   `["--permission-mode", "acceptEdits"]`.
4. `continue_conversation_is_a_bare_flag` — no value after `--continue`.
5. `add_dirs_repeat_the_flag` — two dirs → `--add-dir` appears twice.
6. `mcp_servers_serialize_as_mcp_config_json` — one stdio server →
   `--mcp-config` followed by JSON whose `mcpServers.<name>.command`
   round-trips.
7. `extra_args_with_value_and_without` — `{"foo": Some("1"), "bar": None}`
   → `--foo 1` and bare `--bar`.
8. `system_prompt_custom_vs_preset_append` — `Custom` → `--system-prompt`;
   `Preset{append: Some}` → `--append-system-prompt` (⚠️ VERIFY exact
   upstream semantics before writing this test).
9. `builder_sets_every_field` — build with all fields set, assert
   field-by-field (callbacks via `is_some()`).
10. `permission_mode_serde_roundtrip` — rstest over all 4 variants:
    `serde_json::to_string` → the wire string; back → equal.
11. `mcp_config_deserializes_stdio_without_type_tag` — only if the
    optional-tag behavior is confirmed upstream.
12. `plugins_serialize_into_cli_flag` — one `PluginConfig::Local` →
    the flag (⚠️ VERIFY spelling) appears with JSON whose `type` is
    `"local"` and `path` round-trips.
13. `empty_plugins_produce_no_flag` — default options → no plugins flag.
14. `stderr_callback_is_not_a_cli_flag` — options with `stderr` set →
    `build_cli_args` output identical to the same options without it.
15. `options_debug_marks_callbacks_without_printing_them` —
    `format!("{options:?}")` with `stderr` set contains `"set"` (or the
    chosen marker) and does not panic.
16. `plugin_config_serde_roundtrip` — `Local` variant JSON round-trip.
17. `agents_serialize_as_named_object` — one `AgentDefinition` keyed
    `"reviewer"` → `--agents` flag with JSON
    `{"reviewer":{"description":...,"prompt":...}}`; optional fields
    absent when `None`.
18. `agent_definition_serde_roundtrip`.

Register modules in `src/types.rs` (`pub mod mcp; pub mod options;
pub mod permission;`) and re-export the main names from `lib.rs`.

## Acceptance Gate

```bash
cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test && cargo doc --no-deps
```

## Commits

1. `phase-3: permission mode + mcp config types (tests first)`
2. `phase-3: options struct + builder (tests first)`
3. `phase-3: cli arg builder (green)`
