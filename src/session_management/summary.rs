//! Incremental session summary folding: turns appended transcript
//! entries into the running [`SessionSummaryEntry.data`] bag `list_*`
//! functions read back as [`SDKSessionInfo`]. See `session_summary.py`.

use std::sync::LazyLock;

use regex::Regex;
use serde_json::{Map, Value, json};

use crate::session_management::SDKSessionInfo;
use crate::session_management::iso8601::parse_iso8601_ms;
use crate::types::session_store::{SessionKey, SessionStoreEntry, SessionSummaryEntry};

/// First non-slash-command user prompt, truncated to this many
/// Unicode scalar values (not bytes), with a trailing `…`.
const FIRST_PROMPT_MAX_CHARS: usize = 200;

static COMMAND_NAME_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"<command-name>(.*?)</command-name>").expect("valid regex"));

static SKIP_FIRST_PROMPT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?s)^(?:<local-command-stdout>|<session-start-hook>|<tick>|<goal>|\[Request interrupted by user[^\]]*\]|\s*<ide_opened_file>.*</ide_opened_file>\s*$|\s*<ide_selection>.*</ide_selection>\s*$)",
    )
    .expect("valid regex")
});

/// Folds one append batch into `prev` (or a fresh summary when
/// `None`), returning the updated summary. `mtime` is stamped by the
/// caller (the `SessionStore` adapter) after persisting, not here.
///
/// Exposed publicly so a custom [`crate::SessionStore`] adapter can
/// maintain its own `list_session_summaries` sidecar the same way
/// [`crate::InMemorySessionStore`] does internally.
///
/// # Panics
///
/// Never in practice: `data` is always freshly constructed as a JSON
/// object or cloned from a prior [`SessionSummaryEntry`]'s `data`
/// field, which this function itself only ever populates as an object.
#[must_use]
pub fn fold_session_summary(
    prev: Option<&SessionSummaryEntry>,
    key: &SessionKey,
    entries: &[SessionStoreEntry],
) -> SessionSummaryEntry {
    let mut data = prev.map_or_else(|| Value::Object(Map::new()), |summary| summary.data.clone());
    let data_map = data.as_object_mut().expect("data is always an object");

    for entry in entries {
        fold_one_entry(data_map, entry);
    }

    SessionSummaryEntry {
        session_id: key.session_id.clone(),
        mtime: prev.map(|summary| summary.mtime).unwrap_or_default(),
        data,
    }
}

fn fold_one_entry(data: &mut Map<String, Value>, entry: &Value) {
    if entry.get("isSidechain").and_then(Value::as_bool) == Some(true) {
        set_once(data, "is_sidechain", Value::Bool(true));
    }
    if !data.contains_key("created_at")
        && let Some(timestamp) = entry.get("timestamp").and_then(Value::as_str)
        && let Some(epoch_ms) = parse_iso8601_ms(timestamp)
    {
        data.insert("created_at".to_string(), json!(epoch_ms));
    }
    set_once_str(data, "cwd", entry.get("cwd").and_then(Value::as_str));
    set_last_str(
        data,
        "custom_title",
        entry.get("customTitle").and_then(Value::as_str),
    );
    set_last_str(
        data,
        "ai_title",
        entry.get("aiTitle").and_then(Value::as_str),
    );
    set_last_str(
        data,
        "last_prompt",
        entry.get("lastPrompt").and_then(Value::as_str),
    );
    set_last_str(
        data,
        "summary_hint",
        entry.get("summary").and_then(Value::as_str),
    );
    set_last_str(
        data,
        "git_branch",
        entry.get("gitBranch").and_then(Value::as_str),
    );
    if entry.get("type").and_then(Value::as_str) == Some("tag") {
        match entry.get("tag").and_then(Value::as_str) {
            Some("") | None => {
                data.remove("tag");
            }
            Some(tag) => {
                data.insert("tag".to_string(), json!(tag));
            }
        }
    }

    fold_first_prompt(data, entry);
}

/// Mirrors `_fold_first_prompt`: locks in the first real (non-command)
/// user prompt found; slash-command messages are stashed as a
/// `command_fallback` instead, in case no real prompt ever appears.
fn fold_first_prompt(data: &mut Map<String, Value>, entry: &Value) {
    if data.get("first_prompt_locked").and_then(Value::as_bool) == Some(true) {
        return;
    }
    if entry.get("type").and_then(Value::as_str) != Some("user") {
        return;
    }
    if entry.get("isMeta").and_then(Value::as_bool) == Some(true) {
        return;
    }
    if entry.get("isCompactSummary").and_then(Value::as_bool) == Some(true) {
        return;
    }

    for text in user_text_blocks(entry) {
        let text = text.replace('\n', " ");
        let text = text.trim();
        if text.is_empty() {
            continue;
        }
        if let Some(captures) = COMMAND_NAME_RE.captures(text) {
            if !data.contains_key("command_fallback") {
                data.insert(
                    "command_fallback".to_string(),
                    json!(captures.get(1).map_or("", |m| m.as_str())),
                );
            }
            continue;
        }
        if SKIP_FIRST_PROMPT_RE.is_match(text) {
            continue;
        }
        data.insert("first_prompt".to_string(), json!(truncate_prompt(text)));
        data.insert("first_prompt_locked".to_string(), Value::Bool(true));
        return;
    }
}

/// Extracts the first non-slash-command user prompt from raw JSONL
/// transcript lines — the disk-family counterpart to
/// [`fold_first_prompt`], operating on unparsed text (with fast
/// substring pre-filters before `serde_json::from_str`, to avoid
/// parsing every line of a large lite-read just to find the first
/// prompt) instead of already-loaded entries. Shares the same skip
/// conditions, command regex, and truncation as the store family.
#[must_use]
pub(crate) fn first_prompt_from_text_lines<'a>(
    lines: impl Iterator<Item = &'a str>,
) -> Option<String> {
    let mut command_fallback: Option<String> = None;
    for line in lines {
        if !line.contains("\"type\":\"user\"") && !line.contains("\"type\": \"user\"") {
            continue;
        }
        if line.contains("\"tool_result\"") {
            continue;
        }
        if line.contains("\"isMeta\":true") || line.contains("\"isMeta\": true") {
            continue;
        }
        if line.contains("\"isCompactSummary\":true") || line.contains("\"isCompactSummary\": true")
        {
            continue;
        }
        let Ok(entry) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if entry.get("type").and_then(Value::as_str) != Some("user") {
            continue;
        }

        for text in user_text_blocks(&entry) {
            let text = text.replace('\n', " ");
            let text = text.trim();
            if text.is_empty() {
                continue;
            }
            if let Some(captures) = COMMAND_NAME_RE.captures(text) {
                if command_fallback.is_none() {
                    command_fallback = Some(captures.get(1).map_or("", |m| m.as_str()).to_string());
                }
                continue;
            }
            if SKIP_FIRST_PROMPT_RE.is_match(text) {
                continue;
            }
            return Some(truncate_prompt(text));
        }
    }
    command_fallback
}

/// Extracts every text string from a `user`-role entry's
/// `message.content` — either a plain string, or the `text` field of
/// each `{"type":"text",...}` content block, skipping any block
/// carrying a `tool_result` (mirrors upstream's `tool_result`-presence
/// skip check).
fn user_text_blocks(entry: &Value) -> Vec<String> {
    let content = entry
        .get("message")
        .and_then(|message| message.get("content"));
    match content {
        Some(Value::String(text)) => vec![text.clone()],
        Some(Value::Array(blocks)) => {
            if blocks
                .iter()
                .any(|block| block.get("type").and_then(Value::as_str) == Some("tool_result"))
            {
                return Vec::new();
            }
            blocks
                .iter()
                .filter(|block| block.get("type").and_then(Value::as_str) == Some("text"))
                .filter_map(|block| block.get("text").and_then(Value::as_str))
                .map(str::to_string)
                .collect()
        }
        _ => Vec::new(),
    }
}

/// Truncates `text` to [`FIRST_PROMPT_MAX_CHARS`] Unicode scalar
/// values, trims trailing whitespace, then appends `…` — matching
/// upstream's `result[:200].rstrip() + "…"` (no truncation, and
/// no ellipsis, when the text is already short enough).
fn truncate_prompt(text: &str) -> String {
    if text.chars().count() <= FIRST_PROMPT_MAX_CHARS {
        return text.to_string();
    }
    let truncated: String = text.chars().take(FIRST_PROMPT_MAX_CHARS).collect();
    format!("{}\u{2026}", truncated.trim_end())
}

/// Converts a folded summary into [`SDKSessionInfo`]. Returns `None`
/// for sidechain sessions or ones with no derivable summary text —
/// mirrors `summary_entry_to_sdk_info`.
#[must_use]
pub(crate) fn summary_entry_to_sdk_info(
    entry: &SessionSummaryEntry,
    project_path: Option<&str>,
) -> Option<SDKSessionInfo> {
    let data = entry.data.as_object()?;
    if data.get("is_sidechain").and_then(Value::as_bool) == Some(true) {
        return None;
    }

    let str_field = |key: &str| data.get(key).and_then(Value::as_str).map(str::to_string);
    let custom_title = str_field("custom_title");
    let first_prompt = str_field("first_prompt").or_else(|| str_field("command_fallback"));
    let summary = custom_title
        .clone()
        .or_else(|| str_field("last_prompt"))
        .or_else(|| str_field("summary_hint"))
        .or_else(|| first_prompt.clone())
        .filter(|summary| !summary.is_empty())?;

    Some(SDKSessionInfo {
        session_id: entry.session_id.clone(),
        summary,
        last_modified: entry.mtime,
        file_size: None,
        custom_title,
        first_prompt,
        git_branch: str_field("git_branch"),
        cwd: str_field("cwd").or_else(|| project_path.map(str::to_string)),
        tag: str_field("tag"),
        created_at: data.get("created_at").and_then(Value::as_i64),
    })
}

fn set_once(data: &mut Map<String, Value>, key: &str, value: Value) {
    data.entry(key.to_string()).or_insert(value);
}

fn set_once_str(data: &mut Map<String, Value>, key: &str, value: Option<&str>) {
    if !data.contains_key(key)
        && let Some(value) = value
    {
        data.insert(key.to_string(), json!(value));
    }
}

fn set_last_str(data: &mut Map<String, Value>, key: &str, value: Option<&str>) {
    if let Some(value) = value {
        data.insert(key.to_string(), json!(value));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_prompt_from_text_lines_finds_real_prompt() {
        let lines = [r#"{"type":"user","message":{"content":"hello there"}}"#];
        assert_eq!(
            first_prompt_from_text_lines(lines.into_iter()),
            Some("hello there".to_string())
        );
    }

    #[test]
    fn first_prompt_from_text_lines_skips_tool_result_lines() {
        let lines = [r#"{"type":"user","message":{"content":[{"type":"tool_result"}]}}"#];
        assert_eq!(first_prompt_from_text_lines(lines.into_iter()), None);
    }

    #[test]
    fn first_prompt_from_text_lines_falls_back_to_command() {
        let lines =
            [r#"{"type":"user","message":{"content":"<command-name>compact</command-name>"}}"#];
        assert_eq!(
            first_prompt_from_text_lines(lines.into_iter()),
            Some("compact".to_string())
        );
    }

    fn key() -> SessionKey {
        SessionKey {
            project_key: "proj".to_string(),
            session_id: "sess".to_string(),
            subpath: None,
        }
    }

    fn user_entry(text: &str) -> Value {
        json!({"type": "user", "message": {"role": "user", "content": text}})
    }

    #[test]
    fn folds_first_prompt_from_plain_string_content() {
        let summary = fold_session_summary(None, &key(), &[user_entry("hello there")]);
        assert_eq!(summary.data["first_prompt"], "hello there");
        assert_eq!(summary.data["first_prompt_locked"], true);
    }

    #[test]
    fn slash_command_becomes_fallback_not_first_prompt() {
        let entry = user_entry("<command-name>compact</command-name>");
        let summary = fold_session_summary(None, &key(), std::slice::from_ref(&entry));
        assert!(summary.data.get("first_prompt").is_none());
        assert_eq!(summary.data["command_fallback"], "compact");
    }

    #[test]
    fn real_prompt_after_command_locks_in_over_fallback() {
        let entries = [
            user_entry("<command-name>compact</command-name>"),
            user_entry("what is 2 + 2?"),
        ];
        let summary = fold_session_summary(None, &key(), &entries);
        assert_eq!(summary.data["first_prompt"], "what is 2 + 2?");
        assert_eq!(summary.data["command_fallback"], "compact");
    }

    #[test]
    fn first_prompt_is_locked_after_first_real_match() {
        let entries = [user_entry("first"), user_entry("second")];
        let summary = fold_session_summary(None, &key(), &entries);
        assert_eq!(summary.data["first_prompt"], "first");
    }

    #[test]
    fn skip_pattern_entries_never_become_first_prompt() {
        let entry = user_entry("<local-command-stdout>ok</local-command-stdout>");
        let summary = fold_session_summary(None, &key(), std::slice::from_ref(&entry));
        assert!(summary.data.get("first_prompt").is_none());
    }

    #[test]
    fn is_meta_entries_are_skipped_for_first_prompt() {
        let entry = json!({"type": "user", "isMeta": true, "message": {"role": "user", "content": "meta text"}});
        let summary = fold_session_summary(None, &key(), std::slice::from_ref(&entry));
        assert!(summary.data.get("first_prompt").is_none());
    }

    #[test]
    fn tool_result_content_is_skipped_for_first_prompt() {
        let entry = json!({
            "type": "user",
            "message": {"role": "user", "content": [{"type": "tool_result", "tool_use_id": "t1"}]},
        });
        let summary = fold_session_summary(None, &key(), std::slice::from_ref(&entry));
        assert!(summary.data.get("first_prompt").is_none());
    }

    #[test]
    fn long_prompt_is_truncated_with_ellipsis() {
        let long_text = "a".repeat(250);
        let summary = fold_session_summary(None, &key(), &[user_entry(&long_text)]);
        let first_prompt = summary.data["first_prompt"].as_str().unwrap();
        assert_eq!(first_prompt.chars().count(), 201);
        assert!(first_prompt.ends_with('\u{2026}'));
    }

    #[test]
    fn is_sidechain_is_set_once() {
        let entry = json!({"type": "user", "isSidechain": true, "message": {"content": "x"}});
        let summary = fold_session_summary(None, &key(), std::slice::from_ref(&entry));
        assert_eq!(summary.data["is_sidechain"], true);
    }

    #[test]
    fn custom_title_last_wins() {
        let entries = [
            json!({"type": "custom-title", "customTitle": "first title"}),
            json!({"type": "custom-title", "customTitle": "second title"}),
        ];
        let summary = fold_session_summary(None, &key(), &entries);
        assert_eq!(summary.data["custom_title"], "second title");
    }

    #[test]
    fn tag_entry_with_empty_string_clears_tag() {
        let entries = [
            json!({"type": "tag", "tag": "important"}),
            json!({"type": "tag", "tag": ""}),
        ];
        let summary = fold_session_summary(None, &key(), &entries);
        assert!(summary.data.get("tag").is_none());
    }

    #[test]
    fn created_at_parses_iso_timestamp() {
        let entry = json!({"type": "system", "timestamp": "2024-01-15T10:30:00.000Z"});
        let summary = fold_session_summary(None, &key(), std::slice::from_ref(&entry));
        assert!(summary.data.get("created_at").is_some());
    }

    #[test]
    fn created_at_is_set_once_from_first_timestamp() {
        let entries = [
            json!({"type": "system", "timestamp": "2024-01-15T10:30:00.000Z"}),
            json!({"type": "system", "timestamp": "2025-06-01T00:00:00.000Z"}),
        ];
        let summary = fold_session_summary(None, &key(), &entries);
        let first_created_at = summary.data["created_at"].as_i64().unwrap();
        let second_summary = fold_session_summary(
            Some(&SessionSummaryEntry {
                session_id: "sess".to_string(),
                mtime: 0,
                data: json!({"created_at": first_created_at}),
            }),
            &key(),
            &entries[1..],
        );
        assert_eq!(second_summary.data["created_at"], first_created_at);
    }

    #[test]
    fn summary_entry_to_sdk_info_prefers_custom_title() {
        let entry = SessionSummaryEntry {
            session_id: "sess".to_string(),
            mtime: 42,
            data: json!({"custom_title": "My Session", "first_prompt": "hi"}),
        };
        let info = summary_entry_to_sdk_info(&entry, None).expect("has summary");
        assert_eq!(info.summary, "My Session");
        assert_eq!(info.first_prompt.as_deref(), Some("hi"));
    }

    #[test]
    fn summary_entry_to_sdk_info_none_for_sidechain() {
        let entry = SessionSummaryEntry {
            session_id: "sess".to_string(),
            mtime: 1,
            data: json!({"is_sidechain": true, "first_prompt": "hi"}),
        };
        assert!(summary_entry_to_sdk_info(&entry, None).is_none());
    }

    #[test]
    fn summary_entry_to_sdk_info_none_when_no_summary_derivable() {
        let entry = SessionSummaryEntry {
            session_id: "sess".to_string(),
            mtime: 1,
            data: json!({}),
        };
        assert!(summary_entry_to_sdk_info(&entry, None).is_none());
    }

    #[test]
    fn summary_entry_to_sdk_info_cwd_falls_back_to_project_path() {
        let entry = SessionSummaryEntry {
            session_id: "sess".to_string(),
            mtime: 1,
            data: json!({"first_prompt": "hi"}),
        };
        let info = summary_entry_to_sdk_info(&entry, Some("/proj")).expect("has summary");
        assert_eq!(info.cwd.as_deref(), Some("/proj"));
    }
}
