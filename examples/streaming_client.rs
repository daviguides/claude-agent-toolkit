//! Interactive multi-turn session: two sequential prompts over one
//! connected [`ClaudeClient`], demonstrating
//! [`ClaudeClient::receive_response`].
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
    let mut client = ClaudeClient::connect(ClaudeAgentOptions::default()).await?;

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
