//! Error types for the Claude Agent SDK.
//!
//! One public [`Error`] enum mirrors the upstream Python hierarchy;
//! every fallible public API in this crate returns [`Result`].

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_not_found_display_includes_install_hint() {
        let err = Error::CliNotFound { searched_path: None };
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
