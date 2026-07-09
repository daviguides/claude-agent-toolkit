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
