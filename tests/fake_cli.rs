//! Builds throwaway shell scripts that impersonate the claude CLI.
//!
//! Unix-only: the scripts are `#!/bin/sh`. Integration tests that use
//! this harness are gated with `#[cfg(unix)]` at the call site.
//!
//! This file is shared by multiple test targets, each via its own
//! `mod fake_cli;`/`#[path] mod fake_cli;` inclusion — a helper unused
//! by one target but used by another is not dead code overall, so
//! per-target dead-code warnings here are silenced blanket.
#![allow(dead_code)]

use std::fmt::Write as _;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use tempfile::TempDir;

/// A fake `claude` CLI, backed by a throwaway shell script.
pub struct FakeCli {
    /// Owns the temp directory; dropped (and cleaned up) with `self`.
    pub dir: TempDir,
    /// Path to the executable script.
    pub path: PathBuf,
    /// Path to the file stdin gets echoed to, for `recording()` fakes.
    pub stdin_recording_path: PathBuf,
}

fn shell_single_quote(line: &str) -> String {
    format!("'{}'", line.replace('\'', "'\\''"))
}

/// # Panics
///
/// Panics if the script cannot be written or made executable — a
/// hard test-setup failure, not a case tests should recover from.
fn write_script(dir: TempDir, body: &str) -> FakeCli {
    let path = dir.path().join("claude");
    fs::write(&path, body).expect("write fake CLI script");
    let mut perms = fs::metadata(&path)
        .expect("stat fake CLI script")
        .permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&path, perms).expect("chmod fake CLI script");
    let stdin_recording_path = dir.path().join("stdin_recording.txt");
    FakeCli {
        dir,
        path,
        stdin_recording_path,
    }
}

/// Creates an executable script that writes each given line to stdout
/// (each `printf`'d verbatim, so a caller can pass non-JSON or
/// malformed lines deliberately), then exits with `exit_code`.
///
/// # Panics
///
/// Panics on test-setup failure (temp dir or script creation).
#[must_use]
pub fn scripted(lines: &[&str], exit_code: i32) -> FakeCli {
    let mut body = String::from("#!/bin/sh\n");
    for line in lines {
        body.push_str("printf '%s\\n' ");
        body.push_str(&shell_single_quote(line));
        body.push('\n');
    }
    let _ = writeln!(body, "exit {exit_code}");
    write_script(TempDir::new().expect("create temp dir"), &body)
}

/// Variant of [`scripted`] that also writes to stderr, interleaved:
/// `stderr_lines` are printed first, then `stdout_lines`, then exits.
///
/// # Panics
///
/// Panics on test-setup failure (temp dir or script creation).
#[must_use]
pub fn scripted_with_stderr(
    stdout_lines: &[&str],
    stderr_lines: &[&str],
    exit_code: i32,
) -> FakeCli {
    let mut body = String::from("#!/bin/sh\n");
    for line in stderr_lines {
        body.push_str("printf '%s\\n' ");
        body.push_str(&shell_single_quote(line));
        body.push_str(" >&2\n");
    }
    for line in stdout_lines {
        body.push_str("printf '%s\\n' ");
        body.push_str(&shell_single_quote(line));
        body.push('\n');
    }
    let _ = writeln!(body, "exit {exit_code}");
    write_script(TempDir::new().expect("create temp dir"), &body)
}

/// Variant that first echoes back every stdin line to a file (for
/// asserting what the SDK wrote), then prints the scripted lines.
///
/// # Panics
///
/// Panics on test-setup failure (temp dir or script creation).
#[must_use]
pub fn recording(lines: &[&str], exit_code: i32) -> FakeCli {
    let dir = TempDir::new().expect("create temp dir");
    let recording_path = dir.path().join("stdin_recording.txt");

    let mut body = String::from("#!/bin/sh\n");
    let _ = writeln!(
        body,
        "cat > {}",
        shell_single_quote(&recording_path.display().to_string())
    );
    for line in lines {
        body.push_str("printf '%s\\n' ");
        body.push_str(&shell_single_quote(line));
        body.push('\n');
    }
    let _ = writeln!(body, "exit {exit_code}");
    write_script(dir, &body)
}

/// A fake CLI that reads stdin lines and responds per a simple rule
/// table: each incoming line matching `*pattern*` triggers printing
/// the paired response line to stdout. Once stdin closes, `trailing`
/// lines are printed, then the process exits 0.
///
/// `rules` patterns are plain substrings (no shell-glob escaping) —
/// keep them simple identifiers like `"interrupt"` or `"hook_callback"`.
///
/// # Panics
///
/// Panics on test-setup failure (temp dir or script creation).
#[must_use]
pub fn responding(rules: &[(&str, &str)], trailing: &[&str]) -> FakeCli {
    let dir = TempDir::new().expect("create temp dir");
    let mut body = String::from("#!/bin/sh\nwhile IFS= read -r line; do\n  case \"$line\" in\n");
    for (pattern, response) in rules {
        let _ = writeln!(
            body,
            "    *{pattern}*) printf '%s\\n' {} ;;",
            shell_single_quote(response)
        );
    }
    body.push_str("  esac\ndone\n");
    for line in trailing {
        body.push_str("printf '%s\\n' ");
        body.push_str(&shell_single_quote(line));
        body.push('\n');
    }
    body.push_str("exit 0\n");
    write_script(dir, &body)
}

/// A fake CLI that immediately prints `lines` (independent of stdin),
/// while concurrently recording every stdin line it receives to a
/// file — for tests where the CLI must initiate a control request and
/// the SDK's response needs to be captured afterward. Waits for stdin
/// to close before exiting with `exit_code`.
///
/// # Panics
///
/// Panics on test-setup failure (temp dir or script creation).
#[must_use]
pub fn scripted_and_recording(lines: &[&str], exit_code: i32) -> FakeCli {
    let dir = TempDir::new().expect("create temp dir");
    let recording_path = dir.path().join("stdin_recording.txt");

    // The stdout writer runs backgrounded (it doesn't touch stdin, so
    // backgrounding it is harmless); the stdin recorder stays in the
    // foreground. The reverse — backgrounding `cat > file` — silently
    // loses all input: bash redirects a backgrounded job's stdin to
    // /dev/null in non-interactive scripts when job control is off.
    let mut body = String::from("#!/bin/sh\n(\n");
    for line in lines {
        body.push_str("printf '%s\\n' ");
        body.push_str(&shell_single_quote(line));
        body.push('\n');
    }
    body.push_str(") &\n");
    let _ = writeln!(
        body,
        "cat > {}",
        shell_single_quote(&recording_path.display().to_string())
    );
    body.push_str("wait\n");
    let _ = writeln!(body, "exit {exit_code}");
    write_script(dir, &body)
}

/// A fake CLI for `query()`/`query_stream()` tests: `query()` always
/// runs the `initialize` handshake first (Phase 6 finding), so a fake
/// that doesn't answer it makes every test hang until the control
/// timeout. This variant reads stdin in a loop, records every line to
/// `stdin_recording_path`, and — the moment it sees a line containing
/// `"subtype":"initialize"` — extracts that request's `request_id`
/// (via `sed`, no JSON parsing needed) and replies with a canned
/// success response before printing `lines` and (if any) `stderr_lines`.
/// Exits with `exit_code` once stdin closes.
///
/// # Panics
///
/// Panics on test-setup failure (temp dir or script creation).
#[must_use]
pub fn scripted_with_initialize(lines: &[&str], stderr_lines: &[&str], exit_code: i32) -> FakeCli {
    let dir = TempDir::new().expect("create temp dir");
    let recording_path = dir.path().join("stdin_recording.txt");
    let recording_path_quoted = shell_single_quote(&recording_path.display().to_string());

    let mut body = String::from("#!/bin/sh\nwhile IFS= read -r line; do\n");
    let _ = writeln!(
        body,
        "  printf '%s\\n' \"$line\" >> {recording_path_quoted}"
    );
    body.push_str("  case \"$line\" in\n");
    body.push_str("    *'\"subtype\":\"initialize\"'*)\n");
    body.push_str(
        "      req_id=$(printf '%s' \"$line\" | sed -n 's/.*\"request_id\":\"\\([^\"]*\\)\".*/\\1/p')\n",
    );
    body.push_str(
        "      printf '{\"type\":\"control_response\",\"response\":{\"subtype\":\"success\",\"request_id\":\"%s\",\"response\":{}}}\\n' \"$req_id\"\n",
    );
    for line in stderr_lines {
        body.push_str("      printf '%s\\n' ");
        body.push_str(&shell_single_quote(line));
        body.push_str(" >&2\n");
    }
    for line in lines {
        body.push_str("      printf '%s\\n' ");
        body.push_str(&shell_single_quote(line));
        body.push('\n');
    }
    body.push_str("      ;;\n  esac\ndone\n");
    let _ = writeln!(body, "exit {exit_code}");
    write_script(dir, &body)
}
