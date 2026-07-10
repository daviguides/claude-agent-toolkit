//! One-shot query API.

use std::sync::Arc;
use std::time::Duration;

use futures::stream::{self, BoxStream, Stream, StreamExt};

use crate::callback_adapters::{build_query_handlers, validate_can_use_tool};
use crate::error::{Error, Result};
use crate::protocol::query::Query;
use crate::transport::Transport;
use crate::transport::subprocess::SubprocessTransport;
use crate::types::message::{Message, UserContent, parse_message};
use crate::types::options::{ClaudeAgentOptions, SkillsOption, SystemPrompt};

/// Env var upstream reads to extend the `initialize` handshake timeout
/// beyond its 60s floor (matches `ClaudeSDKClient.connect()`).
const INITIALIZE_TIMEOUT_ENV_VAR: &str = "CLAUDE_CODE_STREAM_CLOSE_TIMEOUT";
const DEFAULT_INITIALIZE_TIMEOUT_MS: u64 = 60_000;
const MIN_INITIALIZE_TIMEOUT: Duration = Duration::from_secs(60);

/// Session id upstream uses for the one-shot string-prompt message.
const ONE_SHOT_SESSION_ID: &str = "";

/// Session id this port uses for each item of a `query_stream()` input
/// stream. Upstream's streaming-input items are raw caller-supplied
/// dicts (no fixed session id); this crate simplifies the item type to
/// [`UserContent`] (see `DEVIATIONS.md`), so a single fixed value
/// stands in for it.
const STREAM_SESSION_ID: &str = "default";

/// Resolves the `initialize` handshake timeout: `CLAUDE_CODE_STREAM_CLOSE_TIMEOUT`
/// (milliseconds) if set and valid, else 60s — and never below 60s
/// regardless of the env var, matching upstream's `max(ms / 1000.0, 60.0)`.
fn resolve_initialize_timeout() -> Duration {
    let ms: u64 = std::env::var(INITIALIZE_TIMEOUT_ENV_VAR)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(DEFAULT_INITIALIZE_TIMEOUT_MS);
    Duration::from_millis(ms).max(MIN_INITIALIZE_TIMEOUT)
}

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
) -> Result<BoxStream<'static, Result<Message>>> {
    let prompt = prompt.into();
    let query = start_and_initialize(options, true).await?;

    query
        .send_user_message(&UserContent::Text(prompt), ONE_SHOT_SESSION_ID)
        .await?;
    query.end_input().await?;

    Ok(message_stream(query))
}

/// Runs a query fed by an async stream of user messages.
///
/// Mirrors upstream `query()` with an `AsyncIterable` prompt: the CLI
/// is spawned in streaming mode, each item is forwarded as a user
/// message as it arrives, and stdin is closed when `prompts` ends.
/// Messages stream back concurrently while input is still being fed —
/// confirmed against upstream's actual implementation (`stream_input`
/// runs as an independent background task), not the simplified
/// "all sent, then all received" framing in its docstring; see
/// `DEVIATIONS.md`.
///
/// # Errors
///
/// Same as [`query`]; additionally, forwarding failures surface as
/// stream items (the feeder logs via `tracing` and stops feeding; the
/// read side surfaces any resulting process error).
pub async fn query_stream(
    prompts: impl Stream<Item = UserContent> + Send + 'static,
    options: ClaudeAgentOptions,
) -> Result<BoxStream<'static, Result<Message>>> {
    let query = Arc::new(start_and_initialize(options, false).await?);

    let feeder_query = Arc::clone(&query);
    tokio::spawn(async move {
        let mut prompts = Box::pin(prompts);
        while let Some(item) = prompts.next().await {
            if let Err(error) = feeder_query
                .send_user_message(&item, STREAM_SESSION_ID)
                .await
            {
                tracing::debug!(%error, "query_stream: failed to forward input item; stopping feed");
                return;
            }
        }
        if let Err(error) = feeder_query.end_input().await {
            tracing::debug!(%error, "query_stream: failed to close input");
        }
    });

    Ok(message_stream_shared(query))
}

/// Connects a fresh [`SubprocessTransport`] built from `options`, then
/// delegates to [`start_and_initialize_over`].
///
/// `prompt_is_string` feeds `can_use_tool`'s mutual-exclusivity check
/// (it requires a streaming prompt, matching upstream); the transport
/// is built from a clone with `permission_prompt_tool_name` resolved
/// to the auto-set `"stdio"` value so the CLI invocation carries the
/// right flag (see `validate_can_use_tool`).
async fn start_and_initialize(
    options: ClaudeAgentOptions,
    prompt_is_string: bool,
) -> Result<Query> {
    let resolved_permission_prompt_tool_name = validate_can_use_tool(&options, prompt_is_string)?;
    let mut transport = SubprocessTransport::new(ClaudeAgentOptions {
        permission_prompt_tool_name: resolved_permission_prompt_tool_name,
        ..options.clone()
    });
    transport.connect().await?;
    start_and_initialize_over(transport, &options, prompt_is_string).await
}

/// Starts the `Query` actor over an already-connected transport and
/// always runs the `initialize` handshake — upstream does this
/// unconditionally for one-shot queries, streaming-input queries, AND
/// `ClaudeClient` sessions alike. Shared by [`query`], [`query_stream`],
/// and Phase 7's `ClaudeClient::connect`/`connect_with_transport`.
pub(crate) async fn start_and_initialize_over(
    transport: impl Transport + 'static,
    options: &ClaudeAgentOptions,
    prompt_is_string: bool,
) -> Result<Query> {
    validate_can_use_tool(options, prompt_is_string)?;
    let (handlers, hooks) = build_query_handlers(options);

    let agents = options
        .agents
        .as_ref()
        .map(serde_json::to_value)
        .transpose()
        .map_err(|source| Error::JsonDecode {
            line: String::new(),
            source,
        })?;

    let exclude_dynamic_sections = match &options.system_prompt {
        Some(SystemPrompt::Preset {
            exclude_dynamic_sections,
            ..
        }) => *exclude_dynamic_sections,
        _ => None,
    };

    // Upstream: "'all' and omitted are equivalent at the wire level (no
    // filter), so only send the field when it's an explicit list."
    let skills = match &options.skills {
        Some(SkillsOption::Named(names)) => Some(names.clone()),
        _ => None,
    };

    let mut query = Query::start(transport, handlers);
    query.set_initialize_timeout(resolve_initialize_timeout());

    query
        .initialize(hooks, agents, exclude_dynamic_sections, skills)
        .await?;

    Ok(query)
}

/// Adapts an owned [`Query`] into a stream of parsed messages,
/// transparently skipping raw values `parse_message` doesn't recognize
/// (forward compatibility, matching upstream's own
/// `if message is not None: yield message`).
fn message_stream(query: Query) -> BoxStream<'static, Result<Message>> {
    message_stream_shared(Arc::new(query))
}

fn message_stream_shared(query: Arc<Query>) -> BoxStream<'static, Result<Message>> {
    // `stream::unfold` panics if polled again after yielding `None` —
    // `.fuse()` makes that safe, matching the fused-stream contract
    // callers reasonably expect from a public API.
    stream::unfold(Some(query), |state| async move {
        let query = state?;
        loop {
            match query.next_message().await {
                None => return None,
                Some(Err(error)) => return Some((Err(error), None)),
                Some(Ok(value)) => match parse_message(value) {
                    Ok(Some(message)) => return Some((Ok(message), Some(query))),
                    Ok(None) => {}
                    Err(error) => return Some((Err(error), Some(query))),
                },
            }
        }
    })
    .fuse()
    .boxed()
}
