# Implementation Plan — Overview

**Audience**: an executor LLM (possibly a small model). Follow this plan
literally. Do not improvise architecture. When a detail is marked
`⚠️ VERIFY`, read the referenced upstream file BEFORE writing code —
the upstream Python SDK is the single source of truth for the wire
protocol, not this plan.

## Mission

Build `claude-agent-toolkit`: an idiomatic Rust port of the official
[claude-agent-sdk-python](https://github.com/anthropics/claude-agent-sdk-python).
It wraps the bundled Claude Code CLI as a subprocess, speaks
newline-delimited JSON ("stream-json") over stdin/stdout, and exposes a
safe, async, strongly-typed Rust API.

Authoritative context (already in this repo — read before starting):

- `docs/foundation/vision.md` — why this project exists, what it is NOT
- `CLAUDE.md` — project conventions

## Non-Goals (repeat: do NOT build these)

- NOT a direct Anthropic Messages API client (no HTTP calls to
  api.anthropic.com)
- NOT a reimplementation of Claude Code — it spawns the existing CLI
- NOT a CLI product — it is a library crate only
- NOT a partial port — every phase below exists to reach upstream parity

## Architecture (decided — do not re-decide)

Single crate `claude-agent-toolkit` (no workspace). A workspace adds
ceremony with zero benefit at this size; revisit only if a separate
macro crate becomes necessary (it is not planned).

```
                 ┌────────────────────────────────────────────┐
 user code       │  query()                ClaudeClient       │  public API
                 └───────────┬──────────────────┬─────────────┘
                             │                  │
                 ┌───────────▼──────────────────▼─────────────┐
                 │  protocol::Query (actor task)              │  control protocol,
                 │  - routes normal messages to a stream      │  request/response
                 │  - answers CLI-initiated control requests  │  correlation
                 │    (can_use_tool, hook_callback,           │
                 │     mcp_message)                           │
                 │  - resolves SDK-initiated control requests │
                 │    (initialize, interrupt, set_*)          │
                 └───────────────────┬────────────────────────┘
                                     │
                 ┌───────────────────▼────────────────────────┐
                 │  transport::SubprocessTransport            │  spawn `claude`,
                 │  stdin writer / stdout line reader /       │  line framing,
                 │  stderr collector / child lifecycle        │  buffer limits
                 └───────────────────┬────────────────────────┘
                                     │
                              claude CLI process
```

### Crate layout (final target — files are created phase by phase)

```
claude-agent-toolkit/
├── Cargo.toml
├── rust-toolchain.toml
├── .github/workflows/ci.yml
├── scripts/
│   └── fetch-reference.sh          # clones upstream python SDK for consultation
├── reference/                      # gitignored; upstream clone lives here
├── src/
│   ├── lib.rs                      # module decls + public re-exports only
│   ├── error.rs                    # thiserror error enum(s)
│   ├── types.rs                    # module decls for types/
│   ├── types/
│   │   ├── message.rs              # Message, content blocks, parse_message()
│   │   ├── options.rs              # ClaudeAgentOptions + builder
│   │   ├── permission.rs           # PermissionMode, PermissionResult, ...
│   │   ├── hook.rs                 # HookEvent, HookMatcher, hook I/O types
│   │   └── mcp.rs                  # McpServerConfig variants, SdkMcpTool
│   ├── transport.rs                # Transport trait + module decl
│   ├── transport/
│   │   └── subprocess.rs           # SubprocessTransport, CLI discovery, args
│   ├── protocol.rs                 # module decl
│   ├── protocol/
│   │   ├── control.rs              # control_request/response wire types
│   │   └── query.rs                # Query actor (router)
│   ├── query.rs                    # public one-shot query()
│   ├── client.rs                   # public ClaudeClient (multi-turn)
│   └── mcp_server.rs               # in-process MCP server (tools/list, tools/call)
├── tests/
│   ├── fixtures/                   # captured JSON lines (wire samples)
│   ├── fake_cli.rs                 # helper that builds the fake CLI script
│   ├── transport_test.rs
│   ├── query_test.rs
│   └── client_test.rs
└── examples/
    ├── quick_start.rs
    ├── streaming_client.rs
    ├── tools_and_hooks.rs
    └── mcp_calculator.rs
```

Style rule: modern module layout — `foo.rs` + `foo/` directory, never
`foo/mod.rs`.

## Dependencies (exact — do not add others without a written reason)

```toml
[dependencies]
tokio = { version = "1", features = ["process", "rt", "rt-multi-thread", "io-util", "sync", "macros", "time"] }
futures = "0.3"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "2"
tracing = "0.1"

[dev-dependencies]
rstest = "0.24"
pretty_assertions = "1"
tempfile = "3"
tracing-subscriber = "0.3"
```

No `anyhow` (library crate — typed errors only). No `reqwest` (no HTTP).
No `async-trait` unless a trait with async methods becomes unavoidable —
prefer generic closures returning boxed futures (spelled out in the
phase files).

## Upstream Source of Truth

Phase 0 clones the upstream repo into `reference/claude-agent-sdk-python`
(gitignored). Key upstream files, referenced throughout this plan:

| Upstream file | What it defines |
|---|---|
| `src/claude_agent_sdk/types.py` | Options fields, message dataclasses, hooks, permission types |
| `src/claude_agent_sdk/_errors.py` | Error hierarchy |
| `src/claude_agent_sdk/_internal/transport/subprocess_cli.py` | CLI discovery, flag mapping, process I/O, buffer limits |
| `src/claude_agent_sdk/_internal/message_parser.py` | JSON → message parsing rules |
| `src/claude_agent_sdk/_internal/query.py` | Control protocol (both directions), hook/permission/MCP routing |
| `src/claude_agent_sdk/query.py` | Public one-shot `query()` behavior |
| `src/claude_agent_sdk/client.py` | Public interactive client behavior |
| `src/claude_agent_sdk/__init__.py` | Public API surface & exports (parity checklist) |

**Rule**: any table in this plan that maps wire formats or CLI flags is
a starting sketch. The executor MUST diff it against the upstream file
listed next to it and follow upstream when they disagree.

## Working Rules for the Executor

1. **TDD, always**: for every phase, write the listed tests FIRST, watch
   them fail to compile/pass, then implement until green. `.unwrap()` is
   allowed inside tests only.
2. **Acceptance gate after every phase** (all must pass before the next
   phase starts):
   ```bash
   cargo fmt --check
   cargo clippy --all-targets -- -D warnings
   cargo test
   cargo doc --no-deps
   ```
3. **Commit + push after every file edit** (per repo owner's global
   rule). Small commits, message format: `phase-N: <what>`.
4. **Rust standards** (already established for this repo):
   - Edition 2024, stable toolchain
   - `thiserror` enums for all fallible public APIs; propagate with `?`;
     never `.unwrap()`/`.expect()` on recoverable paths in `src/`
   - Borrow by default (`&str`, `&[T]`, `impl AsRef<Path>`)
   - `//!` module docs on every module; `///` with `# Errors` on every
     public fallible item
   - Guard clauses / `let-else` over nested matches
   - No magic values — named `const`s
5. **Do not silently downgrade scope.** If something in this plan turns
   out to be impossible or wrong versus upstream, STOP that phase and
   record the exact blocker in `docs/plan/DEVIATIONS.md` (create it on
   first use), then continue with the corrected approach.

## Phase Index (execute strictly in order)

| # | File | Deliverable | Depends on |
|---|------|-------------|-----------|
| 0 | `01-phase-0-scaffold.md` | Buildable empty crate, CI, upstream reference clone | — |
| 1 | `02-phase-1-errors.md` | `error.rs` complete | 0 |
| 2 | `03-phase-2-messages.md` | Message/content types + parser + fixtures | 1 |
| 3 | `04-phase-3-options.md` | `ClaudeAgentOptions` + CLI arg builder | 1 |
| 4 | `05-phase-4-transport.md` | Subprocess transport + fake-CLI test harness | 2, 3 |
| 5 | `06-phase-5-control-protocol.md` | `Query` actor, control request/response | 4 |
| 6 | `07-phase-6-query.md` | Public `query()` one-shot API | 5 |
| 7 | `08-phase-7-client.md` | Public `ClaudeClient` multi-turn API | 5 |
| 8 | `09-phase-8-permissions-hooks.md` | `can_use_tool` + hooks end-to-end | 7 |
| 9 | `10-phase-9-mcp-tools.md` | In-process MCP tools | 7 |
| 10 | `11-phase-10-release.md` | Examples, docs, parity audit, 0.1.0 prep | all |

Appendix: `appendix-a-wire-protocol.md` — wire message samples used as
test fixtures (each carries a `⚠️ VERIFY` pointer).

## Definition of DONE for the whole project

- Public API parity checklist (in `11-phase-10-release.md`) fully checked
  against upstream `__init__.py`
- All acceptance gates green
- Examples compile and run against a real installed `claude` CLI
  (manual smoke, documented output)
- `cargo publish --dry-run` succeeds
