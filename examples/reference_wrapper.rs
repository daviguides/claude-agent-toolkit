//! A compiling Rust translation of a real production caller's wrapper
//! around the Python SDK: refiner's `SDKWrapper`
//! (`refiner/core/sdk_wrapper.py`, part of this project's Step 10.3b
//! reference-use-case audit — see `docs/sync/parity.yaml`'s
//! `reference_use_case` entries).
//!
//! Demonstrates the exact core loop refiner relies on: connect once,
//! run multiple queries on the same session, print tool calls as they
//! happen, read metrics off each `ResultMessage` (cost is the CLI's
//! own running session total — callers compute their own per-query
//! delta; `num_turns` is already scoped to that one query), capture
//! `stderr` into a bounded ring buffer for error diagnostics, and
//! disconnect.
//!
//! Runs with `cwd` pinned to a fresh temp directory rather than
//! wherever this binary happens to be invoked from — see
//! `examples/quick_start.rs`'s doc comment for why.
//!
//! Setup: `npm install -g @anthropic-ai/claude-code`, then either set
//! `ANTHROPIC_API_KEY` or run `claude login` once so the CLI is
//! authenticated.
//!
//! Run: `cargo run --example reference_wrapper`

use std::path::Path;
use std::sync::{Arc, Mutex};

use claude_agent_toolkit::{
    ClaudeAgentOptions, ClaudeClient, ContentBlock, Message, PermissionMode, SystemPrompt,
};
use futures::StreamExt;

/// Bounded ring buffer of the most recent CLI stderr lines, mirroring
/// refiner's own `_stderr_lines` (kept for enriching error messages,
/// not printed on the happy path).
const STDERR_RING_CAPACITY: usize = 50;

/// Outcome of one query, mirroring refiner's `query()` return dict.
#[derive(Debug)]
struct QueryOutcome {
    session_id: Option<String>,
    /// Turns consumed by THIS query (not cumulative).
    turns: u32,
    /// This query's own cost: the CLI's cumulative total minus the
    /// cumulative total recorded before this query started.
    cost_usd: f64,
    last_text: String,
    is_error: bool,
}

/// Wraps [`ClaudeClient`] for multi-query sessions, tracking
/// cumulative cost/turns the way refiner's `SDKWrapper` does.
struct AgentWrapper {
    client: ClaudeClient,
    stderr_lines: Arc<Mutex<Vec<String>>>,
    cumulative_cost: f64,
}

impl AgentWrapper {
    /// Connects with refiner's exact option shape: `claude_code`
    /// system prompt preset with an append, `bypassPermissions`,
    /// partial-message streaming, and a capturing `stderr` callback.
    async fn connect(
        cwd: &Path,
        max_turns: Option<u32>,
        model: Option<&str>,
        system_prompt_append: &str,
    ) -> claude_agent_toolkit::Result<Self> {
        let stderr_lines: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let stderr_lines_for_callback = Arc::clone(&stderr_lines);

        let mut options = ClaudeAgentOptions::builder()
            .cwd(cwd)
            .permission_mode(PermissionMode::BypassPermissions)
            .system_prompt(SystemPrompt::Preset {
                preset: "claude_code".to_string(),
                append: Some(system_prompt_append.to_string()),
                exclude_dynamic_sections: None,
            })
            .include_partial_messages(true)
            .stderr(move |line: &str| {
                let mut lines = stderr_lines_for_callback.lock().unwrap();
                lines.push(line.to_string());
                if lines.len() > STDERR_RING_CAPACITY {
                    lines.remove(0);
                }
            });
        if let Some(turns) = max_turns {
            options = options.max_turns(turns);
        }
        if let Some(model) = model {
            options = options.model(model);
        }

        let client = ClaudeClient::connect(options.build()).await?;
        Ok(Self {
            client,
            stderr_lines,
            cumulative_cost: 0.0,
        })
    }

    /// Sends `prompt` and processes the response, printing tool calls
    /// as refiner does, and returning refiner's metrics shape.
    async fn query(&mut self, prompt: &str) -> claude_agent_toolkit::Result<QueryOutcome> {
        let cost_before = self.cumulative_cost;
        self.client.send(prompt).await?;

        let mut text_blocks = Vec::new();
        let mut outcome = QueryOutcome {
            session_id: None,
            turns: 0,
            cost_usd: 0.0,
            last_text: String::new(),
            is_error: false,
        };

        let mut responses = self.client.receive_response()?;
        while let Some(message) = responses.next().await {
            match message? {
                Message::Assistant(assistant) => {
                    for block in assistant.content {
                        match block {
                            ContentBlock::Text { text } => text_blocks.push(text),
                            ContentBlock::ToolUse { name, .. } => {
                                println!("  [tool] {name}");
                            }
                            ContentBlock::ToolResult {
                                is_error: Some(true),
                                content,
                                ..
                            } => {
                                eprintln!("  [tool error] {content:?}");
                            }
                            _ => {}
                        }
                    }
                }
                Message::Result(result) => {
                    let cumulative_cost = result.total_cost_usd.unwrap_or(0.0);
                    outcome.session_id = Some(result.session_id);
                    outcome.turns = result.num_turns;
                    outcome.cost_usd = cumulative_cost - cost_before;
                    outcome.is_error = result.is_error;

                    self.cumulative_cost = cumulative_cost;
                    break;
                }
                _ => {}
            }
        }

        if let Some(text) = text_blocks.last() {
            outcome.last_text.clone_from(text);
        }
        Ok(outcome)
    }

    /// Last captured stderr lines, for error-detail enrichment.
    fn stderr_tail(&self) -> Vec<String> {
        self.stderr_lines.lock().unwrap().clone()
    }

    async fn disconnect(&mut self) -> claude_agent_toolkit::Result<()> {
        self.client.disconnect().await
    }
}

#[tokio::main]
async fn main() -> claude_agent_toolkit::Result<()> {
    let sandbox = tempfile::tempdir().expect("create sandbox temp dir");

    let mut wrapper = AgentWrapper::connect(
        sandbox.path(),
        Some(1),
        None,
        "You are running inside an automated example; answer briefly.",
    )
    .await?;

    let outcome = wrapper.query("What is 2 + 2?").await?;
    println!("Claude: {}", outcome.last_text);
    println!(
        "session_id={:?} turns={} cost_usd={:.4} is_error={}",
        outcome.session_id, outcome.turns, outcome.cost_usd, outcome.is_error
    );

    let stderr_tail = wrapper.stderr_tail();
    if !stderr_tail.is_empty() {
        println!("stderr tail: {stderr_tail:?}");
    }

    wrapper.disconnect().await?;
    Ok(())
}
