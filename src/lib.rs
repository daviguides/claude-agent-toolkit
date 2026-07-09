//! Idiomatic Rust port of the official Claude Agent SDK.
//!
//! Wraps the Claude Code CLI as a subprocess and exposes a typed,
//! async API for one-shot queries and interactive agent sessions.

mod error;
pub mod types;

pub use error::{Error, Result};
pub use types::message::{
    AssistantMessage, ContentBlock, DeferredToolUse, HookEventMessage, Message, MirrorErrorMessage,
    RateLimitEvent, RateLimitInfo, ResultMessage, StreamEvent, SystemMessage,
    TERMINAL_TASK_STATUSES, TaskNotificationMessage, TaskProgressMessage, TaskStartedMessage,
    TaskUpdatedMessage, TaskUsage, UserContent, UserMessage, parse_message,
};
