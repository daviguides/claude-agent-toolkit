# claude-agent-toolkit — Vision

## Pain

The official Claude Agent SDK — the programmatic interface to Claude Code, wrapping the CLI over async JSON message passing — ships officially for Python and TypeScript only. Rust developers building agentic tooling, CLIs, or backend services have no first-party option.

| Pain | Reality |
|------|---------|
| No official Rust SDK | Anthropic maintains Python and TypeScript; Rust users either shell out to the CLI by hand or reimplement the wire protocol from scratch |
| Fragmented, unmaintained ports | Several independent attempts exist (louloulin/claude-agent-sdk, jimmystridh/claude-agents-sdk, PandelisZ/claude-agent-sdk-rust) but none has sustained maintenance or community adoption |
| Scope drift in newer entrants | Crates published more recently under adjacent names (`claude-agent-sdk-rust`, `claude-agent-rs` ×2) solve different problems — one is a direct Anthropic API client (no CLI, no agent loop), another is a standalone coding agent, not a reusable SDK |
| Protocol reimplementation risk | Without a canonical port, every Rust project that wants Claude Code integration re-derives the subprocess/JSON protocol, the message type hierarchy, and the permission model independently — with no shared source of truth |
| Untapped Rust advantages | The Python SDK leans on runtime validation (Pydantic-style) and dynamic dispatch for message types and tool definitions. Rust's type system can enforce these invariants at compile time — an advantage no existing port fully claims |

## The Shift

claude-agent-toolkit aims to be the canonical, actively maintained, idiomatic Rust translation of the official Claude Agent SDK — not a thin FFI shim, not a reimplementation of Claude Code itself, but a faithful port that trades Python's runtime flexibility for Rust's compile-time guarantees wherever the two are equivalent in capability.

Where the upstream SDK validates message shapes and tool schemas at runtime, this SDK encodes them as Rust types. Where the upstream SDK uses `async`/`await` over asyncio, this SDK uses `async`/`await` over tokio. The porting discipline is: track upstream capability 1:1, express it in the idiom native to Rust.

## Thesis

**A faithful, idiomatic Rust port of the official Claude Agent SDK, maintained as a single source of truth, is worth more to the Rust ecosystem than another one-off wrapper.** The value is not novelty — it's fidelity to upstream plus the type-safety and performance Rust developers already expect from their tooling.

## Core Concepts

Mirrors the upstream SDK's shape, translated to Rust idiom (exact API surface TBD during design):

```
  QUERY (single-turn)
  Async, streaming
  ──────────────────────────────
  One-shot request → stream of
  typed messages back
  ▲
  │
  │  Both talk to the same
  │  underlying CLI subprocess
  │  over JSON message passing
  │
  CLIENT (multi-turn)
  Interactive, bidirectional
  ──────────────────────────────
  Stateful conversation,
  tool use loop, hooks
  │
  │
  ▼
  OPTIONS (configuration)
  ──────────────────────────────
  System prompt, working dir,
  allowed/disallowed tools,
  MCP servers, permission mode,
  hook registration
```

- **Query** — the async, single-turn entry point; returns a stream of typed messages
- **Client** — the interactive, multi-turn counterpart for stateful conversations and tool-use loops
- **Options** — the unified configuration object (system prompt, tools, MCP servers, permissions, hooks)
- **Message types** — the typed hierarchy the CLI protocol emits (assistant/user/system/result messages, text/tool-use/tool-result content blocks), encoded as Rust enums instead of runtime-validated Python classes
- **In-process tools** — custom tools defined as native Rust functions, exposed to Claude without external MCP subprocess overhead
- **Hooks** — deterministic interception points in the agent loop (e.g., pre-tool-use), for permission control and behavior shaping

## What It Is Not

- Not a reimplementation of Claude Code itself — it wraps the existing CLI, it does not replace it
- Not a direct Anthropic Messages API client — that problem is already well served by existing Rust API clients; this SDK's value is the Claude Code agent loop (tools, hooks, sessions, permissions), not raw completions
- Not a standalone coding agent or CLI product — it is a library other Rust programs depend on
- Not a partial or best-effort port — the goal is upstream capability parity, not a subset chosen for implementation convenience

## Naming

**claude-agent-toolkit** — plain and literal by design. Prior attempts at cleverness (`claude-agent-sdk`, `claude-agent`, `claude-agent-sdk-rust`, `claude-agent-rs`) were already taken by unrelated or narrower-scope projects by the time this one started. The name says exactly what the crate is: a toolkit for building Claude agents in Rust.
