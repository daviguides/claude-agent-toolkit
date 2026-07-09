//! Builds throwaway shell scripts that impersonate the claude CLI.
//!
//! Unix-only: the scripts are `#!/bin/sh`. Integration tests that use
//! this harness are gated with `#[cfg(unix)]` at the call site.

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
