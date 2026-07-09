//! Subprocess-based Claude Code CLI transport.

use std::collections::HashMap;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use futures::stream::{self, BoxStream, StreamExt};
use serde_json::Value;
use tokio::io::AsyncWriteExt;
use tokio::process::{Child, ChildStdin, ChildStdout};
use tokio::sync::Mutex as AsyncMutex;

use crate::error::{Error, Result};
use crate::transport::Transport;
use crate::types::options::{ClaudeAgentOptions, DEFAULT_MAX_BUFFER_SIZE, build_cli_args};

/// Environment variable identifying this SDK to the CLI.
const SDK_ENTRYPOINT: &str = "sdk-rust";

/// Minimum supported Claude Code CLI version; below this, a warning is
/// logged (never an error).
const MINIMUM_CLAUDE_CODE_VERSION: (u64, u64, u64) = (2, 0, 0);

/// How long to wait for the process to exit gracefully after stdin is
/// closed, before escalating to a forced kill.
const GRACEFUL_SHUTDOWN_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// How long to wait for the process to exit after a forced kill.
const KILL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// Timeout for the best-effort `claude -v` version check.
const VERSION_CHECK_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);

/// Number of recent stderr lines retained for [`Error::Process`]
/// enrichment (a deliberate improvement over upstream — see
/// `DEVIATIONS.md`).
const STDERR_RING_BUFFER_LINES: usize = 50;

/// Names of executable candidates to probe per directory, in order.
#[cfg(unix)]
const CLI_BINARY_NAMES: &[&str] = &["claude"];
#[cfg(windows)]
const CLI_BINARY_NAMES: &[&str] = &["claude.exe", "claude.cmd", "claude.bat", "claude"];

/// Locates the `claude` binary using the real process environment.
///
/// Search order: explicit `cli_path` argument, `claude` on `PATH`, then
/// a set of well-known install locations.
///
/// # Errors
///
/// Returns [`Error::CliNotFound`] when nothing is found.
pub fn find_cli(cli_path: Option<&Path>) -> Result<PathBuf> {
    let path_var = std::env::var_os("PATH");
    let home = home_dir();
    #[cfg(windows)]
    let appdata = std::env::var_os("APPDATA").map(PathBuf::from);
    #[cfg(not(windows))]
    let appdata: Option<PathBuf> = None;
    find_cli_with(
        cli_path,
        path_var.as_deref(),
        home.as_deref(),
        appdata.as_deref(),
    )
}

#[cfg(unix)]
fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

#[cfg(windows)]
fn home_dir() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE").map(PathBuf::from)
}

/// Pure, dependency-injected CLI discovery — unit-testable without
/// touching real process environment.
///
/// Unlike the upstream Python package, this crate does not ship a
/// bundled CLI binary (no npm-style package directory to search), so
/// that lookup step from `_find_cli()` is skipped — see
/// `DEVIATIONS.md`.
///
/// # Errors
///
/// Returns [`Error::CliNotFound`] when nothing is found.
pub fn find_cli_with(
    cli_path: Option<&Path>,
    path_var: Option<&OsStr>,
    home: Option<&Path>,
    appdata: Option<&Path>,
) -> Result<PathBuf> {
    if let Some(path) = cli_path {
        return Ok(path.to_path_buf());
    }

    if let Some(found) = search_path(path_var) {
        return Ok(found);
    }

    if let Some(home) = home
        && let Some(found) = well_known_locations(home, appdata)
            .into_iter()
            .find(|candidate| candidate.is_file())
    {
        return Ok(found);
    }

    Err(Error::CliNotFound {
        searched_path: None,
    })
}

fn search_path(path_var: Option<&OsStr>) -> Option<PathBuf> {
    let path_var = path_var?;
    for dir in std::env::split_paths(path_var) {
        for name in CLI_BINARY_NAMES {
            let candidate = dir.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

fn well_known_locations(home: &Path, appdata: Option<&Path>) -> Vec<PathBuf> {
    let bases = [
        home.join(".npm-global").join("bin"),
        PathBuf::from("/usr/local/bin"),
        home.join(".local").join("bin"),
        home.join("node_modules").join(".bin"),
        home.join(".yarn").join("bin"),
        home.join(".claude").join("local"),
    ];

    let mut locations = Vec::new();
    for base in &bases {
        for name in CLI_BINARY_NAMES {
            locations.push(base.join(name));
        }
    }

    if let Some(appdata) = appdata {
        let npm_dir = appdata.join("npm");
        for name in CLI_BINARY_NAMES {
            locations.push(npm_dir.join(name));
        }
    }

    locations
}

/// Base flags always present, before the options-derived portion.
fn base_args() -> Vec<String> {
    vec![
        "--output-format".to_string(),
        "stream-json".to_string(),
        "--verbose".to_string(),
    ]
}

/// Builds the full CLI argument list (excluding the binary path
/// itself): base flags, then every options-derived flag from
/// [`build_cli_args`]. The CLI always runs in streaming-input mode —
/// see `DEVIATIONS.md` for why there is no one-shot `--print` path.
#[must_use]
pub fn full_command_args(options: &ClaudeAgentOptions) -> Vec<String> {
    let mut args = base_args();
    args.extend(build_cli_args(options));
    args
}

/// Spawns and manages the Claude Code CLI process.
///
/// Prompt-agnostic by design: writing turn/query messages is the
/// caller's responsibility via [`Transport::write_line`] — matching
/// upstream, where the prompt passed to the transport constructor is
/// never actually read back (see `DEVIATIONS.md`).
pub struct SubprocessTransport {
    options: ClaudeAgentOptions,
    cli_path: Option<PathBuf>,
    child: Option<Arc<AsyncMutex<Child>>>,
    stdin: Option<ChildStdin>,
    stdout: Option<ChildStdout>,
    ready: bool,
    max_buffer_size: usize,
    stderr_lines: Arc<Mutex<Vec<String>>>,
}

impl SubprocessTransport {
    /// Creates a transport for the given options. Does not spawn a
    /// process — call [`Transport::connect`] for that.
    #[must_use]
    pub fn new(options: ClaudeAgentOptions) -> Self {
        let max_buffer_size = options.max_buffer_size.unwrap_or(DEFAULT_MAX_BUFFER_SIZE);
        Self {
            options,
            cli_path: None,
            child: None,
            stdin: None,
            stdout: None,
            ready: false,
            max_buffer_size,
            stderr_lines: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Recent stderr lines captured from the child process (bounded to
    /// the last 50 lines).
    #[must_use]
    pub fn recent_stderr(&self) -> Vec<String> {
        self.stderr_lines
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    fn build_env(&self) -> HashMap<String, String> {
        let mut env: HashMap<String, String> = std::env::vars()
            .filter(|(key, _)| key != "CLAUDECODE")
            .collect();
        env.insert(
            "CLAUDE_CODE_ENTRYPOINT".to_string(),
            SDK_ENTRYPOINT.to_string(),
        );
        for (key, value) in &self.options.env {
            env.insert(key.clone(), value.clone());
        }
        env.insert(
            "CLAUDE_AGENT_SDK_VERSION".to_string(),
            env!("CARGO_PKG_VERSION").to_string(),
        );
        if self.options.enable_file_checkpointing {
            env.insert(
                "CLAUDE_CODE_ENABLE_SDK_FILE_CHECKPOINTING".to_string(),
                "true".to_string(),
            );
        }
        env
    }
}

impl Transport for SubprocessTransport {
    async fn connect(&mut self) -> Result<()> {
        let cli_path = find_cli(self.options.cli_path.as_deref())?;
        self.cli_path = Some(cli_path.clone());

        warn_if_cli_version_outdated(&cli_path).await;

        if let Some(cwd) = &self.options.cwd
            && !cwd.exists()
        {
            return Err(Error::CliConnection {
                message: format!("working directory does not exist: {}", cwd.display()),
                source: None,
            });
        }

        let args = full_command_args(&self.options);
        let mut command = tokio::process::Command::new(&cli_path);
        command.args(&args);
        command.env_clear();
        command.envs(self.build_env());
        if let Some(cwd) = &self.options.cwd {
            command.current_dir(cwd);
        }
        command.stdin(std::process::Stdio::piped());
        command.stdout(std::process::Stdio::piped());
        if self.options.stderr.is_some() {
            command.stderr(std::process::Stdio::piped());
        } else {
            command.stderr(std::process::Stdio::null());
        }

        let mut child = command.spawn().map_err(|source| {
            if source.kind() == std::io::ErrorKind::NotFound {
                Error::CliNotFound {
                    searched_path: Some(cli_path.clone()),
                }
            } else {
                Error::CliConnection {
                    message: format!("failed to start Claude Code CLI: {source}"),
                    source: Some(source),
                }
            }
        })?;

        self.stdin = child.stdin.take();
        self.stdout = child.stdout.take();

        if let Some(stderr) = child.stderr.take() {
            spawn_stderr_reader(
                stderr,
                self.options.stderr.clone(),
                Arc::clone(&self.stderr_lines),
            );
        }

        self.child = Some(Arc::new(AsyncMutex::new(child)));
        self.ready = true;
        Ok(())
    }

    async fn write_line(&mut self, line: &str) -> Result<()> {
        if !self.ready {
            return Err(Error::CliConnection {
                message: "transport is not ready for writing".to_string(),
                source: None,
            });
        }
        let Some(stdin) = self.stdin.as_mut() else {
            return Err(Error::CliConnection {
                message: "transport is not ready for writing".to_string(),
                source: None,
            });
        };

        let mut payload = line.as_bytes().to_vec();
        payload.push(b'\n');
        stdin.write_all(&payload).await.map_err(|source| {
            self.ready = false;
            Error::CliConnection {
                message: format!("failed to write to process stdin: {source}"),
                source: Some(source),
            }
        })?;
        stdin.flush().await.map_err(|source| Error::CliConnection {
            message: format!("failed to flush process stdin: {source}"),
            source: Some(source),
        })
    }

    async fn end_input(&mut self) -> Result<()> {
        if let Some(mut stdin) = self.stdin.take() {
            let _ = stdin.shutdown().await;
        }
        Ok(())
    }

    fn read_messages(&mut self) -> BoxStream<'static, Result<Value>> {
        let Some(stdout) = self.stdout.take() else {
            return stream::empty().boxed();
        };
        let child = self.child.clone();
        let max_buffer_size = self.max_buffer_size;

        let state = ReadState {
            reader: tokio::io::BufReader::new(stdout),
            buffer: Vec::new(),
            max_buffer_size,
            child,
            done: false,
        };

        stream::unfold(state, read_next_message).boxed()
    }

    async fn close(&mut self) -> Result<()> {
        self.ready = false;

        if let Some(mut stdin) = self.stdin.take() {
            let _ = stdin.shutdown().await;
        }
        self.stdout = None;

        let Some(child) = self.child.take() else {
            return Ok(());
        };

        let mut guard = child.lock().await;
        if guard.try_wait().ok().flatten().is_some() {
            return Ok(());
        }

        if tokio::time::timeout(GRACEFUL_SHUTDOWN_TIMEOUT, guard.wait())
            .await
            .is_err()
        {
            let _ = guard.start_kill();
            let _ = tokio::time::timeout(KILL_TIMEOUT, guard.wait()).await;
        }
        Ok(())
    }

    fn is_ready(&self) -> bool {
        self.ready
    }
}

struct ReadState {
    reader: tokio::io::BufReader<ChildStdout>,
    buffer: Vec<u8>,
    max_buffer_size: usize,
    child: Option<Arc<AsyncMutex<Child>>>,
    done: bool,
}

async fn read_next_message(mut state: ReadState) -> Option<(Result<Value>, ReadState)> {
    loop {
        if state.done {
            return None;
        }

        match read_one_line(&mut state.reader, &mut state.buffer, state.max_buffer_size).await {
            Ok(Some(line)) => {
                let Some(value) = parse_stdout_line(&line) else {
                    continue;
                };
                match value {
                    Ok(value) => return Some((Ok(value), state)),
                    Err(error) => {
                        state.done = true;
                        return Some((Err(error), state));
                    }
                }
            }
            Ok(None) => {
                state.done = true;
                return exit_status_result(state.child.as_ref())
                    .await
                    .map(|result| (result, state));
            }
            Err(error) => {
                state.done = true;
                return Some((Err(error), state));
            }
        }
    }
}

/// Reads one `\n`-terminated line as UTF-8, enforcing `max_buffer_size`
/// on the accumulated (still incomplete) line length. Returns `None`
/// at clean EOF with no partial data.
async fn read_one_line(
    reader: &mut tokio::io::BufReader<ChildStdout>,
    buffer: &mut Vec<u8>,
    max_buffer_size: usize,
) -> Result<Option<String>> {
    use tokio::io::AsyncBufReadExt;

    buffer.clear();
    let bytes_read =
        reader
            .read_until(b'\n', buffer)
            .await
            .map_err(|source| Error::CliConnection {
                message: format!("failed to read from process stdout: {source}"),
                source: Some(source),
            })?;

    if bytes_read == 0 {
        return Ok(None);
    }

    if buffer.len() > max_buffer_size {
        return Err(Error::BufferOverflow {
            limit: max_buffer_size,
        });
    }

    while buffer.last() == Some(&b'\n') || buffer.last() == Some(&b'\r') {
        buffer.pop();
    }

    String::from_utf8(buffer.clone())
        .map(Some)
        .map_err(|source| Error::CliConnection {
            message: format!("process stdout was not valid UTF-8: {source}"),
            source: None,
        })
}

/// Parses one complete stdout line. Returns `None` for lines that
/// carry no message (blank, or non-JSON diagnostic output some CLI
/// builds write to stdout) — only a line that looks like JSON but
/// fails to parse is an error.
fn parse_stdout_line(line: &str) -> Option<Result<Value>> {
    let trimmed = line.trim();
    if trimmed.is_empty() || !trimmed.starts_with('{') {
        return None;
    }
    Some(
        serde_json::from_str::<Value>(trimmed).map_err(|source| Error::JsonDecode {
            line: trimmed.to_string(),
            source,
        }),
    )
}

async fn exit_status_result(child: Option<&Arc<AsyncMutex<Child>>>) -> Option<Result<Value>> {
    let child = child?;
    let mut guard = child.lock().await;
    let status = guard.wait().await.ok()?;
    if status.success() {
        None
    } else {
        Some(Err(Error::Process {
            exit_code: status.code(),
            stderr: "process exited with a non-zero status".to_string(),
        }))
    }
}

fn spawn_stderr_reader(
    stderr: tokio::process::ChildStderr,
    callback: Option<crate::types::options::StderrCallback>,
    lines_buffer: Arc<Mutex<Vec<String>>>,
) {
    tokio::spawn(async move {
        let mut reader = tokio::io::BufReader::new(stderr);
        let mut buffer = Vec::new();
        loop {
            use tokio::io::AsyncBufReadExt;
            buffer.clear();
            let bytes_read = match reader.read_until(b'\n', &mut buffer).await {
                Ok(0) | Err(_) => break,
                Ok(n) => n,
            };
            let _ = bytes_read;
            while buffer.last() == Some(&b'\n') || buffer.last() == Some(&b'\r') {
                buffer.pop();
            }
            let Ok(line) = String::from_utf8(buffer.clone()) else {
                continue;
            };
            if line.is_empty() {
                continue;
            }

            if let Ok(mut lines) = lines_buffer.lock() {
                lines.push(line.clone());
                let overflow = lines.len().saturating_sub(STDERR_RING_BUFFER_LINES);
                if overflow > 0 {
                    lines.drain(0..overflow);
                }
            }

            if let Some(callback) = &callback {
                let callback = Arc::clone(callback);
                let line_for_callback = line.clone();
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    callback(&line_for_callback);
                }));
                if result.is_err() {
                    tracing::warn!("stderr callback panicked; continuing");
                }
            }

            tracing::debug!(%line, "claude CLI stderr");
        }
    });
}

/// Best-effort version check: warns (never errors) if the resolved CLI
/// is older than [`MINIMUM_CLAUDE_CODE_VERSION`]. All failures are
/// swallowed, matching upstream's `_check_claude_version`.
async fn warn_if_cli_version_outdated(cli_path: &Path) {
    let Ok(Ok(output)) = tokio::time::timeout(
        VERSION_CHECK_TIMEOUT,
        tokio::process::Command::new(cli_path).arg("-v").output(),
    )
    .await
    else {
        return;
    };

    let Ok(stdout) = String::from_utf8(output.stdout) else {
        return;
    };
    let Some(version) = parse_leading_semver(stdout.trim()) else {
        return;
    };

    if version < MINIMUM_CLAUDE_CODE_VERSION {
        tracing::warn!(
            found = %format!("{}.{}.{}", version.0, version.1, version.2),
            minimum = %format!(
                "{}.{}.{}",
                MINIMUM_CLAUDE_CODE_VERSION.0,
                MINIMUM_CLAUDE_CODE_VERSION.1,
                MINIMUM_CLAUDE_CODE_VERSION.2
            ),
            path = %cli_path.display(),
            "Claude Code CLI version is unsupported by this SDK; some features may not work correctly"
        );
    }
}

fn parse_leading_semver(text: &str) -> Option<(u64, u64, u64)> {
    let mut parts = text.split(['.', ' ', '-']);
    let major: u64 = parts.next()?.parse().ok()?;
    let minor: u64 = parts.next()?.parse().ok()?;
    let patch: u64 = parts.next()?.parse().ok()?;
    Some((major, minor, patch))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_cli_prefers_explicit_path() {
        let result = find_cli_with(Some(Path::new("/explicit/claude")), None, None, None);
        assert_eq!(result.unwrap(), PathBuf::from("/explicit/claude"));
    }

    #[test]
    fn find_cli_returns_not_found_with_empty_search_space() {
        let result = find_cli_with(None, None, None, None);
        assert!(matches!(result, Err(Error::CliNotFound { .. })));
    }

    #[test]
    fn full_command_args_always_ends_with_streaming_input_format() {
        let options = ClaudeAgentOptions::default();
        let args = full_command_args(&options);
        assert_eq!(args[0], "--output-format");
        assert_eq!(args[1], "stream-json");
        assert_eq!(args[2], "--verbose");
        assert_eq!(args[args.len() - 2], "--input-format");
        assert_eq!(args[args.len() - 1], "stream-json");
    }

    #[test]
    fn parse_stdout_line_skips_blank_and_non_json_lines() {
        assert!(parse_stdout_line("").is_none());
        assert!(parse_stdout_line("   ").is_none());
        assert!(parse_stdout_line("[SandboxDebug] hello").is_none());
    }

    #[test]
    fn parse_stdout_line_errors_on_malformed_json_prefixed_line() {
        let result = parse_stdout_line("{not valid json");
        assert!(matches!(result, Some(Err(Error::JsonDecode { .. }))));
    }

    #[test]
    fn parse_stdout_line_parses_valid_json_object() {
        let result = parse_stdout_line(r#"{"type":"system"}"#);
        assert!(matches!(result, Some(Ok(Value::Object(_)))));
    }

    #[test]
    fn parse_leading_semver_parses_dotted_triplet() {
        assert_eq!(parse_leading_semver("2.1.110"), Some((2, 1, 110)));
        assert_eq!(
            parse_leading_semver("1.9.0 (some build info)"),
            Some((1, 9, 0))
        );
        assert_eq!(parse_leading_semver("not a version"), None);
    }

    #[cfg(unix)]
    #[test]
    fn well_known_locations_include_claude_local() {
        let home = Path::new("/home/user");
        let locations = well_known_locations(home, None);
        assert!(locations.contains(&PathBuf::from("/home/user/.claude/local/claude")));
    }
}
