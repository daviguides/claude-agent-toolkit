//! Quick start: a one-shot [`query()`] call.
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
//! Run: `cargo run --example quick_start`

use claude_agent_toolkit::{ClaudeAgentOptions, ContentBlock, Message, query};
use futures::StreamExt;

#[tokio::main]
async fn main() -> claude_agent_toolkit::Result<()> {
    let sandbox = tempfile::tempdir().expect("create sandbox temp dir");
    let options = ClaudeAgentOptions::builder().cwd(sandbox.path()).build();
    let mut stream = query("What is 2 + 2?", options).await?;

    while let Some(message) = stream.next().await {
        match message? {
            Message::Assistant(assistant) => {
                for block in assistant.content {
                    if let ContentBlock::Text { text } = block {
                        println!("Claude: {text}");
                    }
                }
            }
            Message::Result(result) => {
                if let Some(cost) = result.total_cost_usd {
                    println!("\nCost: ${cost:.4}");
                }
            }
            _ => {}
        }
    }

    Ok(())
}
