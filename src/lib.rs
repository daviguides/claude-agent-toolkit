//! Idiomatic Rust port of the official [Claude Agent SDK][upstream].
//!
//! Wraps the `claude` CLI as a subprocess (JSON-over-stdio) and
//! exposes it as a safe, async, strongly-typed Rust API — the same
//! wire protocol the official Python/TypeScript SDKs speak, translated
//! into Rust idioms (the type system over runtime checks, `tokio`
//! throughout, zero-cost wrappers instead of dynamic dispatch).
//!
//! [upstream]: https://github.com/anthropics/claude-agent-sdk-python
//!
//! # Quick start
//!
//! ```no_run
//! use claude_agent_toolkit::{ClaudeAgentOptions, ContentBlock, Message, query};
//! use futures::StreamExt;
//!
//! # async fn run() -> claude_agent_toolkit::Result<()> {
//! let mut stream = query("What is 2 + 2?", ClaudeAgentOptions::default()).await?;
//! while let Some(message) = stream.next().await {
//!     if let Message::Assistant(assistant) = message? {
//!         for block in assistant.content {
//!             if let ContentBlock::Text { text } = block {
//!                 println!("Claude: {text}");
//!             }
//!         }
//!     }
//! }
//! # Ok(())
//! # }
//! ```
//!
//! See `examples/` for one-shot queries, an interactive multi-turn
//! [`ClaudeClient`] session, `can_use_tool`/hook callbacks, and an
//! in-process MCP tool server.
//!
//! # Feature map
//!
//! | Capability | Entry point |
//! |---|---|
//! | One-shot query | [`query()`] |
//! | One-shot query, streamed input | [`query_stream()`] |
//! | Interactive multi-turn session | [`ClaudeClient`] |
//! | Options (model, tools, sandboxing, ...) | [`ClaudeAgentOptions`] |
//! | Typed message model | [`Message`], [`ContentBlock`] |
//! | Tool permission callback | [`ClaudeAgentOptionsBuilder::can_use_tool`] |
//! | Lifecycle hooks | [`HookEvent`], [`HookMatcher`] |
//! | In-process MCP tools | [`create_sdk_mcp_server()`], [`tool()`] |
//! | External MCP servers | [`McpServerConfig`], [`McpServersOption`] |
//! | Session persistence | [`SessionStore`] |
//! | Session listing/rename/tag/fork | [`list_sessions()`], [`rename_session()`], [`fork_session()`] |
//!
//! # Requirements
//!
//! The Claude Code CLI must be installed and authenticated:
//! `npm install -g @anthropic-ai/claude-code`, then either set
//! `ANTHROPIC_API_KEY` or run `claude login` once.

mod callback_adapters;
mod client;
mod error;
mod mcp_server;
mod protocol;
mod query;
mod session_management;
pub mod transport;
pub mod types;

pub use error::{Error, Result};
pub use types::hook::{
    ALL_HOOK_EVENTS, HookCallback, HookContext, HookEvent, HookMatcher, HookOutput, hook_callback,
};
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
pub use types::permission::{
    CanUseToolCallback, PermissionMode, PermissionResult, PermissionRuleValue, PermissionUpdate,
    ToolPermissionRequest, can_use_tool_callback,
};
pub use types::session_store::{
    BoxFuture, SessionKey, SessionListSubkeysKey, SessionStore, SessionStoreEntry,
    SessionStoreFlushMode, SessionStoreListEntry, SessionSummaryEntry,
};

pub use client::ClaudeClient;
pub use mcp_server::{SdkMcpServer, SdkTool, ToolHandler, ToolResult, create_sdk_mcp_server, tool};
pub use query::{query, query_stream};
pub use session_management::{
    ForkSessionResult, InMemorySessionStore, SDKSessionInfo, SessionMessage, delete_session,
    delete_session_via_store, fold_session_summary, fork_session, fork_session_via_store,
    get_session_info, get_session_info_from_store, get_session_messages,
    get_session_messages_from_store, get_subagent_messages, get_subagent_messages_from_store,
    import_session_to_store, list_sessions, list_sessions_from_store, list_subagents,
    list_subagents_from_store, project_key_for_directory, rename_session, rename_session_via_store,
    tag_session, tag_session_via_store,
};
pub use transport::Transport;
pub use transport::subprocess::{SubprocessTransport, find_cli, full_command_args};
