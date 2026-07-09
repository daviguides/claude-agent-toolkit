# Phase 4 — Subprocess Transport

**Objective**: spawn the `claude` CLI, write JSON lines to stdin, read
JSON lines from stdout with a buffer limit, collect stderr, and manage
the child lifecycle. Includes the fake-CLI test harness used by every
later phase.

**Upstream source of truth**:
`reference/.../src/claude_agent_sdk/_internal/transport/subprocess_cli.py`
(read it in full before starting this phase).

## Deliverable A — `src/transport.rs` (trait + module decl)

```rust
//! Transport abstraction over the Claude Code CLI process.

pub mod subprocess;

use futures::stream::BoxStream;
use serde_json::Value;

use crate::error::Result;

/// Low-level bidirectional JSON-line channel to the CLI.
///
/// Implemented by [`subprocess::SubprocessTransport`]; kept as a trait
/// so tests and future transports can substitute it.
pub trait Transport: Send {
    /// Starts the underlying process/connection.
    fn connect(&mut self) -> impl Future<Output = Result<()>> + Send;

    /// Sends one already-serialized JSON line (no trailing newline).
    fn write_line(&mut self, line: &str) -> impl Future<Output = Result<()>> + Send;

    /// Signals end of input (closes stdin).
    fn end_input(&mut self) -> impl Future<Output = Result<()>> + Send;

    /// Stream of parsed JSON values, one per stdout line.
    fn read_messages(&mut self) -> BoxStream<'static, Result<Value>>;

    /// Terminates the process and releases resources.
    fn close(&mut self) -> impl Future<Output = Result<()>> + Send;
}
```

Note: if `impl Future` in traits causes friction with object safety in
Phase 5 (the Query actor may need `Box<dyn Transport>`), switch the
whole trait to `async-trait` (add the dependency, record in
`DEVIATIONS.md`). Decide once, at the START of Phase 5, not midway.

## Deliverable B — CLI discovery (`src/transport/subprocess.rs`)

```rust
/// Environment variable identifying this SDK to the CLI.
const SDK_ENTRYPOINT: &str = "sdk-rust";

/// Locates the `claude` binary.
///
/// Search order (⚠️ VERIFY list and order against `_find_cli()`):
/// 1. explicit `cli_path` argument, if provided
/// 2. `claude` on `PATH` (iterate `std::env::split_paths(&env::var_os("PATH"))`,
///    checking existence of `dir.join("claude")`)
/// 3. well-known install locations, e.g.:
///    - `~/.npm-global/bin/claude`
///    - `/usr/local/bin/claude`
///    - `~/.local/bin/claude`
///    - `~/node_modules/.bin/claude`
///    - `~/.yarn/bin/claude`
///
/// # Errors
///
/// [`Error::CliNotFound`] when nothing is found.
fn find_cli(cli_path: Option<&Path>) -> Result<PathBuf> { todo!() }
```

Home resolution: `std::env::var_os("HOME")` (unix) — this crate targets
macOS/Linux first; Windows support is a recorded non-blocker
(`DEVIATIONS.md`) unless upstream tests demand it.

## Deliverable C — `SubprocessTransport`

```rust
/// Prompt delivery mode for the child process.
#[derive(Debug, Clone)]
pub enum PromptInput {
    /// One-shot: prompt passed via `--print`, stdin closed immediately.
    Text(String),
    /// Streaming: `--input-format stream-json`, messages written to stdin.
    Streaming,
}

/// Spawns and manages the Claude Code CLI process.
pub struct SubprocessTransport {
    options: ClaudeAgentOptions,
    prompt: PromptInput,
    cli_path: Option<PathBuf>,
    child: Option<tokio::process::Child>,
    stdin: Option<tokio::process::ChildStdin>,
    // stdout reader handed out once via read_messages()
    // stderr drained into a shared buffer by a background task
}
```

### Command construction (`connect()`)

1. `find_cli()`.
2. Build args: `["--output-format", "stream-json", "--verbose"]`
   + `build_cli_args(&options)` (Phase 3)
   + mode: `Streaming` → `["--input-format", "stream-json"]`;
     `Text(p)` → `["--print", p]`.
   (⚠️ VERIFY order/base flags against `_build_command()`.)
3. `tokio::process::Command::new(cli)`; apply `options.cwd` via
   `.current_dir()` if set; env = parent env + `options.env` +
   `("CLAUDE_CODE_ENTRYPOINT", SDK_ENTRYPOINT)`; pipe stdin/stdout/stderr;
   `.kill_on_drop(true)`.
4. Spawn errors: if `options.cwd` does not exist, return
   `Error::CliConnection` with a message naming the cwd (⚠️ VERIFY —
   upstream raises a dedicated message for bad cwd).
5. For `Text` mode: drop/close stdin right after spawn.
6. Spawn a stderr task: read lines and, for EACH line:
   - append to an `Arc<Mutex<Vec<String>>>` for use in `ProcessError`;
   - if `options.stderr` is `Some(callback)`, invoke `callback(&line)`
     (clone the `Arc` into the task). A panicking user callback must
     not kill the task: wrap the call in
     `std::panic::catch_unwind(AssertUnwindSafe(...))` and
     `tracing::warn!` on panic;
   - emit via `tracing::debug!`.

   This mirrors upstream's `stderr` option and is REQUIRED by the
   reference use cases (refiner/foreman keep the last 50 stderr lines
   to enrich their error reports).

### Reading (`read_messages()`)

Manual line loop over `tokio::io::BufReader::new(stdout)` wrapped in an
`async_stream`-style generator built with `futures::stream::unfold` (no
new deps):

- Accumulate bytes until `\n`; if accumulated length exceeds
  `options.max_buffer_size.unwrap_or(DEFAULT_MAX_BUFFER_SIZE)` →
  yield `Err(Error::BufferOverflow { limit })` and stop.
- Skip empty lines.
- `serde_json::from_str::<Value>(line)` — on failure yield
  `Err(Error::JsonDecode { line, source })`.
  (⚠️ VERIFY: upstream buffers partial JSON across reads and only
  errors past the limit — using `read_until(b'\n')` gives the same
  net behavior since the CLI writes one JSON object per line. If
  upstream's speculative-parse behavior matters for multi-line JSON,
  match it; note the decision either way.)
- On EOF: check `child.wait()`; non-zero exit → yield
  `Err(Error::Process { exit_code, stderr })` using the collected
  stderr buffer; zero exit → end the stream.

### Writing / closing

- `write_line`: write bytes + `\n`, flush; map broken pipe into
  `Error::CliConnection`.
- `end_input`: `shutdown()` then drop stdin.
- `close`: `start_kill()` if still running, `wait()` with a 5s
  `tokio::time::timeout`, reap, idempotent (second call is a no-op).

## Deliverable D — fake CLI test harness (`tests/fake_cli.rs`)

A tiny helper compiled into integration tests (declare as
`mod fake_cli;` from each test file — NOT a separate crate):

```rust
//! Builds throwaway shell scripts that impersonate the claude CLI.

use std::path::PathBuf;

use tempfile::TempDir;

pub struct FakeCli {
    pub dir: TempDir,
    pub path: PathBuf,
}

/// Creates an executable script that writes each given JSON line to
/// stdout (with a small delay), then exits with `exit_code`.
pub fn scripted(lines: &[&str], exit_code: i32) -> FakeCli {
    // Writes a #!/bin/sh script into a TempDir:
    //   #!/bin/sh
    //   printf '%s\n' '<line1>'
    //   printf '%s\n' '<line2>'
    //   exit <code>
    // chmod +x. Single quotes inside lines must be shell-escaped:
    // replace ' with '\'' before embedding.
    todo!()
}

/// Variant that first echoes back every stdin line to a file (for
/// asserting what the SDK wrote), then prints the scripted lines.
pub fn recording(lines: &[&str], exit_code: i32) -> FakeCli { todo!() }
```

`SubprocessTransport` must therefore accept an explicit `cli_path`
override (constructor argument) so tests can point at the fake script —
this same override is the public "custom CLI path" feature.

## Tests (`tests/transport_test.rs`, write FIRST)

1. `finds_cli_on_path` — put fake script dir at front of `PATH` via a
   scoped env change; `find_cli(None)` returns it. (Env-mutating tests:
   mark `#[serial]`? No extra dep — instead pass the PATH to search as a
   parameter: refactor `find_cli` to take `path_var: Option<&OsStr>`
   internally so the test injects a fake PATH without touching process
   env. Public wrapper uses the real env.)
2. `returns_cli_not_found_with_install_hint` — empty PATH →
   `Error::CliNotFound`, display contains `npm install`.
3. `reads_scripted_messages_in_order` — fake CLI printing 3 JSON lines →
   stream yields 3 `Ok(Value)` in order, then ends (exit 0).
4. `skips_blank_lines` — script with blank lines between JSON.
5. `surfaces_json_decode_error_with_line` — script prints `not json` →
   one `Err(Error::JsonDecode { line, .. })`, `line == "not json"`.
6. `surfaces_process_error_with_stderr_on_nonzero_exit` — script prints
   to stderr and exits 2 → final item `Err(Error::Process { exit_code:
   Some(2), stderr })` containing the stderr text.
7. `enforces_buffer_limit` — `max_buffer_size: Some(64)`, script prints
   a 1000-char line → `Err(Error::BufferOverflow { .. })`.
8. `writes_lines_to_child_stdin` — `recording` fake; `write_line` twice,
   `end_input`, then read the recording file: both lines present,
   newline-terminated.
9. `close_is_idempotent` — call `close()` twice; second returns `Ok`.
10. `one_shot_mode_appends_print_flag` — unit test on the (extracted,
    pure) full-command builder: `Text("hi")` → args end with
    `["--print", "hi"]`; `Streaming` → contains
    `["--input-format", "stream-json"]`.
11. `stderr_callback_receives_each_line` — script writes two stderr
    lines; options carry a callback pushing into an
    `Arc<Mutex<Vec<String>>>`; after the stream ends, the vec equals
    the two lines in order.
12. `stderr_callback_panic_does_not_break_reading` — callback panics on
    the first line; scripted stdout messages are still all delivered
    and the process error/exit path still works.

## Acceptance Gate

```bash
cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test && cargo doc --no-deps
```

## Commits

1. `phase-4: transport trait + fake CLI harness`
2. `phase-4: transport tests (red)`
3. `phase-4: subprocess transport (green)`
