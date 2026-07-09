//! Integration tests for the subprocess transport, against a fake CLI.
//!
//! Unix-only: the fake CLI harness uses `#!/bin/sh` scripts.

#![cfg(unix)]

mod fake_cli;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use claude_agent_toolkit::{ClaudeAgentOptions, Error, SubprocessTransport, Transport};
use futures::StreamExt;

fn options_for(fake: &fake_cli::FakeCli) -> ClaudeAgentOptions {
    ClaudeAgentOptions::builder()
        .cli_path(fake.path.clone())
        .build()
}

#[test]
fn finds_cli_on_path() {
    let fake = fake_cli::scripted(&[], 0);
    let path_var = fake.dir.path().as_os_str().to_owned();
    let home = PathBuf::from("/nonexistent-home");
    let found = claude_agent_toolkit::transport::subprocess::find_cli_with(
        None,
        Some(&path_var),
        Some(&home),
        None,
    )
    .expect("finds fake CLI on PATH");
    assert_eq!(found, fake.path);
}

#[test]
fn returns_cli_not_found_with_install_hint() {
    let empty_path = std::ffi::OsString::from("");
    let home = PathBuf::from("/nonexistent-home");
    let result = claude_agent_toolkit::transport::subprocess::find_cli_with(
        None,
        Some(&empty_path),
        Some(&home),
        None,
    );
    let err = result.expect_err("must not find a CLI");
    assert!(matches!(err, Error::CliNotFound { .. }));
    assert!(err.to_string().contains("npm install"));
}

#[tokio::test]
async fn reads_scripted_messages_in_order() {
    let fake = fake_cli::scripted(
        &[
            r#"{"type":"system","subtype":"init"}"#,
            r#"{"type":"assistant","message":{"model":"m","content":[]}}"#,
            r#"{"type":"result","subtype":"success","duration_ms":1,"duration_api_ms":1,"is_error":false,"num_turns":1,"session_id":"s"}"#,
        ],
        0,
    );
    let mut transport = SubprocessTransport::new(options_for(&fake));
    transport.connect().await.expect("connects");

    let messages: Vec<_> = transport.read_messages().collect().await;
    assert_eq!(messages.len(), 3);
    for message in &messages {
        assert!(message.is_ok());
    }
    assert_eq!(messages[0].as_ref().unwrap()["type"], "system");
    assert_eq!(messages[1].as_ref().unwrap()["type"], "assistant");
    assert_eq!(messages[2].as_ref().unwrap()["type"], "result");

    transport.close().await.expect("closes");
}

#[tokio::test]
async fn skips_blank_lines() {
    let fake = fake_cli::scripted(&["", r#"{"type":"system","subtype":"init"}"#, ""], 0);
    let mut transport = SubprocessTransport::new(options_for(&fake));
    transport.connect().await.expect("connects");

    let messages: Vec<_> = transport.read_messages().collect().await;
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].as_ref().unwrap()["type"], "system");

    transport.close().await.expect("closes");
}

#[tokio::test]
async fn surfaces_json_decode_error_with_line() {
    let fake = fake_cli::scripted(&["{not json"], 0);
    let mut transport = SubprocessTransport::new(options_for(&fake));
    transport.connect().await.expect("connects");

    let messages: Vec<_> = transport.read_messages().collect().await;
    assert_eq!(messages.len(), 1);
    match &messages[0] {
        Err(Error::JsonDecode { line, .. }) => assert_eq!(line, "{not json"),
        other => panic!("expected JsonDecode error, got {other:?}"),
    }

    transport.close().await.expect("closes");
}

#[tokio::test]
async fn surfaces_process_error_on_nonzero_exit() {
    let fake = fake_cli::scripted(&[r#"{"type":"system","subtype":"init"}"#], 2);
    let mut transport = SubprocessTransport::new(options_for(&fake));
    transport.connect().await.expect("connects");

    let messages: Vec<_> = transport.read_messages().collect().await;
    assert_eq!(messages.len(), 2);
    assert!(messages[0].is_ok());
    match &messages[1] {
        Err(Error::Process { exit_code, .. }) => assert_eq!(*exit_code, Some(2)),
        other => panic!("expected Process error, got {other:?}"),
    }

    transport.close().await.expect("closes");
}

#[tokio::test]
async fn enforces_buffer_limit() {
    let long_line = format!(r#"{{"type":"x","pad":"{}"}}"#, "a".repeat(1000));
    let fake = fake_cli::scripted(&[&long_line], 0);
    let options = ClaudeAgentOptions::builder()
        .cli_path(fake.path.clone())
        .max_buffer_size(64)
        .build();
    let mut transport = SubprocessTransport::new(options);
    transport.connect().await.expect("connects");

    let messages: Vec<_> = transport.read_messages().collect().await;
    assert_eq!(messages.len(), 1);
    assert!(matches!(
        messages[0],
        Err(Error::BufferOverflow { limit: 64 })
    ));

    transport.close().await.expect("closes");
}

#[tokio::test]
async fn writes_lines_to_child_stdin() {
    let fake = fake_cli::recording(&[r#"{"type":"system","subtype":"init"}"#], 0);
    let mut transport = SubprocessTransport::new(options_for(&fake));
    transport.connect().await.expect("connects");

    transport
        .write_line(r#"{"type":"user","message":{"content":"one"}}"#)
        .await
        .expect("writes first line");
    transport
        .write_line(r#"{"type":"user","message":{"content":"two"}}"#)
        .await
        .expect("writes second line");
    transport.end_input().await.expect("ends input");

    let messages: Vec<_> = transport.read_messages().collect().await;
    assert_eq!(messages.len(), 1);

    let recorded = std::fs::read_to_string(&fake.stdin_recording_path).expect("reads recording");
    let lines: Vec<&str> = recorded.lines().collect();
    assert_eq!(lines.len(), 2);
    assert!(lines[0].contains("\"content\":\"one\""));
    assert!(lines[1].contains("\"content\":\"two\""));

    transport.close().await.expect("closes");
}

#[tokio::test]
async fn close_is_idempotent() {
    let fake = fake_cli::scripted(&[r#"{"type":"system","subtype":"init"}"#], 0);
    let mut transport = SubprocessTransport::new(options_for(&fake));
    transport.connect().await.expect("connects");

    let _messages: Vec<_> = transport.read_messages().collect().await;

    transport.close().await.expect("first close succeeds");
    transport.close().await.expect("second close is a no-op");
}

#[test]
fn full_command_args_have_expected_base_and_trailing_flags() {
    let options = ClaudeAgentOptions::default();
    let args = claude_agent_toolkit::full_command_args(&options);
    assert_eq!(
        &args[..3],
        &[
            "--output-format".to_string(),
            "stream-json".to_string(),
            "--verbose".to_string()
        ]
    );
    assert_eq!(
        &args[args.len() - 2..],
        &["--input-format".to_string(), "stream-json".to_string()]
    );
}

#[tokio::test]
async fn stderr_callback_receives_each_line() {
    let fake = fake_cli::scripted_with_stderr(
        &[r#"{"type":"system","subtype":"init"}"#],
        &["diag one", "diag two"],
        0,
    );
    let received: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let received_clone = Arc::clone(&received);
    let options = ClaudeAgentOptions::builder()
        .cli_path(fake.path.clone())
        .stderr(move |line: &str| {
            received_clone.lock().unwrap().push(line.to_string());
        })
        .build();
    let mut transport = SubprocessTransport::new(options);
    transport.connect().await.expect("connects");

    let _messages: Vec<_> = transport.read_messages().collect().await;
    transport.close().await.expect("closes");

    // Give the detached stderr-reading task a moment to finish draining.
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let lines = received.lock().unwrap().clone();
    assert_eq!(lines, vec!["diag one".to_string(), "diag two".to_string()]);
}

#[tokio::test]
async fn stderr_callback_panic_does_not_break_reading() {
    let fake =
        fake_cli::scripted_with_stderr(&[r#"{"type":"system","subtype":"init"}"#], &["boom"], 0);
    let options = ClaudeAgentOptions::builder()
        .cli_path(fake.path.clone())
        .stderr(|_line: &str| panic!("callback panics on purpose"))
        .build();
    let mut transport = SubprocessTransport::new(options);
    transport.connect().await.expect("connects");

    let messages: Vec<_> = transport.read_messages().collect().await;
    assert_eq!(messages.len(), 1);
    assert!(messages[0].is_ok());

    transport.close().await.expect("closes");
}

fn assert_send<T: Send>() {}

#[test]
fn subprocess_transport_is_send() {
    assert_send::<SubprocessTransport>();
}
