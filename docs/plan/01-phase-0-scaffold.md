# Phase 0 — Scaffold, Toolchain, CI, Upstream Reference

**Objective**: an empty but fully buildable, lint-clean crate with CI,
plus a local clone of the upstream Python SDK for consultation.

**No SDK logic is written in this phase.**

## Step 0.1 — Initialize the crate

Run in the repo root (the git repo already exists — do NOT `cargo new`,
use `cargo init` which keeps existing files):

```bash
cargo init --lib --name claude-agent-toolkit
```

Then replace the generated `Cargo.toml` with exactly:

```toml
[package]
name = "claude-agent-toolkit"
version = "0.1.0"
edition = "2024"
rust-version = "1.85"
description = "Idiomatic Rust port of the official Claude Agent SDK — build AI agents on the Claude Code CLI"
license = "MIT"
repository = "https://github.com/daviguides/claude-agent-toolkit"
keywords = ["claude", "anthropic", "agent", "ai", "sdk"]
categories = ["api-bindings", "asynchronous"]

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

[lints.rust]
unsafe_code = "forbid"
missing_docs = "warn"

[lints.clippy]
all = { level = "deny", priority = -1 }
pedantic = "warn"
```

Note: `license = "MIT"` — README says TBD; MIT matches the upstream
SDK's spirit of permissiveness. Record this choice in
`docs/plan/DEVIATIONS.md` if the owner later wants Apache-2.0
dual-licensing.

Replace generated `src/lib.rs` with:

```rust
//! Idiomatic Rust port of the official Claude Agent SDK.
//!
//! Wraps the Claude Code CLI as a subprocess and exposes a typed,
//! async API for one-shot queries and interactive agent sessions.
```

## Step 0.2 — Pin the toolchain

Create `rust-toolchain.toml`:

```toml
[toolchain]
channel = "stable"
components = ["rustfmt", "clippy"]
```

## Step 0.3 — Update .gitignore

Append to the existing `.gitignore` (do not delete existing lines):

```
/target
/reference
Cargo.lock
```

Rationale: `Cargo.lock` is ignored because this is a library crate.
`reference/` holds the upstream clone and must never be committed.

## Step 0.4 — Upstream reference script

Create `scripts/fetch-reference.sh`:

```bash
#!/usr/bin/env bash
# Clones the upstream Python SDK (source of truth for the wire
# protocol) into reference/ for local consultation. Never committed.
set -euo pipefail

REFERENCE_DIR="$(cd "$(dirname "$0")/.." && pwd)/reference"
UPSTREAM_URL="https://github.com/anthropics/claude-agent-sdk-python.git"

mkdir -p "${REFERENCE_DIR}"

if [ -d "${REFERENCE_DIR}/claude-agent-sdk-python/.git" ]; then
    git -C "${REFERENCE_DIR}/claude-agent-sdk-python" pull --ff-only
else
    git clone --depth 1 "${UPSTREAM_URL}" "${REFERENCE_DIR}/claude-agent-sdk-python"
fi

echo "Upstream reference ready at ${REFERENCE_DIR}/claude-agent-sdk-python"
```

Then:

```bash
chmod +x scripts/fetch-reference.sh
./scripts/fetch-reference.sh
```

**MANDATORY**: after cloning, read these upstream files end-to-end and
keep them open as reference for later phases:

- `reference/claude-agent-sdk-python/src/claude_agent_sdk/types.py`
- `reference/claude-agent-sdk-python/src/claude_agent_sdk/_errors.py`
- `reference/claude-agent-sdk-python/src/claude_agent_sdk/_internal/transport/subprocess_cli.py`
- `reference/claude-agent-sdk-python/src/claude_agent_sdk/_internal/message_parser.py`
- `reference/claude-agent-sdk-python/src/claude_agent_sdk/_internal/query.py`

Also record the upstream commit hash you cloned:

```bash
git -C reference/claude-agent-sdk-python rev-parse HEAD
```

Write that hash into a new file `docs/plan/UPSTREAM-PIN.md` with one
line: `Ported against upstream commit: <hash> (<date>)`. Every later
phase that says `⚠️ VERIFY` means: check against THIS pinned clone.

## Step 0.5 — CI workflow

Create `.github/workflows/ci.yml`:

```yaml
name: CI

on:
  push:
    branches: [main]
  pull_request:

env:
  CARGO_TERM_COLOR: always

jobs:
  check:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: rustfmt, clippy
      - uses: Swatinem/rust-cache@v2
      - name: Format
        run: cargo fmt --check
      - name: Clippy
        run: cargo clippy --all-targets -- -D warnings
      - name: Test
        run: cargo test
      - name: Docs
        run: cargo doc --no-deps
        env:
          RUSTDOCFLAGS: -D warnings
```

## Acceptance Gate (all must pass)

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test          # zero tests is OK at this phase, must still exit 0
cargo doc --no-deps
test -d reference/claude-agent-sdk-python/src/claude_agent_sdk  # upstream present
```

## Commits for this phase

1. `phase-0: cargo scaffold with lints and pinned toolchain`
2. `phase-0: gitignore target, lockfile, reference dir`
3. `phase-0: upstream reference fetch script + pinned commit doc`
4. `phase-0: github actions CI`
