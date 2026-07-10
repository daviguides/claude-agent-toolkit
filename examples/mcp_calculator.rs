//! An in-process MCP server exposing `add`/`multiply` tools, wired
//! through [`create_sdk_mcp_server`] — no external MCP subprocess is
//! spawned.
//!
//! Tools are addressed by Claude as `mcp__<server>__<tool>` — here
//! `mcp__calc__add`/`mcp__calc__multiply` — matching upstream's naming
//! convention exactly.
//!
//! Runs with `cwd` pinned to a fresh temp directory rather than
//! wherever this binary happens to be invoked from — this is a
//! general Claude Code session and, depending on your own
//! `~/.claude/settings.json` permission mode, may act with more
//! autonomy than you expect; sandboxing `cwd` keeps that blast radius
//! away from directories that matter.
//!
//! Setup: `npm install -g @anthropic-ai/claude-code`, then either set
//! `ANTHROPIC_API_KEY` or run `claude login` once so the CLI is
//! authenticated.
//!
//! Run: `cargo run --example mcp_calculator`

use claude_agent_toolkit::{
    ClaudeAgentOptions, ContentBlock, McpServerConfig, McpServersOption, Message, ToolResult,
    create_sdk_mcp_server, query, tool,
};
use futures::StreamExt;
use serde_json::json;

fn number_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {"a": {"type": "number"}, "b": {"type": "number"}},
        "required": ["a", "b"],
    })
}

#[tokio::main]
async fn main() -> claude_agent_toolkit::Result<()> {
    let add = tool(
        "add",
        "Add two numbers",
        number_schema(),
        |input| async move {
            let a = input["a"].as_f64().unwrap_or_default();
            let b = input["b"].as_f64().unwrap_or_default();
            ToolResult::text(format!("{a} + {b} = {}", a + b))
        },
    );
    let multiply = tool(
        "multiply",
        "Multiply two numbers",
        number_schema(),
        |input| async move {
            let a = input["a"].as_f64().unwrap_or_default();
            let b = input["b"].as_f64().unwrap_or_default();
            ToolResult::text(format!("{a} * {b} = {}", a * b))
        },
    );
    let calculator = create_sdk_mcp_server("calc", "1.0.0", vec![add, multiply]);

    let mut servers = McpServersOption::default();
    let McpServersOption::Servers(map) = &mut servers else {
        unreachable!("McpServersOption::default() is always ::Servers")
    };
    map.insert("calc".to_string(), McpServerConfig::Sdk(calculator));

    let sandbox = tempfile::tempdir().expect("create sandbox temp dir");
    let options = ClaudeAgentOptions::builder()
        .cwd(sandbox.path())
        .mcp_servers(servers)
        .allowed_tools(["mcp__calc__add", "mcp__calc__multiply"])
        .build();

    let mut stream = query("What is 12 times 7? Then add 100 to that result.", options).await?;

    while let Some(message) = stream.next().await {
        if let Message::Assistant(assistant) = message? {
            for block in assistant.content {
                if let ContentBlock::Text { text } = block {
                    println!("Claude: {text}");
                }
            }
        }
    }

    Ok(())
}
