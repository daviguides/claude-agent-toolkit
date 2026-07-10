//! Interactive multi-turn session: two sequential prompts over one
//! connected [`ClaudeClient`], demonstrating
//! [`ClaudeClient::receive_response`].
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
//! Run: `cargo run --example streaming_client`

use claude_agent_toolkit::{ClaudeAgentOptions, ClaudeClient, ContentBlock, Message};
use futures::StreamExt;

#[tokio::main]
async fn main() -> claude_agent_toolkit::Result<()> {
    let sandbox = tempfile::tempdir().expect("create sandbox temp dir");
    let options = ClaudeAgentOptions::builder().cwd(sandbox.path()).build();
    let mut client = ClaudeClient::connect(options).await?;

    for prompt in ["What is the capital of France?", "What is its population?"] {
        println!("> {prompt}");
        client.send(prompt).await?;

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
        println!();
    }

    client.disconnect().await?;
    Ok(())
}
