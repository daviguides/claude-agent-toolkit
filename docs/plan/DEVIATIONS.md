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
