# Changelog

All notable changes to this project are documented here.

## 0.1.0

Initial release: an idiomatic Rust port of the official
[Claude Agent SDK](https://github.com/anthropics/claude-agent-sdk-python),
wrapping the `claude` CLI subprocess protocol.

### Added

- **One-shot query API**: `query()` and `query_stream()` (streaming
  input), yielding typed `Message`s.
- **Interactive multi-turn client**: `ClaudeClient` — connect, send,
  receive responses, interrupt, change permission mode/model
  mid-session, rewind file checkpoints, manage MCP servers, all backed
  by the same control-protocol actor.
- **Full `ClaudeAgentOptions` surface**: model selection, system
  prompt (custom/preset/file), tool allow/deny lists, permission mode,
  sandboxing, thinking budget, session stores, task budgets, plugins,
  and the full CLI argument-building logic this implies.
- **Typed message model**: `User`/`Assistant`/`System`/`Result`
  messages, task lifecycle messages, hook events, rate-limit events,
  and every content block variant (text, thinking, tool use/result,
  server tool use/result).
- **`can_use_tool` permission callback**: async Rust closures deciding
  whether a tool call proceeds, with full context (tool name, input,
  suggestions, agent id, blocked path, decision reason, etc.).
- **Lifecycle hooks**: register callbacks for all 10 upstream hook
  events (`PreToolUse`, `PostToolUse`, `PostToolUseFailure`,
  `UserPromptSubmit`, `Stop`, `SubagentStop`, `PreCompact`,
  `Notification`, `SubagentStart`, `PermissionRequest`).
- **In-process MCP tools**: `tool()` + `create_sdk_mcp_server()` — define
  tools as native async Rust functions, exposed to Claude without an
  external MCP subprocess.
- **External MCP servers**: stdio/SSE/streamable-HTTP server configs.
- Subprocess transport with CLI auto-discovery, bounded stderr
  ring-buffer diagnostics, and graceful shutdown.

### Notes

- Every deliberate deviation from the upstream Python SDK's behavior —
  and every point where this port's behavior was verified line-by-line
  against the pinned upstream reference — is recorded in
  [`docs/plan/DEVIATIONS.md`](docs/plan/DEVIATIONS.md).
- [`docs/sync/PARITY.md`](docs/sync/PARITY.md) is the generated
  upstream↔Rust symbol parity table.
