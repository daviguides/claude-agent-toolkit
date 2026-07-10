# CLAUDE.md

## Project

claude-agent-toolkit — an idiomatic Rust port of the [Claude Agent SDK](https://github.com/anthropics/claude-agent-sdk-python) (currently Python-only, official). Wraps the bundled Claude Code CLI via subprocess/JSON message passing and exposes it as a safe, async, strongly-typed Rust API.

## Status

Implemented through Phase 10 (release prep) — see `docs/plan/` for the phase-by-phase plan and `docs/plan/DEVIATIONS.md` for every point where behavior was verified against (and occasionally diverges from) the pinned upstream reference. `docs/sync/PARITY.md` is the generated upstream↔Rust parity table.

## Quick Start

```bash
cargo build
cargo test
cargo run --example quick_start
```

Layout:

```
claude-agent-toolkit/
├── docs/foundation/    Vision, design decisions
├── docs/plan/          Phase-by-phase implementation plan + DEVIATIONS.md
├── docs/sync/          Machine-readable parity.yaml + generated PARITY.md
├── src/                Single crate: client, query, protocol, transport,
│                       types (options/message/permission/hook/mcp/...),
│                       mcp_server (in-process MCP tools), callback_adapters
├── examples/           Runnable examples (cargo run --example <name>)
└── tests/              Integration tests against a fake CLI harness
```

## Prior Art / Reference

- Upstream source of truth: https://github.com/anthropics/claude-agent-sdk-python
- Existing unofficial Rust ports (unmaintained or narrow scope — study before reinventing, do not blindly copy):
  - https://github.com/louloulin/claude-agent-sdk
  - https://github.com/jimmystridh/claude-agents-sdk
  - https://github.com/PandelisZ/claude-agent-sdk-rust
  - `claude-agent-sdk-rust` on crates.io (Wally869)
  - `claude-agent-rs` on crates.io (junyeong-ai) — direct Anthropic API client, not a Claude Code CLI wrapper
  - `claude-agent-rs` on crates.io (ExpertVagabond) — standalone coding agent, not an SDK

## Code Style

- Rust: idiomatic, async (tokio), favor the type system over runtime checks — this is the core value proposition vs. the Python original
- rustfmt + `cargo clippy --all-targets -- -D warnings` must stay clean
- Doc comment on every public item (`missing_docs` lint is on)
- Unit tests inline (`#[cfg(test)] mod tests`); integration tests in `tests/` against the `tests/fake_cli.rs` fake-CLI harness (`#[cfg(unix)]`-gated)

## Key Design Docs

- `docs/foundation/vision.md` — why this project exists, what it is/isn't

## crates.io

Reserved name: `claude-agent-toolkit` (unpublished).
