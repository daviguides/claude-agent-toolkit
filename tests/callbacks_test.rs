//! Integration tests for `can_use_tool`/hook callbacks routed through
//! the public API (`ClaudeClient::connect`), against a fake CLI.
//!
//! Unlike `src/callback_adapters.rs`'s unit tests (which only exercise
//! the adapter functions in isolation), these drive a real fake CLI
//! that emits `can_use_tool`/`hook_callback` control requests and
//! assert the SDK's recorded stdin responses.
//!
//! Unix-only: the fake CLI harness uses `#!/bin/sh` scripts.

#![cfg(unix)]

mod fake_cli;

use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use claude_agent_toolkit::{
    ClaudeAgentOptions, ClaudeClient, HookEvent, HookMatcher, HookOutput, PermissionResult,
    ToolPermissionRequest, hook_callback,
};

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

/// Polls `recorded_lines_after_initialize` until it is non-empty — the
/// CLI-initiated control request is answered by a background task, so
/// there is no synchronous point in the public API to await it.
async fn wait_for_response(fake: &fake_cli::FakeCli) -> Vec<serde_json::Value> {
    let mut waited = Duration::ZERO;
    loop {
        let lines = recorded_lines_after_initialize(fake);
        if !lines.is_empty() {
            return lines;
        }
        assert!(
            waited <= Duration::from_secs(2),
            "SDK never answered the control request within 2s"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
        waited += Duration::from_millis(20);
    }
}

#[tokio::test]
async fn can_use_tool_callback_allows_and_forwards_updated_input() {
    let fake = fake_cli::scripted_with_initialize(
        &[
            r#"{"type":"control_request","request_id":"cli_req_1","request":{"subtype":"can_use_tool","tool_name":"Bash","input":{"command":"ls"},"tool_use_id":"tu_1"}}"#,
        ],
        &[],
        0,
    );

    let seen: Arc<StdMutex<Option<ToolPermissionRequest>>> = Arc::new(StdMutex::new(None));
    let seen_in_callback = Arc::clone(&seen);
    let options = ClaudeAgentOptions::builder()
        .cli_path(fake.path.clone())
        .can_use_tool(move |request| {
            let seen = Arc::clone(&seen_in_callback);
            async move {
                *seen.lock().unwrap() = Some(request);
                PermissionResult::Allow {
                    updated_input: None,
                    updated_permissions: None,
                }
            }
        })
        .build();

    let mut client = ClaudeClient::connect(options).await.expect("connects");
    let responses = wait_for_response(&fake).await;
    client.disconnect().await.expect("disconnects");

    assert_eq!(responses[0]["response"]["request_id"], "cli_req_1");
    assert_eq!(responses[0]["response"]["subtype"], "success");
    assert_eq!(responses[0]["response"]["response"]["behavior"], "allow");
    assert_eq!(
        responses[0]["response"]["response"]["updatedInput"],
        serde_json::json!({"command": "ls"})
    );

    let request = seen.lock().unwrap().take().expect("callback ran");
    assert_eq!(request.tool_name, "Bash");
    assert_eq!(request.tool_use_id.as_deref(), Some("tu_1"));
}

#[tokio::test]
async fn can_use_tool_callback_denies_with_message() {
    let fake = fake_cli::scripted_with_initialize(
        &[
            r#"{"type":"control_request","request_id":"cli_req_1","request":{"subtype":"can_use_tool","tool_name":"Bash","input":{"command":"rm -rf /"}}}"#,
        ],
        &[],
        0,
    );
    let options = ClaudeAgentOptions::builder()
        .cli_path(fake.path.clone())
        .can_use_tool(|_request| async {
            PermissionResult::Deny {
                message: "not allowed".to_string(),
                interrupt: true,
            }
        })
        .build();

    let mut client = ClaudeClient::connect(options).await.expect("connects");
    let responses = wait_for_response(&fake).await;
    client.disconnect().await.expect("disconnects");

    assert_eq!(responses[0]["response"]["response"]["behavior"], "deny");
    assert_eq!(
        responses[0]["response"]["response"]["message"],
        "not allowed"
    );
    assert_eq!(responses[0]["response"]["response"]["interrupt"], true);
}

#[tokio::test]
async fn missing_can_use_tool_callback_yields_error_response() {
    let fake = fake_cli::scripted_with_initialize(
        &[
            r#"{"type":"control_request","request_id":"cli_req_1","request":{"subtype":"can_use_tool","tool_name":"Bash","input":{"command":"ls"}}}"#,
        ],
        &[],
        0,
    );

    let mut client = ClaudeClient::connect(options_for(&fake))
        .await
        .expect("connects");
    let responses = wait_for_response(&fake).await;
    client.disconnect().await.expect("disconnects");

    assert_eq!(responses[0]["response"]["subtype"], "error");
    assert_eq!(responses[0]["response"]["request_id"], "cli_req_1");
}

#[tokio::test]
async fn hook_callback_is_invoked_and_response_forwarded() {
    let fake = fake_cli::scripted_with_initialize(
        &[
            r#"{"type":"control_request","request_id":"cli_req_1","request":{"subtype":"hook_callback","callback_id":"hook_0","input":{"hook_event_name":"PreToolUse"}}}"#,
        ],
        &[],
        0,
    );
    let options = ClaudeAgentOptions::builder()
        .cli_path(fake.path.clone())
        .hook(
            HookEvent::PreToolUse,
            HookMatcher::new(Some("Bash")).with_hook(hook_callback(
                |_payload, _tool_use_id, _ctx| async {
                    HookOutput {
                        decision: Some("block".to_string()),
                        system_message: Some("blocked by test hook".to_string()),
                        ..HookOutput::default()
                    }
                },
            )),
        )
        .build();

    let mut client = ClaudeClient::connect(options).await.expect("connects");
    let responses = wait_for_response(&fake).await;
    client.disconnect().await.expect("disconnects");

    assert_eq!(responses[0]["response"]["subtype"], "success");
    assert_eq!(responses[0]["response"]["response"]["decision"], "block");
    assert_eq!(
        responses[0]["response"]["response"]["systemMessage"],
        "blocked by test hook"
    );
}

#[tokio::test]
async fn initialize_payload_contains_registered_hooks() {
    let fake = fake_cli::scripted_with_initialize(&[], &[], 0);
    let options = ClaudeAgentOptions::builder()
        .cli_path(fake.path.clone())
        .hook(
            HookEvent::PreToolUse,
            HookMatcher::new(Some("Bash")).with_hook(hook_callback(
                |_payload, _tool_use_id, _ctx| async { HookOutput::default() },
            )),
        )
        .build();

    let mut client = ClaudeClient::connect(options).await.expect("connects");
    client.disconnect().await.expect("disconnects");

    let lines = recorded_lines(&fake);
    let initialize_request = lines
        .iter()
        .find(|line| line["request"]["subtype"] == "initialize")
        .expect("SDK sent an initialize request");
    assert_eq!(
        initialize_request["request"]["hooks"]["PreToolUse"][0],
        serde_json::json!({"matcher": "Bash", "hookCallbackIds": ["hook_0"]})
    );
}
