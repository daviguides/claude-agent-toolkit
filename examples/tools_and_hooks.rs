//! A `can_use_tool` permission callback that denies `Write`, plus a
//! `PreToolUse` hook that logs every tool name Claude attempts.
//!
//! `can_use_tool` requires a connected, streaming-mode session (not a
//! one-shot string prompt), so this uses [`ClaudeClient`] rather than
//! [`claude_agent_toolkit::query`].
//!
//! Two safety choices worth calling out for anyone adapting this
//! example:
//! - `.cwd(...)` is pinned to a fresh temp directory, not the crate's
//!   own working directory — the whole point is to demonstrate a tool
//!   call being *denied*, and if it isn't, the session shouldn't be
//!   able to touch anything that matters.
//! - `.permission_mode(PermissionMode::Default)` is set explicitly.
//!   `can_use_tool` is only consulted when the CLI's own permission
//!   evaluation is ambiguous — a global `~/.claude/settings.json` with
//!   `"defaultMode": "auto"` (or broad per-tool allow rules) can
//!   approve a tool before this callback is ever reached, silently
//!   defeating the demo. Pinning `Default` here avoids inheriting
//!   whatever mode the running user has configured globally.
//!
//! Setup: `npm install -g @anthropic-ai/claude-code`, then either set
//! `ANTHROPIC_API_KEY` or run `claude login` once so the CLI is
//! authenticated.
//!
//! Run: `cargo run --example tools_and_hooks`

use claude_agent_toolkit::{
    ClaudeAgentOptions, ClaudeClient, ContentBlock, HookEvent, HookMatcher, HookOutput, Message,
    PermissionMode, PermissionResult, hook_callback,
};
use futures::StreamExt;

#[tokio::main]
async fn main() -> claude_agent_toolkit::Result<()> {
    let sandbox = tempfile::tempdir().expect("create sandbox temp dir");

    let options = ClaudeAgentOptions::builder()
        .cwd(sandbox.path())
        .permission_mode(PermissionMode::Default)
        .can_use_tool(|request| async move {
            if request.tool_name == "Write" {
                PermissionResult::Deny {
                    message: "Writing files is disabled in this example".to_string(),
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
        .send(
            "Create a file called hello.txt containing the word 'hi', then tell me what happened.",
        )
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
