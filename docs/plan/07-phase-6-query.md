# Phase 6 — Public `query()` API (One-Shot)

**Objective**: the single-turn entry point: prompt in, typed message
stream out.

**Upstream sources of truth**:
- `reference/.../src/claude_agent_sdk/query.py` (public behavior/docs)
- `reference/.../src/claude_agent_sdk/_internal/client.py`
  (`process_query` — how transport/Query/parse compose)

## Deliverable — `src/query.rs`

```rust
//! One-shot query API.

use futures::Stream;

use crate::error::Result;
use crate::types::message::Message;
use crate::types::options::ClaudeAgentOptions;

/// Runs a single-turn query against Claude Code.
///
/// Spawns the CLI, sends `prompt`, and yields typed [`Message`]s until
/// the CLI process ends. The final message of a successful turn is
/// [`Message::Result`].
///
/// # Errors
///
/// The returned stream yields [`crate::Error`] items for connection,
/// protocol, decoding, and process failures. Spawn failures are
/// returned by the initial future itself.
///
/// # Examples
///
/// ```no_run
/// use claude_agent_toolkit::{query, ClaudeAgentOptions, Message};
/// use futures::StreamExt;
///
/// # async fn run() -> claude_agent_toolkit::Result<()> {
/// let mut stream = query("What is 2 + 2?", ClaudeAgentOptions::default()).await?;
/// while let Some(message) = stream.next().await {
///     if let Message::Assistant(m) = message? {
///         println!("{m:?}");
///     }
/// }
/// # Ok(())
/// # }
/// ```
pub async fn query(
    prompt: impl Into<String>,
    options: ClaudeAgentOptions,
) -> Result<impl Stream<Item = Result<Message>> + Send> {
    // 1. Build SubprocessTransport with PromptInput::Text(prompt).
    // 2. transport.connect().await?
    // 3. Query::start(transport, QueryHandlers::default())
    //    NOTE (⚠️ VERIFY): in one-shot --print mode upstream does NOT
    //    run the initialize handshake — check process_query; replicate.
    // 4. Return a stream adapter: map each raw Value through
    //    parse_message(), end after the transport stream ends.
    todo!()
}
```

Design notes (fixed):

- `query()` takes `options` by value (it is a config object; callers
  clone if reusing).
- Environment override support: honor `CLAUDE_AGENT_CLI_PATH` env var
  (checked inside `find_cli`) so users and the smoke tests can pin a
  CLI. Actually — check upstream first (⚠️ VERIFY whether Python reads
  an env var for cli path); if upstream has none, still add it but
  document it as an extension in `DEVIATIONS.md`.
- Hooks/`can_use_tool` in one-shot mode: upstream restricts some
  features to streaming mode (⚠️ VERIFY `query.py` docs). If restricted
  upstream, return `Error::ControlProtocol` with a clear message when
  options carry streaming-only features into `query()`.

Register in `lib.rs`: `mod query; pub use query::query;`

## Tests (`tests/query_test.rs`, write FIRST — all against fake CLI via `CLAUDE_AGENT_CLI_PATH`-style injection or an options-level cli path override; PICK the options override: add `cli_path: Option<PathBuf>` — check first whether upstream options include it (⚠️ VERIFY); if not, thread it via `extra` constructor on the transport and expose a `query_with_cli_path` test-only helper `#[doc(hidden)]`)

Simplest compliant route (fixed decision): make `cli_path` a public
field on `ClaudeAgentOptions` even if upstream lacks it — it is needed
for testability and users ask for it; record in `DEVIATIONS.md`.

1. `yields_typed_messages_in_order` — fake CLI script: system_init,
   assistant_text, result_success fixtures → stream yields
   `Message::System`, `Message::Assistant`, `Message::Result` in order,
   then `None`.
2. `assistant_text_content_is_parsed` — assert the text block value.
3. `stream_ends_after_process_exit` — after `None`, calling `next()`
   again still `None` (fused behavior).
4. `invalid_json_line_yields_decode_error_then_continues_or_stops` —
   match the Phase 4 transport semantics; assert the error item
   position.
5. `nonzero_exit_yields_process_error` — script exits 1 with stderr →
   last item is `Err(Error::Process { .. })`.
6. `prompt_reaches_cli_via_print_flag` — recording fake writes its argv
   to a file (extend harness: script `echo "$@" > args.txt`); assert
   `--print` and the prompt text appear.
7. `spawn_failure_is_returned_eagerly` — nonexistent cli path →
   `query(...).await` itself is `Err(Error::CliNotFound { .. })` (not a
   stream item).

## Acceptance Gate

```bash
cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test && cargo doc --no-deps
```

Doctest note: the `no_run` example above compiles under `cargo test` —
keep it compiling.

## Commits

1. `phase-6: query() tests (red)`
2. `phase-6: query() one-shot API (green)`
