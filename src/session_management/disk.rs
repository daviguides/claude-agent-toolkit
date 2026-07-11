//! Direct-local-disk session management: reads/writes the same
//! `~/.claude/projects/.../<session_id>.jsonl` files the CLI itself
//! writes. See `sessions.py`/`session_mutations.py`.

use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};
use uuid::Uuid;

use crate::error::{Error, Result};
use crate::session_management::iso8601::iso_now;
use crate::session_management::paths::{canonicalize_path, find_project_dir, worktree_paths};
use crate::session_management::store::{build_conversation_chain, build_subagent_chain};
use crate::session_management::summary::fold_session_summary;
use crate::session_management::unicode_sanitize::sanitize_unicode;
use crate::session_management::{
    ForkSessionResult, SDKSessionInfo, SessionMessage, is_valid_session_id,
};
use crate::types::session_store::SessionKey;

/// Bytes read from the head and (independently) the tail of a
/// transcript file for metadata-only parsing — avoids loading a
/// multi-megabyte transcript just to answer `list`/`info` queries.
const LITE_READ_BUF_SIZE: usize = 65536;

/// Resolves every candidate project directory for `directory`
/// (`None` meaning "every project directory"), most-specific first:
/// the exact/prefix match for `directory` itself, then any git
/// worktree siblings.
fn candidate_project_dirs(directory: Option<&str>) -> Vec<PathBuf> {
    let Some(directory) = directory else {
        let Ok(entries) = std::fs::read_dir(crate::session_management::paths::projects_dir())
        else {
            return Vec::new();
        };
        return entries
            .filter_map(std::result::Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.is_dir())
            .collect();
    };

    let canonical = canonicalize_path(directory);
    let mut dirs = Vec::new();
    if let Some(dir) = find_project_dir(&canonical) {
        dirs.push(dir);
    }
    for worktree in worktree_paths(Path::new(&canonical)) {
        if worktree == canonical {
            continue;
        }
        if let Some(dir) = find_project_dir(&worktree) {
            dirs.push(dir);
        }
    }
    dirs
}

/// Resolves the project directory for `directory`, creating it (and
/// any missing ancestors) if this is the first write for that
/// directory — read paths ([`candidate_project_dirs`]) never create
/// anything, but a write must succeed even on a directory with no
/// prior session history.
fn resolve_or_create_project_dir(directory: Option<&str>) -> Result<PathBuf> {
    if let Some(existing) = candidate_project_dirs(directory).into_iter().next() {
        return Ok(existing);
    }
    let canonical = match directory {
        Some(directory) => canonicalize_path(directory),
        None => canonicalize_path(
            &std::env::current_dir()
                .map(|dir| dir.display().to_string())
                .unwrap_or_default(),
        ),
    };
    let dir = crate::session_management::paths::projects_dir()
        .join(crate::session_management::paths::sanitize_path(&canonical));
    std::fs::create_dir_all(&dir).map_err(|source| Error::Session {
        message: format!(
            "failed to create project directory {}: {source}",
            dir.display()
        ),
    })?;
    Ok(dir)
}

fn session_file_path(project_dir: &Path, session_id: &str) -> PathBuf {
    project_dir.join(format!("{session_id}.jsonl"))
}

fn subagents_dir(project_dir: &Path, session_id: &str) -> PathBuf {
    project_dir.join(session_id).join("subagents")
}

/// Reads up to [`LITE_READ_BUF_SIZE`] bytes each from the head and
/// (independently, when the file is larger) the tail of `path`.
/// Returns `None` for an empty file or any I/O failure.
fn read_lite(path: &Path) -> Option<(String, String)> {
    let mut file = File::open(path).ok()?;
    let size = file.metadata().ok()?.len();
    if size == 0 {
        return None;
    }
    let buf_size = u64::try_from(LITE_READ_BUF_SIZE).unwrap_or(u64::MAX);
    let head_len = buf_size.min(size).try_into().unwrap_or(LITE_READ_BUF_SIZE);

    let mut head_buf = vec![0u8; head_len];
    file.read_exact(&mut head_buf).ok()?;
    let head = String::from_utf8_lossy(&head_buf).into_owned();

    let tail = if size <= buf_size {
        head.clone()
    } else {
        file.seek(SeekFrom::Start(size - buf_size)).ok()?;
        let mut tail_buf = vec![0u8; LITE_READ_BUF_SIZE];
        file.read_exact(&mut tail_buf).ok()?;
        String::from_utf8_lossy(&tail_buf).into_owned()
    };

    Some((head, tail))
}

/// Last occurrence of `"key":"value"`/`"key": "value"` in `text`
/// (both spacing variants), JSON-unescaped.
fn extract_last_json_string_field(text: &str, key: &str) -> Option<String> {
    extract_json_string_field_impl(text, key, true)
}

/// First occurrence of `"key":"value"`/`"key": "value"` in `text`.
fn extract_json_string_field(text: &str, key: &str) -> Option<String> {
    extract_json_string_field_impl(text, key, false)
}

fn extract_json_string_field_impl(text: &str, key: &str, last: bool) -> Option<String> {
    let patterns = [format!("\"{key}\":\""), format!("\"{key}\": \"")];
    let bytes = text.as_bytes();
    let mut result = None;

    for pattern in &patterns {
        let mut search_from = 0;
        while let Some(relative) = text[search_from..].find(pattern.as_str()) {
            let start = search_from + relative + pattern.len();
            let mut end = start;
            while end < bytes.len() {
                if bytes[end] == b'\\' {
                    end += 2;
                    continue;
                }
                if bytes[end] == b'"' {
                    break;
                }
                end += 1;
            }
            if end > bytes.len() {
                break;
            }
            let raw = &text[start..end.min(bytes.len())];
            let unescaped = serde_json::from_str::<String>(&format!("\"{raw}\""))
                .unwrap_or_else(|_| raw.to_string());
            result = Some(unescaped);
            search_from = start;
            if !last {
                return result;
            }
        }
    }
    result
}

/// Parses session metadata from a lite head/tail read. Returns `None`
/// for a sidechain-only session or one with no derivable summary.
fn parse_session_info_from_lite(
    session_id: &str,
    head: &str,
    tail: &str,
    project_path: Option<&str>,
) -> Option<SDKSessionInfo> {
    let first_line = head.lines().next().unwrap_or_default();
    if first_line.contains("\"isSidechain\":true") || first_line.contains("\"isSidechain\": true") {
        return None;
    }

    // Title-over-AI-title, tail-over-head at each precedence level.
    let custom_title = extract_last_json_string_field(tail, "customTitle")
        .or_else(|| extract_last_json_string_field(head, "customTitle"))
        .or_else(|| extract_last_json_string_field(tail, "aiTitle"))
        .or_else(|| extract_last_json_string_field(head, "aiTitle"));
    let first_prompt = extract_first_prompt_from_head(head);

    let summary = custom_title
        .clone()
        .or_else(|| extract_last_json_string_field(tail, "lastPrompt"))
        .or_else(|| extract_last_json_string_field(tail, "summary"))
        .or_else(|| first_prompt.clone())
        .filter(|s| !s.is_empty())?;

    let git_branch = extract_last_json_string_field(tail, "gitBranch")
        .or_else(|| extract_json_string_field(head, "gitBranch"));
    let cwd = extract_json_string_field(head, "cwd").or_else(|| project_path.map(str::to_string));
    let tag = tail
        .split('\n')
        .rev()
        .find(|line| line.starts_with("{\"type\":\"tag\""))
        .and_then(|line| extract_last_json_string_field(line, "tag"));
    let created_at = extract_json_string_field(head, "timestamp")
        .and_then(|ts| crate::session_management::iso8601::parse_iso8601_ms(&ts));

    Some(SDKSessionInfo {
        session_id: session_id.to_string(),
        summary,
        last_modified: 0,
        file_size: None,
        custom_title,
        first_prompt,
        git_branch,
        cwd,
        tag,
        created_at,
    })
}

/// Extracts the first non-slash-command user prompt from a lite
/// head read, truncated + `…`-suffixed. Mirrors `session_summary.rs`'s
/// fold-based extraction, operating on raw text instead of parsed
/// entries (both share the same skip/command regexes).
fn extract_first_prompt_from_head(head: &str) -> Option<String> {
    use crate::session_management::summary::first_prompt_from_text_lines;
    first_prompt_from_text_lines(head.lines())
}

fn read_transcript_entries(path: &Path) -> Vec<Value> {
    let Ok(contents) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    parse_transcript_entries(&contents)
}

fn parse_transcript_entries(contents: &str) -> Vec<Value> {
    contents
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|entry| {
            entry.is_object()
                && matches!(
                    entry.get("type").and_then(Value::as_str),
                    Some("user" | "assistant" | "progress" | "system" | "attachment")
                )
                && entry.get("uuid").and_then(Value::as_str).is_some()
        })
        .collect()
}

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

/// Recursively collects `agent-*.jsonl` files under `dir`, sorted per
/// directory level for determinism (supports arbitrary nesting, e.g.
/// `subagents/workflows/<run>/agent-<id>.jsonl`).
fn collect_agent_files(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut entries: Vec<PathBuf> = entries
        .filter_map(std::result::Result::ok)
        .map(|e| e.path())
        .collect();
    entries.sort();

    let mut files = Vec::new();
    for entry in entries {
        if entry.is_dir() {
            files.extend(collect_agent_files(&entry));
        } else if entry.extension().and_then(|ext| ext.to_str()) == Some("jsonl")
            && entry
                .file_stem()
                .and_then(|stem| stem.to_str())
                .is_some_and(|stem| stem.starts_with("agent-"))
        {
            files.push(entry);
        }
    }
    files
}

fn agent_id_from_path(path: &Path) -> Option<String> {
    let name = path.file_stem()?.to_str()?;
    name.strip_prefix("agent-").map(str::to_string)
}

/// Lists sessions across every project directory matching `directory`
/// (or all project directories when `None`), most-recently-modified
/// first, deduplicated by session id (keeping the highest
/// `last_modified` on a collision).
///
/// # Errors
///
/// This function does not fail on missing directories/files — a
/// nonexistent project directory simply yields no sessions.
pub fn list_sessions(
    directory: Option<&str>,
    limit: Option<usize>,
    offset: usize,
) -> Result<Vec<SDKSessionInfo>> {
    let mut by_id: HashMap<String, SDKSessionInfo> = HashMap::new();
    for project_dir in candidate_project_dirs(directory) {
        let Ok(entries) = std::fs::read_dir(&project_dir) else {
            continue;
        };
        for entry in entries.filter_map(std::result::Result::ok) {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                continue;
            }
            let Some(session_id) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            if !is_valid_session_id(session_id) {
                continue;
            }
            let Some((head, tail)) = read_lite(&path) else {
                continue;
            };
            let Ok(metadata) = std::fs::metadata(&path) else {
                continue;
            };
            let Ok(modified) = metadata.modified() else {
                continue;
            };
            let mtime = modified
                .duration_since(std::time::UNIX_EPOCH)
                .map(crate::session_management::iso8601::duration_to_millis)
                .unwrap_or_default();
            let Some(mut info) =
                parse_session_info_from_lite(session_id, &head, &tail, project_dir.to_str())
            else {
                continue;
            };
            info.last_modified = mtime;
            info.file_size = i64::try_from(metadata.len()).ok();

            match by_id.get(session_id) {
                Some(existing) if existing.last_modified >= mtime => {}
                _ => {
                    by_id.insert(session_id.to_string(), info);
                }
            }
        }
    }

    let mut infos: Vec<SDKSessionInfo> = by_id.into_values().collect();
    infos.sort_by(|a, b| b.last_modified.cmp(&a.last_modified));
    Ok(paginate(infos, limit, offset))
}

/// Fetches summary information for one session by scanning every
/// candidate project directory. Returns `None` for an invalid session
/// id or one that can't be found.
#[must_use]
pub fn get_session_info(session_id: &str, directory: Option<&str>) -> Option<SDKSessionInfo> {
    if !is_valid_session_id(session_id) {
        return None;
    }
    for project_dir in candidate_project_dirs(directory) {
        let path = session_file_path(&project_dir, session_id);
        let Some((head, tail)) = read_lite(&path) else {
            continue;
        };
        let Ok(metadata) = std::fs::metadata(&path) else {
            continue;
        };
        let Some(mut info) =
            parse_session_info_from_lite(session_id, &head, &tail, project_dir.to_str())
        else {
            continue;
        };
        info.last_modified = metadata
            .modified()
            .ok()
            .and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok())
            .map(crate::session_management::iso8601::duration_to_millis)
            .unwrap_or_default();
        info.file_size = i64::try_from(metadata.len()).ok();
        return Some(info);
    }
    None
}

/// Fetches the visible message chain for one session, chronological
/// order, paginated. Returns an empty list for an invalid or unknown
/// session id.
#[must_use]
pub fn get_session_messages(
    session_id: &str,
    directory: Option<&str>,
    limit: Option<usize>,
    offset: usize,
) -> Vec<SessionMessage> {
    if !is_valid_session_id(session_id) {
        return Vec::new();
    }
    for project_dir in candidate_project_dirs(directory) {
        let path = session_file_path(&project_dir, session_id);
        if !path.is_file() {
            continue;
        }
        let entries = read_transcript_entries(&path);
        let messages: Vec<SessionMessage> = build_conversation_chain(&entries)
            .into_iter()
            .filter(is_visible_message)
            .filter_map(|entry| entry_to_session_message(&entry, session_id))
            .collect();
        return paginate(messages, limit, offset);
    }
    Vec::new()
}

/// Lists subagent ids that have transcripts under `session_id`.
#[must_use]
pub fn list_subagents(session_id: &str, directory: Option<&str>) -> Vec<String> {
    if !is_valid_session_id(session_id) {
        return Vec::new();
    }
    for project_dir in candidate_project_dirs(directory) {
        let dir = subagents_dir(&project_dir, session_id);
        if !dir.is_dir() {
            continue;
        }
        return collect_agent_files(&dir)
            .into_iter()
            .filter_map(|path| agent_id_from_path(&path))
            .collect();
    }
    Vec::new()
}

/// Fetches the message chain for one subagent transcript under
/// `session_id`, searching nested `subagents/**/agent-<id>.jsonl`
/// files. Returns an empty list when nothing matches.
#[must_use]
pub fn get_subagent_messages(
    session_id: &str,
    agent_id: &str,
    directory: Option<&str>,
    limit: Option<usize>,
    offset: usize,
) -> Vec<SessionMessage> {
    if !is_valid_session_id(session_id) {
        return Vec::new();
    }
    let target_name = format!("agent-{agent_id}.jsonl");
    for project_dir in candidate_project_dirs(directory) {
        let dir = subagents_dir(&project_dir, session_id);
        if !dir.is_dir() {
            continue;
        }
        let Some(path) = collect_agent_files(&dir)
            .into_iter()
            .find(|path| path.file_name().and_then(|n| n.to_str()) == Some(target_name.as_str()))
        else {
            continue;
        };
        let entries = read_transcript_entries(&path);
        let messages: Vec<SessionMessage> = build_subagent_chain(&entries)
            .into_iter()
            .filter(is_visible_message)
            .filter_map(|entry| entry_to_session_message(&entry, session_id))
            .collect();
        return paginate(messages, limit, offset);
    }
    Vec::new()
}

fn append_entry_to_session_file(
    session_id: &str,
    directory: Option<&str>,
    entry: &Value,
) -> Result<()> {
    let project_dir = resolve_or_create_project_dir(directory)?;
    let path = session_file_path(&project_dir, session_id);
    let mut file = OpenOptions::new()
        .append(true)
        .create(true)
        .open(&path)
        .map_err(|source| Error::Session {
            message: format!("failed to open {}: {source}", path.display()),
        })?;
    writeln!(file, "{entry}").map_err(|source| Error::Session {
        message: format!("failed to write {}: {source}", path.display()),
    })
}

/// Renames a session by appending a `custom-title` entry directly to
/// its transcript file (no `uuid`/`timestamp` — matches upstream's
/// disk-path wire shape, distinct from the `_via_store` variant).
/// `title` is trimmed before storing.
///
/// # Errors
///
/// Returns [`Error::InvalidSessionId`] for a malformed `session_id`,
/// or [`Error::Session`] if `title` is empty after trimming, or the
/// session file can't be found/written.
pub fn rename_session(session_id: &str, title: &str, directory: Option<&str>) -> Result<()> {
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
    let entry = json!({"type": "custom-title", "customTitle": stripped, "sessionId": session_id});
    append_entry_to_session_file(session_id, directory, &entry)
}

/// Tags a session (or, with `tag: None`, clears its tag) by appending
/// a `tag` entry directly to its transcript file. `tag` is sanitized
/// (`sanitize_unicode`) then trimmed — an explicit `Some("")` (or a
/// tag that sanitizes/trims down to nothing) is rejected, not treated
/// as clearing; only `None` clears.
///
/// # Errors
///
/// Returns [`Error::InvalidSessionId`] for a malformed `session_id`,
/// [`Error::Session`] if an explicit (non-`None`) tag is empty after
/// sanitizing and trimming, or the session file can't be found/written.
pub fn tag_session(session_id: &str, tag: Option<&str>, directory: Option<&str>) -> Result<()> {
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
    let entry = json!({"type": "tag", "tag": sanitized, "sessionId": session_id});
    append_entry_to_session_file(session_id, directory, &entry)
}

/// Deletes a session's transcript file (and its subagent transcripts
/// directory, if any).
///
/// # Errors
///
/// Returns [`Error::InvalidSessionId`] for a malformed `session_id`.
/// Filesystem errors while removing files are swallowed (mirrors a
/// best-effort delete — nothing to undo if a file is already gone).
pub fn delete_session(session_id: &str, directory: Option<&str>) -> Result<()> {
    if !is_valid_session_id(session_id) {
        return Err(Error::InvalidSessionId {
            session_id: session_id.to_string(),
        });
    }
    for project_dir in candidate_project_dirs(directory) {
        let path = session_file_path(&project_dir, session_id);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir_all(project_dir.join(session_id));
    }
    Ok(())
}

/// Forks `session_id` at `up_to_message_id` (or the transcript's end)
/// into a brand-new session file. Uses `O_EXCL` semantics
/// (`create_new`) so it never silently overwrites an existing file for
/// the (extremely unlikely) fresh-uuid collision case.
///
/// # Errors
///
/// Returns [`Error::InvalidSessionId`] for a malformed `session_id`,
/// [`Error::Session`] if the source has no messages (or
/// `up_to_message_id` doesn't match any message), or on I/O failure.
pub fn fork_session(
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
    let project_dirs = candidate_project_dirs(directory);
    let project_dir = project_dirs
        .into_iter()
        .next()
        .ok_or_else(|| Error::Session {
            message: format!("no project directory found for session {session_id}"),
        })?;
    let source_path = session_file_path(&project_dir, session_id);
    let entries = read_transcript_entries(&source_path);

    let forked_id = Uuid::new_v4().to_string();
    let key = SessionKey {
        project_key: String::new(),
        session_id: session_id.to_string(),
        subpath: None,
    };
    let derive_title = || {
        let summary = fold_session_summary(None, &key, &entries);
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
    let lines = build_fork_lines(
        &entries,
        session_id,
        &forked_id,
        up_to_message_id,
        title,
        derive_title,
    )?;

    let target_path = session_file_path(&project_dir, &forked_id);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&target_path)
        .map_err(|source| Error::Session {
            message: format!("failed to create {}: {source}", target_path.display()),
        })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = file.set_permissions(std::fs::Permissions::from_mode(0o600));
    }
    let body = lines
        .iter()
        .map(std::string::ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    file.write_all(body.as_bytes())
        .map_err(|source| Error::Session {
            message: format!("failed to write {}: {source}", target_path.display()),
        })?;
    file.write_all(b"\n").map_err(|source| Error::Session {
        message: format!("failed to write {}: {source}", target_path.display()),
    })?;

    Ok(ForkSessionResult {
        session_id: forked_id,
    })
}

/// Imports `entries` into a `SessionStore`-backed session, mirroring
/// what a live session mirror would have written: filters out
/// non-transcript records the store shouldn't retain.
///
/// # Errors
///
/// Propagates any adapter-specific error from `store.append`.
pub async fn import_session_to_store(
    store: &dyn crate::types::session_store::SessionStore,
    session_id: &str,
    directory: Option<&str>,
    entries: Vec<Value>,
) -> Result<()> {
    let key = SessionKey {
        project_key: crate::session_management::paths::project_key_for_directory(directory),
        session_id: session_id.to_string(),
        subpath: None,
    };
    store.append(&key, entries).await
}

/// Shared fork-line-rebuilding transform: remaps every entry's uuid,
/// rebuilds the `parentUuid` chain (skipping `progress`-typed
/// ancestors), remaps `logicalParentUuid`, stamps a fresh timestamp
/// only on the last written entry, strips state-leak fields
/// (`teamName`/`agentName`/`slug`/`sourceToolAssistantUUID`), and
/// appends a final `custom-title` entry. Shared by
/// [`fork_session`]/`fork_session_via_store`.
///
/// # Errors
///
/// Returns [`Error::Session`] if the source has no non-sidechain
/// messages, `up_to_message_id` doesn't match any message, or no
/// messages remain once `progress`-typed entries are dropped.
pub(crate) fn build_fork_lines(
    entries: &[Value],
    source_session_id: &str,
    forked_session_id: &str,
    up_to_message_id: Option<&str>,
    title: Option<&str>,
    derive_title: impl FnOnce() -> Option<String>,
) -> Result<Vec<Value>> {
    let transcript: Vec<Value> = entries
        .iter()
        .filter(|entry| entry.get("isSidechain").and_then(Value::as_bool) != Some(true))
        .cloned()
        .collect();
    if transcript.is_empty() {
        return Err(Error::Session {
            message: format!("session {source_session_id} has no messages to fork"),
        });
    }

    let transcript: Vec<Value> = if let Some(up_to) = up_to_message_id {
        let cutoff = transcript
            .iter()
            .position(|entry| entry.get("uuid").and_then(Value::as_str) == Some(up_to))
            .ok_or_else(|| Error::Session {
                message: format!("Message {up_to} not found in session {source_session_id}"),
            })?;
        transcript[..=cutoff].to_vec()
    } else {
        transcript
    };

    let uuid_mapping: HashMap<String, String> = transcript
        .iter()
        .filter_map(|entry| entry.get("uuid").and_then(Value::as_str))
        .map(|uuid| (uuid.to_string(), Uuid::new_v4().to_string()))
        .collect();

    let writable: Vec<&Value> = transcript
        .iter()
        .filter(|entry| entry.get("type").and_then(Value::as_str) != Some("progress"))
        .collect();
    if writable.is_empty() {
        return Err(Error::Session {
            message: format!("session {source_session_id} has no messages to fork"),
        });
    }

    let now = iso_now();
    let last_index = writable.len() - 1;
    let mut lines: Vec<Value> = writable
        .iter()
        .enumerate()
        .map(|(i, entry)| {
            remap_fork_entry(
                entry,
                &transcript,
                &uuid_mapping,
                forked_session_id,
                source_session_id,
                i == last_index,
                &now,
            )
        })
        .collect();

    let final_title = title.map(str::trim).filter(|t| !t.is_empty()).map_or_else(
        || {
            format!(
                "{} (fork)",
                derive_title().unwrap_or_else(|| "Forked session".to_string())
            )
        },
        str::to_string,
    );
    lines.push(json!({
        "type": "custom-title",
        "customTitle": final_title,
        "sessionId": forked_session_id,
        "uuid": Uuid::new_v4().to_string(),
        "timestamp": iso_now(),
    }));

    Ok(lines)
}

/// Remaps one transcript entry for [`build_fork_lines`]: fresh uuid,
/// `parentUuid` walked past any `progress`-typed ancestor and remapped,
/// `logicalParentUuid` remapped (always present in the output, as
/// `null` when absent or pointing outside the mapped set — upstream's
/// dict literal never omits the key), fork bookkeeping fields set,
/// state-leak fields stripped, and — only for the last written entry —
/// a fresh timestamp.
fn remap_fork_entry(
    entry: &Value,
    transcript: &[Value],
    uuid_mapping: &HashMap<String, String>,
    forked_session_id: &str,
    source_session_id: &str,
    is_last: bool,
    timestamp: &str,
) -> Value {
    let type_of = |uuid: &str| -> Option<&str> {
        transcript
            .iter()
            .find(|e| e.get("uuid").and_then(Value::as_str) == Some(uuid))
            .and_then(|e| e.get("type"))
            .and_then(Value::as_str)
    };
    let parent_of = |uuid: &str| -> Option<String> {
        transcript
            .iter()
            .find(|e| e.get("uuid").and_then(Value::as_str) == Some(uuid))
            .and_then(|e| e.get("parentUuid"))
            .and_then(Value::as_str)
            .map(str::to_string)
    };

    let mut object: Map<String, Value> = entry.as_object().cloned().unwrap_or_default();
    let original_uuid = entry
        .get("uuid")
        .and_then(Value::as_str)
        .unwrap_or_default();

    if let Some(new_uuid) = uuid_mapping.get(original_uuid) {
        object.insert("uuid".to_string(), json!(new_uuid));
    }

    let mut parent_uuid = entry
        .get("parentUuid")
        .and_then(Value::as_str)
        .map(str::to_string);
    while let Some(candidate) = parent_uuid.clone() {
        if type_of(&candidate) == Some("progress") {
            parent_uuid = parent_of(&candidate);
        } else {
            break;
        }
    }
    let new_parent = parent_uuid.and_then(|parent| uuid_mapping.get(&parent).cloned());
    object.insert(
        "parentUuid".to_string(),
        new_parent.map_or(Value::Null, |p| json!(p)),
    );

    // Upstream always sets this key (via a dict literal), `None`/`null`
    // included, whether the original entry had one or not — never
    // omits it.
    let new_logical_parent = entry
        .get("logicalParentUuid")
        .and_then(Value::as_str)
        .and_then(|parent| uuid_mapping.get(parent));
    object.insert(
        "logicalParentUuid".to_string(),
        new_logical_parent.map_or(Value::Null, |p| json!(p)),
    );

    object.insert("sessionId".to_string(), json!(forked_session_id));
    object.insert("isSidechain".to_string(), Value::Bool(false));
    object.insert(
        "forkedFrom".to_string(),
        json!({"sessionId": source_session_id, "messageUuid": original_uuid}),
    );
    if is_last {
        object.insert("timestamp".to_string(), json!(timestamp));
    }
    object.remove("teamName");
    object.remove("agentName");
    object.remove("slug");
    object.remove("sourceToolAssistantUUID");

    Value::Object(object)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_first_json_string_field_finds_first_occurrence() {
        let text = r#"{"a":"first"}
{"a":"second"}"#;
        assert_eq!(
            extract_json_string_field(text, "a").as_deref(),
            Some("first")
        );
    }

    #[test]
    fn extract_last_json_string_field_finds_last_occurrence() {
        let text = r#"{"a":"first"}
{"a":"second"}"#;
        assert_eq!(
            extract_last_json_string_field(text, "a").as_deref(),
            Some("second")
        );
    }

    #[test]
    fn extract_json_string_field_handles_escaped_quotes() {
        let text = r#"{"a":"has \"quotes\" inside"}"#;
        assert_eq!(
            extract_json_string_field(text, "a").as_deref(),
            Some("has \"quotes\" inside")
        );
    }

    #[test]
    fn parse_session_info_from_lite_none_for_sidechain() {
        let head = r#"{"type":"user","isSidechain":true}"#;
        assert!(parse_session_info_from_lite("s", head, head, None).is_none());
    }

    #[test]
    fn parse_session_info_from_lite_reads_custom_title() {
        let head = r#"{"type":"user","message":{"content":"hi"}}"#;
        let tail = r#"{"type":"custom-title","customTitle":"My Title"}"#;
        let info = parse_session_info_from_lite("s", head, tail, None).expect("has info");
        assert_eq!(info.custom_title.as_deref(), Some("My Title"));
        assert_eq!(info.summary, "My Title");
    }

    #[test]
    fn parse_session_info_from_lite_tag_ignores_tool_use_inputs() {
        let head = r#"{"type":"user","message":{"content":"hi"}}"#;
        let tail = r#"{"type":"assistant","message":{"content":[{"type":"tool_use","input":{"tag":"not-a-session-tag"}}]}}"#;
        let info = parse_session_info_from_lite("s", head, tail, None).expect("has info");
        assert_eq!(info.tag, None);
    }

    #[test]
    fn build_fork_lines_errors_on_empty_transcript() {
        let err = build_fork_lines(&[], "src", "fork", None, None, || None).unwrap_err();
        assert!(matches!(err, Error::Session { .. }));
    }

    #[test]
    fn build_fork_lines_remaps_uuids_and_chain() {
        let entries = vec![
            json!({"type": "user", "uuid": "u1", "parentUuid": null, "message": {"content": "hi"}}),
            json!({"type": "assistant", "uuid": "a1", "parentUuid": "u1", "message": {"content": "hey"}}),
        ];
        let lines =
            build_fork_lines(&entries, "src", "fork", None, Some("My Fork"), || None).unwrap();
        assert_eq!(lines.len(), 3); // 2 messages + 1 custom-title
        assert_ne!(lines[0]["uuid"], "u1");
        assert_ne!(lines[1]["uuid"], "a1");
        assert_eq!(lines[1]["parentUuid"], lines[0]["uuid"]);
        assert_eq!(lines[2]["type"], "custom-title");
        assert_eq!(lines[2]["customTitle"], "My Fork");
    }

    #[test]
    fn build_fork_lines_stops_at_up_to_message_id() {
        let entries = vec![
            json!({"type": "user", "uuid": "u1", "parentUuid": null, "message": {"content": "hi"}}),
            json!({"type": "assistant", "uuid": "a1", "parentUuid": "u1", "message": {"content": "hey"}}),
        ];
        let lines = build_fork_lines(&entries, "src", "fork", Some("u1"), None, || None).unwrap();
        // 1 message (up to u1) + 1 custom-title.
        assert_eq!(lines.len(), 2);
    }

    #[test]
    fn build_fork_lines_up_to_message_id_not_found_errors() {
        let entries = vec![json!({"type": "user", "uuid": "u1", "parentUuid": null})];
        let err =
            build_fork_lines(&entries, "src", "fork", Some("missing"), None, || None).unwrap_err();
        assert!(matches!(err, Error::Session { .. }));
    }

    #[test]
    fn build_fork_lines_uses_derived_title_with_fork_suffix() {
        let entries = vec![
            json!({"type": "user", "uuid": "u1", "parentUuid": null, "message": {"content": "hi"}}),
        ];
        let lines = build_fork_lines(&entries, "src", "fork", None, None, || {
            Some("Derived".to_string())
        })
        .unwrap();
        let last = lines.last().unwrap();
        assert_eq!(last["customTitle"], "Derived (fork)");
    }

    #[test]
    fn build_fork_lines_strips_state_leak_fields() {
        let entries = vec![json!({
            "type": "user", "uuid": "u1", "parentUuid": null,
            "teamName": "team", "agentName": "agent", "slug": "s", "sourceToolAssistantUUID": "x",
            "message": {"content": "hi"},
        })];
        let lines = build_fork_lines(&entries, "src", "fork", None, None, || None).unwrap();
        let first = &lines[0];
        assert!(first.get("teamName").is_none());
        assert!(first.get("agentName").is_none());
        assert!(first.get("slug").is_none());
        assert!(first.get("sourceToolAssistantUUID").is_none());
    }

    #[test]
    fn get_session_info_none_for_invalid_uuid() {
        assert!(get_session_info("not-a-uuid", None).is_none());
    }

    #[test]
    fn list_sessions_and_delete_round_trip_in_tempdir() {
        let temp = tempfile::tempdir().unwrap();
        crate::session_management::paths::set_test_projects_dir_override(Some(
            temp.path().to_path_buf(),
        ));

        let session_id = "550e8400-e29b-41d4-a716-446655440000";
        rename_session(session_id, "Test Session", Some("/some/project")).unwrap();

        let info = get_session_info(session_id, Some("/some/project"));
        assert!(info.is_some());
        assert_eq!(info.unwrap().custom_title.as_deref(), Some("Test Session"));

        delete_session(session_id, Some("/some/project")).unwrap();
        assert!(get_session_info(session_id, Some("/some/project")).is_none());

        crate::session_management::paths::set_test_projects_dir_override(None);
    }

    #[test]
    fn rename_session_rejects_empty_title() {
        let temp = tempfile::tempdir().unwrap();
        crate::session_management::paths::set_test_projects_dir_override(Some(
            temp.path().to_path_buf(),
        ));
        let session_id = "550e8400-e29b-41d4-a716-446655440000";
        let err = rename_session(session_id, "   ", Some("/some/project")).unwrap_err();
        assert!(matches!(err, Error::Session { .. }));
        crate::session_management::paths::set_test_projects_dir_override(None);
    }

    #[test]
    fn rename_session_trims_title_before_storing() {
        let temp = tempfile::tempdir().unwrap();
        crate::session_management::paths::set_test_projects_dir_override(Some(
            temp.path().to_path_buf(),
        ));
        let session_id = "550e8400-e29b-41d4-a716-446655440000";
        rename_session(session_id, "  Padded Title  ", Some("/some/project")).unwrap();
        let info = get_session_info(session_id, Some("/some/project")).expect("has info");
        assert_eq!(info.custom_title.as_deref(), Some("Padded Title"));
        crate::session_management::paths::set_test_projects_dir_override(None);
    }

    #[test]
    fn tag_session_rejects_explicit_empty_tag_instead_of_clearing() {
        let temp = tempfile::tempdir().unwrap();
        crate::session_management::paths::set_test_projects_dir_override(Some(
            temp.path().to_path_buf(),
        ));
        let session_id = "550e8400-e29b-41d4-a716-446655440000";
        let err = tag_session(session_id, Some(""), Some("/some/project")).unwrap_err();
        assert!(matches!(err, Error::Session { .. }));
        crate::session_management::paths::set_test_projects_dir_override(None);
    }

    #[test]
    fn build_fork_lines_always_includes_logical_parent_uuid_key() {
        let entries = vec![
            json!({"type": "user", "uuid": "u1", "parentUuid": null, "message": {"content": "hi"}}),
        ];
        let lines = build_fork_lines(&entries, "src", "fork", None, None, || None).unwrap();
        // No logicalParentUuid on the original entry -> present as null,
        // never omitted (matches upstream's dict-literal construction).
        assert_eq!(lines[0]["logicalParentUuid"], serde_json::Value::Null);
        assert!(
            lines[0]
                .as_object()
                .unwrap()
                .contains_key("logicalParentUuid")
        );
    }
}
