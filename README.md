# claude-agent-toolkit

An idiomatic Rust port of the official [Claude Agent SDK](https://github.com/anthropics/claude-agent-sdk-python) — build AI agents powered by Claude Code with Rust's type safety, async model, and zero-cost abstractions.

## Status

Implemented through Phase 10 of the port plan (see [`docs/plan/`](docs/plan/)): one-shot queries, an interactive multi-turn client, `can_use_tool` permission callbacks, lifecycle hooks, in-process MCP tools, and the full `ClaudeAgentOptions` surface. See [`docs/sync/PARITY.md`](docs/sync/PARITY.md) for the generated upstream↔Rust parity table.

## Installation

```bash
cargo add claude-agent-toolkit
```

Requires the Claude Code CLI, installed and authenticated:

```bash
npm install -g @anthropic-ai/claude-code
claude login   # or set ANTHROPIC_API_KEY
```

## Usage

One-shot query:

```rust
use claude_agent_toolkit::{ClaudeAgentOptions, ContentBlock, Message, query};
use futures::StreamExt;

# async fn run() -> claude_agent_toolkit::Result<()> {
let mut stream = query("What is 2 + 2?", ClaudeAgentOptions::default()).await?;
while let Some(message) = stream.next().await {
    if let Message::Assistant(assistant) = message? {
        for block in assistant.content {
            if let ContentBlock::Text { text } = block {
                println!("Claude: {text}");
            }
        }
    }
}
# Ok(())
# }
```

Interactive multi-turn session:

```rust
use claude_agent_toolkit::{ClaudeAgentOptions, ClaudeClient};

# async fn run() -> claude_agent_toolkit::Result<()> {
let mut client = ClaudeClient::connect(ClaudeAgentOptions::default()).await?;
client.send("What is the capital of France?").await?;
// ... read client.receive_response() ...
client.disconnect().await?;
# Ok(())
# }
```

More complete, runnable examples live in [`examples/`](examples/):

- [`quick_start.rs`](examples/quick_start.rs) — one-shot query
- [`streaming_client.rs`](examples/streaming_client.rs) — multi-turn session
- [`tools_and_hooks.rs`](examples/tools_and_hooks.rs) — `can_use_tool` + `PreToolUse` hook
- [`mcp_calculator.rs`](examples/mcp_calculator.rs) — in-process MCP tools

Run any of them with `cargo run --example <name>`.

## Why

The official Claude Agent SDK ships for Python and TypeScript only. A handful of independent Rust ports exist, but each is either unmaintained, narrow in scope, or a different kind of project entirely (a direct Anthropic API client rather than a Claude Code CLI wrapper, or a standalone coding agent rather than an SDK). This crate aims to be the canonical, actively maintained, idiomatic Rust equivalent of the upstream SDK.

## Design

Same wire protocol as the official Python/TypeScript SDKs (JSON-over-stdio with the `claude` CLI subprocess), translated into Rust idioms:

- `async`/`await` via `tokio` throughout
- the type system in place of runtime validation — invalid states are unrepresentable where Rust's types allow it
- zero-cost wrappers around the CLI subprocess protocol, no dynamic dispatch where a generic suffices

See [`docs/foundation/vision.md`](docs/foundation/vision.md) for the full rationale, and [`docs/plan/`](docs/plan/) for the phase-by-phase implementation plan and [`DEVIATIONS.md`](docs/plan/DEVIATIONS.md) for every point where this port's behavior was verified against (and occasionally diverges from) the pinned upstream reference.

## crates.io

Reserved name: [`claude-agent-toolkit`](https://crates.io/crates/claude-agent-toolkit).

## License

MIT
