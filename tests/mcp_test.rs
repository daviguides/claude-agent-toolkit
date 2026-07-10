//! Integration tests for in-process MCP tools routed through the
//! public API (`ClaudeClient::connect`), against a fake CLI, plus
//! `--mcp-config` stub serialization.
//!
//! Unix-only: the fake CLI harness uses `#!/bin/sh` scripts.

#![cfg(unix)]

mod fake_cli;

use std::time::Duration;

use claude_agent_toolkit::{
    ClaudeAgentOptions, ClaudeClient, McpServerConfig, McpServersOption, ToolResult,
    build_cli_args, create_sdk_mcp_server, tool,
};

fn calculator_server() -> claude_agent_toolkit::SdkMcpServer {
    let add = tool(
        "add",
        "Add two numbers",
        serde_json::json!({"type": "object", "properties": {"a": {}, "b": {}}}),
        |input: serde_json::Value| async move {
            let a = input["a"].as_f64().unwrap_or_default();
            let b = input["b"].as_f64().unwrap_or_default();
            ToolResult::text((a + b).to_string())
        },
    );
    create_sdk_mcp_server("calc", "1.0.0", vec![add])
}

#[test]
fn mcp_config_serializes_sdk_server_as_stub() {
    let mut servers = McpServersOption::default();
    let McpServersOption::Servers(map) = &mut servers else {
        unreachable!()
    };
    map.insert(
        "calc".to_string(),
        McpServerConfig::Sdk(calculator_server()),
    );

    let options = ClaudeAgentOptions::builder().mcp_servers(servers).build();
    let args = build_cli_args(&options);
    let flag_index = args
        .iter()
        .position(|arg| arg == "--mcp-config")
        .expect("--mcp-config flag present");
    let json: serde_json::Value = serde_json::from_str(&args[flag_index + 1]).expect("valid json");

    assert_eq!(
        json["mcpServers"]["calc"],
        serde_json::json!({"type": "sdk", "name": "calc"})
    );
    // No handler leakage: the stub is exactly the two documented keys.
    assert_eq!(json["mcpServers"]["calc"].as_object().unwrap().len(), 2);
}

fn recorded_lines_after_initialize(fake: &fake_cli::FakeCli) -> Vec<serde_json::Value> {
    let recorded = std::fs::read_to_string(&fake.stdin_recording_path).unwrap_or_default();
    recorded
        .lines()
        .map(|line| serde_json::from_str(line).expect("valid json"))
        .filter(|value: &serde_json::Value| value["request"]["subtype"] != "initialize")
        .collect()
}

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

fn options_with_calculator(fake: &fake_cli::FakeCli) -> ClaudeAgentOptions {
    let mut servers = McpServersOption::default();
    let McpServersOption::Servers(map) = &mut servers else {
        unreachable!()
    };
    map.insert(
        "calc".to_string(),
        McpServerConfig::Sdk(calculator_server()),
    );

    ClaudeAgentOptions::builder()
        .cli_path(fake.path.clone())
        .mcp_servers(servers)
        .build()
}

#[tokio::test]
async fn mcp_message_request_routes_to_server() {
    let fake = fake_cli::scripted_with_initialize(
        &[
            r#"{"type":"control_request","request_id":"cli_req_1","request":{"subtype":"mcp_message","server_name":"calc","message":{"jsonrpc":"2.0","id":1,"method":"tools/list"}}}"#,
        ],
        &[],
        0,
    );

    let mut client = ClaudeClient::connect(options_with_calculator(&fake))
        .await
        .expect("connects");
    let responses = wait_for_response(&fake).await;
    client.disconnect().await.expect("disconnects");

    assert_eq!(responses[0]["response"]["subtype"], "success");
    let mcp_response = &responses[0]["response"]["response"]["mcp_response"];
    assert_eq!(mcp_response["result"]["tools"][0]["name"], "add");
}

#[tokio::test]
async fn mcp_message_tools_call_runs_the_real_handler() {
    let fake = fake_cli::scripted_with_initialize(
        &[
            r#"{"type":"control_request","request_id":"cli_req_1","request":{"subtype":"mcp_message","server_name":"calc","message":{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"add","arguments":{"a":2,"b":3}}}}}"#,
        ],
        &[],
        0,
    );

    let mut client = ClaudeClient::connect(options_with_calculator(&fake))
        .await
        .expect("connects");
    let responses = wait_for_response(&fake).await;
    client.disconnect().await.expect("disconnects");

    let mcp_response = &responses[0]["response"]["response"]["mcp_response"];
    assert_eq!(
        mcp_response["result"]["content"],
        serde_json::json!([{"type": "text", "text": "5"}])
    );
}

#[tokio::test]
async fn unknown_server_name_yields_jsonrpc_error_in_success_response() {
    let fake = fake_cli::scripted_with_initialize(
        &[
            r#"{"type":"control_request","request_id":"cli_req_1","request":{"subtype":"mcp_message","server_name":"missing","message":{"jsonrpc":"2.0","id":1,"method":"tools/list"}}}"#,
        ],
        &[],
        0,
    );

    let mut client = ClaudeClient::connect(options_with_calculator(&fake))
        .await
        .expect("connects");
    let responses = wait_for_response(&fake).await;
    client.disconnect().await.expect("disconnects");

    // Confirmed against upstream: an unknown server name is a JSON-RPC
    // error *inside* a successful control response, not a
    // control-protocol-level error (see `DEVIATIONS.md`).
    assert_eq!(responses[0]["response"]["subtype"], "success");
    let mcp_response = &responses[0]["response"]["response"]["mcp_response"];
    assert_eq!(mcp_response["error"]["code"], -32601);
}
