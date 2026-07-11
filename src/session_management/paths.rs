//! Project-directory naming: the CLI's own convention for turning a
//! working directory into a stable on-disk (and `SessionStore`)
//! project key. Shared by both session-management families — see
//! `sessions.py`'s `_simple_hash`/`_sanitize_path`/`_canonicalize_path`/
//! `_get_projects_dir`/`_find_project_dir`.

use std::path::{Path, PathBuf};

use unicode_normalization::UnicodeNormalization;

/// Sanitized-name length above which a hash suffix (of the original,
/// unsanitized name) is appended instead of truncating silently.
const MAX_SANITIZED_LENGTH: usize = 200;

/// Base36 alphabet used by [`simple_hash`]'s output encoding.
const BASE36_DIGITS: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";

/// Hashes `s` with the CLI's own 32-bit rolling hash (JS `|= 0`
/// wraparound semantics), base36-encoded. Only used once a sanitized
/// name would exceed [`MAX_SANITIZED_LENGTH`].
#[must_use]
pub(crate) fn simple_hash(s: &str) -> String {
    let mut hash: i64 = 0;
    for ch in s.chars() {
        hash = (hash << 5) - hash + i64::from(u32::from(ch));
        hash &= 0xFFFF_FFFF;
        if hash >= 0x8000_0000 {
            hash -= 0x1_0000_0000;
        }
    }
    let mut n = hash.unsigned_abs();
    if n == 0 {
        return "0".to_string();
    }
    let mut out = Vec::new();
    while n > 0 {
        out.push(BASE36_DIGITS[(n % 36) as usize]);
        n /= 36;
    }
    out.reverse();
    String::from_utf8(out).expect("base36 digits are ASCII")
}

/// Replaces every non-ASCII-alphanumeric character with `-`; if the
/// result exceeds [`MAX_SANITIZED_LENGTH`], truncates to that length
/// and appends a hash (of the ORIGINAL, unsanitized name) suffix.
#[must_use]
pub(crate) fn sanitize_path(name: &str) -> String {
    let sanitized: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    if sanitized.chars().count() <= MAX_SANITIZED_LENGTH {
        return sanitized;
    }
    let hash = simple_hash(name);
    let prefix: String = sanitized.chars().take(MAX_SANITIZED_LENGTH).collect();
    format!("{prefix}-{hash}")
}

/// Resolves `directory` to its canonical, NFC-normalized form,
/// tolerating a directory that doesn't exist (falls back to
/// normalizing the input as-is, matching upstream's own
/// try/except-`OSError` fallback).
#[must_use]
pub(crate) fn canonicalize_path(directory: &str) -> String {
    match std::fs::canonicalize(directory) {
        Ok(resolved) => resolved.display().to_string().nfc().collect(),
        Err(_) => directory.nfc().collect(),
    }
}

/// Turns a working directory into the stable project key used to
/// scope both on-disk project directories and `SessionStore` entries.
/// `directory` defaults to the current working directory when `None`.
///
/// # Panics
///
/// Panics if the current working directory cannot be read (mirrors
/// `std::env::current_dir`'s own contract — a process without one is
/// already in an unrecoverable state).
#[must_use]
pub fn project_key_for_directory(directory: Option<&str>) -> String {
    let directory = directory.map_or_else(
        || {
            std::env::current_dir()
                .expect("current working directory is unavailable")
                .display()
                .to_string()
        },
        str::to_string,
    );
    sanitize_path(&canonicalize_path(&directory))
}

// Tests need to point `projects_dir()` at a throwaway temp directory.
// `std::env::set_var` requires `unsafe` since edition 2024 (thread-safety),
// and this crate forbids unsafe code outright — a thread-local override
// sidesteps both the global mutable state and the `unsafe` block, and
// (since every test here uses a plain `#[test]`, not a multi-threaded
// `#[tokio::test]`) never crosses OS threads mid-test.
#[cfg(test)]
thread_local! {
    static TEST_PROJECTS_DIR_OVERRIDE: std::cell::RefCell<Option<PathBuf>> =
        const { std::cell::RefCell::new(None) };
}

/// Test-only: overrides [`projects_dir`] for the current thread. Pass
/// `None` to restore the real (env/home-based) resolution.
#[cfg(test)]
pub(crate) fn set_test_projects_dir_override(path: Option<PathBuf>) {
    TEST_PROJECTS_DIR_OVERRIDE.with(|cell| *cell.borrow_mut() = path);
}

/// Base directory the CLI stores project transcripts under:
/// `$CLAUDE_CONFIG_DIR/projects` if set, else `~/.claude/projects`.
#[must_use]
pub(crate) fn projects_dir() -> PathBuf {
    #[cfg(test)]
    {
        if let Some(path) = TEST_PROJECTS_DIR_OVERRIDE.with(|cell| cell.borrow().clone()) {
            return path;
        }
    }
    let base = std::env::var_os("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .or_else(|| std::env::home_dir().map(|home| home.join(".claude")))
        .unwrap_or_else(|| PathBuf::from(".claude"));
    base.join("projects")
}

/// Resolves the on-disk project directory for `canonical_directory`
/// (already canonicalized), if one exists: an exact
/// `sanitize_path`-named match first, else — only when the sanitized
/// name was hash-truncated — a prefix scan for any directory
/// beginning with the same 200-char prefix (tolerates the CLI and
/// this SDK computing different hash suffixes for the same long path).
#[must_use]
pub(crate) fn find_project_dir(canonical_directory: &str) -> Option<PathBuf> {
    let projects_dir = projects_dir();
    let sanitized = sanitize_path(canonical_directory);
    let exact = projects_dir.join(&sanitized);
    if exact.is_dir() {
        return Some(exact);
    }
    if sanitized.chars().count() <= MAX_SANITIZED_LENGTH {
        return None;
    }
    let prefix: String = sanitized.chars().take(MAX_SANITIZED_LENGTH).collect();
    let prefix_with_dash = format!("{prefix}-");
    let entries = std::fs::read_dir(&projects_dir).ok()?;
    entries
        .filter_map(std::result::Result::ok)
        .map(|entry| entry.path())
        .find(|path| {
            path.is_dir()
                && path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with(&prefix_with_dash))
        })
}

/// Lists `git worktree list --porcelain` paths for the repository
/// containing `cwd`, NFC-normalized. Never fails — any error (not a
/// git repo, `git` missing, timeout) yields an empty list, matching
/// upstream's own best-effort behavior.
#[must_use]
pub(crate) fn worktree_paths(cwd: &Path) -> Vec<String> {
    let Ok(output) = std::process::Command::new("git")
        .args(["worktree", "list", "--porcelain"])
        .current_dir(cwd)
        .output()
    else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    let Ok(stdout) = String::from_utf8(output.stdout) else {
        return Vec::new();
    };
    stdout
        .lines()
        .filter_map(|line| line.strip_prefix("worktree "))
        .map(|path| path.nfc().collect())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_hash_of_empty_string_is_zero() {
        assert_eq!(simple_hash(""), "0");
    }

    #[test]
    fn simple_hash_is_deterministic() {
        assert_eq!(simple_hash("hello"), simple_hash("hello"));
        assert_ne!(simple_hash("hello"), simple_hash("world"));
    }

    #[test]
    fn sanitize_path_replaces_non_alphanumeric() {
        assert_eq!(sanitize_path("/Users/dev/my-repo"), "-Users-dev-my-repo");
    }

    #[test]
    fn sanitize_path_short_name_has_no_hash_suffix() {
        let sanitized = sanitize_path("/short/path");
        assert!(!sanitized.contains('-') || sanitized == "-short-path");
    }

    #[test]
    fn sanitize_path_truncates_long_names_with_hash_suffix() {
        let long_name = "/".to_string() + &"a".repeat(300);
        let sanitized = sanitize_path(&long_name);
        let prefix_len = MAX_SANITIZED_LENGTH;
        let (prefix, suffix) = sanitized.split_at(prefix_len);
        assert_eq!(prefix, "-".to_string() + &"a".repeat(199));
        assert!(suffix.starts_with('-'));
        assert_eq!(&suffix[1..], simple_hash(&long_name));
    }

    #[test]
    fn project_key_for_directory_is_stable_for_same_input() {
        assert_eq!(
            project_key_for_directory(Some("/tmp/example")),
            project_key_for_directory(Some("/tmp/example"))
        );
    }

    #[test]
    fn worktree_paths_on_non_git_dir_is_empty() {
        let dir = tempfile::tempdir().expect("create temp dir");
        assert_eq!(worktree_paths(dir.path()), Vec::<String>::new());
    }
}
