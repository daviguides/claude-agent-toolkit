//! Quick start: a one-shot [`query()`] call.
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
    let mut stream = query("What is 2 + 2?", ClaudeAgentOptions::default()).await?;

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
