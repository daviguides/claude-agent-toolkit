# CLAUDE.md

## Project

claude-agent-toolkit — an idiomatic Rust port of the [Claude Agent SDK](https://github.com/anthropics/claude-agent-sdk-python) (currently Python-only, official). Wraps the bundled Claude Code CLI via subprocess/JSON message passing and exposes it as a safe, async, strongly-typed Rust API.

## Status

Early planning. No code yet — see `docs/foundation/vision.md` for the rationale and scope before writing any implementation.

## Quick Start

Not yet buildable. Planned layout (Rust workspace, subject to change once implementation starts):

```
claude-agent-toolkit/
├── docs/foundation/    Vision, design decisions (write before code)
├── src/ or crates/     TBD — single crate vs workspace decided during design
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
- No code has landed yet; conventions get established with the first crate, not invented in advance

## Key Design Docs

- `docs/foundation/vision.md` — why this project exists, what it is/isn't

## crates.io

Reserved name: `claude-agent-toolkit` (unpublished).
