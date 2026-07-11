//! Session listing/query/mutation: upstream's `sessions.py`/
//! `session_mutations.py`/`session_store.py` surface. Two families,
//! sharing the pure types and path-resolution logic in this module:
//! - `store` — the `_from_store`/`_via_store` family, built entirely
//!   on the [`SessionStore`] trait Phase 3 already defined.
//! - `disk` — the direct-local-disk family, reading/writing the same
//!   `~/.claude/projects/...` JSONL files the CLI itself writes.
//!
//! See `DEVIATIONS.md`'s Phase 10 entry for why these shipped later
//! than the rest of this port.

pub mod disk;
pub(crate) mod iso8601;
pub(crate) mod paths;
mod store;
mod summary;
pub(crate) mod unicode_sanitize;

use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub use disk::{
    delete_session, fork_session, get_session_info, get_session_messages, get_subagent_messages,
    import_session_to_store, list_sessions, list_subagents, rename_session, tag_session,
};
pub use paths::project_key_for_directory;
pub use store::{
    InMemorySessionStore, delete_session_via_store, fork_session_via_store,
    get_session_info_from_store, get_session_messages_from_store, get_subagent_messages_from_store,
    list_sessions_from_store, list_subagents_from_store, rename_session_via_store,
    tag_session_via_store,
};
pub use summary::fold_session_summary;

/// Summary information about one session, as returned by
/// [`get_session_info_from_store`] and [`list_sessions_from_store`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SDKSessionInfo {
    /// Session identifier.
    pub session_id: String,
    /// Best-available human-readable summary (custom title, last
    /// prompt, summary hint, or first prompt, in that priority order).
    pub summary: String,
    /// Last-modified time, in Unix epoch milliseconds.
    pub last_modified: i64,
    /// Transcript size in bytes, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_size: Option<i64>,
    /// User-set title, when one was given via [`rename_session_via_store`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_title: Option<String>,
    /// First non-command user prompt in the session, when derivable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_prompt: Option<String>,
    /// Git branch active when the session started, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_branch: Option<String>,
    /// Working directory the session ran in, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    /// User-set tag, when one was given via [`tag_session_via_store`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
    /// Session creation time, in Unix epoch milliseconds, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<i64>,
}

/// One top-level transcript message, as returned by
/// [`get_session_messages_from_store`]/[`get_subagent_messages_from_store`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionMessage {
    /// `"user"` or `"assistant"`.
    #[serde(rename = "type")]
    pub message_type: String,
    /// Unique message identifier.
    pub uuid: String,
    /// Session this message belongs to.
    pub session_id: String,
    /// Raw message payload (role/content), kept as-is.
    pub message: Value,
    /// Always `None` for top-level messages — sidechain/tool-use
    /// entries are filtered out before the chain is built.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_tool_use_id: Option<String>,
}

/// Result of [`fork_session_via_store`]: the id of the newly created
/// session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ForkSessionResult {
    /// Id of the forked session.
    pub session_id: String,
}

static UUID_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$")
        .expect("valid regex")
});

/// Whether `session_id` is a well-formed UUID. Read functions return
/// an empty/`None` result for an invalid id; mutations reject it with
/// [`crate::Error::InvalidSessionId`].
#[must_use]
pub(crate) fn is_valid_session_id(session_id: &str) -> bool {
    UUID_RE.is_match(session_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sdk_session_info_omits_absent_optional_fields() {
        let info = SDKSessionInfo {
            session_id: "s".to_string(),
            summary: "hello".to_string(),
            last_modified: 1,
            file_size: None,
            custom_title: None,
            first_prompt: None,
            git_branch: None,
            cwd: None,
            tag: None,
            created_at: None,
        };
        let json = serde_json::to_value(&info).expect("serializes");
        assert_eq!(
            json,
            serde_json::json!({"session_id": "s", "summary": "hello", "last_modified": 1})
        );
    }

    #[test]
    fn session_message_uses_type_wire_key() {
        let message = SessionMessage {
            message_type: "user".to_string(),
            uuid: "u".to_string(),
            session_id: "s".to_string(),
            message: serde_json::json!({"role": "user", "content": "hi"}),
            parent_tool_use_id: None,
        };
        let json = serde_json::to_value(&message).expect("serializes");
        assert_eq!(json["type"], "user");
        assert!(json.get("parent_tool_use_id").is_none());
    }

    #[test]
    fn is_valid_session_id_accepts_uuid_and_rejects_garbage() {
        assert!(is_valid_session_id("550e8400-e29b-41d4-a716-446655440000"));
        assert!(is_valid_session_id("550E8400-E29B-41D4-A716-446655440000"));
        assert!(!is_valid_session_id("not-a-uuid"));
        assert!(!is_valid_session_id(""));
    }

    #[test]
    fn fork_session_result_round_trips() {
        let result = ForkSessionResult {
            session_id: "forked-1".to_string(),
        };
        let json = serde_json::to_value(&result).expect("serializes");
        let parsed: ForkSessionResult = serde_json::from_value(json).expect("deserializes");
        assert_eq!(parsed, result);
    }
}
