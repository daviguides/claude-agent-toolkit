# claude-agent-toolkit

An idiomatic Rust port of the official [Claude Agent SDK](https://github.com/anthropics/claude-agent-sdk-python) — build AI agents powered by Claude Code with Rust's type safety, async model, and zero-cost abstractions.

## Status

**Early planning — no code yet.** This repository currently exists to establish the project's vision and design direction before implementation starts. See [`docs/foundation/vision.md`](docs/foundation/vision.md).

## Why

The official Claude Agent SDK ships for Python and TypeScript only. A handful of independent Rust ports exist, but each is either unmaintained, narrow in scope, or a different kind of project entirely (a direct Anthropic API client rather than a Claude Code CLI wrapper, or a standalone coding agent rather than an SDK). None has become the canonical, actively maintained, idiomatic Rust equivalent of the upstream SDK.

## Goal

Track the upstream Python SDK's capabilities (query API, interactive multi-turn client, in-process MCP tools, hooks, permission modes, structured message types) while translating them into Rust idioms — `async`/`await` via tokio, the type system in place of runtime validation, and zero-cost wrappers around the Claude Code CLI subprocess protocol.

## crates.io

Reserved name: [`claude-agent-toolkit`](https://crates.io/crates/claude-agent-toolkit) (unpublished).

## License

TBD.
