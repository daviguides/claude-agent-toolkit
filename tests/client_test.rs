//! Integration tests for the public `ClaudeClient` API, against a
//! fake CLI.
//!
//! Unix-only: the fake CLI harness uses `#!/bin/sh` scripts. Every
//! test's fake must answer the `initialize` handshake (Phase 6/7
//! finding: `connect()` always runs it) — `scripted_with_initialize`
//! handles that.

#![cfg(unix)]

mod fake_cli;

use claude_agent_toolkit::{
    ClaudeAgentOptions, ClaudeClient, ContentBlock, Error, Message, PermissionMode, UserContent,
};
use futures::StreamExt;

fn options_for(fake: &fake_cli::FakeCli) -> ClaudeAgentOptions {
    ClaudeAgentOptions::builder()
        .cli_path(fake.path.clone())
        .build()
}

fn recorded_lines(fake: &fake_cli::FakeCli) -> Vec<serde_json::Value> {
    let recorded = std::fs::read_to_string(&fake.stdin_recording_path).expect("reads recording");
    recorded
        .lines()
        .map(|line| serde_json::from_str(line).expect("valid json"))
        .collect()
}

fn recorded_lines_after_initialize(fake: &fake_cli::FakeCli) -> Vec<serde_json::Value> {
    recorded_lines(fake)
        .into_iter()
        .filter(|value| value["request"]["subtype"] != "initialize")
        .collect()
}

#[tokio::test]
async fn connect_performs_initialize_handshake() {
    let fake = fake_cli::scripted_with_initialize(&[], &[], 0);
    let mut client = ClaudeClient::connect(options_for(&fake))
        .await
        .expect("connects");

    client.disconnect().await.expect("disconnects");

    let lines = recorded_lines(&fake);
    assert!(
        lines
            .iter()
            .any(|line| line["request"]["subtype"] == "initialize")
    );
}

#[tokio::test]
async fn connect_fails_when_initialize_rejected() {
    let fake = fake_cli::dynamic_responding(
        &[(
            "initialize",
            r#"{"type":"control_response","response":{"subtype":"error","request_id":"%s","response":{}}}"#,
        )],
        0,
    );
    let options = options_for(&fake);
    match claude_agent_toolkit::ClaudeClient::connect(options).await {
        Err(Error::ControlProtocol { .. }) => {}
        other => panic!(
            "expected initialize rejection to fail connect, got ok={}",
            other.is_ok()
        ),
    }
}

#[tokio::test]
async fn send_writes_stream_json_user_message() {
    let fake = fake_cli::scripted_with_initialize(&[], &[], 0);
    let client = ClaudeClient::connect(options_for(&fake))
        .await
        .expect("connects");

    client.send("hello there").await.expect("sends");
    // Give the write a moment to land before disconnecting.
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let mut client = client;
    client.disconnect().await.expect("disconnects");

    let lines = recorded_lines_after_initialize(&fake);
    assert_eq!(
        lines[0],
        serde_json::json!({
            "type": "user",
            "message": {"role": "user", "content": "hello there"},
            "parent_tool_use_id": null,
            "session_id": "default"
        })
    );
}

#[tokio::test]
async fn receive_response_stops_after_result_inclusive() {
    let fake = fake_cli::scripted_with_initialize(
        &[
            r#"{"type":"assistant","message":{"model":"m","content":[{"type":"text","text":"one"}]}}"#,
            r#"{"type":"assistant","message":{"model":"m","content":[{"type":"text","text":"two"}]}}"#,
            r#"{"type":"result","subtype":"success","duration_ms":1,"duration_api_ms":1,"is_error":false,"num_turns":1,"session_id":"s"}"#,
            r#"{"type":"assistant","message":{"model":"m","content":[{"type":"text","text":"extra"}]}}"#,
        ],
        &[],
        0,
    );
    let mut client = ClaudeClient::connect(options_for(&fake))
        .await
        .expect("connects");

    let items: Vec<_> = client
        .receive_response()
        .expect("has stream")
        .collect::<Vec<_>>()
        .await;
    assert_eq!(items.len(), 3);
    assert!(matches!(items[0], Ok(Message::Assistant(_))));
    assert!(matches!(items[1], Ok(Message::Assistant(_))));
    assert!(matches!(items[2], Ok(Message::Result(_))));

    client.disconnect().await.expect("disconnects");
}

#[tokio::test]
async fn receive_messages_continues_past_result() {
    let fake = fake_cli::scripted_with_initialize(
        &[
            r#"{"type":"assistant","message":{"model":"m","content":[{"type":"text","text":"one"}]}}"#,
            r#"{"type":"assistant","message":{"model":"m","content":[{"type":"text","text":"two"}]}}"#,
            r#"{"type":"result","subtype":"success","duration_ms":1,"duration_api_ms":1,"is_error":false,"num_turns":1,"session_id":"s"}"#,
            r#"{"type":"assistant","message":{"model":"m","content":[{"type":"text","text":"extra"}]}}"#,
        ],
        &[],
        0,
    );
    let mut client = ClaudeClient::connect(options_for(&fake))
        .await
        .expect("connects");

    // `receive_messages()` never auto-stops (matches upstream: it only
    // ends when the underlying stream ends) — the fake CLI stays alive
    // reading stdin after printing its scripted lines, so `.collect()`
    // on the raw stream would hang forever. Bound it instead.
    let items: Vec<_> = client
        .receive_messages()
        .expect("has stream")
        .take(4)
        .collect::<Vec<_>>()
        .await;
    assert_eq!(items.len(), 4);

    client.disconnect().await.expect("disconnects");
}

#[tokio::test]
async fn interrupt_sends_control_request_and_resolves() {
    let fake = fake_cli::dynamic_responding(
        &[
            (
                "initialize",
                r#"{"type":"control_response","response":{"subtype":"success","request_id":"%s","response":{}}}"#,
            ),
            (
                "interrupt",
                r#"{"type":"control_response","response":{"subtype":"success","request_id":"%s","response":{}}}"#,
            ),
        ],
        0,
    );
    let mut client = ClaudeClient::connect(options_for(&fake))
        .await
        .expect("connects");
    client.interrupt().await.expect("interrupt resolves");
    client.disconnect().await.expect("disconnects");
}

#[tokio::test]
async fn set_permission_mode_sends_wire_string() {
    let fake = fake_cli::dynamic_responding(
        &[
            (
                "initialize",
                r#"{"type":"control_response","response":{"subtype":"success","request_id":"%s","response":{}}}"#,
            ),
            (
                "set_permission_mode",
                r#"{"type":"control_response","response":{"subtype":"success","request_id":"%s","response":{}}}"#,
            ),
        ],
        0,
    );
    let client = ClaudeClient::connect(options_for(&fake))
        .await
        .expect("connects");

    client
        .set_permission_mode(PermissionMode::AcceptEdits)
        .await
        .expect("sets permission mode");
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let mut client = client;
    client.disconnect().await.expect("disconnects");

    let lines = recorded_lines_after_initialize(&fake);
    assert_eq!(lines[0]["request"]["mode"], "acceptEdits");
}

#[tokio::test]
async fn set_model_sends_model_name() {
    let fake = fake_cli::dynamic_responding(
        &[
            (
                "initialize",
                r#"{"type":"control_response","response":{"subtype":"success","request_id":"%s","response":{}}}"#,
            ),
            (
                "set_model",
                r#"{"type":"control_response","response":{"subtype":"success","request_id":"%s","response":{}}}"#,
            ),
        ],
        0,
    );
    let client = ClaudeClient::connect(options_for(&fake))
        .await
        .expect("connects");

    client
        .set_model(Some("claude-opus-4-8"))
        .await
        .expect("sets model");
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let mut client = client;
    client.disconnect().await.expect("disconnects");

    let lines = recorded_lines_after_initialize(&fake);
    assert_eq!(lines[0]["request"]["model"], "claude-opus-4-8");
}

#[tokio::test]
async fn send_after_disconnect_returns_connection_error() {
    let fake = fake_cli::scripted_with_initialize(&[], &[], 0);
    let mut client = ClaudeClient::connect(options_for(&fake))
        .await
        .expect("connects");
    client.disconnect().await.expect("disconnects");

    let err = client
        .send("hello")
        .await
        .expect_err("must fail after disconnect");
    assert!(matches!(err, Error::CliConnection { .. }));
}

#[tokio::test]
async fn disconnect_twice_is_ok() {
    let fake = fake_cli::scripted_with_initialize(&[], &[], 0);
    let mut client = ClaudeClient::connect(options_for(&fake))
        .await
        .expect("connects");
    client
        .disconnect()
        .await
        .expect("first disconnect succeeds");
    client
        .disconnect()
        .await
        .expect("second disconnect is a no-op");
}

#[tokio::test]
async fn send_content_blocks_writes_block_json() {
    let fake = fake_cli::scripted_with_initialize(&[], &[], 0);
    let client = ClaudeClient::connect(options_for(&fake))
        .await
        .expect("connects");

    client
        .send_content(UserContent::Blocks(vec![ContentBlock::ToolResult {
            tool_use_id: "toolu_1".to_string(),
            content: Some(serde_json::json!("ok")),
            is_error: Some(false),
        }]))
        .await
        .expect("sends block content");
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let mut client = client;
    client.disconnect().await.expect("disconnects");

    let lines = recorded_lines_after_initialize(&fake);
    assert_eq!(
        lines[0]["message"]["content"],
        serde_json::json!([{"type": "tool_result", "tool_use_id": "toolu_1", "content": "ok", "is_error": false}])
    );
}

#[tokio::test]
async fn send_stream_forwards_all_items_and_keeps_session_open() {
    let fake = fake_cli::scripted_with_initialize(&[], &[], 0);
    let client = ClaudeClient::connect(options_for(&fake))
        .await
        .expect("connects");

    let items = futures::stream::iter(vec![
        UserContent::Text("first".to_string()),
        UserContent::Text("second".to_string()),
    ]);
    client.send_stream(items).await.expect("streams items");
    client.send("third").await.expect("sends after stream");
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let mut client = client;
    client.disconnect().await.expect("disconnects");

    let lines = recorded_lines_after_initialize(&fake);
    assert_eq!(lines.len(), 3);
    assert_eq!(lines[0]["message"]["content"], "first");
    assert_eq!(lines[1]["message"]["content"], "second");
    assert_eq!(lines[2]["message"]["content"], "third");
}

#[tokio::test]
async fn server_info_available_after_connect() {
    let fake = fake_cli::dynamic_responding(
        &[(
            "initialize",
            r#"{"type":"control_response","response":{"subtype":"success","request_id":"%s","response":{"commands":["x"]}}}"#,
        )],
        0,
    );
    let mut client = ClaudeClient::connect(options_for(&fake))
        .await
        .expect("connects");

    let info = client.server_info().await.expect("has server info");
    assert_eq!(info["commands"], serde_json::json!(["x"]));

    client.disconnect().await.expect("disconnects");
}
