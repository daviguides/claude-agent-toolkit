//! Interactive multi-turn client.

use futures::stream::{self, BoxStream, Stream, StreamExt};
use serde_json::Value;

use crate::callback_adapters::validate_can_use_tool;
use crate::error::{Error, Result};
use crate::protocol::query::Query;
use crate::query::start_and_initialize_over;
use crate::transport::Transport;
use crate::transport::subprocess::SubprocessTransport;
use crate::types::message::{Message, UserContent, parse_message};
use crate::types::options::ClaudeAgentOptions;
use crate::types::permission::PermissionMode;

/// Session id used by every `send`/`send_content`/`send_stream` call.
///
/// Upstream's `query(prompt, session_id="default")` accepts a custom
/// session id per call; this port's fixed method signatures don't
/// surface that parameter (no reference use case needs it — see
/// `DEVIATIONS.md`). `Query::send_user_message` still accepts an
/// arbitrary session id internally, so adding a custom-session variant
/// later is a small, low-risk extension.
const DEFAULT_SESSION_ID: &str = "default";

/// A stateful, bidirectional session with Claude Code.
///
/// Lifecycle: [`ClaudeClient::connect`] → [`send`](Self::send) /
/// [`receive_response`](Self::receive_response) (repeat) →
/// [`disconnect`](Self::disconnect). Dropping without calling
/// `disconnect()` still cleans up: the underlying `Query` has a
/// best-effort `Drop` that signals the CLI subprocess to close (see
/// `DEVIATIONS.md` — no separate `Drop` is needed on this type).
pub struct ClaudeClient {
    query: Option<Query>,
}

impl ClaudeClient {
    /// Spawns the CLI in streaming mode and performs the `initialize`
    /// control handshake.
    ///
    /// Upstream's `connect()` accepts an optional initial prompt only
    /// to satisfy a transport constructor that always needs a
    /// prompt/iterable; this crate's transport is prompt-agnostic
    /// (Phase 4), so there is nothing to satisfy — call
    /// [`send`](Self::send)/[`send_content`](Self::send_content)/
    /// [`send_stream`](Self::send_stream) after connecting instead.
    ///
    /// # Errors
    ///
    /// [`Error::CliNotFound`], [`Error::CliConnection`], or
    /// [`Error::ControlProtocol`] when spawn or the handshake fails
    /// (including `can_use_tool`'s mutual-exclusivity checks — see
    /// `validate_can_use_tool`).
    pub async fn connect(options: ClaudeAgentOptions) -> Result<Self> {
        // `ClaudeClient` never takes a string prompt (see the note
        // above), so `can_use_tool`'s streaming-mode requirement is
        // always satisfied here.
        let resolved_permission_prompt_tool_name = validate_can_use_tool(&options, false)?;
        let mut transport = SubprocessTransport::new(ClaudeAgentOptions {
            permission_prompt_tool_name: resolved_permission_prompt_tool_name,
            ..options.clone()
        });
        transport.connect().await?;
        Self::connect_with_transport(transport, &options).await
    }

    /// Like [`connect`](Self::connect), but drives the session over a
    /// caller-supplied [`Transport`] (e.g. a remote Claude Code
    /// connection) instead of spawning a subprocess. Mirrors upstream
    /// `ClaudeSDKClient(transport=...)`.
    ///
    /// # Errors
    ///
    /// Same as [`connect`](Self::connect).
    pub async fn connect_with_transport(
        transport: impl Transport + 'static,
        options: &ClaudeAgentOptions,
    ) -> Result<Self> {
        let query = start_and_initialize_over(transport, options, false).await?;
        Ok(Self { query: Some(query) })
    }

    fn query(&self) -> Result<&Query> {
        self.query.as_ref().ok_or_else(|| Error::CliConnection {
            message: "not connected: call connect() first, or the session was already disconnected"
                .to_string(),
            source: None,
        })
    }

    /// Sends a plain-text user prompt into the session.
    ///
    /// # Errors
    ///
    /// [`Error::CliConnection`] when the session is closed.
    pub async fn send(&self, prompt: impl Into<String>) -> Result<()> {
        self.send_content(UserContent::Text(prompt.into())).await
    }

    /// Sends structured content (text or blocks) into the session.
    ///
    /// Covers upstream `client.query()` with a content-blocks payload.
    ///
    /// # Errors
    ///
    /// [`Error::CliConnection`] when the session is closed.
    pub async fn send_content(&self, content: UserContent) -> Result<()> {
        self.query()?
            .send_user_message(&content, DEFAULT_SESSION_ID)
            .await
    }

    /// Feeds a whole async stream of messages into the session.
    ///
    /// Covers upstream `client.query()` with an `AsyncIterable`
    /// argument. Unlike [`crate::query_stream`], this does NOT close
    /// stdin afterwards — the session stays open for further sends.
    ///
    /// # Errors
    ///
    /// [`Error::CliConnection`] on a broken session mid-feed.
    pub async fn send_stream(&self, prompts: impl Stream<Item = UserContent> + Send) -> Result<()> {
        let query = self.query()?;
        let mut prompts = Box::pin(prompts);
        while let Some(item) = prompts.next().await {
            query.send_user_message(&item, DEFAULT_SESSION_ID).await?;
        }
        Ok(())
    }

    /// Streams messages until (and including) the next
    /// [`Message::Result`] — i.e. one complete response. Safe to call
    /// again for the next turn after a prior `send`.
    ///
    /// # Errors
    ///
    /// Stream items are [`Result<Message>`]: connection/protocol/decode
    /// errors surface as `Err` items.
    pub fn receive_response(&self) -> Result<BoxStream<'_, Result<Message>>> {
        let query = self.query()?;
        Ok(stream::unfold((query, false), |(query, done)| async move {
            if done {
                return None;
            }
            loop {
                match query.next_message().await {
                    None => return None,
                    Some(Err(error)) => return Some((Err(error), (query, true))),
                    Some(Ok(value)) => match parse_message(value) {
                        Ok(Some(message)) => {
                            let is_result = matches!(message, Message::Result(_));
                            return Some((Ok(message), (query, is_result)));
                        }
                        Ok(None) => {}
                        Err(error) => return Some((Err(error), (query, true))),
                    },
                }
            }
        })
        .fuse()
        .boxed())
    }

    /// Streams every message as it arrives (does not stop at results).
    ///
    /// # Errors
    ///
    /// Stream items are [`Result<Message>`]: connection/protocol/decode
    /// errors surface as `Err` items.
    pub fn receive_messages(&self) -> Result<BoxStream<'_, Result<Message>>> {
        let query = self.query()?;
        Ok(stream::unfold(query, |query| async move {
            loop {
                match query.next_message().await {
                    None => return None,
                    Some(Err(error)) => return Some((Err(error), query)),
                    Some(Ok(value)) => match parse_message(value) {
                        Ok(Some(message)) => return Some((Ok(message), query)),
                        Ok(None) => {}
                        Err(error) => return Some((Err(error), query)),
                    },
                }
            }
        })
        .fuse()
        .boxed())
    }

    /// Sends an interrupt control request.
    ///
    /// # Errors
    ///
    /// [`Error::ControlProtocol`] if the CLI rejects or times out.
    pub async fn interrupt(&self) -> Result<()> {
        self.query()?.interrupt().await
    }

    /// Changes the permission mode mid-session.
    ///
    /// # Errors
    ///
    /// [`Error::ControlProtocol`] on rejection/timeout.
    pub async fn set_permission_mode(&self, mode: PermissionMode) -> Result<()> {
        self.query()?.set_permission_mode(mode.as_str()).await
    }

    /// Changes the model mid-session. `None` resets to the CLI default.
    ///
    /// # Errors
    ///
    /// [`Error::ControlProtocol`] on rejection/timeout.
    pub async fn set_model(&self, model: Option<&str>) -> Result<()> {
        self.query()?.set_model(model.map(str::to_string)).await
    }

    /// Rewinds tracked files to their state at a specific user message.
    ///
    /// Requires `enable_file_checkpointing` to have been set on the
    /// connecting options.
    ///
    /// # Errors
    ///
    /// [`Error::ControlProtocol`] on rejection/timeout.
    pub async fn rewind_files(&self, user_message_id: &str) -> Result<()> {
        self.query()?.rewind_files(user_message_id).await
    }

    /// Reconnects a disconnected or failed MCP server.
    ///
    /// # Errors
    ///
    /// [`Error::ControlProtocol`] on rejection/timeout.
    pub async fn reconnect_mcp_server(&self, server_name: &str) -> Result<()> {
        self.query()?.reconnect_mcp_server(server_name).await
    }

    /// Enables or disables an MCP server.
    ///
    /// # Errors
    ///
    /// [`Error::ControlProtocol`] on rejection/timeout.
    pub async fn toggle_mcp_server(&self, server_name: &str, enabled: bool) -> Result<()> {
        self.query()?.toggle_mcp_server(server_name, enabled).await
    }

    /// Stops a running background task.
    ///
    /// # Errors
    ///
    /// [`Error::ControlProtocol`] on rejection/timeout.
    pub async fn stop_task(&self, task_id: &str) -> Result<()> {
        self.query()?.stop_task(task_id).await
    }

    /// Gets current MCP server connection status.
    ///
    /// # Errors
    ///
    /// [`Error::ControlProtocol`] on rejection/timeout.
    pub async fn get_mcp_status(&self) -> Result<Value> {
        self.query()?.get_mcp_status().await
    }

    /// Gets a breakdown of current context window usage by category.
    ///
    /// # Errors
    ///
    /// [`Error::ControlProtocol`] on rejection/timeout.
    pub async fn get_context_usage(&self) -> Result<Value> {
        self.query()?.get_context_usage().await
    }

    /// Returns the cached `initialize` response (available commands,
    /// output styles, capabilities), or `None` if not connected.
    pub async fn server_info(&self) -> Option<Value> {
        match &self.query {
            Some(query) => query.server_info().await,
            None => None,
        }
    }

    /// Ends input, terminates the CLI, and releases resources. Safe to
    /// call more than once.
    ///
    /// # Errors
    ///
    /// [`Error::CliConnection`] only for unexpected cleanup failures.
    pub async fn disconnect(&mut self) -> Result<()> {
        if let Some(query) = self.query.take() {
            query.close().await?;
        }
        Ok(())
    }
}
