//! Unified configuration for queries and clients.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::types::mcp::{McpServersOption, PluginConfig};
use crate::types::permission::PermissionMode;
use crate::types::session_store::{SessionStore, SessionStoreFlushMode};

/// Default stdout line-buffer limit in bytes (upstream default: 1 MiB).
pub const DEFAULT_MAX_BUFFER_SIZE: usize = 1024 * 1024;

/// Default timeout for each `session_store` `load`/`list_subkeys` call
/// during resume materialization, in milliseconds.
pub const DEFAULT_LOAD_TIMEOUT_MS: u64 = 60_000;

/// The only beta feature the CLI recognizes today (see
/// <https://docs.anthropic.com/en/api/beta-headers>). `betas` is a raw
/// `Vec<String>` rather than a closed enum since this whitelist grows
/// over time and the wire format is just a comma-joined string.
pub const BETA_CONTEXT_1M: &str = "context-1m-2025-08-07";

/// Callback invoked once per CLI stderr line.
pub type StderrCallback = Arc<dyn Fn(&str) + Send + Sync>;

/// Filesystem settings source to load.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SettingSource {
    /// Global user settings (`~/.claude/settings.json`).
    #[serde(rename = "user")]
    User,
    /// Project settings (`.claude/settings.json`).
    #[serde(rename = "project")]
    Project,
    /// Local settings (`.claude/settings.local.json`).
    #[serde(rename = "local")]
    Local,
}

impl SettingSource {
    /// Wire string used by `--setting-sources`.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Project => "project",
            Self::Local => "local",
        }
    }
}

/// How much effort Claude puts into a response.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EffortLevel {
    /// Minimal thinking, fastest responses.
    #[serde(rename = "low")]
    Low,
    /// Moderate thinking.
    #[serde(rename = "medium")]
    Medium,
    /// Deep reasoning (default).
    #[serde(rename = "high")]
    High,
    /// Extended reasoning depth (falls back to `High` on unsupported models).
    #[serde(rename = "xhigh")]
    XHigh,
    /// Maximum effort.
    #[serde(rename = "max")]
    Max,
}

impl EffortLevel {
    /// Wire string used by `--effort`.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::XHigh => "xhigh",
            Self::Max => "max",
        }
    }
}

/// The base set of built-in tools available to the session.
#[derive(Debug, Clone, PartialEq)]
pub enum ToolsOption {
    /// Specific tool names. An empty list disables all built-in tools.
    Named(Vec<String>),
    /// All default Claude Code tools (`{"type":"preset","preset":"claude_code"}`).
    Preset,
}

/// Which skills to enable for the main session.
#[derive(Debug, Clone, PartialEq)]
pub enum SkillsOption {
    /// Enable every discovered skill.
    All,
    /// Enable only the listed skills.
    Named(Vec<String>),
}

/// System prompt configuration.
#[derive(Debug, Clone, PartialEq)]
pub enum SystemPrompt {
    /// Replace the system prompt entirely.
    Custom(String),
    /// Use a named preset, optionally appending text.
    Preset {
        /// Preset name (currently always `"claude_code"`).
        preset: String,
        /// Text appended after the preset.
        append: Option<String>,
        /// Strip per-user dynamic sections so the prompt stays static
        /// and cacheable across users.
        exclude_dynamic_sections: Option<bool>,
    },
    /// Load the system prompt from a file path.
    File {
        /// Path to the system prompt file.
        path: String,
    },
}

/// A programmatic subagent definition.
///
/// Mirrors upstream `AgentDefinition` in `types.py`. Not sent as a CLI
/// flag — delivered via the control protocol's `initialize` request
/// (Phase 5), so this type is a data shape only at this phase.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentDefinition {
    /// When to use this agent (shown to the orchestrator).
    pub description: String,
    /// The agent's system prompt.
    pub prompt: String,
    /// Tools available to the agent; `None` inherits all.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<String>>,
    /// Tools explicitly disallowed for the agent.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "disallowedTools"
    )]
    pub disallowed_tools: Option<Vec<String>>,
    /// Model alias (`"sonnet"`, `"opus"`, `"haiku"`, `"inherit"`) or a
    /// full model id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Skills enabled for the agent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skills: Option<Vec<String>>,
    /// Memory scope for the agent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory: Option<String>,
    /// MCP servers available to the agent (server name, or an inline
    /// `{name: config}` object). Kept as raw JSON — not otherwise
    /// modeled by this crate at this phase.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "mcpServers"
    )]
    pub mcp_servers: Option<Vec<Value>>,
    /// Prompt automatically sent when the agent starts.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "initialPrompt"
    )]
    pub initial_prompt: Option<String>,
    /// Maximum turns for the agent.
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "maxTurns")]
    pub max_turns: Option<u32>,
    /// Whether the agent runs in the background.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background: Option<bool>,
    /// Effort level, or a raw integer effort value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<AgentEffort>,
    /// Permission mode override for the agent.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "permissionMode"
    )]
    pub permission_mode: Option<PermissionMode>,
}

/// Agent effort: a named level, or a raw model-specific integer.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum AgentEffort {
    /// A named effort level.
    Level(EffortLevel),
    /// A raw, model-specific effort value.
    Raw(i64),
}

/// Network configuration for sandboxed command execution.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SandboxNetworkConfig {
    /// Domains sandboxed processes may access.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allowed_domains: Vec<String>,
    /// Domains always blocked, even if matched by `allowed_domains`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub denied_domains: Vec<String>,
    /// When true, only managed-settings `allowed_domains` are respected.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allow_managed_domains_only: Option<bool>,
    /// Unix socket paths accessible in the sandbox.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allow_unix_sockets: Vec<String>,
    /// Allow all Unix sockets (less secure).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allow_all_unix_sockets: Option<bool>,
    /// Allow binding to localhost ports (macOS only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allow_local_binding: Option<bool>,
    /// macOS XPC/Mach service names to allow.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allow_mach_lookup: Vec<String>,
    /// HTTP proxy port, if bringing your own proxy.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub http_proxy_port: Option<u16>,
    /// SOCKS5 proxy port, if bringing your own proxy.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub socks_proxy_port: Option<u16>,
}

/// Violations to ignore in a sandboxed session.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SandboxIgnoreViolations {
    /// File paths for which violations should be ignored.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub file: Vec<String>,
    /// Network hosts for which violations should be ignored.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub network: Vec<String>,
}

/// Sandbox settings controlling filesystem/network isolation for bash
/// commands. Filesystem and network restrictions themselves are
/// configured via permission rules, not here.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SandboxSettings {
    /// Enable bash sandboxing (macOS/Linux only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    /// Auto-approve bash commands when sandboxed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_allow_bash_if_sandboxed: Option<bool>,
    /// Commands that should run outside the sandbox.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub excluded_commands: Vec<String>,
    /// Allow commands to bypass the sandbox explicitly.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allow_unsandboxed_commands: Option<bool>,
    /// Network configuration for the sandbox.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub network: Option<SandboxNetworkConfig>,
    /// Violations to ignore.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ignore_violations: Option<SandboxIgnoreViolations>,
    /// Enable a weaker sandbox for unprivileged Docker environments
    /// (Linux only). Reduces security.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enable_weaker_nested_sandbox: Option<bool>,
}

/// Controls whether thinking text is returned summarized or omitted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThinkingDisplay {
    /// Return a summarized version of the thinking text.
    #[serde(rename = "summarized")]
    Summarized,
    /// Omit thinking text (signature-only).
    #[serde(rename = "omitted")]
    Omitted,
}

/// Controls Claude's thinking/reasoning behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ThinkingConfig {
    /// Claude decides when and how much to think.
    #[serde(rename = "adaptive")]
    Adaptive {
        /// How thinking text is returned.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        display: Option<ThinkingDisplay>,
    },
    /// Fixed thinking token budget.
    #[serde(rename = "enabled")]
    Enabled {
        /// Maximum tokens for thinking.
        budget_tokens: u32,
        /// How thinking text is returned.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        display: Option<ThinkingDisplay>,
    },
    /// No extended thinking.
    #[serde(rename = "disabled")]
    Disabled,
}

/// API-side task budget in tokens.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskBudget {
    /// Total token budget.
    pub total: u64,
}

/// Configuration for `query()` and `ClaudeClient` (added in later phases).
///
/// Construct with [`ClaudeAgentOptions::builder()`]; [`Default`] gives
/// upstream-equivalent defaults. `can_use_tool` and `hooks` are added
/// in Phase 8, once the hook I/O types they depend on exist.
// Mirrors upstream's flat `@dataclass ClaudeAgentOptions` exactly: each
// bool is an independent CLI flag, not combinable state — a state
// machine would misrepresent the domain.
#[allow(clippy::struct_excessive_bools)]
#[derive(Clone)]
#[non_exhaustive]
pub struct ClaudeAgentOptions {
    /// Base set of available built-in tools.
    pub tools: Option<ToolsOption>,
    /// Tool names auto-allowed without prompting for permission.
    pub allowed_tools: Vec<String>,
    /// System prompt configuration.
    pub system_prompt: Option<SystemPrompt>,
    /// MCP server configurations.
    pub mcp_servers: McpServersOption,
    /// When true, only use MCP servers passed via `mcp_servers`.
    pub strict_mcp_config: bool,
    /// Permission mode for the session.
    pub permission_mode: Option<PermissionMode>,
    /// Continue the most recent conversation instead of starting a new one.
    pub continue_conversation: bool,
    /// Session id to resume.
    pub resume: Option<String>,
    /// Use a specific session id instead of an auto-generated one.
    pub session_id: Option<String>,
    /// Maximum conversation turns before the query stops.
    pub max_turns: Option<u32>,
    /// Maximum budget in USD for the query.
    pub max_budget_usd: Option<f64>,
    /// Tool names disallowed even if otherwise allowed.
    pub disallowed_tools: Vec<String>,
    /// Claude model to use.
    pub model: Option<String>,
    /// Fallback model if the primary model is unavailable.
    pub fallback_model: Option<String>,
    /// Beta features to enable (see [`BETA_CONTEXT_1M`]).
    pub betas: Vec<String>,
    /// MCP tool name to route permission prompts through.
    pub permission_prompt_tool_name: Option<String>,
    /// Working directory for the session.
    pub cwd: Option<PathBuf>,
    /// Path to the Claude Code CLI executable.
    pub cli_path: Option<PathBuf>,
    /// Path to an additional settings JSON file.
    pub settings: Option<String>,
    /// Additional directories Claude can access.
    pub add_dirs: Vec<PathBuf>,
    /// Environment variables for the subprocess.
    pub env: HashMap<String, String>,
    /// Additional raw CLI arguments (escape hatch for new flags).
    pub extra_args: HashMap<String, Option<String>>,
    /// Maximum bytes to buffer when reading CLI stdout.
    pub max_buffer_size: Option<usize>,
    /// Callback invoked once per CLI stderr line.
    pub stderr: Option<StderrCallback>,
    /// User identifier the subprocess is spawned as (OS-level, not a
    /// CLI flag — consumed by the Phase 4 transport).
    pub user: Option<String>,
    /// Include partial/streaming message events in the output.
    pub include_partial_messages: bool,
    /// Include hook lifecycle events in the message stream.
    pub include_hook_events: bool,
    /// Fork resumed sessions to a new session id.
    pub fork_session: bool,
    /// Programmatically defined subagents, keyed by name.
    pub agents: Option<HashMap<String, AgentDefinition>>,
    /// Which filesystem settings sources to load.
    pub setting_sources: Option<Vec<SettingSource>>,
    /// Skills to enable for the main session.
    pub skills: Option<SkillsOption>,
    /// Sandbox settings for command execution isolation.
    pub sandbox: Option<SandboxSettings>,
    /// Plugins to load for this session.
    pub plugins: Vec<PluginConfig>,
    /// Deprecated: maximum thinking tokens. Prefer `thinking`.
    pub max_thinking_tokens: Option<u32>,
    /// Controls Claude's thinking/reasoning behavior.
    pub thinking: Option<ThinkingConfig>,
    /// Effort level for the response.
    pub effort: Option<EffortLevel>,
    /// Output format configuration for structured responses.
    pub output_format: Option<Value>,
    /// Enable file checkpointing for `rewind_files`.
    pub enable_file_checkpointing: bool,
    /// Mirror session transcripts to an external store.
    pub session_store: Option<Arc<dyn SessionStore>>,
    /// When to flush mirrored entries to `session_store`.
    pub session_store_flush: SessionStoreFlushMode,
    /// Timeout for each `session_store` load call during resume, in ms.
    pub load_timeout_ms: u64,
    /// API-side task budget in tokens.
    pub task_budget: Option<TaskBudget>,
}

impl Default for ClaudeAgentOptions {
    fn default() -> Self {
        Self {
            tools: None,
            allowed_tools: Vec::new(),
            system_prompt: None,
            mcp_servers: McpServersOption::default(),
            strict_mcp_config: false,
            permission_mode: None,
            continue_conversation: false,
            resume: None,
            session_id: None,
            max_turns: None,
            max_budget_usd: None,
            disallowed_tools: Vec::new(),
            model: None,
            fallback_model: None,
            betas: Vec::new(),
            permission_prompt_tool_name: None,
            cwd: None,
            cli_path: None,
            settings: None,
            add_dirs: Vec::new(),
            env: HashMap::new(),
            extra_args: HashMap::new(),
            max_buffer_size: None,
            stderr: None,
            user: None,
            include_partial_messages: false,
            include_hook_events: false,
            fork_session: false,
            agents: None,
            setting_sources: None,
            skills: None,
            sandbox: None,
            plugins: Vec::new(),
            max_thinking_tokens: None,
            thinking: None,
            effort: None,
            output_format: None,
            enable_file_checkpointing: false,
            session_store: None,
            session_store_flush: SessionStoreFlushMode::default(),
            load_timeout_ms: DEFAULT_LOAD_TIMEOUT_MS,
            task_budget: None,
        }
    }
}

impl std::fmt::Debug for ClaudeAgentOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClaudeAgentOptions")
            .field("tools", &self.tools)
            .field("allowed_tools", &self.allowed_tools)
            .field("system_prompt", &self.system_prompt)
            .field("mcp_servers", &self.mcp_servers)
            .field("strict_mcp_config", &self.strict_mcp_config)
            .field("permission_mode", &self.permission_mode)
            .field("continue_conversation", &self.continue_conversation)
            .field("resume", &self.resume)
            .field("session_id", &self.session_id)
            .field("max_turns", &self.max_turns)
            .field("max_budget_usd", &self.max_budget_usd)
            .field("disallowed_tools", &self.disallowed_tools)
            .field("model", &self.model)
            .field("fallback_model", &self.fallback_model)
            .field("betas", &self.betas)
            .field(
                "permission_prompt_tool_name",
                &self.permission_prompt_tool_name,
            )
            .field("cwd", &self.cwd)
            .field("cli_path", &self.cli_path)
            .field("settings", &self.settings)
            .field("add_dirs", &self.add_dirs)
            .field("env", &self.env)
            .field("extra_args", &self.extra_args)
            .field("max_buffer_size", &self.max_buffer_size)
            .field("stderr", &callback_marker(self.stderr.is_some()))
            .field("user", &self.user)
            .field("include_partial_messages", &self.include_partial_messages)
            .field("include_hook_events", &self.include_hook_events)
            .field("fork_session", &self.fork_session)
            .field("agents", &self.agents)
            .field("setting_sources", &self.setting_sources)
            .field("skills", &self.skills)
            .field("sandbox", &self.sandbox)
            .field("plugins", &self.plugins)
            .field("max_thinking_tokens", &self.max_thinking_tokens)
            .field("thinking", &self.thinking)
            .field("effort", &self.effort)
            .field("output_format", &self.output_format)
            .field("enable_file_checkpointing", &self.enable_file_checkpointing)
            .field(
                "session_store",
                &callback_marker(self.session_store.is_some()),
            )
            .field("session_store_flush", &self.session_store_flush)
            .field("load_timeout_ms", &self.load_timeout_ms)
            .field("task_budget", &self.task_budget)
            .finish()
    }
}

fn callback_marker(is_set: bool) -> &'static str {
    if is_set { "set" } else { "unset" }
}

impl ClaudeAgentOptions {
    /// Starts building a [`ClaudeAgentOptions`] with upstream-equivalent
    /// defaults.
    #[must_use]
    pub fn builder() -> ClaudeAgentOptionsBuilder {
        ClaudeAgentOptionsBuilder::default()
    }
}

/// Fluent builder for [`ClaudeAgentOptions`].
#[derive(Clone, Default)]
pub struct ClaudeAgentOptionsBuilder {
    options: ClaudeAgentOptions,
}

impl ClaudeAgentOptionsBuilder {
    /// Finalizes the options.
    #[must_use]
    pub fn build(self) -> ClaudeAgentOptions {
        self.options
    }

    /// Sets the base set of built-in tools.
    #[must_use]
    pub fn tools(mut self, tools: ToolsOption) -> Self {
        self.options.tools = Some(tools);
        self
    }

    /// Sets the tools auto-allowed without prompting.
    #[must_use]
    pub fn allowed_tools(mut self, tools: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.options.allowed_tools = tools.into_iter().map(Into::into).collect();
        self
    }

    /// Sets the system prompt.
    #[must_use]
    pub fn system_prompt(mut self, prompt: SystemPrompt) -> Self {
        self.options.system_prompt = Some(prompt);
        self
    }

    /// Sets the MCP server configuration.
    #[must_use]
    pub fn mcp_servers(mut self, servers: McpServersOption) -> Self {
        self.options.mcp_servers = servers;
        self
    }

    /// Sets `strict_mcp_config`.
    #[must_use]
    pub fn strict_mcp_config(mut self, strict: bool) -> Self {
        self.options.strict_mcp_config = strict;
        self
    }

    /// Sets the permission mode.
    #[must_use]
    pub fn permission_mode(mut self, mode: PermissionMode) -> Self {
        self.options.permission_mode = Some(mode);
        self
    }

    /// Sets `continue_conversation`.
    #[must_use]
    pub fn continue_conversation(mut self, continue_conversation: bool) -> Self {
        self.options.continue_conversation = continue_conversation;
        self
    }

    /// Sets the session id to resume.
    #[must_use]
    pub fn resume(mut self, session_id: impl Into<String>) -> Self {
        self.options.resume = Some(session_id.into());
        self
    }

    /// Sets a specific session id for the conversation.
    #[must_use]
    pub fn session_id(mut self, session_id: impl Into<String>) -> Self {
        self.options.session_id = Some(session_id.into());
        self
    }

    /// Sets the maximum conversation turns.
    #[must_use]
    pub fn max_turns(mut self, max_turns: u32) -> Self {
        self.options.max_turns = Some(max_turns);
        self
    }

    /// Sets the maximum budget in USD.
    #[must_use]
    pub fn max_budget_usd(mut self, budget: f64) -> Self {
        self.options.max_budget_usd = Some(budget);
        self
    }

    /// Sets the disallowed tools.
    #[must_use]
    pub fn disallowed_tools(mut self, tools: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.options.disallowed_tools = tools.into_iter().map(Into::into).collect();
        self
    }

    /// Sets the model.
    #[must_use]
    pub fn model(mut self, model: impl Into<String>) -> Self {
        self.options.model = Some(model.into());
        self
    }

    /// Sets the fallback model.
    #[must_use]
    pub fn fallback_model(mut self, model: impl Into<String>) -> Self {
        self.options.fallback_model = Some(model.into());
        self
    }

    /// Sets the beta features to enable.
    #[must_use]
    pub fn betas(mut self, betas: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.options.betas = betas.into_iter().map(Into::into).collect();
        self
    }

    /// Sets the permission prompt tool name.
    #[must_use]
    pub fn permission_prompt_tool_name(mut self, name: impl Into<String>) -> Self {
        self.options.permission_prompt_tool_name = Some(name.into());
        self
    }

    /// Sets the working directory.
    #[must_use]
    pub fn cwd(mut self, cwd: impl Into<PathBuf>) -> Self {
        self.options.cwd = Some(cwd.into());
        self
    }

    /// Sets an explicit path to the Claude Code CLI executable.
    #[must_use]
    pub fn cli_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.options.cli_path = Some(path.into());
        self
    }

    /// Sets the path to an additional settings JSON file (or an inline
    /// JSON string).
    #[must_use]
    pub fn settings(mut self, settings: impl Into<String>) -> Self {
        self.options.settings = Some(settings.into());
        self
    }

    /// Sets additional accessible directories.
    #[must_use]
    pub fn add_dirs(mut self, dirs: impl IntoIterator<Item = impl Into<PathBuf>>) -> Self {
        self.options.add_dirs = dirs.into_iter().map(Into::into).collect();
        self
    }

    /// Sets environment variables for the subprocess.
    #[must_use]
    pub fn env(mut self, env: impl IntoIterator<Item = (String, String)>) -> Self {
        self.options.env = env.into_iter().collect();
        self
    }

    /// Sets additional raw CLI arguments.
    #[must_use]
    pub fn extra_args(mut self, args: impl IntoIterator<Item = (String, Option<String>)>) -> Self {
        self.options.extra_args = args.into_iter().collect();
        self
    }

    /// Sets the maximum stdout buffer size in bytes.
    #[must_use]
    pub fn max_buffer_size(mut self, size: usize) -> Self {
        self.options.max_buffer_size = Some(size);
        self
    }

    /// Registers a callback invoked once per CLI stderr line.
    #[must_use]
    pub fn stderr<F>(mut self, callback: F) -> Self
    where
        F: Fn(&str) + Send + Sync + 'static,
    {
        self.options.stderr = Some(Arc::new(callback));
        self
    }

    /// Sets the OS-level user the subprocess is spawned as.
    #[must_use]
    pub fn user(mut self, user: impl Into<String>) -> Self {
        self.options.user = Some(user.into());
        self
    }

    /// Enables partial/streaming message events.
    #[must_use]
    pub fn include_partial_messages(mut self, include: bool) -> Self {
        self.options.include_partial_messages = include;
        self
    }

    /// Enables hook lifecycle events in the message stream.
    #[must_use]
    pub fn include_hook_events(mut self, include: bool) -> Self {
        self.options.include_hook_events = include;
        self
    }

    /// Enables forking resumed sessions to a new session id.
    #[must_use]
    pub fn fork_session(mut self, fork: bool) -> Self {
        self.options.fork_session = fork;
        self
    }

    /// Sets programmatically defined subagents.
    #[must_use]
    pub fn agents(mut self, agents: impl IntoIterator<Item = (String, AgentDefinition)>) -> Self {
        self.options.agents = Some(agents.into_iter().collect());
        self
    }

    /// Sets which filesystem settings sources to load.
    #[must_use]
    pub fn setting_sources(mut self, sources: impl IntoIterator<Item = SettingSource>) -> Self {
        self.options.setting_sources = Some(sources.into_iter().collect());
        self
    }

    /// Sets which skills to enable.
    #[must_use]
    pub fn skills(mut self, skills: SkillsOption) -> Self {
        self.options.skills = Some(skills);
        self
    }

    /// Sets sandbox settings.
    #[must_use]
    pub fn sandbox(mut self, sandbox: SandboxSettings) -> Self {
        self.options.sandbox = Some(sandbox);
        self
    }

    /// Adds a plugin.
    #[must_use]
    pub fn plugin(mut self, plugin: PluginConfig) -> Self {
        self.options.plugins.push(plugin);
        self
    }

    /// Sets the deprecated `max_thinking_tokens` option.
    #[must_use]
    pub fn max_thinking_tokens(mut self, tokens: u32) -> Self {
        self.options.max_thinking_tokens = Some(tokens);
        self
    }

    /// Sets the thinking/reasoning configuration.
    #[must_use]
    pub fn thinking(mut self, thinking: ThinkingConfig) -> Self {
        self.options.thinking = Some(thinking);
        self
    }

    /// Sets the response effort level.
    #[must_use]
    pub fn effort(mut self, effort: EffortLevel) -> Self {
        self.options.effort = Some(effort);
        self
    }

    /// Sets a structured output format (e.g. a JSON schema).
    #[must_use]
    pub fn output_format(mut self, format: Value) -> Self {
        self.options.output_format = Some(format);
        self
    }

    /// Enables file checkpointing.
    #[must_use]
    pub fn enable_file_checkpointing(mut self, enable: bool) -> Self {
        self.options.enable_file_checkpointing = enable;
        self
    }

    /// Sets a session-store adapter to mirror transcripts to.
    #[must_use]
    pub fn session_store(mut self, store: Arc<dyn SessionStore>) -> Self {
        self.options.session_store = Some(store);
        self
    }

    /// Sets when to flush mirrored entries to the session store.
    #[must_use]
    pub fn session_store_flush(mut self, mode: SessionStoreFlushMode) -> Self {
        self.options.session_store_flush = mode;
        self
    }

    /// Sets the session-store load timeout, in milliseconds.
    #[must_use]
    pub fn load_timeout_ms(mut self, timeout_ms: u64) -> Self {
        self.options.load_timeout_ms = timeout_ms;
        self
    }

    /// Sets the API-side task budget.
    #[must_use]
    pub fn task_budget(mut self, total: u64) -> Self {
        self.options.task_budget = Some(TaskBudget { total });
        self
    }
}

/// Computes the effective `allowed_tools` and `setting_sources` after
/// applying `options.skills` defaults, mirroring upstream's
/// `_apply_skills_defaults()`. Does not mutate `options`.
fn apply_skills_defaults(
    options: &ClaudeAgentOptions,
) -> (Vec<String>, Option<Vec<SettingSource>>) {
    let mut allowed_tools = options.allowed_tools.clone();
    let mut setting_sources = options.setting_sources.clone();

    let Some(skills) = &options.skills else {
        return (allowed_tools, setting_sources);
    };

    match skills {
        SkillsOption::All => {
            if !allowed_tools.iter().any(|tool| tool == "Skill") {
                allowed_tools.push("Skill".to_string());
            }
        }
        SkillsOption::Named(names) => {
            for name in names {
                let pattern = format!("Skill({name})");
                if !allowed_tools.contains(&pattern) {
                    allowed_tools.push(pattern);
                }
            }
        }
    }

    if setting_sources.is_none() {
        setting_sources = Some(vec![SettingSource::User, SettingSource::Project]);
    }

    (allowed_tools, setting_sources)
}

/// Converts options into the CLI flags they imply.
///
/// Base flags (`--output-format`, `--verbose`, `--input-format`) and
/// one-shot-vs-streaming mode flags are appended by the transport
/// (Phase 4), not here. `can_use_tool`/`hooks` never produce CLI flags
/// (control-protocol only); `agents` is delivered via the `initialize`
/// control request, not a flag, per upstream's
/// `_build_command()` (comment: "Agents are always sent via initialize
/// request... No --agents CLI flag needed").
///
/// Several flags are gated on upstream's Python truthiness (`if x:`)
/// rather than presence (`is not None`) — an explicit `Some(0)` or
/// `Some(String::new())` therefore still omits the flag in a few
/// cases, faithfully matching upstream's (slightly surprising) wire
/// behavior. Each such case is called out inline below.
#[must_use]
pub fn build_cli_args(options: &ClaudeAgentOptions) -> Vec<String> {
    let mut args = Vec::new();

    push_system_prompt_args(&mut args, options.system_prompt.as_ref());
    push_tools_args(&mut args, options.tools.as_ref());

    let (effective_allowed_tools, effective_setting_sources) = apply_skills_defaults(options);
    if !effective_allowed_tools.is_empty() {
        args.push("--allowedTools".to_string());
        args.push(effective_allowed_tools.join(","));
    }

    push_turn_budget_and_model_args(&mut args, options);
    push_session_and_settings_args(&mut args, options);
    push_toggle_and_extra_args(&mut args, options, effective_setting_sources);

    push_thinking_args(&mut args, options);

    if let Some(effort) = options.effort {
        args.push("--effort".to_string());
        args.push(effort.as_str().to_string());
    }

    push_output_format_args(&mut args, options.output_format.as_ref());

    args.push("--input-format".to_string());
    args.push("stream-json".to_string());

    args
}

fn push_turn_budget_and_model_args(args: &mut Vec<String>, options: &ClaudeAgentOptions) {
    // Upstream: `if self._options.max_turns:` — a truthy check, so
    // `Some(0)` also omits the flag (0 turns is indistinguishable from
    // unset on the wire).
    if let Some(max_turns) = options.max_turns
        && max_turns != 0
    {
        args.push("--max-turns".to_string());
        args.push(max_turns.to_string());
    }

    if let Some(budget) = options.max_budget_usd {
        args.push("--max-budget-usd".to_string());
        args.push(budget.to_string());
    }

    if !options.disallowed_tools.is_empty() {
        args.push("--disallowedTools".to_string());
        args.push(options.disallowed_tools.join(","));
    }

    if let Some(task_budget) = options.task_budget {
        args.push("--task-budget".to_string());
        args.push(task_budget.total.to_string());
    }

    // Upstream: `if self._options.model:` — truthy, so `Some("")` omits.
    if let Some(model) = &options.model
        && !model.is_empty()
    {
        args.push("--model".to_string());
        args.push(model.clone());
    }

    if let Some(model) = &options.fallback_model
        && !model.is_empty()
    {
        args.push("--fallback-model".to_string());
        args.push(model.clone());
    }

    if !options.betas.is_empty() {
        args.push("--betas".to_string());
        args.push(options.betas.join(","));
    }

    if let Some(name) = &options.permission_prompt_tool_name
        && !name.is_empty()
    {
        args.push("--permission-prompt-tool".to_string());
        args.push(name.clone());
    }

    if let Some(mode) = options.permission_mode {
        args.push("--permission-mode".to_string());
        args.push(mode.as_str().to_string());
    }
}

fn push_session_and_settings_args(args: &mut Vec<String>, options: &ClaudeAgentOptions) {
    if options.continue_conversation {
        args.push("--continue".to_string());
    }

    if let Some(resume) = &options.resume
        && !resume.is_empty()
    {
        args.push("--resume".to_string());
        args.push(resume.clone());
    }

    if let Some(session_id) = &options.session_id
        && !session_id.is_empty()
    {
        args.push("--session-id".to_string());
        args.push(session_id.clone());
    }

    if let Some(settings) = &options.settings
        && !settings.is_empty()
    {
        args.push("--settings".to_string());
        args.push(settings.clone());
    }

    for dir in &options.add_dirs {
        args.push("--add-dir".to_string());
        args.push(dir.display().to_string());
    }

    push_mcp_servers_args(args, &options.mcp_servers);
}

fn push_toggle_and_extra_args(
    args: &mut Vec<String>,
    options: &ClaudeAgentOptions,
    effective_setting_sources: Option<Vec<SettingSource>>,
) {
    if options.include_partial_messages {
        args.push("--include-partial-messages".to_string());
    }

    if options.include_hook_events {
        args.push("--include-hook-events".to_string());
    }

    if options.strict_mcp_config {
        args.push("--strict-mcp-config".to_string());
    }

    if options.fork_session {
        args.push("--fork-session".to_string());
    }

    if options.session_store.is_some() {
        args.push("--session-mirror".to_string());
    }

    // `Some(vec![])` (explicit "disable filesystem settings") still
    // emits the flag with an empty value — `is not None`, not truthy.
    if let Some(sources) = effective_setting_sources {
        let joined = sources
            .iter()
            .map(|source| source.as_str())
            .collect::<Vec<_>>()
            .join(",");
        args.push(format!("--setting-sources={joined}"));
    }

    for plugin in &options.plugins {
        let PluginConfig::Local { path } = plugin;
        args.push("--plugin-dir".to_string());
        args.push(path.display().to_string());
    }

    for (flag, value) in &options.extra_args {
        match value {
            None => args.push(format!("--{flag}")),
            Some(value) => {
                args.push(format!("--{flag}"));
                args.push(value.clone());
            }
        }
    }
}

fn push_system_prompt_args(args: &mut Vec<String>, system_prompt: Option<&SystemPrompt>) {
    match system_prompt {
        None => {
            args.push("--system-prompt".to_string());
            args.push(String::new());
        }
        Some(SystemPrompt::Custom(text)) => {
            args.push("--system-prompt".to_string());
            args.push(text.clone());
        }
        Some(SystemPrompt::File { path }) => {
            args.push("--system-prompt-file".to_string());
            args.push(path.clone());
        }
        // Upstream: a preset without `append` produces no flag at all —
        // confirmed, not a bug in this port.
        Some(SystemPrompt::Preset {
            append: Some(append),
            ..
        }) => {
            args.push("--append-system-prompt".to_string());
            args.push(append.clone());
        }
        Some(SystemPrompt::Preset { append: None, .. }) => {}
    }
}

fn push_tools_args(args: &mut Vec<String>, tools: Option<&ToolsOption>) {
    match tools {
        None => {}
        Some(ToolsOption::Named(names)) if names.is_empty() => {
            args.push("--tools".to_string());
            args.push(String::new());
        }
        Some(ToolsOption::Named(names)) => {
            args.push("--tools".to_string());
            args.push(names.join(","));
        }
        Some(ToolsOption::Preset) => {
            args.push("--tools".to_string());
            args.push("default".to_string());
        }
    }
}

fn push_mcp_servers_args(args: &mut Vec<String>, mcp_servers: &McpServersOption) {
    match mcp_servers {
        McpServersOption::Servers(servers) if !servers.is_empty() => {
            let wrapped = serde_json::json!({ "mcpServers": servers });
            args.push("--mcp-config".to_string());
            args.push(wrapped.to_string());
        }
        McpServersOption::Path(path) if !path.is_empty() => {
            args.push("--mcp-config".to_string());
            args.push(path.clone());
        }
        McpServersOption::Servers(_) | McpServersOption::Path(_) => {}
    }
}

fn push_thinking_args(args: &mut Vec<String>, options: &ClaudeAgentOptions) {
    match &options.thinking {
        Some(ThinkingConfig::Adaptive { display }) => {
            args.push("--thinking".to_string());
            args.push("adaptive".to_string());
            if let Some(display) = display {
                args.push("--thinking-display".to_string());
                args.push(thinking_display_str(*display).to_string());
            }
        }
        Some(ThinkingConfig::Enabled {
            budget_tokens,
            display,
        }) => {
            args.push("--max-thinking-tokens".to_string());
            args.push(budget_tokens.to_string());
            if let Some(display) = display {
                args.push("--thinking-display".to_string());
                args.push(thinking_display_str(*display).to_string());
            }
        }
        Some(ThinkingConfig::Disabled) => {
            args.push("--thinking".to_string());
            args.push("disabled".to_string());
        }
        None => {
            if let Some(tokens) = options.max_thinking_tokens {
                args.push("--max-thinking-tokens".to_string());
                args.push(tokens.to_string());
            }
        }
    }
}

fn thinking_display_str(display: ThinkingDisplay) -> &'static str {
    match display {
        ThinkingDisplay::Summarized => "summarized",
        ThinkingDisplay::Omitted => "omitted",
    }
}

fn push_output_format_args(args: &mut Vec<String>, output_format: Option<&Value>) {
    let Some(format) = output_format else {
        return;
    };
    if format.get("type").and_then(Value::as_str) != Some("json_schema") {
        return;
    }
    let Some(schema) = format.get("schema") else {
        return;
    };
    args.push("--json-schema".to_string());
    args.push(schema.to_string());
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::types::mcp::{McpServerConfig, McpServers};

    fn args_of(options: &ClaudeAgentOptions) -> Vec<String> {
        build_cli_args(options)
    }

    fn adjacent_pair(args: &[String], flag: &str) -> Option<(String, String)> {
        args.windows(2)
            .find(|pair| pair[0] == flag)
            .map(|pair| (pair[0].clone(), pair[1].clone()))
    }

    #[test]
    fn default_options_still_emit_base_system_prompt_and_input_format() {
        // Upstream always emits --system-prompt (empty when unset) and
        // always appends --input-format stream-json — there is no
        // "zero flags" case anymore.
        let args = args_of(&ClaudeAgentOptions::default());
        assert_eq!(
            adjacent_pair(&args, "--system-prompt"),
            Some(("--system-prompt".to_string(), String::new()))
        );
        assert_eq!(args.last(), Some(&"stream-json".to_string()));
        assert_eq!(
            args[args.len() - 2],
            "--input-format".to_string(),
            "input-format must be the final flag pair"
        );
    }

    #[test]
    fn allowed_tools_are_comma_joined() {
        let options = ClaudeAgentOptions::builder()
            .allowed_tools(["Read", "Bash"])
            .build();
        let args = args_of(&options);
        assert_eq!(
            adjacent_pair(&args, "--allowedTools"),
            Some(("--allowedTools".to_string(), "Read,Bash".to_string()))
        );
    }

    #[test]
    fn permission_mode_uses_wire_string() {
        let options = ClaudeAgentOptions::builder()
            .permission_mode(PermissionMode::AcceptEdits)
            .build();
        let args = args_of(&options);
        assert_eq!(
            adjacent_pair(&args, "--permission-mode"),
            Some(("--permission-mode".to_string(), "acceptEdits".to_string()))
        );
    }

    #[test]
    fn continue_conversation_is_a_bare_flag() {
        let options = ClaudeAgentOptions::builder()
            .continue_conversation(true)
            .build();
        let args = args_of(&options);
        let index = args
            .iter()
            .position(|a| a == "--continue")
            .expect("present");
        assert_ne!(args.get(index + 1), Some(&String::new()));
        assert!(
            index + 1 == args.len() || args[index + 1].starts_with("--"),
            "no bare value after --continue"
        );
    }

    #[test]
    fn add_dirs_repeat_the_flag() {
        let options = ClaudeAgentOptions::builder().add_dirs(["/a", "/b"]).build();
        let args = args_of(&options);
        let count = args.iter().filter(|a| *a == "--add-dir").count();
        assert_eq!(count, 2);
    }

    #[test]
    fn mcp_servers_serialize_as_mcp_config_json() {
        let mut servers = McpServers::new();
        servers.insert(
            "calc".to_string(),
            McpServerConfig::Stdio {
                command: "calc-server".to_string(),
                args: Vec::new(),
                env: HashMap::new(),
            },
        );
        let options = ClaudeAgentOptions::builder()
            .mcp_servers(McpServersOption::Servers(servers))
            .build();
        let args = args_of(&options);
        let (_, json) = adjacent_pair(&args, "--mcp-config").expect("present");
        let value: Value = serde_json::from_str(&json).expect("valid json");
        assert_eq!(value["mcpServers"]["calc"]["command"], "calc-server");
    }

    #[test]
    fn extra_args_with_value_and_without() {
        let options = ClaudeAgentOptions::builder()
            .extra_args([
                ("foo".to_string(), Some("1".to_string())),
                ("bar".to_string(), None),
            ])
            .build();
        let args = args_of(&options);
        assert_eq!(
            adjacent_pair(&args, "--foo"),
            Some(("--foo".to_string(), "1".to_string()))
        );
        assert!(args.contains(&"--bar".to_string()));
        let bar_index = args.iter().position(|a| a == "--bar").unwrap();
        assert!(
            bar_index + 1 == args.len() || args[bar_index + 1].starts_with("--"),
            "no bare value after --bar"
        );
    }

    #[test]
    fn system_prompt_custom_vs_preset_append() {
        let custom = ClaudeAgentOptions::builder()
            .system_prompt(SystemPrompt::Custom("be terse".to_string()))
            .build();
        assert_eq!(
            adjacent_pair(&args_of(&custom), "--system-prompt"),
            Some(("--system-prompt".to_string(), "be terse".to_string()))
        );

        let preset = ClaudeAgentOptions::builder()
            .system_prompt(SystemPrompt::Preset {
                preset: "claude_code".to_string(),
                append: Some("extra rules".to_string()),
                exclude_dynamic_sections: None,
            })
            .build();
        assert_eq!(
            adjacent_pair(&args_of(&preset), "--append-system-prompt"),
            Some((
                "--append-system-prompt".to_string(),
                "extra rules".to_string()
            ))
        );
    }

    #[test]
    fn system_prompt_preset_without_append_produces_no_flag() {
        let options = ClaudeAgentOptions::builder()
            .system_prompt(SystemPrompt::Preset {
                preset: "claude_code".to_string(),
                append: None,
                exclude_dynamic_sections: None,
            })
            .build();
        let args = args_of(&options);
        assert!(!args.contains(&"--system-prompt".to_string()));
        assert!(!args.contains(&"--append-system-prompt".to_string()));
    }

    #[test]
    fn system_prompt_file_variant_uses_dedicated_flag() {
        let options = ClaudeAgentOptions::builder()
            .system_prompt(SystemPrompt::File {
                path: "/prompts/base.md".to_string(),
            })
            .build();
        let args = args_of(&options);
        assert_eq!(
            adjacent_pair(&args, "--system-prompt-file"),
            Some((
                "--system-prompt-file".to_string(),
                "/prompts/base.md".to_string()
            ))
        );
    }

    #[test]
    // One assertion per field, on purpose: this test's entire value is
    // proving the builder round-trips EVERY field. Splitting it would
    // hide a missed field behind test-name granularity instead of
    // catching it here.
    #[allow(clippy::too_many_lines)]
    fn builder_sets_every_field() {
        let options = ClaudeAgentOptions::builder()
            .tools(ToolsOption::Preset)
            .allowed_tools(["Read"])
            .system_prompt(SystemPrompt::Custom("x".to_string()))
            .strict_mcp_config(true)
            .permission_mode(PermissionMode::Plan)
            .continue_conversation(true)
            .resume("sess-1")
            .session_id("sess-2")
            .max_turns(5)
            .max_budget_usd(1.5)
            .disallowed_tools(["Bash"])
            .model("claude-sonnet-5")
            .fallback_model("claude-haiku-5")
            .betas([BETA_CONTEXT_1M])
            .permission_prompt_tool_name("mcp__perms__ask")
            .cwd("/work")
            .cli_path("/usr/local/bin/claude")
            .settings("{}")
            .add_dirs(["/extra"])
            .env([("KEY".to_string(), "value".to_string())])
            .extra_args([("flag".to_string(), None)])
            .max_buffer_size(2048)
            .stderr(|_line| {})
            .user("svc-account")
            .include_partial_messages(true)
            .include_hook_events(true)
            .fork_session(true)
            .agents([(
                "reviewer".to_string(),
                AgentDefinition {
                    description: "reviews code".to_string(),
                    prompt: "be critical".to_string(),
                    tools: None,
                    disallowed_tools: None,
                    model: None,
                    skills: None,
                    memory: None,
                    mcp_servers: None,
                    initial_prompt: None,
                    max_turns: None,
                    background: None,
                    effort: None,
                    permission_mode: None,
                },
            )])
            .setting_sources([SettingSource::Project])
            .skills(SkillsOption::All)
            .sandbox(SandboxSettings::default())
            .plugin(PluginConfig::Local {
                path: "/plugins/one".into(),
            })
            .max_thinking_tokens(1000)
            .thinking(ThinkingConfig::Adaptive { display: None })
            .effort(EffortLevel::High)
            .output_format(serde_json::json!({"type": "json_schema"}))
            .enable_file_checkpointing(true)
            .session_store_flush(SessionStoreFlushMode::Eager)
            .load_timeout_ms(5000)
            .task_budget(10_000)
            .build();

        assert!(matches!(options.tools, Some(ToolsOption::Preset)));
        assert_eq!(options.allowed_tools, vec!["Read".to_string()]);
        assert!(options.system_prompt.is_some());
        assert!(options.strict_mcp_config);
        assert_eq!(options.permission_mode, Some(PermissionMode::Plan));
        assert!(options.continue_conversation);
        assert_eq!(options.resume.as_deref(), Some("sess-1"));
        assert_eq!(options.session_id.as_deref(), Some("sess-2"));
        assert_eq!(options.max_turns, Some(5));
        assert_eq!(options.max_budget_usd, Some(1.5));
        assert_eq!(options.disallowed_tools, vec!["Bash".to_string()]);
        assert_eq!(options.model.as_deref(), Some("claude-sonnet-5"));
        assert_eq!(options.fallback_model.as_deref(), Some("claude-haiku-5"));
        assert_eq!(options.betas, vec![BETA_CONTEXT_1M.to_string()]);
        assert_eq!(
            options.permission_prompt_tool_name.as_deref(),
            Some("mcp__perms__ask")
        );
        assert_eq!(options.cwd, Some(PathBuf::from("/work")));
        assert_eq!(
            options.cli_path,
            Some(PathBuf::from("/usr/local/bin/claude"))
        );
        assert_eq!(options.settings.as_deref(), Some("{}"));
        assert_eq!(options.add_dirs, vec![PathBuf::from("/extra")]);
        assert_eq!(options.env.get("KEY"), Some(&"value".to_string()));
        assert!(options.extra_args.contains_key("flag"));
        assert_eq!(options.max_buffer_size, Some(2048));
        assert!(options.stderr.is_some());
        assert_eq!(options.user.as_deref(), Some("svc-account"));
        assert!(options.include_partial_messages);
        assert!(options.include_hook_events);
        assert!(options.fork_session);
        assert!(options.agents.as_ref().unwrap().contains_key("reviewer"));
        assert_eq!(options.setting_sources, Some(vec![SettingSource::Project]));
        assert!(matches!(options.skills, Some(SkillsOption::All)));
        assert!(options.sandbox.is_some());
        assert_eq!(options.plugins.len(), 1);
        assert_eq!(options.max_thinking_tokens, Some(1000));
        assert!(options.thinking.is_some());
        assert_eq!(options.effort, Some(EffortLevel::High));
        assert!(options.output_format.is_some());
        assert!(options.enable_file_checkpointing);
        assert_eq!(options.session_store_flush, SessionStoreFlushMode::Eager);
        assert_eq!(options.load_timeout_ms, 5000);
        assert_eq!(options.task_budget, Some(TaskBudget { total: 10_000 }));
    }

    #[rstest]
    #[case(PermissionMode::Default)]
    #[case(PermissionMode::AcceptEdits)]
    #[case(PermissionMode::Plan)]
    #[case(PermissionMode::BypassPermissions)]
    #[case(PermissionMode::DontAsk)]
    #[case(PermissionMode::Auto)]
    fn permission_mode_serde_roundtrip(#[case] mode: PermissionMode) {
        let json = serde_json::to_string(&mode).expect("serializes");
        let parsed: PermissionMode = serde_json::from_str(&json).expect("deserializes");
        assert_eq!(parsed, mode);
    }

    #[test]
    fn mcp_config_deserializes_stdio_without_type_tag() {
        let value = serde_json::json!({"command": "npx"});
        let config: McpServerConfig = serde_json::from_value(value).expect("deserializes");
        assert!(matches!(config, McpServerConfig::Stdio { .. }));
    }

    #[test]
    fn plugins_serialize_into_plugin_dir_flag() {
        // Corrected vs the original plan sketch: upstream sends
        // repeated --plugin-dir flags, never a JSON --plugins flag.
        let options = ClaudeAgentOptions::builder()
            .plugin(PluginConfig::Local {
                path: "/plugins/a".into(),
            })
            .plugin(PluginConfig::Local {
                path: "/plugins/b".into(),
            })
            .build();
        let args = args_of(&options);
        let count = args.iter().filter(|a| *a == "--plugin-dir").count();
        assert_eq!(count, 2);
        assert!(args.contains(&"/plugins/a".to_string()));
        assert!(args.contains(&"/plugins/b".to_string()));
        assert!(!args.iter().any(|a| a == "--plugins"));
    }

    #[test]
    fn empty_plugins_produce_no_flag() {
        let args = args_of(&ClaudeAgentOptions::default());
        assert!(!args.contains(&"--plugin-dir".to_string()));
    }

    #[test]
    fn stderr_callback_is_not_a_cli_flag() {
        let with_callback = ClaudeAgentOptions::builder().stderr(|_| {}).build();
        let without_callback = ClaudeAgentOptions::default();
        assert_eq!(args_of(&with_callback), args_of(&without_callback));
    }

    #[test]
    fn options_debug_marks_callbacks_without_printing_them() {
        let options = ClaudeAgentOptions::builder().stderr(|_| {}).build();
        let debug = format!("{options:?}");
        assert!(debug.contains("set"));
    }

    #[test]
    fn plugin_config_serde_roundtrip() {
        let config = PluginConfig::Local {
            path: "/plugins/x".into(),
        };
        let json = serde_json::to_value(&config).expect("serializes");
        let parsed: PluginConfig = serde_json::from_value(json).expect("deserializes");
        assert_eq!(parsed, config);
    }

    #[test]
    fn agents_do_not_produce_a_cli_flag() {
        // Corrected vs the original plan sketch: upstream sends agents
        // via the control-protocol initialize request (Phase 5), never
        // a --agents CLI flag.
        let mut agents = HashMap::new();
        agents.insert(
            "reviewer".to_string(),
            AgentDefinition {
                description: "reviews code".to_string(),
                prompt: "be critical".to_string(),
                tools: None,
                disallowed_tools: None,
                model: None,
                skills: None,
                memory: None,
                mcp_servers: None,
                initial_prompt: None,
                max_turns: None,
                background: None,
                effort: None,
                permission_mode: None,
            },
        );
        let options = ClaudeAgentOptions {
            agents: Some(agents),
            ..ClaudeAgentOptions::default()
        };
        let args = args_of(&options);
        assert!(!args.iter().any(|a| a == "--agents"));
    }

    #[test]
    fn agent_definition_serde_roundtrip() {
        let agent = AgentDefinition {
            description: "reviews code".to_string(),
            prompt: "be critical".to_string(),
            tools: Some(vec!["Read".to_string()]),
            disallowed_tools: Some(vec!["Bash".to_string()]),
            model: Some("sonnet".to_string()),
            skills: Some(vec!["skill-a".to_string()]),
            memory: Some("project".to_string()),
            mcp_servers: None,
            initial_prompt: Some("start here".to_string()),
            max_turns: Some(3),
            background: Some(true),
            effort: Some(AgentEffort::Level(EffortLevel::High)),
            permission_mode: Some(PermissionMode::Plan),
        };
        let json = serde_json::to_value(&agent).expect("serializes");
        assert_eq!(json["disallowedTools"], serde_json::json!(["Bash"]));
        assert_eq!(json["maxTurns"], 3);
        assert_eq!(json["permissionMode"], "plan");
        let parsed: AgentDefinition = serde_json::from_value(json).expect("deserializes");
        assert_eq!(parsed, agent);
    }

    #[test]
    fn setting_sources_join_with_equals_sign() {
        // Corrected vs the original plan sketch: upstream emits a
        // single `--setting-sources=a,b` argument, not two args.
        let options = ClaudeAgentOptions::builder()
            .setting_sources([SettingSource::User, SettingSource::Project])
            .build();
        let args = args_of(&options);
        assert!(args.contains(&"--setting-sources=user,project".to_string()));
    }

    #[test]
    fn setting_sources_empty_vec_still_emits_flag() {
        // `Some(vec![])` means "explicitly disable" and must still be
        // sent — this is an `is not None` check upstream, not truthy.
        let options = ClaudeAgentOptions::builder().setting_sources([]).build();
        let args = args_of(&options);
        assert!(args.contains(&"--setting-sources=".to_string()));
    }

    #[test]
    fn skills_all_appends_bare_skill_tool() {
        let options = ClaudeAgentOptions::builder()
            .skills(SkillsOption::All)
            .build();
        let args = args_of(&options);
        assert!(
            adjacent_pair(&args, "--allowedTools")
                .is_some_and(|(_, value)| value.split(',').any(|t| t == "Skill"))
        );
        // skills defaults setting_sources to user,project when unset.
        assert!(args.contains(&"--setting-sources=user,project".to_string()));
    }

    #[test]
    fn skills_named_appends_skill_specifiers() {
        let options = ClaudeAgentOptions::builder()
            .skills(SkillsOption::Named(vec!["my-skill".to_string()]))
            .build();
        let args = args_of(&options);
        let (_, value) = adjacent_pair(&args, "--allowedTools").expect("present");
        assert!(value.split(',').any(|t| t == "Skill(my-skill)"));
    }

    #[test]
    fn max_turns_zero_omits_flag() {
        // Upstream's `if self._options.max_turns:` is a truthiness
        // check — 0 is falsy in Python, so this deliberately matches.
        let options = ClaudeAgentOptions::builder().max_turns(0).build();
        let args = args_of(&options);
        assert!(!args.contains(&"--max-turns".to_string()));
    }

    #[test]
    fn max_budget_usd_zero_still_emits_flag() {
        // `is not None` check upstream, unlike max_turns.
        let options = ClaudeAgentOptions::builder().max_budget_usd(0.0).build();
        let args = args_of(&options);
        assert!(args.contains(&"--max-budget-usd".to_string()));
    }

    #[test]
    fn thinking_adaptive_maps_to_thinking_flag() {
        let options = ClaudeAgentOptions::builder()
            .thinking(ThinkingConfig::Adaptive {
                display: Some(ThinkingDisplay::Summarized),
            })
            .build();
        let args = args_of(&options);
        assert_eq!(
            adjacent_pair(&args, "--thinking"),
            Some(("--thinking".to_string(), "adaptive".to_string()))
        );
        assert_eq!(
            adjacent_pair(&args, "--thinking-display"),
            Some(("--thinking-display".to_string(), "summarized".to_string()))
        );
    }

    #[test]
    fn thinking_enabled_maps_to_max_thinking_tokens_flag() {
        let options = ClaudeAgentOptions::builder()
            .thinking(ThinkingConfig::Enabled {
                budget_tokens: 2048,
                display: None,
            })
            .build();
        let args = args_of(&options);
        assert_eq!(
            adjacent_pair(&args, "--max-thinking-tokens"),
            Some(("--max-thinking-tokens".to_string(), "2048".to_string()))
        );
    }

    #[test]
    fn thinking_takes_precedence_over_deprecated_max_thinking_tokens() {
        let options = ClaudeAgentOptions::builder()
            .max_thinking_tokens(999)
            .thinking(ThinkingConfig::Disabled)
            .build();
        let args = args_of(&options);
        assert_eq!(
            adjacent_pair(&args, "--thinking"),
            Some(("--thinking".to_string(), "disabled".to_string()))
        );
        assert!(!args.contains(&"--max-thinking-tokens".to_string()));
    }

    #[test]
    fn deprecated_max_thinking_tokens_used_when_thinking_absent() {
        let options = ClaudeAgentOptions::builder()
            .max_thinking_tokens(500)
            .build();
        let args = args_of(&options);
        assert_eq!(
            adjacent_pair(&args, "--max-thinking-tokens"),
            Some(("--max-thinking-tokens".to_string(), "500".to_string()))
        );
    }

    #[test]
    fn output_format_json_schema_produces_flag() {
        let options = ClaudeAgentOptions::builder()
            .output_format(serde_json::json!({
                "type": "json_schema",
                "schema": {"type": "object"}
            }))
            .build();
        let args = args_of(&options);
        let (_, schema_json) = adjacent_pair(&args, "--json-schema").expect("present");
        let value: Value = serde_json::from_str(&schema_json).expect("valid json");
        assert_eq!(value["type"], "object");
    }

    #[test]
    fn output_format_non_json_schema_produces_no_flag() {
        let options = ClaudeAgentOptions::builder()
            .output_format(serde_json::json!({"type": "other"}))
            .build();
        let args = args_of(&options);
        assert!(!args.contains(&"--json-schema".to_string()));
    }

    #[test]
    fn session_store_presence_emits_session_mirror_flag() {
        struct NoopStore;
        impl SessionStore for NoopStore {
            fn append<'a>(
                &'a self,
                _key: &'a crate::types::session_store::SessionKey,
                _entries: Vec<Value>,
            ) -> crate::types::session_store::BoxFuture<'a, crate::Result<()>> {
                Box::pin(async { Ok(()) })
            }

            fn load<'a>(
                &'a self,
                _key: &'a crate::types::session_store::SessionKey,
            ) -> crate::types::session_store::BoxFuture<'a, crate::Result<Option<Vec<Value>>>>
            {
                Box::pin(async { Ok(None) })
            }
        }

        let options = ClaudeAgentOptions::builder()
            .session_store(Arc::new(NoopStore))
            .build();
        let args = args_of(&options);
        assert!(args.contains(&"--session-mirror".to_string()));
    }

    #[test]
    fn tools_preset_maps_to_default_string() {
        let options = ClaudeAgentOptions::builder()
            .tools(ToolsOption::Preset)
            .build();
        let args = args_of(&options);
        assert_eq!(
            adjacent_pair(&args, "--tools"),
            Some(("--tools".to_string(), "default".to_string()))
        );
    }

    #[test]
    fn tools_empty_list_maps_to_empty_string() {
        let options = ClaudeAgentOptions::builder()
            .tools(ToolsOption::Named(Vec::new()))
            .build();
        let args = args_of(&options);
        assert_eq!(
            adjacent_pair(&args, "--tools"),
            Some(("--tools".to_string(), String::new()))
        );
    }

    #[test]
    fn sandbox_settings_default_all_fields_absent() {
        let json = serde_json::to_value(SandboxSettings::default()).expect("serializes");
        assert_eq!(json, serde_json::json!({}));
    }

    #[test]
    fn thinking_config_disabled_serde_roundtrip() {
        let config = ThinkingConfig::Disabled;
        let json = serde_json::to_value(config).expect("serializes");
        assert_eq!(json["type"], "disabled");
        let parsed: ThinkingConfig = serde_json::from_value(json).expect("deserializes");
        assert_eq!(parsed, config);
    }

    #[test]
    fn task_budget_maps_to_flag() {
        let options = ClaudeAgentOptions::builder().task_budget(50_000).build();
        let args = args_of(&options);
        assert_eq!(
            adjacent_pair(&args, "--task-budget"),
            Some(("--task-budget".to_string(), "50000".to_string()))
        );
    }
}
