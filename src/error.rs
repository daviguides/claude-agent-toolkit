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
        searched_path_suffix(searched_path.as_ref())
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

fn searched_path_suffix(searched_path: Option<&PathBuf>) -> String {
    match searched_path {
        Some(path) => format!(" at {}", path.display()),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_not_found_display_includes_install_hint() {
        let err = Error::CliNotFound {
            searched_path: None,
        };
        assert!(
            err.to_string()
                .contains("npm install -g @anthropic-ai/claude-code")
        );
    }

    #[test]
    fn cli_not_found_display_includes_path_when_given() {
        let err = Error::CliNotFound {
            searched_path: Some("/opt/claude".into()),
        };
        assert!(err.to_string().contains("/opt/claude"));
    }

    #[test]
    fn process_error_display_includes_exit_code_and_stderr() {
        let err = Error::Process {
            exit_code: Some(1),
            stderr: "boom".to_string(),
        };
        let display = err.to_string();
        assert!(display.contains('1'));
        assert!(display.contains("boom"));
    }

    #[test]
    fn json_decode_error_preserves_source() {
        let source = serde_json::from_str::<serde_json::Value>("not json").unwrap_err();
        let err = Error::JsonDecode {
            line: "not json".to_string(),
            source,
        };
        assert!(std::error::Error::source(&err).is_some());
    }

    #[test]
    fn message_parse_error_display_includes_message() {
        let err = Error::MessageParse {
            message: "unexpected shape".to_string(),
            data: serde_json::Value::Null,
        };
        assert!(err.to_string().contains("unexpected shape"));
    }

    fn assert_send_sync<T: Send + Sync>() {}

    #[test]
    fn errors_are_send_and_sync() {
        assert_send_sync::<Error>();
    }
}
