//! Idiomatic Rust port of the official Claude Agent SDK.
//!
//! Wraps the Claude Code CLI as a subprocess and exposes a typed,
//! async API for one-shot queries and interactive agent sessions.

mod error;
mod protocol;
pub mod transport;
pub mod types;

pub use error::{Error, Result};
pub use types::mcp::{McpServerConfig, McpServers, McpServersOption, PluginConfig};
pub use types::message::{
    AssistantMessage, ContentBlock, DeferredToolUse, HookEventMessage, Message, MirrorErrorMessage,
    RateLimitEvent, RateLimitInfo, ResultMessage, StreamEvent, SystemMessage,
    TERMINAL_TASK_STATUSES, TaskNotificationMessage, TaskProgressMessage, TaskStartedMessage,
    TaskUpdatedMessage, TaskUsage, UserContent, UserMessage, parse_message,
};
pub use types::options::{
    AgentDefinition, AgentEffort, ClaudeAgentOptions, ClaudeAgentOptionsBuilder,
    DEFAULT_LOAD_TIMEOUT_MS, DEFAULT_MAX_BUFFER_SIZE, EffortLevel, SandboxIgnoreViolations,
    SandboxNetworkConfig, SandboxSettings, SettingSource, SkillsOption, StderrCallback,
    SystemPrompt, TaskBudget, ThinkingConfig, ThinkingDisplay, ToolsOption, build_cli_args,
};
pub use types::permission::PermissionMode;
pub use types::session_store::{
    BoxFuture, SessionKey, SessionListSubkeysKey, SessionStore, SessionStoreEntry,
    SessionStoreFlushMode, SessionStoreListEntry, SessionSummaryEntry,
};

pub use transport::Transport;
pub use transport::subprocess::{SubprocessTransport, find_cli, full_command_args};
