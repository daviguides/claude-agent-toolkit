//! The `_from_store`/`_via_store` family: session listing/query/
//! mutation built entirely on the [`SessionStore`] trait. See
//! `session_store.py` (functions) and `session_store.py`'s
//! `InMemorySessionStore` (the reference adapter).

use std::collections::{HashMap, HashSet};
use std::sync::Mutex as StdMutex;

use serde_json::{Value, json};
use uuid::Uuid;

use crate::error::{Error, Result};
use crate::session_management::iso8601::{iso_now, now_millis};
use crate::session_management::paths::project_key_for_directory;
use crate::session_management::summary::{fold_session_summary, summary_entry_to_sdk_info};
use crate::session_management::unicode_sanitize::sanitize_unicode;
use crate::session_management::{
    ForkSessionResult, SDKSessionInfo, SessionMessage, is_valid_session_id,
};
use crate::types::session_store::{
    BoxFuture, SessionKey, SessionListSubkeysKey, SessionStore, SessionStoreEntry,
    SessionStoreListEntry, SessionSummaryEntry,
};

/// Bound on concurrent per-session `load()` calls in
/// [`list_sessions_from_store`]'s slow path (no `list_session_summaries`).
const LIST_LOAD_CONCURRENCY: usize = 16;

/// Reference [`SessionStore`] implementation backed by in-process
/// `HashMap`s — a Rust port of upstream's own `InMemorySessionStore`
/// (used in its conformance test suite; suitable here for tests and
/// small single-process tools, not production persistence).
#[derive(Default)]
pub struct InMemorySessionStore {
    state: StdMutex<State>,
}

#[derive(Default)]
struct State {
    entries: HashMap<String, Vec<SessionStoreEntry>>,
    mtimes: HashMap<String, i64>,
    summaries: HashMap<(String, String), SessionSummaryEntry>,
    last_mtime: i64,
}

impl State {
    /// Monotonic millisecond clock: never returns the same value twice
    /// in a row, even across back-to-back calls within the same
    /// millisecond.
    fn next_mtime(&mut self) -> i64 {
        let now = now_millis();
        let next = if now > self.last_mtime {
            now
        } else {
            self.last_mtime + 1
        };
        self.last_mtime = next;
        next
    }
}

fn storage_key(key: &SessionKey) -> String {
    match &key.subpath {
        Some(subpath) => format!("{}/{}/{}", key.project_key, key.session_id, subpath),
        None => format!("{}/{}", key.project_key, key.session_id),
    }
}

impl InMemorySessionStore {
    /// Creates an empty store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Test/inspection helper: all entries stored under `key`, or
    /// `None` if nothing was ever appended there.
    #[must_use]
    pub fn get_entries(&self, key: &SessionKey) -> Option<Vec<SessionStoreEntry>> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .entries
            .get(&storage_key(key))
            .cloned()
    }

    /// Test/inspection helper: total number of distinct storage keys
    /// (main transcripts + subagent transcripts combined).
    #[must_use]
    pub fn size(&self) -> usize {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .entries
            .len()
    }

    /// Test/inspection helper: clears all stored state.
    pub fn clear(&self) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *state = State::default();
    }
}

impl SessionStore for InMemorySessionStore {
    fn append<'a>(
        &'a self,
        key: &'a SessionKey,
        entries: Vec<SessionStoreEntry>,
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let mut state = self
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let mtime = state.next_mtime();

            if key.subpath.is_none() {
                let summary_key = (key.project_key.clone(), key.session_id.clone());
                let prev = state.summaries.get(&summary_key).cloned();
                let mut summary = fold_session_summary(prev.as_ref(), key, &entries);
                summary.mtime = mtime;
                state.summaries.insert(summary_key, summary);
            }

            let key = storage_key(key);
            state
                .entries
                .entry(key.clone())
                .or_default()
                .extend(entries);
            state.mtimes.insert(key, mtime);
            Ok(())
        })
    }

    fn load<'a>(
        &'a self,
        key: &'a SessionKey,
    ) -> BoxFuture<'a, Result<Option<Vec<SessionStoreEntry>>>> {
        Box::pin(async move {
            let state = self
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            Ok(state.entries.get(&storage_key(key)).cloned())
        })
    }

    fn list_sessions<'a>(
        &'a self,
        project_key: &'a str,
    ) -> BoxFuture<'a, Result<Vec<SessionStoreListEntry>>> {
        Box::pin(async move {
            let state = self
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let prefix = format!("{project_key}/");
            Ok(state
                .mtimes
                .iter()
                .filter_map(|(key, mtime)| {
                    let rest = key.strip_prefix(&prefix)?;
                    (!rest.contains('/')).then(|| SessionStoreListEntry {
                        session_id: rest.to_string(),
                        mtime: *mtime,
                    })
                })
                .collect())
        })
    }

    fn list_session_summaries<'a>(
        &'a self,
        project_key: &'a str,
    ) -> BoxFuture<'a, Result<Vec<SessionSummaryEntry>>> {
        Box::pin(async move {
            let state = self
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            Ok(state
                .summaries
                .iter()
                .filter(|((entry_project_key, _), _)| entry_project_key == project_key)
                .map(|(_, summary)| summary.clone())
                .collect())
        })
    }

    fn delete<'a>(&'a self, key: &'a SessionKey) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let mut state = self
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let target = storage_key(key);
            state.entries.remove(&target);
            state.mtimes.remove(&target);

            if key.subpath.is_none() {
                let prefix = format!("{}/{}/", key.project_key, key.session_id);
                let subkeys: Vec<String> = state
                    .entries
                    .keys()
                    .filter(|entry_key| entry_key.starts_with(&prefix))
                    .cloned()
                    .collect();
                for subkey in subkeys {
                    state.entries.remove(&subkey);
                    state.mtimes.remove(&subkey);
                }
                state
                    .summaries
                    .remove(&(key.project_key.clone(), key.session_id.clone()));
            }
            Ok(())
        })
    }

    fn list_subkeys<'a>(
        &'a self,
        key: &'a SessionListSubkeysKey,
    ) -> BoxFuture<'a, Result<Vec<String>>> {
        Box::pin(async move {
            let state = self
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let prefix = format!("{}/{}/", key.project_key, key.session_id);
            Ok(state
                .entries
                .keys()
                .filter_map(|entry_key| entry_key.strip_prefix(&prefix))
                .map(str::to_string)
                .collect())
        })
    }
}

/// Applies `offset` then `limit` to `items`. `limit=Some(0)` means
/// "unlimited" (matches upstream's truthy/`is not None and > 0` check
/// on `limit`), not "return nothing".
fn paginate<T>(mut items: Vec<T>, limit: Option<usize>, offset: usize) -> Vec<T> {
    if offset > 0 {
        if offset >= items.len() {
            return Vec::new();
        }
        items.drain(..offset);
    }
    if let Some(limit) = limit
        && limit > 0
    {
        items.truncate(limit);
    }
    items
}

fn is_not_implemented(error: &Error) -> bool {
    matches!(error, Error::NotImplemented { .. })
}

/// Lists sessions under `directory` (defaults to the current working
/// directory), most-recently-modified first.
///
/// Prefers `store.list_session_summaries` (one batch call), gap-filling
/// any session `list_sessions` reports as newer than its cached summary
/// with a fresh `load()`. Falls back to loading every session directly
/// (bounded at `LIST_LOAD_CONCURRENCY` concurrent loads) when the
/// store doesn't implement `list_session_summaries`.
///
/// # Errors
///
/// Returns [`Error::Session`] when the store implements neither
/// `list_sessions` nor `list_session_summaries`.
pub async fn list_sessions_from_store(
    store: &dyn SessionStore,
    directory: Option<&str>,
    limit: Option<usize>,
    offset: usize,
) -> Result<Vec<SDKSessionInfo>> {
    let project_key = project_key_for_directory(directory);

    let summaries = match store.list_session_summaries(&project_key).await {
        Ok(summaries) => Some(summaries),
        Err(error) if is_not_implemented(&error) => None,
        Err(error) => return Err(error),
    };

    let infos = if let Some(summaries) = summaries {
        list_sessions_via_summaries(store, &project_key, summaries, directory).await?
    } else {
        list_sessions_via_full_loads(store, &project_key, directory, limit, offset).await?
    };

    let mut infos = infos;
    infos.sort_by(|a, b| b.last_modified.cmp(&a.last_modified));
    Ok(paginate(infos, limit, offset))
}

async fn list_sessions_via_summaries(
    store: &dyn SessionStore,
    project_key: &str,
    summaries: Vec<SessionSummaryEntry>,
    directory: Option<&str>,
) -> Result<Vec<SDKSessionInfo>> {
    let known_mtimes: HashMap<String, i64> = match store.list_sessions(project_key).await {
        Ok(sessions) => sessions
            .into_iter()
            .map(|entry| (entry.session_id, entry.mtime))
            .collect(),
        Err(error) if is_not_implemented(&error) => HashMap::new(),
        Err(error) => return Err(error),
    };

    let mut by_id: HashMap<String, SessionSummaryEntry> = summaries
        .into_iter()
        .map(|summary| (summary.session_id.clone(), summary))
        .collect();

    let stale_ids: Vec<String> = known_mtimes
        .iter()
        .filter(|(session_id, mtime)| {
            by_id
                .get(*session_id)
                .is_none_or(|summary| summary.mtime < **mtime)
        })
        .map(|(session_id, _)| session_id.clone())
        .collect();

    for session_id in stale_ids {
        let key = SessionKey {
            project_key: project_key.to_string(),
            session_id: session_id.clone(),
            subpath: None,
        };
        if let Ok(Some(entries)) = store.load(&key).await {
            let mtime = known_mtimes.get(&session_id).copied().unwrap_or_default();
            let mut summary = fold_session_summary(None, &key, &entries);
            summary.mtime = mtime;
            by_id.insert(session_id, summary);
        }
    }

    Ok(by_id
        .into_values()
        .filter_map(|summary| summary_entry_to_sdk_info(&summary, directory))
        .collect())
}

async fn list_sessions_via_full_loads(
    store: &dyn SessionStore,
    project_key: &str,
    directory: Option<&str>,
    limit: Option<usize>,
    offset: usize,
) -> Result<Vec<SDKSessionInfo>> {
    let sessions = match store.list_sessions(project_key).await {
        Ok(sessions) => sessions,
        Err(error) if is_not_implemented(&error) => {
            return Err(Error::Session {
                message: "store implements neither list_sessions nor list_session_summaries"
                    .to_string(),
            });
        }
        Err(error) => return Err(error),
    };

    // Sort + paginate BEFORE loading, so full-load cost is bounded by
    // page size rather than the whole corpus.
    let mut sessions = sessions;
    sessions.sort_by(|a, b| b.mtime.cmp(&a.mtime));
    let page = paginate(sessions, limit, offset);

    let mut infos = Vec::with_capacity(page.len());
    for chunk in page.chunks(LIST_LOAD_CONCURRENCY) {
        let loads = chunk.iter().map(|session| async {
            let key = SessionKey {
                project_key: project_key.to_string(),
                session_id: session.session_id.clone(),
                subpath: None,
            };
            let entries = store.load(&key).await.ok().flatten().unwrap_or_default();
            let mut summary = fold_session_summary(None, &key, &entries);
            summary.mtime = session.mtime;
            summary_entry_to_sdk_info(&summary, directory)
        });
        infos.extend(futures::future::join_all(loads).await.into_iter().flatten());
    }
    Ok(infos)
}

/// Fetches summary information for one session. Returns `None` for an
/// invalid session id, a session never written, or one with no
/// derivable summary (e.g. a sidechain-only session).
///
/// # Errors
///
/// Propagates any adapter-specific error from `store.load`.
pub async fn get_session_info_from_store(
    store: &dyn SessionStore,
    session_id: &str,
    directory: Option<&str>,
) -> Result<Option<SDKSessionInfo>> {
    if !is_valid_session_id(session_id) {
        return Ok(None);
    }
    let key = SessionKey {
        project_key: project_key_for_directory(directory),
        session_id: session_id.to_string(),
        subpath: None,
    };
    let Some(entries) = store.load(&key).await? else {
        return Ok(None);
    };
    let summary = fold_session_summary(None, &key, &entries);
    Ok(summary_entry_to_sdk_info(&summary, directory))
}

/// Fetches the visible (user/assistant, non-sidechain, non-meta)
/// message chain for one session, chronological order, paginated.
/// Returns an empty list for an invalid or unknown session id.
///
/// # Errors
///
/// Propagates any adapter-specific error from `store.load`.
pub async fn get_session_messages_from_store(
    store: &dyn SessionStore,
    session_id: &str,
    directory: Option<&str>,
    limit: Option<usize>,
    offset: usize,
) -> Result<Vec<SessionMessage>> {
    if !is_valid_session_id(session_id) {
        return Ok(Vec::new());
    }
    let key = SessionKey {
        project_key: project_key_for_directory(directory),
        session_id: session_id.to_string(),
        subpath: None,
    };
    let Some(entries) = store.load(&key).await? else {
        return Ok(Vec::new());
    };

    let messages: Vec<SessionMessage> = build_conversation_chain(&entries)
        .into_iter()
        .filter(is_visible_message)
        .filter_map(|entry| entry_to_session_message(&entry, session_id))
        .collect();
    Ok(paginate(messages, limit, offset))
}

/// Lists subagent ids that have transcripts under `session_id`.
/// Returns an empty list for an invalid session id.
///
/// # Errors
///
/// Returns [`Error::NotImplemented`] if the store doesn't implement
/// `list_subkeys`.
pub async fn list_subagents_from_store(
    store: &dyn SessionStore,
    session_id: &str,
    directory: Option<&str>,
) -> Result<Vec<String>> {
    if !is_valid_session_id(session_id) {
        return Ok(Vec::new());
    }
    let key = SessionListSubkeysKey {
        project_key: project_key_for_directory(directory),
        session_id: session_id.to_string(),
    };
    let subkeys = store.list_subkeys(&key).await?;

    let mut seen = HashSet::new();
    let mut agent_ids = Vec::new();
    for subkey in subkeys {
        let Some(rest) = subkey.strip_prefix("subagents/") else {
            continue;
        };
        let Some(segment) = rest.rsplit('/').next() else {
            continue;
        };
        let Some(agent_id) = segment.strip_prefix("agent-") else {
            continue;
        };
        if seen.insert(agent_id.to_string()) {
            agent_ids.push(agent_id.to_string());
        }
    }
    Ok(agent_ids)
}

/// Fetches the message chain for one subagent transcript under
/// `session_id`. Resolves the exact subpath via `list_subkeys` (so
/// nested `subagents/workflows/<run>/agent-<id>` transcripts are
/// found); falls back to the flat `subagents/agent-{id}` path when the
/// store doesn't implement `list_subkeys`. Returns an empty list for
/// an invalid or unknown session/agent id.
///
/// # Errors
///
/// Propagates any adapter-specific error from `store.load`.
pub async fn get_subagent_messages_from_store(
    store: &dyn SessionStore,
    session_id: &str,
    agent_id: &str,
    directory: Option<&str>,
    limit: Option<usize>,
    offset: usize,
) -> Result<Vec<SessionMessage>> {
    if !is_valid_session_id(session_id) {
        return Ok(Vec::new());
    }
    let project_key = project_key_for_directory(directory);
    let suffix = format!("agent-{agent_id}");

    let subkeys_key = SessionListSubkeysKey {
        project_key: project_key.clone(),
        session_id: session_id.to_string(),
    };
    let subpath = match store.list_subkeys(&subkeys_key).await {
        Ok(subkeys) => subkeys
            .into_iter()
            .find(|subkey| subkey.starts_with("subagents/") && subkey.ends_with(&suffix)),
        Err(error) if is_not_implemented(&error) => None,
        Err(error) => return Err(error),
    }
    .unwrap_or_else(|| format!("subagents/{suffix}"));

    let key = SessionKey {
        project_key,
        session_id: session_id.to_string(),
        subpath: Some(subpath),
    };
    let Some(entries) = store.load(&key).await? else {
        return Ok(Vec::new());
    };
    let entries: Vec<Value> = entries
        .into_iter()
        .filter(|entry| entry.get("type").and_then(Value::as_str) != Some("agent_metadata"))
        .collect();

    let messages: Vec<SessionMessage> = build_subagent_chain(&entries)
        .into_iter()
        .filter(is_visible_message)
        .filter_map(|entry| entry_to_session_message(&entry, session_id))
        .collect();
    Ok(paginate(messages, limit, offset))
}

/// Appends a `custom-title` entry setting `session_id`'s display
/// title. `title` is trimmed before storing.
///
/// # Errors
///
/// Returns [`Error::InvalidSessionId`] for a malformed `session_id`,
/// or [`Error::Session`] if `title` is empty after trimming.
/// Propagates any adapter-specific error from `store.append`.
pub async fn rename_session_via_store(
    store: &dyn SessionStore,
    session_id: &str,
    title: &str,
    directory: Option<&str>,
) -> Result<()> {
    if !is_valid_session_id(session_id) {
        return Err(Error::InvalidSessionId {
            session_id: session_id.to_string(),
        });
    }
    let stripped = title.trim();
    if stripped.is_empty() {
        return Err(Error::Session {
            message: "title must be non-empty".to_string(),
        });
    }
    let key = SessionKey {
        project_key: project_key_for_directory(directory),
        session_id: session_id.to_string(),
        subpath: None,
    };
    let entry = json!({
        "type": "custom-title",
        "customTitle": stripped,
        "sessionId": session_id,
        "uuid": Uuid::new_v4().to_string(),
        "timestamp": iso_now(),
    });
    store.append(&key, vec![entry]).await
}

/// Appends a `tag` entry setting (or, with `tag: None`, clearing)
/// `session_id`'s tag. `tag` is sanitized (`sanitize_unicode`) then
/// trimmed — an explicit `Some("")` (or a tag that sanitizes/trims
/// down to nothing) is rejected, not treated as clearing; only `None`
/// clears.
///
/// # Errors
///
/// Returns [`Error::InvalidSessionId`] for a malformed `session_id`, or
/// [`Error::Session`] when an explicit (non-`None`) tag is empty after
/// sanitizing and trimming. Propagates any adapter-specific error from
/// `store.append`.
pub async fn tag_session_via_store(
    store: &dyn SessionStore,
    session_id: &str,
    tag: Option<&str>,
    directory: Option<&str>,
) -> Result<()> {
    if !is_valid_session_id(session_id) {
        return Err(Error::InvalidSessionId {
            session_id: session_id.to_string(),
        });
    }
    let sanitized = match tag {
        None => String::new(),
        Some(tag) => {
            let cleaned = sanitize_unicode(tag).trim().to_string();
            if cleaned.is_empty() {
                return Err(Error::Session {
                    message: "tag must be non-empty".to_string(),
                });
            }
            cleaned
        }
    };

    let key = SessionKey {
        project_key: project_key_for_directory(directory),
        session_id: session_id.to_string(),
        subpath: None,
    };
    let entry = json!({
        "type": "tag",
        "tag": sanitized,
        "sessionId": session_id,
        "uuid": Uuid::new_v4().to_string(),
        "timestamp": iso_now(),
    });
    store.append(&key, vec![entry]).await
}

/// Deletes a session (and its subagent transcripts). A no-op, not an
/// error, when the store doesn't implement `delete` — correct for
/// write-once/append-only backends.
///
/// # Errors
///
/// Returns [`Error::InvalidSessionId`] for a malformed `session_id`.
/// Propagates any other adapter-specific error from `store.delete`.
pub async fn delete_session_via_store(
    store: &dyn SessionStore,
    session_id: &str,
    directory: Option<&str>,
) -> Result<()> {
    if !is_valid_session_id(session_id) {
        return Err(Error::InvalidSessionId {
            session_id: session_id.to_string(),
        });
    }
    let key = SessionKey {
        project_key: project_key_for_directory(directory),
        session_id: session_id.to_string(),
        subpath: None,
    };
    match store.delete(&key).await {
        Ok(()) | Err(Error::NotImplemented { .. }) => Ok(()),
        Err(error) => Err(error),
    }
}

/// Forks `session_id` at `up_to_message_id` (or the end of the
/// transcript when `None`) into a brand-new session, remapping every
/// message uuid and rebuilding the `parentUuid` chain. The forked
/// session gets a title: `title` if given, else derived from the
/// source's own title/first-prompt, suffixed `" (fork)"`.
///
/// # Errors
///
/// Returns [`Error::InvalidSessionId`] for a malformed `session_id`,
/// [`Error::Session`] if the source session has no messages (or
/// `up_to_message_id` doesn't match any message). Propagates any
/// adapter-specific error from `store.load`/`store.append`.
pub async fn fork_session_via_store(
    store: &dyn SessionStore,
    session_id: &str,
    directory: Option<&str>,
    up_to_message_id: Option<&str>,
    title: Option<&str>,
) -> Result<ForkSessionResult> {
    if !is_valid_session_id(session_id) {
        return Err(Error::InvalidSessionId {
            session_id: session_id.to_string(),
        });
    }
    let project_key = project_key_for_directory(directory);
    let source_key = SessionKey {
        project_key: project_key.clone(),
        session_id: session_id.to_string(),
        subpath: None,
    };
    let entries = store.load(&source_key).await?.unwrap_or_default();

    let forked_id = Uuid::new_v4().to_string();
    let derive_title = || {
        let summary = fold_session_summary(None, &source_key, &entries);
        summary
            .data
            .get("custom_title")
            .and_then(Value::as_str)
            .or_else(|| summary.data.get("ai_title").and_then(Value::as_str))
            .map(str::to_string)
            .or_else(|| {
                summary
                    .data
                    .get("first_prompt")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
    };
    let lines = crate::session_management::disk::build_fork_lines(
        &entries,
        session_id,
        &forked_id,
        up_to_message_id,
        title,
        derive_title,
    )?;

    let target_key = SessionKey {
        project_key,
        session_id: forked_id.clone(),
        subpath: None,
    };
    store.append(&target_key, lines).await?;
    Ok(ForkSessionResult {
        session_id: forked_id,
    })
}

/// Whether an entry belongs in a caller-visible message chain: a
/// `user`/`assistant` entry that isn't a sidechain, a meta entry, or
/// attributed to a team member.
fn is_visible_message(entry: &Value) -> bool {
    matches!(
        entry.get("type").and_then(Value::as_str),
        Some("user" | "assistant")
    ) && entry.get("isMeta").and_then(Value::as_bool) != Some(true)
        && entry.get("isSidechain").and_then(Value::as_bool) != Some(true)
        && entry.get("teamName").is_none()
}

fn entry_to_session_message(entry: &Value, session_id: &str) -> Option<SessionMessage> {
    Some(SessionMessage {
        message_type: entry.get("type")?.as_str()?.to_string(),
        uuid: entry.get("uuid")?.as_str()?.to_string(),
        session_id: session_id.to_string(),
        message: entry.get("message").cloned().unwrap_or(Value::Null),
        parent_tool_use_id: None,
    })
}

/// Builds the chronological chain a caller cares about from a full
/// (possibly branching, possibly containing sidechains/compaction
/// records) transcript: finds every entry no other entry points to as
/// its parent, walks each back to the nearest `user`/`assistant`
/// ancestor (a leaf candidate), prefers the latest (highest file-order
/// index) main-chain (non-sidechain/team/meta) candidate — falling
/// back to the latest candidate overall if none qualify — then walks
/// that leaf's `parentUuid` chain back to the root and returns it in
/// chronological order. Cycle-protected.
pub(crate) fn build_conversation_chain(entries: &[Value]) -> Vec<Value> {
    let by_uuid: HashMap<&str, usize> = entries
        .iter()
        .enumerate()
        .filter_map(|(index, entry)| {
            entry
                .get("uuid")
                .and_then(Value::as_str)
                .map(|uuid| (uuid, index))
        })
        .collect();

    let mut referenced: HashSet<&str> = HashSet::new();
    for entry in entries {
        if let Some(parent) = entry.get("parentUuid").and_then(Value::as_str) {
            referenced.insert(parent);
        }
    }

    let terminals: Vec<usize> = entries
        .iter()
        .enumerate()
        .filter_map(|(index, entry)| {
            let uuid = entry.get("uuid").and_then(Value::as_str)?;
            (!referenced.contains(uuid)).then_some(index)
        })
        .collect();

    let walk_to_message = |start: usize| -> Option<usize> {
        let mut index = start;
        let mut seen = HashSet::new();
        loop {
            if !seen.insert(index) {
                return None;
            }
            let entry = &entries[index];
            if matches!(
                entry.get("type").and_then(Value::as_str),
                Some("user" | "assistant")
            ) {
                return Some(index);
            }
            let parent_uuid = entry.get("parentUuid").and_then(Value::as_str)?;
            index = *by_uuid.get(parent_uuid)?;
        }
    };

    let candidates: Vec<usize> = terminals.into_iter().filter_map(walk_to_message).collect();

    let is_main_entry = |index: &usize| {
        let entry = &entries[*index];
        entry.get("isSidechain").and_then(Value::as_bool) != Some(true)
            && entry.get("teamName").is_none()
            && entry.get("isMeta").and_then(Value::as_bool) != Some(true)
    };
    let main_leaves: Vec<usize> = candidates.iter().copied().filter(is_main_entry).collect();

    let leaf = if main_leaves.is_empty() {
        candidates.into_iter().max()
    } else {
        main_leaves.into_iter().max()
    };

    let Some(mut index) = leaf else {
        return Vec::new();
    };
    let mut chain_indices = Vec::new();
    let mut seen = HashSet::new();
    loop {
        if !seen.insert(index) {
            break;
        }
        chain_indices.push(index);
        let Some(parent_uuid) = entries[index].get("parentUuid").and_then(Value::as_str) else {
            break;
        };
        let Some(&parent_index) = by_uuid.get(parent_uuid) else {
            break;
        };
        index = parent_index;
    }
    chain_indices.reverse();
    chain_indices
        .into_iter()
        .map(|i| entries[i].clone())
        .collect()
}

/// Builds a subagent transcript's chain: subagent transcripts are
/// linear (no compaction/sidechains/preserved-segments), so this is
/// just "last `user`/`assistant` entry in file order, walk `parentUuid`
/// back to the root."
pub(crate) fn build_subagent_chain(entries: &[Value]) -> Vec<Value> {
    let by_uuid: HashMap<&str, usize> = entries
        .iter()
        .enumerate()
        .filter_map(|(index, entry)| {
            entry
                .get("uuid")
                .and_then(Value::as_str)
                .map(|uuid| (uuid, index))
        })
        .collect();

    let Some(mut index) = entries.iter().enumerate().rev().find_map(|(index, entry)| {
        matches!(
            entry.get("type").and_then(Value::as_str),
            Some("user" | "assistant")
        )
        .then_some(index)
    }) else {
        return Vec::new();
    };

    let mut chain_indices = Vec::new();
    let mut seen = HashSet::new();
    loop {
        if !seen.insert(index) {
            break;
        }
        chain_indices.push(index);
        let Some(parent_uuid) = entries[index].get("parentUuid").and_then(Value::as_str) else {
            break;
        };
        let Some(&parent_index) = by_uuid.get(parent_uuid) else {
            break;
        };
        index = parent_index;
    }
    chain_indices.reverse();
    chain_indices
        .into_iter()
        .map(|i| entries[i].clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(project_key: &str, session_id: &str) -> SessionKey {
        SessionKey {
            project_key: project_key.to_string(),
            session_id: session_id.to_string(),
            subpath: None,
        }
    }

    fn user_message(uuid: &str, parent: Option<&str>, text: &str) -> Value {
        json!({
            "type": "user",
            "uuid": uuid,
            "parentUuid": parent,
            "message": {"role": "user", "content": text},
        })
    }

    #[tokio::test]
    async fn append_then_load_round_trips() {
        let store = InMemorySessionStore::new();
        let key = key("proj", "sess");
        store
            .append(&key, vec![user_message("u1", None, "hi")])
            .await
            .expect("appends");
        let loaded = store.load(&key).await.expect("loads").expect("has entries");
        assert_eq!(loaded.len(), 1);
    }

    #[tokio::test]
    async fn load_of_unknown_key_is_none() {
        let store = InMemorySessionStore::new();
        assert_eq!(store.load(&key("proj", "missing")).await.unwrap(), None);
    }

    #[tokio::test]
    async fn list_sessions_only_returns_main_transcripts() {
        let store = InMemorySessionStore::new();
        store
            .append(&key("proj", "sess"), vec![user_message("u1", None, "hi")])
            .await
            .unwrap();
        store
            .append(
                &SessionKey {
                    project_key: "proj".to_string(),
                    session_id: "sess".to_string(),
                    subpath: Some("subagents/agent-1".to_string()),
                },
                vec![user_message("u2", None, "sub")],
            )
            .await
            .unwrap();
        let sessions = store.list_sessions("proj").await.unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].session_id, "sess");
    }

    #[tokio::test]
    async fn delete_cascades_to_subkeys_and_summary() {
        let store = InMemorySessionStore::new();
        let main_key = key("proj", "sess");
        store
            .append(&main_key, vec![user_message("u1", None, "hi")])
            .await
            .unwrap();
        let sub_key = SessionKey {
            project_key: "proj".to_string(),
            session_id: "sess".to_string(),
            subpath: Some("subagents/agent-1".to_string()),
        };
        store
            .append(&sub_key, vec![user_message("u2", None, "sub")])
            .await
            .unwrap();

        store.delete(&main_key).await.unwrap();

        assert_eq!(store.load(&main_key).await.unwrap(), None);
        assert_eq!(store.load(&sub_key).await.unwrap(), None);
        assert!(
            store
                .list_session_summaries("proj")
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn list_subkeys_returns_subpaths_under_session() {
        let store = InMemorySessionStore::new();
        let sub_key = SessionKey {
            project_key: "proj".to_string(),
            session_id: "sess".to_string(),
            subpath: Some("subagents/agent-1".to_string()),
        };
        store
            .append(&sub_key, vec![user_message("u1", None, "sub")])
            .await
            .unwrap();
        let subkeys = store
            .list_subkeys(&SessionListSubkeysKey {
                project_key: "proj".to_string(),
                session_id: "sess".to_string(),
            })
            .await
            .unwrap();
        assert_eq!(subkeys, vec!["subagents/agent-1".to_string()]);
    }

    #[tokio::test]
    async fn get_session_info_from_store_none_for_invalid_uuid() {
        let store = InMemorySessionStore::new();
        assert_eq!(
            get_session_info_from_store(&store, "not-a-uuid", None)
                .await
                .unwrap(),
            None
        );
    }

    #[tokio::test]
    async fn get_session_info_from_store_none_for_unknown_session() {
        let store = InMemorySessionStore::new();
        assert_eq!(
            get_session_info_from_store(&store, "550e8400-e29b-41d4-a716-446655440000", None)
                .await
                .unwrap(),
            None
        );
    }

    #[tokio::test]
    async fn get_session_info_from_store_returns_summary() {
        let store = InMemorySessionStore::new();
        let session_id = "550e8400-e29b-41d4-a716-446655440000";
        let key = key("proj", session_id);
        store
            .append(&key, vec![user_message("u1", None, "hello world")])
            .await
            .unwrap();
        let info = get_session_info_from_store(&store, session_id, Some("proj"))
            .await
            .unwrap()
            .expect("has info");
        assert_eq!(info.first_prompt.as_deref(), Some("hello world"));
    }

    #[tokio::test]
    async fn get_session_messages_from_store_empty_for_invalid_uuid() {
        let store = InMemorySessionStore::new();
        let messages = get_session_messages_from_store(&store, "bad", None, None, 0)
            .await
            .unwrap();
        assert!(messages.is_empty());
    }

    #[tokio::test]
    async fn get_session_messages_from_store_builds_linear_chain() {
        let store = InMemorySessionStore::new();
        let session_id = "550e8400-e29b-41d4-a716-446655440000";
        let key = key("proj", session_id);
        store
            .append(
                &key,
                vec![
                    user_message("u1", None, "first"),
                    json!({"type": "assistant", "uuid": "a1", "parentUuid": "u1", "message": {"role": "assistant", "content": "reply"}}),
                ],
            )
            .await
            .unwrap();
        let messages = get_session_messages_from_store(&store, session_id, Some("proj"), None, 0)
            .await
            .unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].uuid, "u1");
        assert_eq!(messages[1].uuid, "a1");
    }

    #[tokio::test]
    async fn get_session_messages_from_store_filters_sidechain() {
        let store = InMemorySessionStore::new();
        let session_id = "550e8400-e29b-41d4-a716-446655440000";
        let key = key("proj", session_id);
        store
            .append(
                &key,
                vec![
                    json!({"type": "user", "uuid": "u1", "parentUuid": null, "isSidechain": true, "message": {"content": "side"}}),
                ],
            )
            .await
            .unwrap();
        let messages = get_session_messages_from_store(&store, session_id, None, None, 0)
            .await
            .unwrap();
        assert!(messages.is_empty());
    }

    #[tokio::test]
    async fn rename_session_via_store_rejects_invalid_uuid() {
        let store = InMemorySessionStore::new();
        let err = rename_session_via_store(&store, "bad", "title", None)
            .await
            .unwrap_err();
        assert!(matches!(err, Error::InvalidSessionId { .. }));
    }

    #[tokio::test]
    async fn rename_session_via_store_rejects_empty_title() {
        let store = InMemorySessionStore::new();
        let session_id = "550e8400-e29b-41d4-a716-446655440000";
        let err = rename_session_via_store(&store, session_id, "   ", None)
            .await
            .unwrap_err();
        assert!(matches!(err, Error::Session { .. }));
    }

    #[tokio::test]
    async fn rename_session_via_store_trims_title_before_storing() {
        let store = InMemorySessionStore::new();
        let session_id = "550e8400-e29b-41d4-a716-446655440000";
        rename_session_via_store(&store, session_id, "  Padded  ", None)
            .await
            .unwrap();
        let info = get_session_info_from_store(&store, session_id, None)
            .await
            .unwrap()
            .expect("has info");
        assert_eq!(info.custom_title.as_deref(), Some("Padded"));
    }

    #[tokio::test]
    async fn rename_session_via_store_appends_custom_title() {
        let store = InMemorySessionStore::new();
        let session_id = "550e8400-e29b-41d4-a716-446655440000";
        rename_session_via_store(&store, session_id, "My Title", None)
            .await
            .unwrap();
        let info = get_session_info_from_store(&store, session_id, None)
            .await
            .unwrap()
            .expect("has info");
        assert_eq!(info.custom_title.as_deref(), Some("My Title"));
    }

    #[tokio::test]
    async fn tag_session_via_store_sets_and_clears_tag() {
        let store = InMemorySessionStore::new();
        let session_id = "550e8400-e29b-41d4-a716-446655440000";
        // A tag alone gives get_session_info_from_store nothing to
        // derive a summary from (matches upstream: a metadata-only
        // session with no derivable summary text is filtered out
        // entirely) — append a real prompt first.
        store
            .append(
                &key("proj", session_id),
                vec![user_message("u1", None, "hi")],
            )
            .await
            .unwrap();
        tag_session_via_store(&store, session_id, Some("important"), Some("proj"))
            .await
            .unwrap();
        let info = get_session_info_from_store(&store, session_id, Some("proj"))
            .await
            .unwrap()
            .expect("has info");
        assert_eq!(info.tag.as_deref(), Some("important"));

        tag_session_via_store(&store, session_id, None, Some("proj"))
            .await
            .unwrap();
        let info = get_session_info_from_store(&store, session_id, Some("proj"))
            .await
            .unwrap()
            .expect("has info");
        assert_eq!(info.tag, None);
    }

    #[tokio::test]
    async fn tag_session_via_store_rejects_purely_invisible_tag() {
        let store = InMemorySessionStore::new();
        let session_id = "550e8400-e29b-41d4-a716-446655440000";
        let err = tag_session_via_store(&store, session_id, Some("\u{200b}"), None)
            .await
            .unwrap_err();
        assert!(matches!(err, Error::Session { .. }));
    }

    #[tokio::test]
    async fn tag_session_via_store_rejects_explicit_empty_tag_instead_of_clearing() {
        let store = InMemorySessionStore::new();
        let session_id = "550e8400-e29b-41d4-a716-446655440000";
        let err = tag_session_via_store(&store, session_id, Some(""), None)
            .await
            .unwrap_err();
        assert!(matches!(err, Error::Session { .. }));
    }

    #[tokio::test]
    async fn delete_session_via_store_removes_session() {
        let store = InMemorySessionStore::new();
        let session_id = "550e8400-e29b-41d4-a716-446655440000";
        rename_session_via_store(&store, session_id, "title", None)
            .await
            .unwrap();
        delete_session_via_store(&store, session_id, None)
            .await
            .unwrap();
        assert_eq!(
            get_session_info_from_store(&store, session_id, None)
                .await
                .unwrap(),
            None
        );
    }

    #[tokio::test]
    async fn fork_session_via_store_creates_new_session_with_remapped_chain() {
        let store = InMemorySessionStore::new();
        let session_id = "550e8400-e29b-41d4-a716-446655440000";
        let key = key("proj", session_id);
        store
            .append(
                &key,
                vec![
                    user_message("u1", None, "first"),
                    json!({"type": "assistant", "uuid": "a1", "parentUuid": "u1", "message": {"content": "reply"}}),
                ],
            )
            .await
            .unwrap();

        let result = fork_session_via_store(&store, session_id, Some("proj"), None, None)
            .await
            .unwrap();
        assert_ne!(result.session_id, session_id);

        let forked_messages =
            get_session_messages_from_store(&store, &result.session_id, Some("proj"), None, 0)
                .await
                .unwrap();
        assert_eq!(forked_messages.len(), 2);
        assert_ne!(forked_messages[0].uuid, "u1");
    }

    #[tokio::test]
    async fn fork_session_via_store_errors_on_empty_source() {
        let store = InMemorySessionStore::new();
        let err = fork_session_via_store(
            &store,
            "550e8400-e29b-41d4-a716-446655440000",
            None,
            None,
            None,
        )
        .await
        .unwrap_err();
        assert!(matches!(err, Error::Session { .. }));
    }

    #[tokio::test]
    async fn list_sessions_from_store_paginates_and_sorts_desc() {
        let store = InMemorySessionStore::new();
        for id in [
            "550e8400-e29b-41d4-a716-446655440001",
            "550e8400-e29b-41d4-a716-446655440002",
            "550e8400-e29b-41d4-a716-446655440003",
        ] {
            rename_session_via_store(&store, id, "t", Some("proj"))
                .await
                .unwrap();
        }
        let page = list_sessions_from_store(&store, Some("proj"), Some(2), 0)
            .await
            .unwrap();
        assert_eq!(page.len(), 2);
    }

    #[tokio::test]
    async fn list_sessions_from_store_limit_zero_means_unlimited() {
        let store = InMemorySessionStore::new();
        for id in [
            "550e8400-e29b-41d4-a716-446655440001",
            "550e8400-e29b-41d4-a716-446655440002",
        ] {
            rename_session_via_store(&store, id, "t", Some("proj"))
                .await
                .unwrap();
        }
        let all = list_sessions_from_store(&store, Some("proj"), Some(0), 0)
            .await
            .unwrap();
        assert_eq!(all.len(), 2);
    }

    #[tokio::test]
    async fn list_subagents_from_store_extracts_agent_ids() {
        let store = InMemorySessionStore::new();
        let session_id = "550e8400-e29b-41d4-a716-446655440000";
        store
            .append(
                &SessionKey {
                    project_key: "proj".to_string(),
                    session_id: session_id.to_string(),
                    subpath: Some("subagents/agent-abc".to_string()),
                },
                vec![user_message("u1", None, "hi")],
            )
            .await
            .unwrap();
        let agents = list_subagents_from_store(&store, session_id, Some("proj"))
            .await
            .unwrap();
        assert_eq!(agents, vec!["abc".to_string()]);
    }

    #[tokio::test]
    async fn get_subagent_messages_from_store_resolves_direct_path() {
        let store = InMemorySessionStore::new();
        let session_id = "550e8400-e29b-41d4-a716-446655440000";
        store
            .append(
                &SessionKey {
                    project_key: "proj".to_string(),
                    session_id: session_id.to_string(),
                    subpath: Some("subagents/agent-abc".to_string()),
                },
                vec![user_message("u1", None, "sub hi")],
            )
            .await
            .unwrap();
        let messages =
            get_subagent_messages_from_store(&store, session_id, "abc", Some("proj"), None, 0)
                .await
                .unwrap();
        assert_eq!(messages.len(), 1);
    }

    #[tokio::test]
    async fn get_subagent_messages_from_store_drops_agent_metadata_entries() {
        let store = InMemorySessionStore::new();
        let session_id = "550e8400-e29b-41d4-a716-446655440000";
        store
            .append(
                &SessionKey {
                    project_key: "proj".to_string(),
                    session_id: session_id.to_string(),
                    subpath: Some("subagents/agent-abc".to_string()),
                },
                vec![
                    json!({"type": "agent_metadata", "uuid": "m1"}),
                    user_message("u1", None, "sub hi"),
                ],
            )
            .await
            .unwrap();
        let messages =
            get_subagent_messages_from_store(&store, session_id, "abc", Some("proj"), None, 0)
                .await
                .unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].uuid, "u1");
    }

    #[test]
    fn in_memory_store_test_helpers_round_trip() {
        let store = InMemorySessionStore::new();
        assert_eq!(store.size(), 0);
        store.clear();
        assert_eq!(store.size(), 0);
    }
}
