# Phase 1 — Error Types

**Objective**: complete `src/error.rs` mirroring the upstream error
hierarchy with idiomatic `thiserror` enums.

**Upstream source of truth**: `reference/claude-agent-sdk-python/src/claude_agent_sdk/_errors.py`

## Upstream error hierarchy (sketch — ⚠️ VERIFY against `_errors.py`)

| Python class | Meaning | Rust variant |
|---|---|---|
| `ClaudeSDKError` | base class | the enum itself: `Error` |
| `CLINotFoundError` | `claude` binary not found | `Error::CliNotFound` |
| `CLIConnectionError` | failed to spawn/talk to subprocess | `Error::CliConnection` |
| `ProcessError` | CLI exited non-zero (carries exit code + stderr) | `Error::Process` |
| `CLIJSONDecodeError` | a stdout line was not valid JSON (carries the raw line) | `Error::JsonDecode` |
| `MessageParseError` | JSON was valid but not a known message shape | `Error::MessageParse` |

If upstream has variants not in this table (e.g. a control-protocol
timeout error), ADD them following the same pattern and note the
addition in `DEVIATIONS.md`.

## Deliverable — `src/error.rs` (write tests first, see below)

```rust
//! Error types for the Claude Agent SDK.
//!
//! One public [`Error`] enum mirrors the upstream Python hierarchy;
//! every fallible public API in this crate returns [`Result`].

use std::path::PathBuf;

/// Convenience alias used across the crate.
pub type Result<T> = std::result::Result<T, Error>;

/// All errors produced by this crate.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// The `claude` CLI binary could not be located.
    #[error(
        "Claude Code CLI not found{}. Install it with: npm install -g @anthropic-ai/claude-code",
        searched_path_suffix(searched_path)
    )]
    CliNotFound {
        /// Path that was checked, if a specific one was given.
        searched_path: Option<PathBuf>,
    },

    /// Failed to spawn or communicate with the CLI subprocess.
    #[error("failed to connect to Claude Code CLI: {message}")]
    CliConnection {
        /// Human-readable connection failure description.
        message: String,
        /// Underlying I/O error, when one exists.
        #[source]
        source: Option<std::io::Error>,
    },

    /// The CLI process exited with a non-zero status.
    #[error("Claude Code CLI exited with status {exit_code:?}: {stderr}")]
    Process {
        /// Exit code if the process terminated normally.
        exit_code: Option<i32>,
        /// Captured stderr output.
        stderr: String,
    },

    /// A line received from the CLI was not valid JSON.
    #[error("failed to decode JSON from CLI output: {source} (line: {line:.200})")]
    JsonDecode {
        /// The offending raw line (may be truncated in Display).
        line: String,
        /// The serde decode failure.
        #[source]
        source: serde_json::Error,
    },

    /// Valid JSON that does not match any known message shape.
    #[error("failed to parse message: {message}")]
    MessageParse {
        /// What was wrong with the shape.
        message: String,
        /// The JSON value that failed to parse.
        data: serde_json::Value,
    },

    /// A control-protocol request was rejected or timed out.
    #[error("control protocol error: {message}")]
    ControlProtocol {
        /// Failure description from the CLI or the timeout path.
        message: String,
    },

    /// The stdout buffer limit was exceeded before a newline arrived.
    #[error("buffer exceeded {limit} bytes while reading CLI output")]
    BufferOverflow {
        /// Configured limit in bytes.
        limit: usize,
    },
}

fn searched_path_suffix(searched_path: &Option<PathBuf>) -> String {
    match searched_path {
        Some(path) => format!(" at {}", path.display()),
        None => String::new(),
    }
}
```

Notes for the executor:

- `#[non_exhaustive]` is deliberate: the protocol evolves; consumers
  must keep a catch-all match arm.
- The `{stderr}` / `{line:.200}` display choices: error messages must
  carry context (which line failed, what stderr said) — never a bare
  "process failed".
- Register the module in `src/lib.rs`:

```rust
mod error;

pub use error::{Error, Result};
```

## Tests (write FIRST, in `#[cfg(test)] mod tests` at bottom of `error.rs`)

Each test is one behavior, named after it:

1. `cli_not_found_display_includes_install_hint` — construct
   `Error::CliNotFound { searched_path: None }`, assert its `to_string()`
   contains `"npm install -g @anthropic-ai/claude-code"`.
2. `cli_not_found_display_includes_path_when_given` — with
   `searched_path: Some("/opt/claude".into())`, assert display contains
   `"/opt/claude"`.
3. `process_error_display_includes_exit_code_and_stderr` — exit_code
   `Some(1)`, stderr `"boom"`; assert display contains `"1"` and `"boom"`.
4. `json_decode_error_preserves_source` — build from a real
   `serde_json::from_str::<serde_json::Value>("not json").unwrap_err()`;
   assert `std::error::Error::source(&err).is_some()`.
5. `message_parse_error_display_includes_message` — assert display
   contains the message text.
6. `errors_are_send_and_sync` — compile-time check:

```rust
fn assert_send_sync<T: Send + Sync>() {}

#[test]
fn errors_are_send_and_sync() {
    assert_send_sync::<Error>();
}
```

## Acceptance Gate

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo doc --no-deps
```

## Commits

1. `phase-1: error enum tests (red)`
2. `phase-1: error enum implementation (green)`
