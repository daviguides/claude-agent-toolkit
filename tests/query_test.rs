//! Integration tests for the public `query()`/`query_stream()` API,
//! against a fake CLI.
//!
//! Unix-only: the fake CLI harness uses `#!/bin/sh` scripts. `query()`
//! always runs the `initialize` handshake first (a Phase 6 finding),
//! so every fake here uses `scripted_with_initialize`, which acks it
//! before behaving like a plain scripted/recording fake — see
//! `DEVIATIONS.md`. Because it records every stdin line, the
//! `initialize` request itself is always the first recorded line;
//! tests that inspect recorded input skip it explicitly.

#![cfg(unix)]

mod fake_cli;

use claude_agent_toolkit::{
    ClaudeAgentOptions, ContentBlock, Error, Message, UserContent, query, query_stream,
};
use futures::StreamExt;

fn options_for(fake: &fake_cli::FakeCli) -> ClaudeAgentOptions {
    ClaudeAgentOptions::builder()
        .cli_path(fake.path.clone())
        .build()
}

fn recorded_lines_after_initialize(fake: &fake_cli::FakeCli) -> Vec<serde_json::Value> {
    let recorded = std::fs::read_to_string(&fake.stdin_recording_path).expect("reads recording");
    recorded
        .lines()
        .map(|line| serde_json::from_str(line).expect("valid json"))
        .filter(|value: &serde_json::Value| value["request"]["subtype"] != "initialize")
        .collect()
}

#[tokio::test]
async fn yields_typed_messages_in_order() {
    let fake = fake_cli::scripted_with_initialize(
        &[
            r#"{"type":"system","subtype":"init","session_id":"s"}"#,
            r#"{"type":"assistant","message":{"model":"m","content":[{"type":"text","text":"hi"}]}}"#,
            r#"{"type":"result","subtype":"success","duration_ms":1,"duration_api_ms":1,"is_error":false,"num_turns":1,"session_id":"s"}"#,
        ],
        &[],
        0,
    );
    let mut stream = query("hello", options_for(&fake))
        .await
        .expect("query starts");

    let first = stream.next().await.expect("has item").expect("ok");
    assert!(matches!(first, Message::System(_)));
    let second = stream.next().await.expect("has item").expect("ok");
    assert!(matches!(second, Message::Assistant(_)));
    let third = stream.next().await.expect("has item").expect("ok");
    assert!(matches!(third, Message::Result(_)));
    assert!(stream.next().await.is_none());
}

#[tokio::test]
async fn assistant_text_content_is_parsed() {
    let fake = fake_cli::scripted_with_initialize(
        &[
            r#"{"type":"assistant","message":{"model":"m","content":[{"type":"text","text":"2 + 2 = 4"}]}}"#,
        ],
        &[],
        0,
    );
    let mut stream = query("what is 2+2", options_for(&fake))
        .await
        .expect("query starts");

    let message = stream.next().await.expect("has item").expect("ok");
    let Message::Assistant(assistant) = message else {
        panic!("expected Message::Assistant");
    };
    let ContentBlock::Text { text } = &assistant.content[0] else {
        panic!("expected ContentBlock::Text");
    };
    assert_eq!(text, "2 + 2 = 4");
}

#[tokio::test]
async fn stream_ends_after_process_exit() {
    let fake =
        fake_cli::scripted_with_initialize(&[r#"{"type":"system","subtype":"init"}"#], &[], 0);
    let mut stream = query("hi", options_for(&fake)).await.expect("query starts");

    assert!(stream.next().await.is_some());
    assert!(stream.next().await.is_none());
    // Fused: calling again still yields None, no panic.
    assert!(stream.next().await.is_none());
}

#[tokio::test]
async fn invalid_json_line_yields_decode_error_then_stops() {
    let fake = fake_cli::scripted_with_initialize(
        &[r#"{"type":"system","subtype":"init"}"#, "{not valid json"],
        &[],
        0,
    );
    let mut stream = query("hi", options_for(&fake)).await.expect("query starts");

    let first = stream.next().await.expect("has item").expect("ok");
    assert!(matches!(first, Message::System(_)));

    let second = stream.next().await.expect("has item");
    assert!(matches!(second, Err(Error::JsonDecode { .. })));

    assert!(stream.next().await.is_none());
}

#[tokio::test]
async fn nonzero_exit_yields_process_error() {
    let fake = fake_cli::scripted_with_initialize(
        &[r#"{"type":"system","subtype":"init"}"#],
        &["boom"],
        1,
    );
    let mut stream = query("hi", options_for(&fake)).await.expect("query starts");

    let first = stream.next().await.expect("has item").expect("ok");
    assert!(matches!(first, Message::System(_)));

    let second = stream.next().await.expect("has item");
    assert!(matches!(
        second,
        Err(Error::Process {
            exit_code: Some(1),
            ..
        })
    ));
}

#[tokio::test]
async fn prompt_reaches_cli_via_stdin_line() {
    let fake =
        fake_cli::scripted_with_initialize(&[r#"{"type":"system","subtype":"init"}"#], &[], 0);
    let mut stream = query("what is 2+2?", options_for(&fake))
        .await
        .expect("query starts");
    while stream.next().await.is_some() {}

    let lines = recorded_lines_after_initialize(&fake);
    assert_eq!(
        lines[0],
        serde_json::json!({
            "type": "user",
            "message": {"role": "user", "content": "what is 2+2?"},
            "parent_tool_use_id": null,
            "session_id": ""
        })
    );
}

#[tokio::test]
async fn spawn_failure_is_returned_eagerly() {
    let options = ClaudeAgentOptions::builder()
        .cli_path("/nonexistent/path/to/claude")
        .build();
    match query("hi", options).await {
        Err(Error::CliNotFound { .. } | Error::CliConnection { .. }) => {}
        other => panic!("expected an eager CLI spawn error, got {}", other.is_ok()),
    }
}

#[tokio::test]
async fn stream_prompt_items_are_forwarded_in_order() {
    let fake =
        fake_cli::scripted_with_initialize(&[r#"{"type":"system","subtype":"init"}"#], &[], 0);
    let items = futures::stream::iter(vec![
        UserContent::Text("first".to_string()),
        UserContent::Text("second".to_string()),
    ]);
    let mut stream = query_stream(items, options_for(&fake))
        .await
        .expect("query_stream starts");
    while stream.next().await.is_some() {}

    let lines = recorded_lines_after_initialize(&fake);
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0]["message"]["content"], "first");
    assert_eq!(lines[0]["session_id"], "default");
    assert_eq!(lines[1]["message"]["content"], "second");
}

#[tokio::test]
async fn responses_flow_while_input_still_open() {
    let fake =
        fake_cli::scripted_with_initialize(&[r#"{"type":"system","subtype":"init"}"#], &[], 0);
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<UserContent>();
    let items = tokio_stream_from_unbounded(rx);

    let mut stream = query_stream(items, options_for(&fake))
        .await
        .expect("query_stream starts");

    tx.send(UserContent::Text("first".to_string()))
        .expect("sends first");

    // The scripted message must arrive without needing a second input
    // item — proves there's no input-drain barrier before reading.
    let message = stream.next().await.expect("has item").expect("ok");
    assert!(matches!(message, Message::System(_)));

    drop(tx);
    // Drain the rest so the process exits cleanly.
    while stream.next().await.is_some() {}
}

#[tokio::test]
async fn input_stream_end_closes_stdin() {
    let fake = fake_cli::scripted_with_initialize(
        &[
            r#"{"type":"result","subtype":"success","duration_ms":1,"duration_api_ms":1,"is_error":false,"num_turns":1,"session_id":"s"}"#,
        ],
        &[],
        0,
    );
    let items = futures::stream::iter(vec![UserContent::Text("only".to_string())]);
    let mut stream = query_stream(items, options_for(&fake))
        .await
        .expect("query_stream starts");

    let message = stream.next().await.expect("has item").expect("ok");
    assert!(matches!(message, Message::Result(_)));
    assert!(stream.next().await.is_none());
}

#[tokio::test]
async fn block_content_items_serialize_as_blocks() {
    let fake =
        fake_cli::scripted_with_initialize(&[r#"{"type":"system","subtype":"init"}"#], &[], 0);
    let items = futures::stream::iter(vec![UserContent::Blocks(vec![ContentBlock::Text {
        text: "hi".to_string(),
    }])]);
    let mut stream = query_stream(items, options_for(&fake))
        .await
        .expect("query_stream starts");
    while stream.next().await.is_some() {}

    let lines = recorded_lines_after_initialize(&fake);
    assert_eq!(
        lines[0]["message"]["content"],
        serde_json::json!([{"type": "text", "text": "hi"}])
    );
}

/// Adapts a `tokio::sync::mpsc::UnboundedReceiver` into a `futures::Stream`,
/// staying open (never yielding `None`) until every sender is dropped —
/// used to hold a `query_stream()` feeder open across an assertion.
fn tokio_stream_from_unbounded<T: Send + 'static>(
    mut rx: tokio::sync::mpsc::UnboundedReceiver<T>,
) -> impl futures::Stream<Item = T> + Send + 'static {
    futures::stream::poll_fn(move |cx| rx.poll_recv(cx))
}
