//! Adapter for mirroring session transcripts to external storage.
//!
//! The subprocess still writes to local disk; a configured
//! [`SessionStore`] receives a secondary copy. Only [`SessionStore::append`]
//! and [`SessionStore::load`] are required — the rest default to
//! [`Error::NotImplemented`], matching
//! upstream's `NotImplementedError`-as-absence-marker convention.

use std::future::Future;
use std::pin::Pin;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{Error, Result};

/// A future boxed for dyn-compatible async trait methods.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Identifies a session transcript, or a subagent transcript, in a store.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionKey {
    /// Caller-defined scope; defaults to the sanitized cwd.
    pub project_key: String,
    /// Session identifier.
    pub session_id: String,
    /// Set for subagent transcripts (e.g. `"subagents/agent-{id}"`);
    /// omitted for the main transcript.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subpath: Option<String>,
}

/// One JSONL transcript line, as observed by a [`SessionStore`] adapter.
///
/// The concrete shape is the CLI's internal transcript format; adapters
/// treat entries as opaque pass-through JSON.
pub type SessionStoreEntry = Value;

/// Entry returned by [`SessionStore::list_sessions`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionStoreListEntry {
    /// Session identifier.
    pub session_id: String,
    /// Last-modified time, in Unix epoch milliseconds.
    pub mtime: i64,
}

/// Incrementally-maintained session summary, returned by
/// [`SessionStore::list_session_summaries`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionSummaryEntry {
    /// Session identifier.
    pub session_id: String,
    /// Storage write time of the summary, in Unix epoch milliseconds.
    pub mtime: i64,
    /// Opaque SDK-owned summary state; adapters persist it verbatim.
    pub data: Value,
}

/// Key argument to [`SessionStore::list_subkeys`] (no `subpath`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionListSubkeysKey {
    /// Caller-defined scope.
    pub project_key: String,
    /// Session identifier.
    pub session_id: String,
}

/// Controls when transcript-mirror entries are flushed to a
/// [`SessionStore`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum SessionStoreFlushMode {
    /// Buffer entries and flush once per turn, or when the pending
    /// buffer exceeds 500 entries / 1 MiB.
    #[default]
    #[serde(rename = "batched")]
    Batched,
    /// Trigger a background flush after every mirrored frame.
    #[serde(rename = "eager")]
    Eager,
}

/// Adapter for mirroring session transcripts to external storage.
///
/// Only [`append`](SessionStore::append) and [`load`](SessionStore::load)
/// are required; the rest default to
/// [`Error::NotImplemented`] so a minimal adapter can implement just the
/// two mandatory methods.
pub trait SessionStore: Send + Sync {
    /// Mirrors a batch of transcript entries.
    ///
    /// Called after the subprocess's local write already succeeded —
    /// durability is already guaranteed locally.
    ///
    /// # Errors
    ///
    /// Returns an adapter-specific error on persistence failure.
    fn append<'a>(
        &'a self,
        key: &'a SessionKey,
        entries: Vec<SessionStoreEntry>,
    ) -> BoxFuture<'a, Result<()>>;

    /// Loads a full session for resume. Returns `None` for a key that
    /// was never written.
    ///
    /// # Errors
    ///
    /// Returns an adapter-specific error on read failure.
    fn load<'a>(
        &'a self,
        key: &'a SessionKey,
    ) -> BoxFuture<'a, Result<Option<Vec<SessionStoreEntry>>>>;

    /// Lists sessions for `project_key`.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NotImplemented`] unless overridden.
    fn list_sessions<'a>(
        &'a self,
        _project_key: &'a str,
    ) -> BoxFuture<'a, Result<Vec<SessionStoreListEntry>>> {
        Box::pin(async { Err(not_implemented("list_sessions")) })
    }

    /// Returns incrementally-maintained summaries for all sessions
    /// under `project_key`.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NotImplemented`] unless overridden.
    fn list_session_summaries<'a>(
        &'a self,
        _project_key: &'a str,
    ) -> BoxFuture<'a, Result<Vec<SessionSummaryEntry>>> {
        Box::pin(async { Err(not_implemented("list_session_summaries")) })
    }

    /// Deletes a session. A main-transcript key (no `subpath`) cascades
    /// to all subkeys under that session.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NotImplemented`] unless overridden.
    fn delete<'a>(&'a self, _key: &'a SessionKey) -> BoxFuture<'a, Result<()>> {
        Box::pin(async { Err(not_implemented("delete")) })
    }

    /// Lists all subpath keys under a session (e.g. subagent transcripts).
    ///
    /// # Errors
    ///
    /// Returns [`Error::NotImplemented`] unless overridden.
    fn list_subkeys<'a>(
        &'a self,
        _key: &'a SessionListSubkeysKey,
    ) -> BoxFuture<'a, Result<Vec<String>>> {
        Box::pin(async { Err(not_implemented("list_subkeys")) })
    }
}

fn not_implemented(operation: &str) -> Error {
    Error::NotImplemented {
        operation: operation.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MinimalStore;

    impl SessionStore for MinimalStore {
        fn append<'a>(
            &'a self,
            _key: &'a SessionKey,
            _entries: Vec<SessionStoreEntry>,
        ) -> BoxFuture<'a, Result<()>> {
            Box::pin(async { Ok(()) })
        }

        fn load<'a>(
            &'a self,
            _key: &'a SessionKey,
        ) -> BoxFuture<'a, Result<Option<Vec<SessionStoreEntry>>>> {
            Box::pin(async { Ok(None) })
        }
    }

    fn sample_key() -> SessionKey {
        SessionKey {
            project_key: "proj".to_string(),
            session_id: "sess".to_string(),
            subpath: None,
        }
    }

    #[tokio::test]
    async fn minimal_store_implements_required_methods() {
        let store = MinimalStore;
        let key = sample_key();
        store.append(&key, vec![]).await.expect("append succeeds");
        assert_eq!(store.load(&key).await.expect("load succeeds"), None);
    }

    #[tokio::test]
    async fn optional_methods_default_to_not_implemented() {
        let store = MinimalStore;
        let key = sample_key();
        let subkeys_key = SessionListSubkeysKey {
            project_key: "proj".to_string(),
            session_id: "sess".to_string(),
        };

        assert!(matches!(
            store.list_sessions("proj").await,
            Err(Error::NotImplemented { .. })
        ));
        assert!(matches!(
            store.list_session_summaries("proj").await,
            Err(Error::NotImplemented { .. })
        ));
        assert!(matches!(
            store.delete(&key).await,
            Err(Error::NotImplemented { .. })
        ));
        assert!(matches!(
            store.list_subkeys(&subkeys_key).await,
            Err(Error::NotImplemented { .. })
        ));
    }

    #[test]
    fn session_store_flush_mode_defaults_to_batched() {
        assert_eq!(
            SessionStoreFlushMode::default(),
            SessionStoreFlushMode::Batched
        );
    }

    #[test]
    fn session_store_flush_mode_serde_wire_strings() {
        assert_eq!(
            serde_json::to_string(&SessionStoreFlushMode::Batched).unwrap(),
            "\"batched\""
        );
        assert_eq!(
            serde_json::to_string(&SessionStoreFlushMode::Eager).unwrap(),
            "\"eager\""
        );
    }

    #[test]
    fn session_key_skips_subpath_when_none() {
        let key = sample_key();
        let json = serde_json::to_value(&key).expect("serializes");
        assert!(json.get("subpath").is_none());
    }
}
