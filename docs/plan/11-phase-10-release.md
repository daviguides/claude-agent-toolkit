# Phase 10 — Examples, Parity Audit, Docs, Release Prep

**Objective**: prove the SDK end-to-end against the real CLI, close the
parity gap with upstream, and make the crate publishable.

## Step 10.1 — Examples (each must compile via `cargo build --examples`)

Port the upstream `examples/` directory patterns
(⚠️ consult `reference/.../examples/` for scenarios):

1. `examples/quick_start.rs` — one-shot `query()` printing assistant
   text and final cost from `ResultMessage`.
2. `examples/streaming_client.rs` — `ClaudeClient`: two sequential
   prompts, printing responses; demonstrates `receive_response()`.
3. `examples/tools_and_hooks.rs` — a `can_use_tool` callback denying
   `Bash`, and a `PreToolUse` hook logging tool names.
4. `examples/mcp_calculator.rs` — `create_sdk_mcp_server` with `add` /
   `multiply` tools; options wire the server plus
   `allowed_tools: ["mcp__calc__add", "mcp__calc__multiply"]`
   (⚠️ VERIFY the `mcp__<server>__<tool>` naming convention upstream).

Each example: `//!` header comment stating required setup
(`npm install -g @anthropic-ai/claude-code`, `ANTHROPIC_API_KEY` or
logged-in CLI).

## Step 10.2 — Real-CLI smoke test (manual, evidence required)

With a real `claude` CLI installed and authenticated:

```bash
cargo run --example quick_start
cargo run --example streaming_client
cargo run --example mcp_calculator
```

Record actual output snippets into `docs/plan/SMOKE-RESULTS.md`
(create it). A phase is NOT done on "it should work" — paste the real
output. If a smoke test fails, the failure analysis goes in the same
file and gets fixed before release.

Optional guarded integration test: `tests/live_test.rs` with
`#[ignore]` attribute, run via `cargo test -- --ignored` only when
`CLAUDE_LIVE_TESTS=1` — asserts a `query("Reply with exactly: pong")`
round-trip contains "pong".

## Step 10.3 — Parity audit (the anti-"partial port" gate)

**Machine-readable source of truth**: unlike a prior draft of this
plan, the parity table is NOT hand-written markdown. It is
`docs/sync/parity.yaml`, a structured file — because a future tool
(`catk-sync`, a separate consumer crate/repo, NOT part of this crate —
see its own `docs/foundation/vision.md` and `docs/plan/`) diffs
upstream commits against it programmatically. Writing it as data now
avoids a breaking-format migration later.

Create `docs/sync/parity.yaml`:

```yaml
# Source of truth for upstream↔Rust API parity. Machine-read by the
# catk-sync tool (a separate project). Do not hand-edit PARITY.md
# — it is generated from this file (see below).
version: 1
upstream_pin: fdee0adc99f46e65ae9d6d029a6f4fb31bb8cffa  # keep in sync with docs/plan/UPSTREAM-PIN.md
entries:
  - upstream_symbol: "claude_agent_sdk.query"
    upstream_kind: function       # function | class | method | field | message_type | hook_event | error_variant | content_block
    category: public_api          # public_api | options_field | message_type | hook | permission | error | client_method | mcp
    rust_equivalent: "query::query"
    status: ported                # ported | partial | not_ported | justified_gap
    tested: true
    notes: ""
    justification: null           # REQUIRED (non-null) when status == justified_gap
  - upstream_symbol: "claude_agent_sdk.ClaudeSDKClient"
    upstream_kind: class
    category: public_api
    rust_equivalent: "client::ClaudeClient"
    status: ported
    tested: true
    notes: ""
    justification: null
  # ... one entry per upstream export in __init__.py, per field in
  # types.py's dataclasses, per message/hook/permission/error variant.
```

Every entry must have `status: ported` (with `rust_equivalent` and
`tested: true`) or `status: justified_gap` (with a non-empty
`justification`). `not_ported`/`partial` entries left over at the end
of this phase mean the phase is NOT done — walk `types.py` and
`__init__.py` export by export until the file is exhaustive.

**Generate the human-readable view**: write a tiny internal script
(`scripts/render-parity.sh` calling a short one-off Rust binary or, if
simpler, a `xtask`-style `cargo run --bin render-parity` — pick
whichever is less ceremony; a shell script piping through `yq`/`jq` is
also acceptable) that reads `docs/sync/parity.yaml` and writes
`docs/sync/PARITY.md` as a markdown table grouped by `category`, with
a header comment: `<!-- GENERATED from parity.yaml — do not edit -->`.
Run it and commit both files together, always in the same commit.

## Step 10.3b — Reference use-case audit (hard gate)

Read the three reference modules listed in `00-overview.md`
(refiner/foreman `sdk_wrapper.py`, prisma `claude_runner.py`) line by
line. For every SDK symbol, option field, message field, and behavior
they touch, add entries tagged `category: reference_use_case` to
`docs/sync/parity.yaml` proving the Rust equivalent exists AND is
tested (these render into their own section of `PARITY.md`).
Non-negotiable rows (from the audit already performed during
planning):

- multi-query session reuse on one connected client, with cumulative
  `total_cost_usd` semantics across queries (cost is cumulative,
  `num_turns` is per-query — assert this in a client test with two
  scripted result messages)
- `stderr` callback delivering every CLI stderr line
- `plugins` option reaching the CLI invocation
- `system_prompt` preset `claude_code` + `append` (exact wire form)
- `settings` accepting both a file path and an inline JSON string
- `ResultMessage`: `subtype`, `num_turns`, `total_cost_usd`,
  `duration_ms`, `duration_api_ms`, `is_error`, `session_id`,
  `result`, `usage.input_tokens`/`usage.output_tokens` readable
- `resume`, `add_dirs`, `include_partial_messages`, `max_turns`,
  `allowed_tools`, `permission_mode`, `cwd`, `model`

As the final proof, write `examples/reference_wrapper.rs`: a compiling
Rust translation of refiner's `SDKWrapper` core loop (connect → query
→ typed message loop with tool-call printing → metrics from
`ResultMessage` → disconnect, with stderr capture). If any line of
that example cannot be expressed, the gap goes back to the owning
phase and gets fixed — the project is NOT done with the gap open.

## Step 10.4 — Documentation polish

1. `lib.rs` crate docs: overview, quick-start example (compiling
   doctest, `no_run`), feature map, link to upstream.
2. Every public item documented (`missing_docs` warning must be clean).
3. Update `README.md`: replace the "Early planning" status with actual
   usage snippets (mirror the quick_start example), installation
   section (`cargo add claude-agent-toolkit`), CLI prerequisite note.
4. Update `CLAUDE.md`: status line ("implemented through phase 10"),
   crate layout section replacing the "TBD" block.
5. Add `CHANGELOG.md` with `0.1.0` entry listing capabilities.

## Step 10.4b — CI matrix expansion

Extend `.github/workflows/ci.yml` with a `windows-latest` job (build +
`cargo test --lib` for the unix-gated integration tests) and
`macos-latest` (full suite). Discovery unit tests must pass on all
three platforms.

## Step 10.5 — Release checklist

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo doc --no-deps          # RUSTDOCFLAGS="-D warnings"
cargo package --list         # inspect: no reference/, no tests/fixtures junk beyond need
cargo publish --dry-run
```

License decision: confirm `MIT` (or switch to `MIT OR Apache-2.0`,
the Rust-ecosystem default — recommend the dual license; update
`Cargo.toml` + add `LICENSE-MIT`/`LICENSE-APACHE` files). This needs
the repo owner's sign-off — list it as the ONE open question in the
final report; do not publish before it is answered.

**Do NOT run `cargo publish` (non-dry-run) without explicit owner
approval.**

## Step 10.6 — Final report

Summarize in the session's final message: what was built, parity table
status, smoke evidence, remaining open questions (license, Windows
support, any `DEVIATIONS.md` entries needing a decision).

## Acceptance Gate (project DONE)

- All Step 10.5 commands green
- `docs/sync/parity.yaml` complete with zero `not_ported`/`partial`
  entries — INCLUDING every `reference_use_case` entry from Step 10.3b
- `docs/sync/PARITY.md` regenerated and committed alongside the YAML
- `examples/reference_wrapper.rs` compiles and runs
- `SMOKE-RESULTS.md` contains real output for all examples
- README/CLAUDE.md/CHANGELOG updated

## Commits

1. `phase-10: examples`
2. `phase-10: smoke results`
3. `phase-10: parity audit`
4. `phase-10: docs + readme + changelog`
5. `phase-10: release dry-run fixes`
