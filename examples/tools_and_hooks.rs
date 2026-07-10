//! A `can_use_tool` permission callback that denies `Bash`, plus a
//! `PreToolUse` hook that logs every tool name Claude attempts.
//!
//! `can_use_tool` requires a connected, streaming-mode session (not a
//! one-shot string prompt), so this uses [`ClaudeClient`] rather than
//! [`claude_agent_toolkit::query`].
//!
//! Setup: `npm install -g @anthropic-ai/claude-code`, then either set
//! `ANTHROPIC_API_KEY` or run `claude login` once so the CLI is
//! authenticated.
//!
//! Run: `cargo run --example tools_and_hooks`

use claude_agent_toolkit::{
    ClaudeAgentOptions, ClaudeClient, ContentBlock, HookEvent, HookMatcher, HookOutput, Message,
    PermissionResult, hook_callback,
};
use futures::StreamExt;

#[tokio::main]
async fn main() -> claude_agent_toolkit::Result<()> {
    let options = ClaudeAgentOptions::builder()
        .can_use_tool(|request| async move {
            if request.tool_name == "Bash" {
                PermissionResult::Deny {
                    message: "Bash is disabled in this example".to_string(),
                    interrupt: false,
                }
            } else {
                PermissionResult::Allow {
                    updated_input: None,
                    updated_permissions: None,
                }
            }
        })
        .hook(
            HookEvent::PreToolUse,
            HookMatcher::new(None::<String>).with_hook(hook_callback(
                |payload, _tool_use_id, _ctx| async move {
                    if let Some(tool_name) = payload.get("tool_name").and_then(|v| v.as_str()) {
                        println!("[hook] about to run tool: {tool_name}");
                    }
                    HookOutput::default()
                },
            )),
        )
        .build();

    let mut client = ClaudeClient::connect(options).await?;
    client
        .send("Run `echo hello` in bash, then tell me today's date without using any tools.")
        .await?;

    {
        let mut responses = client.receive_response()?;
        while let Some(message) = responses.next().await {
            if let Message::Assistant(assistant) = message? {
                for block in assistant.content {
                    if let ContentBlock::Text { text } = block {
                        println!("Claude: {text}");
                    }
                }
            }
        }
    }

    client.disconnect().await?;
    Ok(())
}
