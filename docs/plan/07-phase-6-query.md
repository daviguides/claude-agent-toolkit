# Phase 6 — Public `query()` API (One-Shot + Streaming Input)

**Objective**: the single-turn entry point: prompt in, typed message
stream out — in BOTH upstream input modes: plain string and async
stream of messages (`AsyncIterable` in Python).

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

## Deliverable B — `query_stream()` (streaming input, upstream parity)

Upstream `query()` accepts `str | AsyncIterable[dict]`. Rust splits
this into two functions (idiomatic — no untagged unions of unrelated
types):

```rust
/// Runs a query fed by an async stream of user messages.
///
/// Mirrors upstream `query()` with an `AsyncIterable` prompt: the CLI
/// is spawned in streaming mode, each item is forwarded as a user
/// message as it arrives, and stdin is closed when `prompts` ends.
/// Messages stream back concurrently while input is still being fed.
///
/// # Errors
///
/// Same as [`query`]; additionally, forwarding failures surface as
/// stream items.
pub async fn query_stream(
    prompts: impl Stream<Item = UserContent> + Send + 'static,
    options: ClaudeAgentOptions,
) -> Result<impl Stream<Item = Result<Message>> + Send> {
    // 1. SubprocessTransport with PromptInput::Streaming.
    // 2. connect; Query::start with handlers from options.
    // 3. ⚠️ VERIFY in _internal/client.py: whether streaming-mode
    //    query() runs the initialize handshake (the interactive
    //    client does; confirm for one-shot streaming input) — do
    //    exactly what upstream does.
    // 4. tokio::spawn a feeder task: for each item, send_user_message
    //    (session_id "default"); when the stream ends, end_input().
    //    A send failure inside the feeder: log via tracing and stop
    //    feeding (the read side will surface the process error).
    // 5. Return the same parse_message-mapped output stream as query().
    todo!()
}
```

Design note (fixed): items are `UserContent` (text or blocks), which
covers upstream's dict messages for the user role; if upstream's
iterable also accepts non-user message dicts (⚠️ VERIFY in
`_internal/client.py` / `query.py`), widen the item type to a small
`InputMessage` enum mirroring exactly what upstream forwards.

Register in `lib.rs`: `mod query; pub use query::{query, query_stream};`

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

Streaming input (`query_stream`):

8. `stream_prompt_items_are_forwarded_in_order` — feed 2 items via
   `futures::stream::iter`; recording fake shows two user-message
   lines in order, then stdin closed.
9. `stream_input_uses_streaming_mode_flags` — recorded argv contains
   `--input-format stream-json` and no `--print`.
10. `responses_flow_while_input_still_open` — feeder stream built from
    an `mpsc` channel held open; scripted CLI emits an assistant
    message immediately; assert the message arrives BEFORE the test
    sends the second prompt item (proves no input-drain barrier).
11. `input_stream_end_closes_stdin` — after the iter ends, the fake
    CLI (reading stdin until EOF) proceeds to print its result line;
    stream completes.
12. `block_content_items_serialize_as_blocks` — feed a
    `UserContent::Blocks` item → recorded line carries the JSON array
    form.

## Acceptance Gate

```bash
cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test && cargo doc --no-deps
```

Doctest note: the `no_run` example above compiles under `cargo test` —
keep it compiling.

## Commits

1. `phase-6: query() tests (red)`
2. `phase-6: query() one-shot API (green)`
