//! Transport abstraction over the Claude Code CLI process.

pub mod subprocess;

use futures::stream::BoxStream;
use serde_json::Value;

use crate::error::Result;

/// Low-level bidirectional JSON-line channel to the CLI.
///
/// Implemented by [`subprocess::SubprocessTransport`]; kept as a trait
/// so tests and future transports can substitute it. Native `async fn`
/// (RPITIT) is used rather than `async-trait`: nothing in this phase
/// needs `dyn Transport` — only a concrete `SubprocessTransport` is
/// driven directly. Revisit at the start of Phase 5 if the `Query`
/// actor needs dynamic dispatch.
pub trait Transport: Send {
    /// Starts the underlying process/connection.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::CliNotFound`] or
    /// [`crate::Error::CliConnection`] on failure to locate or spawn
    /// the CLI.
    fn connect(&mut self) -> impl Future<Output = Result<()>> + Send;

    /// Sends one already-serialized JSON line (no trailing newline —
    /// the transport appends it).
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::CliConnection`] if the transport is not
    /// ready or the underlying pipe is broken.
    fn write_line(&mut self, line: &str) -> impl Future<Output = Result<()>> + Send;

    /// Signals end of input (closes stdin).
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::CliConnection`] on failure to close.
    fn end_input(&mut self) -> impl Future<Output = Result<()>> + Send;

    /// Stream of parsed JSON values, one per stdout line. May be
    /// called only once — subsequent calls yield an empty stream.
    fn read_messages(&mut self) -> BoxStream<'static, Result<Value>>;

    /// Terminates the process and releases resources. Idempotent.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::CliConnection`] only for unexpected
    /// cleanup failures; a already-exited or already-closed process is
    /// not an error.
    fn close(&mut self) -> impl Future<Output = Result<()>> + Send;

    /// Whether the transport is ready to send/receive messages.
    fn is_ready(&self) -> bool;
}
