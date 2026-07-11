# Deviations from the Plan

Records every point where the pinned upstream (see `UPSTREAM-PIN.md`)
disagreed with a plan sketch, or where a `⚠️ VERIFY` resolved to
something other than the plan's guess. Per `00-overview.md`: "Do not
silently downgrade scope... record the exact blocker... then continue
with the corrected approach."

## Phase 2 — Message types

**Finding**: the pinned upstream commit is substantially newer/larger
than the message model sketched in `03-phase-2-messages.md`. Upstream
`types.py`/`message_parser.py` now include an entire additional
surface not mentioned anywhere in this plan: task lifecycle messages
(`TaskStartedMessage`, `TaskProgressMessage`, `TaskNotificationMessage`,
`TaskUpdatedMessage`), `HookEventMessage` (emitted only when the
`include_hook_events` option — itself not in any phase's options list —
is set), `MirrorErrorMessage` (belongs to an entire session-store
subsystem no phase in this plan covers), and a new top-level
`rate_limit_event` message type with its own `RateLimitEvent`/
`RateLimitInfo` types.

**Decision**: implement the core conversational message surface
(`User`, `Assistant`, `System`, `Result`, `StreamEvent`) fully and
faithfully against the CURRENT upstream shape — including fields the
original phase-2 sketch omitted (`AssistantMessage.error/usage/
message_id/stop_reason/session_id/uuid`, `UserMessage.uuid/
tool_use_result`, `ResultMessage.structured_output/model_usage/
permission_denials/deferred_tool_use/errors/api_error_status/uuid`,
and `ContentBlock::ServerToolUse`/`ServerToolResult`). These are
confirmed (via upstream's own `tests/test_message_parser.py`) to occur
in ordinary conversation turns — omitting them would silently drop
real content, which is a correctness bug, not a scope reduction.

**Update (phase 2b)**: task lifecycle messages, `HookEventMessage`,
`MirrorErrorMessage`, and `RateLimitEvent` were initially deferred here
as out of scope. Corrected per repo-owner direction: the upstream
Python reference repo is the actual source of truth for this port, not
the plan's original phase sketches — a feature present upstream is not
"out of scope" just because no phase file mentions it. All four are
now implemented as full `Message` variants (`TaskStarted`,
`TaskProgress`, `TaskNotification`, `TaskUpdated`, `MirrorError`,
`HookEvent`, `RateLimitEvent`), tested against
`reference/.../tests/test_message_parser.py`'s cases. The only
remaining gap is a language-level one, not a scope choice: upstream
models several of these as `SystemMessage` subclasses so
`isinstance(x, SystemMessage)` keeps matching old call sites; Rust has
no inheritance, so each gets its own top-level `Message` variant
instead. No data is lost either way. A `system` subtype this port
still doesn't specifically recognize (a genuinely new one upstream adds
later) falls through to a generic `SystemMessage { subtype, data }`
carrying the full raw JSON — exactly mirroring upstream's own fallback
for subtypes IT doesn't recognize (see
`unknown_system_subtype_yields_generic_system_message`). This
"full-fidelity, defer nothing observed upstream" standard now applies
to every remaining phase, not just Phase 2.

**Confirmed ⚠️ VERIFY resolution — unknown message type**: the plan's
sketch guessed unknown types are a parse error. Upstream
`message_parser.py`'s `case _:` fallback returns `None` (forward
compatibility, logged at debug level), NOT an error. This changes
`parse_message`'s signature project-wide from `Result<Message>` (as
literally shown in the phase-2 deliverable snippet) to
`Result<Option<Message>>`. Test `rejects_unknown_message_type` from
the phase-2 spec is implemented as `skips_unknown_message_type`
instead, asserting `Ok(None)`.

**Confirmed ⚠️ VERIFY resolution — unknown content block type**:
upstream's `match block["type"]:` has no wildcard case, so a block
whose `"type"` doesn't match a known variant is silently dropped from
the parsed content list (not an error). A block whose `"type"` DOES
match a known variant but is missing a required field still raises
`MessageParseError` (Python's per-case `KeyError` path). This port
mirrors both behaviors: `parse_content_block` returns `Ok(None)`
(skip) for unrecognized tags and `Err(Error::MessageParse)` for
recognized-but-malformed blocks.

**Minor simplification**: upstream's content-block match arms differ
slightly between `user` (text/tool_use/tool_result only) and
`assistant` (adds thinking/server_tool_use/advisor_tool_result) role
content. This port uses one shared set of 6 recognized block types for
both roles. A `thinking` or server-tool block appearing inside a
`user`-role message (not something upstream itself is ever observed to
emit) parses instead of being silently dropped — strictly more
permissive, and unlikely to matter since the CLI does not emit those
block types on user-role messages in practice.

**Wire-tag note**: the `ServerToolResult` content block's wire tag is
`"advisor_tool_result"`, not `"server_tool_result"` — asymmetric with
`ServerToolUse`'s `"server_tool_use"` tag. Mirrored via an explicit
`#[serde(rename = "advisor_tool_result")]`.

**`AssistantMessage.error`**: kept as `Option<String>` (raw pass
through) rather than a closed Rust enum, matching upstream's own
unconstrained-`str` treatment — preserves forward compatibility with
new error kinds the CLI may start emitting.

## Phase 3 — Options and CLI argument builder

**Finding**: `04-phase-3-options.md`'s field table (~22 fields) covers
roughly half of the actual `@dataclass ClaudeAgentOptions` in the
pinned upstream `types.py` (~40 fields). Per the full-fidelity standard
established in phase 2b, every upstream field not already deferred by
an explicit dependency-ordering reason (see below) is implemented.

**Newly included beyond the original sketch**: `tools` (base tool
preset), `strict_mcp_config`, `session_id`, `max_budget_usd`,
`fallback_model`, `betas`, `cli_path`, `include_hook_events`, `skills`,
`sandbox` (+ nested `SandboxNetworkConfig`/`SandboxIgnoreViolations`),
`max_thinking_tokens`, `thinking` (+ `ThinkingDisplay`), `effort`,
`output_format`, `enable_file_checkpointing`, `session_store` (a new
`SessionStore` trait — hand-rolled boxed-future methods, not
`async-trait`, per the crate's stated dependency policy — plus
`SessionKey`/`SessionStoreEntry`/`SessionStoreListEntry`/
`SessionSummaryEntry`/`SessionListSubkeysKey`/`SessionStoreFlushMode`),
`session_store_flush`, `load_timeout_ms`, `task_budget`. `AgentDefinition`
is expanded from the sketch's 4 fields to upstream's full 12
(`disallowedTools`, `skills`, `memory`, `mcpServers`, `initialPrompt`,
`maxTurns`, `background`, `effort`, `permissionMode`).
`PermissionMode` gained `dontAsk`/`auto` (6 variants, not 4).
`debug_stderr` is the one upstream field NOT ported: it is explicitly
documented upstream as "Deprecated and no longer read by the
transport" — a dead field, not a gap.

**Still deferred, unchanged from the original plan** (not a new gap —
an explicit, already-documented dependency-ordering decision):
`can_use_tool` and `hooks`. Both need hook I/O types
(`HookMatcher`, `HookCallback`, `PermissionResult`, etc.) that Phase 8
defines; adding the fields now would mean typing them against
placeholders and revising twice.

**Confirmed ⚠️ VERIFY resolutions in `_build_command()`** (all
diverge from the plan's guesses; upstream wins):
- `system_prompt`: when unset, upstream ALWAYS emits `--system-prompt ""`
  (never zero flags). A `Preset` with no `append` emits NO flag at all
  (not `--system-prompt` with the preset name). A new `File` variant
  maps to `--system-prompt-file`.
- `plugins` → repeated `--plugin-dir <path>` per local plugin, NOT a
  single JSON `--plugins` flag. Only the `local` type exists.
- `agents` → **no CLI flag at all**. Upstream's own comment: "Agents
  are always sent via initialize request (matching TypeScript SDK). No
  --agents CLI flag needed." Delivered via the Phase 5 control
  protocol instead; the options field exists for structural
  completeness but `build_cli_args` never touches it.
- `setting_sources` → single joined `--setting-sources=a,b,c` argument
  (`=`-joined), not two separate args. An explicit empty list
  (`Some(vec![])`, meaning "disable filesystem settings") still emits
  `--setting-sources=` — this is an `is not None` check upstream, not
  a truthiness check.
- `skills`: not a CLI flag itself — `"all"` injects a bare `Skill` into
  `allowed_tools`, a name list injects `Skill(name)` per entry, and
  `setting_sources` defaults to `[user, project]` when unset. Mirrored
  as `apply_skills_defaults()`.
- `mcp_servers` accepts a dict OR a bare path/inline-JSON string
  (`McpServersOption::Servers` / `::Path`), not just a dict.
- `user` is not a CLI flag at all — it is an OS-level parameter to the
  subprocess spawn call itself (like `sudo -u`), consumed by the Phase
  4 transport, not `build_cli_args`.
- The CLI is now ALWAYS invoked in streaming-input mode — upstream
  unconditionally appends `--input-format stream-json` at the end of
  `_build_command()`, with a comment confirming there is no more
  `--print <prompt>` one-shot invocation path. This affects Phase 4/6
  significantly; flagged here so those phases aren't blindsided.

**Python-truthiness quirks mirrored faithfully** (each locked in by a
named test — `max_turns_zero_omits_flag`,
`max_budget_usd_zero_still_emits_flag`): several upstream checks are
`if x:` (falsy on `0`/`""`/`[]`) rather than `is not None`. `max_turns`,
`model`, `fallback_model`, `permission_prompt_tool_name`, `resume`,
`session_id`, and `settings` all use the truthy form — an explicit
`Some(0)` or `Some(String::new())` silently omits the flag, exactly as
it does upstream. `max_budget_usd`, `task_budget`, `effort`, and
`setting_sources` use `is not None` and so DO emit on zero/empty.

## Phase 4 — Subprocess transport

**Finding — no more one-shot `--print` mode**: `05-phase-4-transport.md`
sketches a `PromptInput::{Text, Streaming}` enum where `Text` maps to
`--print <prompt>`. Upstream's actual `SubprocessCLITransport.__init__`
sets `self._is_streaming = True` unconditionally with the comment
"Always use streaming mode internally (matching TypeScript SDK)". A
grep for `--print` across `subprocess_cli.py` finds nothing. This
already surfaced in Phase 3: `build_cli_args` unconditionally appends
`--input-format stream-json` at the end, with no branch for a `--print`
mode — so Phase 3's implementation was already correct against current
upstream. Phase 4's `PromptInput` enum and the `--print` flag are
dropped entirely; `SubprocessTransport` is prompt-agnostic. One-shot
`query()` vs. multi-turn `ClaudeClient` (Phase 6/7) differ only in how
many messages get written to stdin and when `end_input()` is called —
never in the CLI invocation itself.

**Finding — `prompt` constructor argument is dead code upstream**:
`SubprocessCLITransport.__init__(self, prompt, options)` stores
`self._prompt = prompt` but no method in the class ever reads it back
— confirmed by grepping `self._prompt` (one hit: the assignment). The
Rust `SubprocessTransport` therefore takes no prompt parameter at all;
writing prompt/turn messages is entirely the caller's job via
`write_line`, matching what upstream actually does at runtime despite
what the constructor signature implies.

**CLI discovery — bundled-CLI lookup skipped (structural, not a
gap)**: upstream's `_find_cli()` first checks a `_bundled/claude(.exe)`
path relative to the installed Python package directory (the npm/pip
package ships a vendored binary). A Rust crate installed via `cargo`
has no equivalent packaged-binary directory — there is nothing to
search. This step is omitted; discovery here is: explicit `cli_path` →
`claude` on `PATH` → 6 well-known install locations (`~/.npm-global/
bin/claude`, `/usr/local/bin/claude`, `~/.local/bin/claude`,
`~/node_modules/.bin/claude`, `~/.yarn/bin/claude`,
`~/.claude/local/claude` — the plan's sketch listed only 5, missing the
last one; corrected here).

**`CliNotFound` message enriched to match upstream**: upstream's actual
message is multi-line and more actionable than Phase 1's original
short text: it also suggests `export PATH="$HOME/node_modules/.bin:
$PATH"` and passing `cli_path` via options. `error.rs`'s `Display` impl
is updated to include this guidance (still passes the original
`cli_not_found_display_includes_install_hint` test, which only checks
a substring).

**`Error::Process.stderr` — deliberately richer than upstream, not a
gap**: upstream's post-EOF exit-code check builds `ProcessError` with
a HARDCODED placeholder string `"Check stderr output for details"` —
it does not actually attach captured stderr text to the error; callers
are expected to have collected stderr themselves via the `stderr`
callback (confirmed: refiner/foreman keep their own last-50-lines ring
buffer for this exact reason). Since error message content is not part
of the wire protocol, this port keeps Phase 4's originally-planned
behavior instead: the transport maintains its own bounded ring buffer
of recent stderr lines (independent of whether a caller-supplied
callback is set) and attaches it to `Error::Process`. This is strictly
more helpful than upstream, not a functional gap, and does not
contradict the "upstream is source of truth" standard, which governs
wire-visible capability, not internal error-message ergonomics.

**Simplified process-shutdown escalation**: upstream's `close()` does
a three-stage escalation (graceful wait after stdin EOF → SIGTERM →
wait → SIGKILL) using `anyio` cancel-scope shielding with no direct
Tokio equivalent, plus a process-wide `atexit` orphan reaper
(`_ACTIVE_CHILDREN`). This port implements a two-stage version (close
stdin → bounded graceful wait → force-kill, which on Tokio is
`Child::start_kill()`, SIGKILL-equivalent on unix / `TerminateProcess`
on Windows) and does not implement the SIGTERM intermediate step (would
require the `nix` crate — not in the fixed dependency list — for a
signal Tokio doesn't expose directly) or the atexit-style global orphan
reaper (no direct Rust equivalent without additional unsafe global
state). Revisit if graceful-shutdown data loss (the scenario upstream's
comment references, issue #625) is observed in practice.

**Best-effort CLI version check ported**: `_check_claude_version()` —
spawn `claude -v` with a timeout, warn (never error) if below
`2.0.0` — is ported as `warn_if_cli_version_outdated`, called from
`connect()`, with all failures swallowed exactly as upstream does.

**Non-JSON stdout lines are skipped, not errors**: `_parse_stdout_line`
skips blank lines AND lines that don't start with `{` (some CLI builds
write non-JSON diagnostic lines like `[SandboxDebug] ...` to stdout) —
only a line that starts with `{` and fails to parse is a
`JsonDecode` error. Mirrored exactly; the phase-4 spec's sketch only
mentioned skipping blank lines.

## Phase 5 — Control protocol and the Query actor

**Finding — far more outbound control-request subtypes than the plan
lists**: `06-phase-5-control-protocol.md` sketches only `initialize`,
`interrupt`, `set_permission_mode`, `set_model`. Upstream `query.py`
actually implements 9: those 4 plus `rewind_files` (file
checkpointing), `mcp_reconnect`/`mcp_toggle` (`serverName` wire key,
camelCase), `stop_task` (pairs with Phase 2b's task-lifecycle
messages), `mcp_status`, and `get_context_usage`. All 9 are
implemented as `ControlRequestBody` variants plus matching `pub(crate)`
convenience methods on `Query` (`interrupt`, `set_permission_mode`,
`set_model`, `rewind_files`, `reconnect_mcp_server`, `toggle_mcp_server`,
`stop_task`, `get_mcp_status`, `get_context_usage`), mirroring
`query.py` 1:1 — Phase 7's `ClaudeClient` will thinly wrap these rather
than reimplementing them.

**Confirmed — inbound (CLI-initiated) subtypes are exactly the plan's
3**: `_handle_control_request`'s dispatch only recognizes `can_use_tool`,
`hook_callback`, `mcp_message` — anything else raises "Unsupported
control request subtype". No expansion needed here; `InboundControlRequestBody`
has exactly 3 variants, matching upstream's `SDKControlPermissionRequest`
(9 fields, not the plan's smaller sketch — `tool_use_id`, `agent_id`,
`blocked_path`, `decision_reason`, `title`, `display_name`,
`description` all included) and `SDKHookCallbackRequest`.

**Confirmed — `control_cancel_request` exists**: the plan flagged this
`⚠️ VERIFY`. Upstream's read loop handles it: look up the cancelled
`request_id` in the in-flight spawned-handler-task map, abort it, no
response is written back. Implemented identically.

**Confirmed — timeout value**: upstream's `_send_control_request`
default timeout is `60.0` seconds for ALL control requests (not just
`initialize`); `Query.__init__`'s `initialize_timeout` parameter
defaults to the same 60.0s but is independently overridable. Both are
modeled as `Duration` fields on `Query`, defaulting to 60s, overridable
per-instance (tests use a short override, matching the plan's own
suggested test design).

**Request-id format**: upstream generates
`f"req_{counter}_{os.urandom(4).hex()}"`. This crate has no `rand`
dependency (not in the fixed dependency list), so the random suffix is
derived from `SystemTime` subsec-nanos instead of OS randomness — IDs
only need to be unique-enough for in-process correlation and log
readability, not cryptographically random, so this is a safe,
dependency-free substitution. Kept injectable (a `suffix` the
constructor can pin, e.g. `"test"`) so tests get deterministic ids
(`req_1_test`, `req_2_test`, ...) exactly as the phase-5 spec's test
design requires.

**Two-task design, ownership refined (not redesigned)**: the plan's
"write task: single owner of `transport.write_line`... read task:
consumes `transport.read_messages()`" leaves an unaddressed Rust
ownership question — both tasks would need `&mut self.transport`
simultaneously, which the borrow checker forbids without wrapping the
whole transport in a lock (defeating "zero locks around the
transport"). Resolution: `read_messages()` is called once,
synchronously, before spawning (yielding an owned `'static` stream that
no longer borrows `transport`); the `Transport` value itself then moves
into the write task, which also becomes the sole place `end_input()`
and `close()` execute (routed through the same outbound channel as a
`WriteCommand` enum — `Line(String)`, `EndInput`, `Close(oneshot::Sender<...>)`
— rather than as raw strings). This preserves the prescribed
architecture (one task reads, one task is the sole transport owner,
all writes serialize through a channel) while resolving how
`Query::close()`/`end_input()` reach a transport that has moved into a
spawned task.

**Deferred — `TranscriptMirrorBatcher` / live session-store write
path**: upstream peels `"transcript_mirror"` messages off the read
loop and hands them to a batcher (`_internal/transcript_mirror_batcher.py`,
not read as part of this phase) that buffers and flushes entries to
the `session_store` configured in Phase 3, with flush-before-result
timing and its own error-reporting path (`report_mirror_error` →
`MirrorErrorMessage`, already modeled in Phase 2b). This is a
self-contained subsystem with its own file and batching/flush
semantics, not "control protocol" proper, and it only activates when a
caller has configured `session_store` (an advanced, opt-in feature).
This port currently drops `"transcript_mirror"` messages in the read
loop (recognized and skipped, never forwarded to consumers or
misparsed) — matching upstream's own behavior in the common case where
no batcher is attached. Building the batcher itself is left as a
follow-up scoped investigation, not silently forgotten.

**Added beyond upstream — structured-error enrichment on process
exit**: ported upstream's `_last_error_result_text` tracking (remember
the `errors`/`subtype` of the last `is_error: true` result message; if
the transport then reports `Error::Process` from the CLI's expected
non-zero exit after an error result, replace the generic "exit code
N" message with the structured error text) since it directly improves
diagnostics and upstream already does it — not new scope, a faithful
port of an existing upstream behavior the phase-5 spec's sketch simply
didn't mention.

**Tests live inline in `query.rs`, not `tests/protocol_test.rs`**:
`Query`, `QueryHandlers`, and the control wire types are deliberately
`pub(crate)` — internal plumbing until Phase 6/7 build the public API
on top, matching the phase-5 spec's own framing ("everything public
sits on top"). An external integration-test crate (anything under
`tests/`) can only see a crate's `pub` surface, so it structurally
cannot reach `pub(crate)` items — `tests/protocol_test.rs` as literally
specified would not compile. Tests instead live in
`#[cfg(test)] mod tests` inside `src/protocol/query.rs`, sharing the
`tests/fake_cli.rs` harness via `#[path = "../../tests/fake_cli.rs"]
mod fake_cli;` declared at the top of `query.rs` (not nested inside
`mod tests`, which would compute the wrong virtual base directory for
the `#[path]` and fail to find the file).

**Fake-CLI harness bug found and fixed**: the first `scripted_and_recording`
implementation backgrounded the stdin-recording `cat`  (`cat > file &`)
so it could also emit unprompted stdout lines. This silently loses
all input: bash redirects a backgrounded job's stdin to `/dev/null` in
non-interactive scripts when job control is off (confirmed by manual
repro outside the test suite). Fixed by backgrounding the *stdout
writer* instead and keeping the stdin recorder (`cat > file`) in the
foreground — the writer doesn't touch stdin, so backgrounding it is
harmless, and the recorder now correctly inherits the real stdin.

## Phase 6 — Public `query()` API

**Confirmed ⚠️ VERIFY — initialize handshake ALWAYS runs**: the plan's
sketch guessed one-shot `--print` mode skips `initialize()`. There is
no `--print` mode at all (Phase 4/5 finding), and
`_process_query_inner` calls `await query.initialize()`
unconditionally for both string and async-iterable prompts, with the
comment "Always initialize to send agents via stdin (matching
TypeScript SDK)". Ported identically: `query()` always initializes.

**Confirmed — one-shot string prompt is `session_id: ""`, not a
placeholder**: upstream writes
`{"type":"user","session_id":"","message":{"role":"user","content":prompt},"parent_tool_use_id":null}`
— empty string, not `"default"` or similar. Phase 5's
`send_user_message` test used `"default"` only as an arbitrary example
value for its own parameter; `query()` itself calls it with `""`.

**Internal-only difference — write path for the one-shot message**:
upstream writes the one-shot user message via
`chosen_transport.write()` directly, bypassing `Query`'s own queuing.
This crate's `Query::start` moves the transport into its driver task
by the time `start()` returns, so nothing outside `Query` can reach
the transport directly anymore — `query()` instead calls
`Query::send_user_message`, which produces the identical wire line
through the same single-writer channel every other `Query` write
already goes through. Same bytes on the wire; different internal path,
required by Rust's ownership model (Python's asyncio has no analogous
constraint).

**Confirmed — responses genuinely stream concurrently with input
being fed**: `query.py`'s own docstring says streaming-mode
"still unidirectional... All prompts are sent, then all responses
received," which reads as sequential. The actual implementation is
not: `stream_input(prompt)` is spawned as an independent background
task (`query.spawn_task(...)`) fully concurrent with the foreground
`async for data in query.receive_messages()` loop. The docstring is a
simplified mental model for callers, not a description of the
implementation. `query_stream()` is built concurrent, matching the
real behavior (and the phase-6 spec's own test #10 expectation).

**`initialize_timeout` reads an environment variable, with a floor**:
`CLAUDE_CODE_STREAM_CLOSE_TIMEOUT` (milliseconds, default `"60000"`),
then `max(ms / 1000.0, 60.0)` — the timeout can only be raised above
60s by the env var, never lowered below it. Ported identically as
`resolve_initialize_timeout()`.

**No `CLAUDE_AGENT_CLI_PATH`-equivalent env var added**: the plan
proposed one for test injectability if upstream lacked a mechanism.
Upstream's `_find_cli()` has no environment-variable lookup at all
(confirmed by the full Phase 4 read) — but this crate already has
`ClaudeAgentOptions.cli_path` (a REAL upstream field, ported in Phase
3, not an extension), which fully satisfies the stated need for
test/user CLI-path injection. No new env var is introduced.

**`can_use_tool` + string-prompt mutual exclusivity — logic deferred,
not the validation gap it looks like**: upstream raises if
`can_use_tool` is set with a string prompt (requires the streaming
iterable form), and separately if `can_use_tool` and
`permission_prompt_tool_name` are both set. `ClaudeAgentOptions` has
no `can_use_tool` field yet — deferred to Phase 8 by design (Phase 3's
own documented dependency-ordering decision, since the callback type
needs Phase 8's hook/permission I/O types). There is nothing to
validate against yet; Phase 8 must add this check when it adds the
field. Noted here so it isn't forgotten.

**Session-store-specific setup left out of `query()`**: upstream's
`process_query` also calls `validate_session_store_options`,
`materialize_resume_session` (loads a resumed session from the store
into a temp `CLAUDE_CONFIG_DIR`), and `build_mirror_batcher` — all
part of the session-store subsystem already deferred in Phase 5
(`TranscriptMirrorBatcher`). `query()` accepts `options.session_store`
as a value (Phase 3's field) but does not yet wire the load/mirror
machinery, consistent with that standing deferral.

**Test list correction — no `--print` flag to assert**: the phase-6
spec's test #6 (`prompt_reaches_cli_via_print_flag`) asserts a
`--print` CLI argument that no longer exists. Replaced with a test
asserting the prompt reaches the CLI as a written stdin JSON line
(`{"type":"user","session_id":"",...}`), which is how the one-shot
prompt is actually delivered now. Test #9
(`stream_input_uses_streaming_mode_flags`) is dropped rather than
replaced: it asserted the CLI invocation differs between one-shot and
streaming-input modes, which no longer applies now that both always
use the identical always-streaming command — already covered by
`full_command_args_have_expected_base_and_trailing_flags` in
`transport_test.rs`.

**Phase 5 revisited — driver never closed the transport on read-loop
end/error, and `Query` had no cleanup-on-drop**: writing `query()`'s
message stream surfaced two real gaps in Phase 5's driver: (1) on a
read error that wasn't a clean process exit, the child could still be
running when `transport` simply fell out of scope — no explicit
`close()`/kill, a potential leak; (2) if a caller drops the message
stream before it naturally ends (e.g. `query()`'s returned stream
dropped mid-iteration), `Query` had no `Drop` impl, so the driver task
and its owned transport/child would keep running detached. Both fixed
in `src/protocol/query.rs`: the driver now calls `transport.close()`
on every loop-exit path (clean EOF too, for deterministic handle
release rather than whenever `transport` happens to drop), and `Query`
gained a best-effort `Drop` that signals the driver to close even
without an explicit `.close().await`.

**Phase 5 revisited — `next_message`/`close` changed from `&mut self`
to `&self`**: `query_stream()` needs a reading loop and a concurrent
input-feeding task to share one `Query` (matching upstream's own
`stream_input` running as an independent background task — see
above). `messages`/`driver_task` moved behind `tokio::sync::Mutex` to
enable this; not a genuine multi-reader-contention design (exactly one
task calls each in practice), purely a type-system device so every
`Query` method takes `&self`, letting callers hold one shared
`Arc<Query>` instead of splitting reader/writer halves.

**`futures::stream::unfold` is not fused by default**: it panics if
polled again after returning `None`. `query()`/`query_stream()`'s
returned stream applies `.fuse()` before boxing so callers get the
ordinary "poll after completion is safe, yields `None`" contract public
Rust streams are expected to have (caught by the phase-6 spec's own
test #3, `stream_ends_after_process_exit`, which polls three times).

**New fake-CLI harness helper — `scripted_with_initialize`**: every
`query()`/`query_stream()` test needs its fake CLI to answer the
`initialize` handshake (confirmed always-sent, see above) before
anything else happens, or the call hangs for the full 60s control
timeout. Added `scripted_with_initialize(lines, stderr_lines,
exit_code)`: reads stdin in a loop, records every line, and — on
seeing `"subtype":"initialize"` — extracts that request's
`request_id` via `sed` (no JSON parsing needed) and replies with a
canned success before printing the scripted output. Existing
`scripted`/`recording`/`responding`/`scripted_and_recording` helpers
are untouched (Phase 5's tests call `Query::start`/`start_with`
directly and never trigger `initialize`, so they don't need this).

## Phase 7 — Public `ClaudeClient`

**Finding — far more public methods than the plan lists**: the plan's
`ClaudeClient` sketch has 9 methods. Upstream `ClaudeSDKClient` has 16:
the 9 plus `rewind_files`, `reconnect_mcp_server`, `toggle_mcp_server`,
`stop_task`, `get_mcp_status`, `get_context_usage`, `get_server_info`.
All 7 additions are thin wrappers over `Query` convenience methods
Phase 5 already built (for exactly this reason — Phase 5's
DEVIATIONS.md entry already flagged that Phase 7 would "thinly wrap
these rather than reimplementing them"). No new `ControlRequestBody`
variants were needed; all 9 already exist.

**`get_server_info()` needed a new cache on `Query`**: upstream returns
`self._query._initialization_result` — the raw response from `connect()`'s
own `initialize()` call, cached, not re-fetched. `Query` gained an
`initialization_result: tokio::sync::Mutex<Option<Value>>` field,
populated inside `initialize()` on success; `server_info(&self) ->
Option<Value>` exposes a clone of it.

**`connect()` takes no initial-prompt parameter — a deliberate,
justified Rust simplification**: upstream's `connect(prompt: str |
AsyncIterable | None = None)` exists because `SubprocessCLITransport`'s
constructor requires *some* prompt/iterable — a bare interactive
connect (`__aenter__`, i.e. `async with ClaudeSDKClient()`) has to pass
a synthetic async generator that "never yields, but indicates this
function is an iterator and keeps the connection open" just to satisfy
that constructor. This crate's `SubprocessTransport` is prompt-agnostic
from Phase 4 (confirmed: the `prompt` constructor argument upstream
stores is dead code, never read back) — there is no constructor to
satisfy, so no workaround is needed. `ClaudeClient::connect(options)`
takes no prompt; callers use `send()`/`send_content()`/`send_stream()`
immediately after connecting for the equivalent of
`connect(prompt=...)`. Functionally identical outcome, no capability
lost.

**Added — `connect_with_transport()` for custom transports**: upstream's
`ClaudeSDKClient(transport=...)` accepts a caller-supplied `Transport`
(documented for e.g. remote Claude Code connections). `Query::start`
is already generic over `impl Transport + 'static` and — importantly —
returns a *non-generic* `Query` (the transport type is fully erased
once it moves into the spawned driver task), so `ClaudeClient` itself
never needs to be generic. Both `connect()` (builds a
`SubprocessTransport` from `options`) and the new
`connect_with_transport(transport, options)` (options used only for
the `initialize` handshake's hooks/agents/skills — the transport is
already built) share one internal helper.

**No `Drop` impl needed on `ClaudeClient` — already covered
transitively**: the phase-7 spec's sketch suggested a best-effort
`Drop` via `kill_on_drop(true)`, which Phase 4 deliberately did NOT set
(it chose graceful close-then-escalate instead — see Phase 4's
DEVIATIONS.md entry). `ClaudeClient` holds a `Query` field, and `Query`
already has its own best-effort `Drop` (added retroactively in Phase
6) that signals the driver to close on drop. Dropping a `ClaudeClient`
drops its `Query` field, which gets this cleanup for free — no
additional `Drop` impl needed or added.

**No public `session_id` parameter on `send`/`send_content`/`send_stream`**:
upstream's `query(prompt, session_id="default")` accepts a custom
session id per call. The phase-7 spec's own fixed method sketch omits
this parameter, and none of the three reference use cases exercise
multi-session usage on one client. All three methods use
`DEFAULT_SESSION_ID = "default"`. The underlying `Query::send_user_message`
(Phase 5) already accepts an arbitrary `session_id`, so exposing a
custom-session variant later is a small, low-risk addition if ever
needed — not a capability actually lost, just not surfaced publicly
yet.

**`can_use_tool` mutual-exclusivity validation — same deferral as
Phase 6**: `_connect_inner` validates `can_use_tool` + string-prompt
and `can_use_tool` + `permission_prompt_tool_name` combinations.
`ClaudeAgentOptions.can_use_tool` doesn't exist yet (Phase 8). Nothing
to validate against yet; noted for Phase 8 to add.

**Two real test-harness hangs found and fixed while writing
`client_test.rs`** — both are genuine bugs the test run caught, not
plan-vs-upstream deviations, recorded here because the fix pattern
matters for any future test in this style:
1. `set_permission_mode_sends_wire_string`/`set_model_sends_model_name`
   originally used `scripted_with_initialize` (acks only `initialize`).
   `Query::set_permission_mode`/`set_model` await a real control
   response with the default 60s timeout — since nothing ever
   acknowledged those specific requests, both tests hung for a full
   minute each before failing. Fixed by switching to
   `dynamic_responding` with explicit rules for both the `initialize`
   and the specific outbound subtype under test.
2. `receive_messages_continues_past_result` called `.collect()` on the
   raw `receive_messages()` stream. That stream never auto-terminates
   (matches upstream — only `receive_response()` stops at a `Result`);
   the fake CLI, after printing its scripted lines, stays alive reading
   stdin (never closed in this test), so the process never exits and
   `.collect()` blocked forever. Fixed with `.take(4)` before
   collecting.

**New fake-CLI harness helper — `dynamic_responding`**: public-API
tests (`ClaudeClient::connect`, `query()`) always go through the
crate's default non-deterministic `RequestIdGenerator` — there is no
way to pin a deterministic id through public API (only Phase 5's
internal `Query::start_with` allows that, for Phase 5's own
`pub(crate)`-only tests). `dynamic_responding(rules, exit_code)`
generalizes `scripted_with_initialize`'s `sed`-based request-id
extraction to arbitrary rules: each response is a printf format string
with one `%s`, substituted with the real extracted request_id at
runtime. Used for `interrupt`/`set_permission_mode`/`set_model`/
`initialize`-rejection/`server_info` tests — anywhere a test needs a
control response to actually match whatever id production code
generated.

## Phase 8 — `can_use_tool` permission callback and hooks

**Finding — `HookEvent` has 10 upstream variants, not 6**: the plan's
sketch lists `PreToolUse`, `PostToolUse`, `UserPromptSubmit`, `Stop`,
`SubagentStop`, `PreCompact`. Upstream's `types.py` union has 4 more:
`PostToolUseFailure`, `Notification`, `SubagentStart`,
`PermissionRequest`. All 10 are modeled — omitting any would be a real
registration capability gap (a caller genuinely could not hook that
event at all).

**`HookInput` stays a raw JSON payload, not 10 discriminated
structs — a deliberate, bounded simplification**: upstream types each
hook event's input as its own TypedDict with event-specific required
fields (e.g. `PostToolUseHookInput` adds `tool_response`,
`SubagentStopHookInput` adds `agent_id`/`agent_transcript_path`/
`agent_type`/`stop_hook_active`, etc.) — a real, if narrow, union.
Modeling all 10 with their per-event field sets is substantial
additional surface whose only consumer is the SAME closure a caller
already writes for their own hook logic; the raw payload already gives
full access to every field via `serde_json::Value` indexing — no
capability is lost, only compile-time field name checking for a type
callers write themselves anyway. The plan's own sketch already fixed
this design (`pub struct HookInput { pub payload: Value }`); kept as
specified.

**`CanUseTool` callback carries the FULL upstream `ToolPermissionContext`,
not just tool_name/input/suggestions**: the plan's `ToolPermissionRequest`
sketch has 3 fields; upstream's actual callback signature is
`(tool_name, input, ToolPermissionContext)` where the context has 8
fields (`suggestions`, `tool_use_id`, `agent_id`, `blocked_path`,
`decision_reason`, `title`, `display_name`, `description` — `signal`
is an unused future-abort-signal placeholder, omitted). All 8 were
already captured on `InboundControlRequestBody::CanUseTool` in Phase
5 (confirmed exhaustive against `SDKControlPermissionRequest` back
then) — Phase 8 only had to plumb them through to the public
callback type instead of discarding them.

**`can_use_tool` validation now implemented (was deferred since Phase
6)**: `ClaudeAgentOptions.can_use_tool` didn't exist before this
phase, so the mutual-exclusivity checks upstream's `_connect_inner`/
`_process_query_inner` perform had nothing to validate. Now
implemented exactly as upstream: (1) `can_use_tool` + a plain-string
`query()` prompt is rejected (`Error::ControlProtocol`, matching
upstream's `ValueError`) — `ClaudeClient::connect()` never takes a
string prompt in this port's design (Phase 7 deviation), so only
`query()`'s string-prompt path needs this check; (2) `can_use_tool` +
an explicit `permission_prompt_tool_name` is rejected; (3) when
`can_use_tool` is set and no conflict exists, `permission_prompt_tool_name`
is auto-set to `"stdio"`, matching upstream's `replace(options,
permission_prompt_tool_name="stdio")`.

**`_warn_if_can_use_tool_shadowed` ported as a `tracing::warn!`**:
upstream uses Python's `warnings.warn`; this crate has no equivalent
warning registry, so the advisory (allowed_tools entries or
`bypassPermissions` mode that would auto-approve a call before the
callback is ever consulted) is logged via `tracing::warn!` at
connect/query time instead. Same trigger conditions, same message
content, different delivery mechanism — a language-level adaptation,
not a lost capability (the crate's users are expected to have tracing
configured; anyone who isn't already loses log lines everywhere, not
specially here).

**Hook `hook_{i}` id assignment made deterministic via explicit event
order, not `HashMap` iteration order**: `ClaudeAgentOptions.hooks` is
keyed by `HookEvent`; Rust `HashMap` iteration order is unspecified
and would make `hook_0`, `hook_1`, ... assignment nondeterministic
across runs (breaking both reproducibility and the phase-8 spec's own
test expectations for exact id sequences). IDs are assigned by walking
a fixed `HookEvent::ALL` array in declaration order, then each event's
matcher list in registration order, then each matcher's callback list
in registration order — matching upstream's dict-insertion-order
iteration (Python dicts preserve insertion order) as closely as a
`HashMap`-keyed design can.

**`PermissionUpdate` fields are all optional per-variant, not
required**: the plan's sketch makes `rules`/`behavior`/`destination`
etc. required struct-variant fields. Upstream's `to_dict()` conditionally
includes each one only `if self.X is not None` — a `PermissionUpdate`
can carry a `type` and nothing else. All fields use
`skip_serializing_if = "Option::is_none"`; `rename_all = "camelCase"`
on both the enum (for the `type` tag: `addRules`, `setMode`, etc.) and
each struct variant's fields (`toolName`, `ruleContent`) reproduces
`to_dict()`'s exact conditional shape without a custom `Serialize` impl.

## Phase 9 — In-process MCP tools

**Confirmed ⚠️ VERIFY — `initialize`'s `protocolVersion` is hardcoded,
not echoed**: the plan's sketch guessed the requested `protocolVersion`
is echoed back. `_internal/query.py`'s `_handle_sdk_mcp_request`
hardcodes `"protocolVersion": "2024-11-05"` in the `initialize` result
regardless of what the CLI sent — no read of the request's own
`params.protocolVersion` anywhere in the method. Ported as a `const
MCP_PROTOCOL_VERSION: &str = "2024-11-05"`, always returned verbatim.

**Confirmed ⚠️ VERIFY — `notifications/initialized` gets a real
response, not "no reply"**: the plan's sketch guessed this notification
gets no response at the control-protocol layer. Upstream's
`_handle_sdk_mcp_request` returns `{"jsonrpc": "2.0", "result": {}}`
for this method like any other, and `_handle_control_request`
unconditionally wraps whatever `_handle_sdk_mcp_request` returns as
`{"mcp_response": ...}` inside a success control response — there is no
branch anywhere for "no response". `SdkMcpServer::handle_message`
therefore returns `Value` (not `Option<Value>` as the plan's sketch
typed it) and always produces a concrete JSON-RPC response, matching
`_handle_sdk_mcp_request`'s own `-> dict[str, Any]` signature (never
`None`).

**Confirmed ⚠️ VERIFY — unknown MCP server name is a JSON-RPC error
inside a SUCCESS control response, not a control-protocol error**: the
plan's sketch guessed "Unknown server → error control response".
Upstream's `_handle_sdk_mcp_request` checks `if server_name not in
self.sdk_mcp_servers` and returns a normal JSON-RPC error object
(`{"jsonrpc":"2.0","id":...,"error":{"code":-32601,"message":"Server
'{name}' not found"}}`) — no exception raised — which the caller then
wraps as `response_data = {"mcp_response": mcp_response}` and sends via
the ordinary "send success response" path. Only a genuinely malformed
request (`_handle_control_request`'s own `if not server_name or not
mcp_message: raise Exception(...)`, i.e. one or both fields missing
from the control request itself) produces a control-protocol-level
error response. `protocol/query.rs`'s `McpMessage` dispatch arm is
corrected to match: an unrecognized `server_name` builds the JSON-RPC
error value locally (same code/message) and returns `Ok(...)`, not
`Err(...)`.

**`SdkMcpServer` stores tools in a `Vec<SdkTool>`, not the plan
sketch's `HashMap<String, SdkTool>`**: upstream builds `cached_tool_list`
once, in registration order, and `tools/list` always returns that exact
cached list — the list's order is part of its behavior, not incidental.
A `HashMap`-backed store would make `tools/list`'s order unspecified
across runs, repeating the exact nondeterminism problem Phase 8's hook
`hook_{i}` ids already hit and fixed (see Phase 8's entry above) for no
benefit, since the field is private (`tools` has no `pub` in the plan's
own sketch) — lookup-by-name during `tools/call` is a linear scan over
what is expected to be a small, human-authored tool list.

**`McpServerConfig` loses its `Serialize`/`PartialEq` derives once the
`Sdk` variant is added — both replaced deliberately, not silently
dropped**: adding `Sdk(SdkMcpServer)` (which holds `Arc<dyn Fn>` tool
handlers) makes a derived `Serialize`/`PartialEq` over the whole enum
impossible to generate (closures implement neither). Per the plan's own
fixed choice, serialization is replaced by a dedicated
`to_cli_config_json(&McpServers) -> Value` function (manually
reproducing each variant's previous wire shape, including the
empty-collection `skip_serializing_if` behavior) rather than a manual
`Serialize` impl. `PartialEq` has no dedicated replacement — comparing
two live server objects (arbitrary closures) has no sensible
definition, so it is dropped outright from both `McpServerConfig` and
the `McpServersOption` enum that wraps it (same policy already applied
to `ClaudeAgentOptions` in Phase 8 once callbacks were added: drop the
derive wholesale rather than special-case it). The small number of
existing tests that compared these types with `assert_eq!` are updated
to `matches!`/field-level assertions.

**`annotations` (upstream `ToolAnnotations`) kept as raw
`Option<serde_json::Value>`, not a typed struct — a bounded, deliberate
gap, not silently dropped**: `SdkMcpTool.annotations` upstream is typed
`mcp.types.ToolAnnotations | None`, a type from the external `mcp` PyPI
package (imported via `from mcp.types import ToolAnnotations`) — it is
not vendored anywhere in this port's pinned `reference/` checkout, and
this crate has no dependency on any MCP client/server crate (out of
scope per `vision.md`). Since `_handle_sdk_mcp_request`'s `tools/list`
handling forwards `tool.annotations.model_dump(exclude_none=True)`
verbatim under an `annotations` key with no further interpretation by
this SDK layer, a raw JSON value forwarded the same way loses no
observable capability — callers can build any shape the real MCP spec
allows; only compile-time field-name checking for a type this crate
would have to reverse-engineer from an unvendored dependency is given
up.

**`_meta`/`anthropic/maxResultSizeChars` NOT ported — bounded, low-value
gap**: upstream's `_build_meta` attaches a `_meta` key to each
`tools/list` entry containing `{"anthropic/maxResultSizeChars": N}`
when `tool_def.annotations.maxResultSizeChars` is set — an
Anthropic-specific CLI-internal hint (controls a large-tool-result
storage/spill threshold) piggybacked onto the same external, unvendored
`ToolAnnotations` object referenced above. This is advisory-only
CLI-side behavior, not a protocol capability a caller loses access to
(nothing stops a caller from encoding the same key inside the
`annotations` raw JSON value if they know upstream's convention);
omitted rather than guessing at a schema this port cannot verify.

**Unknown tool name in `tools/call` → JSON-RPC code `-32602` (Invalid
params), not independently verifiable against upstream's exact code**:
the calling code raises a Python `ValueError` for an unknown tool name,
but the actual JSON-RPC error code produced for that exception is
generated deep inside the external `mcp` PyPI package's request-dispatch
machinery (`mcp.server.lowlevel.server`), not this repo's pinned
`reference/` checkout — fetching that package's current source (the
`modelcontextprotocol/python-sdk` repo) did not conclusively resolve
the exact version/code pinned by this SDK's dependency lock either.
`-32602` is used as the standards-correct JSON-RPC 2.0 choice ("a valid
method invoked with an invalid parameter value" — the tool name is the
invalid parameter), clearly distinct from `-32601` ("method not found",
used for genuinely unrecognized top-level JSON-RPC methods and unknown
MCP server names elsewhere in this same file). Documented here rather
than left as a silent guess.

**Tool-handler panics are caught, matching Phase 8's callback-safety
pattern**: `tools/call` wraps the user handler invocation in
`AssertUnwindSafe(...).catch_unwind()` (same pattern already used for
`can_use_tool`/hook callbacks in `callback_adapters.rs`), converting a
panic into a JSON-RPC `-32603` (Internal error) response instead of
crashing the query actor's read loop. Upstream's own equivalent `except
Exception as e: return {"error": {"code": -32603, ...}}` around the
handler call is the direct analogue; Rust needs the explicit
panic-catching machinery Python's exception handling gets for free.

**No Python-style dict/TypedDict `input_schema` shorthand — a harmless,
already-implied simplification**: upstream's `input_schema: type[T] |
dict[str, Any]` accepts a `{"param_name": python_type}` shorthand that
`_build_schema` expands into a full JSON Schema object at server-
creation time (`_python_type_to_json_schema`). Rust has no runtime type
reflection to replicate this, and the plan's own fixed `tool()` sketch
already types `input_schema: Value` (a raw JSON Schema value only) — no
capability is lost since any JSON Schema achievable via the Python
shorthand is directly expressible as a `serde_json::json!` literal.

## Phase 10 — Parity audit, examples, release prep

**Session listing/query/mutation subsystem — an acknowledged,
unresolved scope gap, not a false justification**: the exhaustive
`docs/sync/parity.yaml` walk of `__init__.py`'s exports (Step 10.3)
surfaced 25 upstream symbols with no Rust equivalent anywhere in this
port: `list_sessions`, `get_session_info`, `get_session_messages`,
`list_subagents`, `get_subagent_messages`, `SDKSessionInfo`,
`SessionMessage`, `InMemorySessionStore`, `fold_session_summary`,
`project_key_for_directory`, and the `*_from_store` /
`rename_session` / `tag_session` / `delete_session` / `fork_session`
(+ `ForkSessionResult`) families built on top of them. All of these
operate ON a `SessionStore` instance — Phase 3 ported the trait itself
plus its data types (`SessionKey`, `SessionStoreEntry`,
`SessionStoreFlushMode`, `SessionStoreListEntry`, `SessionSummaryEntry`,
`SessionListSubkeysKey`) since `ClaudeAgentOptions.session_store`
depends on them, and Phase 5 already flagged the related
`TranscriptMirrorBatcher` write path as a deferred, self-contained
follow-up — but no phase in this 10-phase plan ever scoped the
query/listing/mutation *functions* built on top of the trait, and
Step 10.3b's reference-use-case audit (refiner, foreman, prisma) found
none of the three real production callers exercise any of them either.

This is recorded as `status: justified_gap` (not `not_ported`) in
`parity.yaml` because the absence itself is now fully documented and
deliberate, not accidental — but the underlying capability gap is
real and, unlike every other `justified_gap` entry in this file, has
no completed Rust implementation standing behind it. Flagged
explicitly for the repo owner: this needs a decision (a dedicated
follow-up phase, or a permanent scope exclusion recorded in
`docs/foundation/vision.md`) — it is not something this phase resolved
on its own authority.

**Update — repo owner requested implementation; all 25 symbols now
ported** (`src/session_management.rs` + submodules `disk.rs`/`store.rs`/
`summary.rs`/`paths.rs`/`unicode_sanitize.rs`/`iso8601.rs`). The 25
symbols split into two architecturally distinct families, confirmed by
a dedicated research pass over `sessions.py` (1925 lines),
`session_mutations.py` (962 lines), `session_summary.py`, and
`session_store.py`:

- **`_from_store`/`_via_store` family** (9 functions +
  `InMemorySessionStore` + the pure helpers `fold_session_summary`/
  `project_key_for_directory`): built entirely on the `SessionStore`
  trait Phase 3 already shipped. `InMemorySessionStore` is a faithful
  port of upstream's own reference adapter (in-process `HashMap`s for
  entries/mtimes/summaries, a monotonic millisecond clock so
  back-to-back appends never collide, cascading delete, prefix-scanned
  `list_subkeys`).
- **Direct-local-disk family** (`list_sessions`, `get_session_info`,
  `get_session_messages`, `list_subagents`, `get_subagent_messages`,
  `rename_session`, `tag_session`, `delete_session`, `fork_session`,
  `import_session_to_store`): reads/writes the same
  `~/.claude/projects/<sanitized>/...` JSONL files the CLI itself
  writes, replicating CLI-internal conventions rather than any part of
  the wire protocol this crate otherwise wraps.

**Confirmed ⚠️ VERIFY resolutions, ported exactly**:
- `_simple_hash`: a 32-bit rolling hash (`h = (h<<5)-h+char`, masked to
  32 bits with JS-style signed wraparound), base36-encoded — used only
  once a sanitized path name exceeds 200 characters, appended as a
  `-<hash>` suffix on the 200-char prefix (not the full name).
- `_extract_first_prompt_from_head`'s skip conditions
  (`isMeta`/`isCompactSummary`/`tool_result`-carrying content, a
  `<command-name>…</command-name>` slash-command fallback that never
  wins over a real prompt, and a `_SKIP_FIRST_PROMPT_PATTERN` regex for
  IDE-injected/interrupt markers) and its 200-Unicode-scalar-value
  truncation (`rstrip()` then append `…`, no truncation/ellipsis when
  already short enough) are ported verbatim into
  `session_management::summary` — shared between the store family
  (operating on parsed entries) and the disk family (operating on raw
  JSONL lines, with fast substring pre-filters before `serde_json`
  parsing, mirroring upstream's own perf-motivated pre-filter).
- `_sanitize_unicode` (session tags): NFKC normalize → strip
  Format/PrivateUse/Unassigned Unicode general categories → strip an
  explicit zero-width/BOM/directional-mark/private-use range set,
  iterated to a fixed point or 10 rounds, whichever comes first.
- `_build_fork_lines`: one shared transform
  (`session_management::disk::build_fork_lines`) called by both
  `fork_session` and `fork_session_via_store`, exactly as upstream
  shares it — fresh uuid per entry (including `progress`-typed ones,
  needed to walk the parent chain), `progress` entries dropped from the
  *written* output only after the chain is rebuilt, `parentUuid`
  walked past any `progress` ancestor, `logicalParentUuid` remapped or
  dropped (never left stale) if it pointed outside the mapped set, a
  fresh timestamp on only the last written entry, `teamName`/
  `agentName`/`slug`/`sourceToolAssistantUUID` stripped, and a final
  `custom-title` entry titled explicitly or derived
  (`<derived> (fork)`, falling back to the literal `"Forked session"`
  when nothing is derivable).
- Wire-shape asymmetry confirmed and preserved: the disk-path
  `rename_session`/`tag_session` append a 3-key entry
  (`type`/`customTitle-or-tag`/`sessionId`, no `uuid`/`timestamp`); the
  `_via_store` siblings append 5 keys (adding `uuid` + `timestamp`).
  Neither carries a `parentUuid` — both are metadata-only entries, not
  part of the `user`/`assistant` chain.
- `list_sessions`/`list_sessions_from_store`'s `limit=0` means
  "unlimited," not "return nothing" (upstream's `is not None and > 0`
  check, not a truthiness check) — same Python-truthiness-quirk
  standard already established in Phase 3, re-verified here with a
  named test (`list_sessions_from_store_limit_zero_means_unlimited`).
- An unknown MCP-style edge case here too: a tag line is matched by an
  exact `{"type":"tag"` *prefix* scan, not a substring search — so a
  `tool_use` input that happens to contain a `"tag"` field (e.g. a
  `git tag`/Docker invocation) never gets misread as a session tag.

**One deliberate, bounded simplification**: `_build_conversation_chain`'s
multi-leaf/terminal-hunting algorithm is ported with index-based
lookups (`HashMap<uuid, index>` into the transcript slice) rather than
upstream's pointer/object-identity-based walk — semantically identical
(same terminal-detection, same main-vs-any-leaf preference, same
highest-file-order-wins tie-break, same cycle protection via a
visited-set), just expressed in terms the Rust ownership model prefers
over shared mutable graph nodes.

**Rust-specific plumbing decisions, none upstream-visible**: `unsafe_code
= "forbid"` (already a crate-wide policy) rules out
`std::env::set_var`/`remove_var` for pointing `projects_dir()` at a
test's temp directory (both require `unsafe` since edition 2024) — a
`#[cfg(test)]` thread-local override
(`paths::set_test_projects_dir_override`) sidesteps both the global
mutable state and the `unsafe` block; safe since every disk-family test
using it is a plain `#[test]`, never a multi-threaded `#[tokio::test]`,
so the override never needs to cross OS threads. Also: the write paths
(`rename_session`/`tag_session`/`fork_session`) needed a
`resolve_or_create_project_dir` distinct from the read paths'
`candidate_project_dirs`, since a first write to a directory with no
prior session history must create `~/.claude/projects/<sanitized>/`
rather than erroring — upstream's CLI does this same on-demand creation
implicitly; the plan's own research pass didn't call it out explicitly,
but it falls straight out of "first write to a new directory must
succeed," a correctness requirement, not a scope choice. New
dependencies added for this phase, all narrowly-scoped and
well-maintained: `regex` (the several verbatim-ported patterns above),
`uuid` (RFC-4122 v4 generation for fork/rename/tag entries — genuine
UUIDs written into real transcript files, unlike Phase 5's internal
request-id correlation which deliberately avoided a `rand` dependency
since it never needed cryptographic randomness), `unicode-normalization`
+ `unicode-general-category` (NFC/NFKC + Unicode general-category
lookups, no `std` equivalent exists).

`docs/sync/parity.yaml` updated: all 25 entries flipped from
`justified_gap` to `ported` with `tested: true`. 83 new unit tests
across the six new modules (path/hash/sanitize, ISO-8601 parse/format,
first-prompt extraction, summary folding, Unicode tag sanitization, the
`InMemorySessionStore` conformance suite, both chain builders, and
`build_fork_lines`'s edge cases — empty transcript, `up_to_message_id`
not found, state-leak field stripping, derived-vs-explicit titles).

**Post-landing audit found and fixed 4 real bugs** (an independent
fork re-verified the implementation against actual upstream source
rather than trusting the research reports at face value — worth doing
given the algorithm density here):

1. `summary_entry_to_sdk_info` dropped upstream's `custom_title or
   ai_title or None` fallback entirely (only read `custom_title`), and
   didn't treat an empty string the same as absent for any string
   field — upstream's `x or None` pattern applies broadly. Fixed:
   `custom_title` now falls back to `ai_title`, and `str_field` filters
   out empty strings before returning, matching upstream for
   `git_branch`/`cwd`/`tag` too.
2. `rename_session`/`rename_session_via_store` stored the raw `title`
   verbatim with no validation — upstream strips it and rejects an
   empty-after-trim result (`ValueError`). Fixed: both now trim and
   return `Error::Session` on an empty result.
3. `tag_session`/`tag_session_via_store` sanitized in the wrong order
   (trim-then-sanitize instead of upstream's sanitize-then-trim,
   `_sanitize_unicode(tag).strip()`) and silently treated an explicit
   `Some("")` the same as `None` (clearing the tag) — upstream only
   clears on `None`; an explicit empty/whitespace-only tag raises.
   Fixed: sanitize-then-trim order corrected, and only `None` clears
   now — an explicit non-`None` tag that's empty after
   sanitizing+trimming is rejected.
4. `build_fork_lines`'s `remap_fork_entry` removed the
   `logicalParentUuid` key when the original entry had none or it
   mapped outside the fork's uuid set. Upstream always sets this key
   via a dict literal (`None`/`null` included) — never omits it. Fixed:
   the key is now always present, `null` when there's nothing to map.

9 new tests cover these (ai_title fallback, empty-string-as-absent,
title trimming/rejection for both families, tag order/rejection for
both families, `logicalParentUuid` always-present). Acceptance gate
re-verified green after the fixes.
