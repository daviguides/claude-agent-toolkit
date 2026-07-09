# catk-sync — Vision

## Pain

`claude-agent-toolkit` is a faithful Rust port of a Python SDK that
keeps moving. Every upstream commit is a potential parity regression:
a new `ClaudeAgentOptions` field, a new message variant, a changed wire
shape. Nothing today notices when upstream moves — parity was audited
once, by hand, during initial implementation (`docs/sync/parity.yaml`,
pinned at `docs/plan/UPSTREAM-PIN.md`). Six months from now, upstream
will have drifted and nobody will know until a user hits a missing
field in production.

| Pain | Reality |
|------|---------|
| No drift detection | Upstream releases go unnoticed; parity silently rots |
| Manual re-audit is expensive | Re-reading `types.py`/`message_parser.py`/`query.py` end-to-end by hand doesn't scale as a recurring task |
| Semantic judgment required | Not every upstream diff matters — a docstring fix is noise, a new `PermissionResult` field is not. This needs an agent, not a `diff` |
| Trust but verify | An agent that proposes a port of itself must not also be the one who merges it |

## The Shift

`catk-sync` is a small, separate CLI that treats `claude-agent-toolkit`
as a maintenance target and uses `claude-agent-toolkit` itself as the
engine to analyze and (optionally) close the gap — the SDK maintains
itself, under human supervision at the one step that matters: merging
to `main`.

It is deliberately NOT part of the `claude-agent-toolkit` crate.
`vision.md` for the SDK is explicit: *"Not a standalone coding agent or
CLI product — it is a library other Rust programs depend on."*
`catk-sync` is exactly such a program: an ordinary consumer of the
published crate, living in its own repository, with its own release
cycle.

## Thesis

**Drift detection should be free and automatic; gap analysis should be
cheap and frequent; code changes should be proposed, never
self-approved.** Three different trust levels, three different
commands, one pipeline.

## Core Concepts

```
  CHECK                          ANALYZE                       APPLY
  deterministic, no LLM          read-only LLM session          write LLM session
  ──────────────────────         ──────────────────────         ──────────────────────
  git fetch + diff vs             diff + parity.yaml →           branch + TDD implementation
  pinned commit → drift           structured gap report          → parity.yaml update → PR
  y/n + touched files                                            (never touches main)
        │                               │                               │
        ▼                               ▼                               ▼
   exit 0 = clean              gaps.json (severity,             open PR, sync branch
   exit 1 = drift               upstream_ref, suggested                  │
                                 rust location)                          ▼
                                                                      REVIEW
                                                                 human-gated LLM session
                                                                 ──────────────────────
                                                                 re-run gate on PR branch +
                                                                 independent adversarial pass
                                                                 → approve/reject
                                                                 approve (+ confirm) → rebase
                                                                 onto main, merge, close loop
```

- **check** — pure git/filesystem, zero tokens. Fetches
  `reference/claude-agent-sdk-python`, compares `HEAD` against
  `docs/plan/UPSTREAM-PIN.md`, filters to source-relevant paths, prints
  a compact commit/file summary. This is what a cron job runs every
  day.
- **analyze** — a READ-ONLY `ClaudeClient` session (using
  `claude-agent-toolkit` itself — the dogfooding loop) that reads the
  upstream diff, `docs/sync/parity.yaml`, and the current Rust source,
  and produces a structured gap report: what's new, what changed shape,
  severity, suggested Rust location. This is the expensive-but-rare
  step — run when `check` finds drift, not on every commit.
- **apply** — a WRITE-capable session on a fresh branch, implementing
  gaps under this repo's own TDD discipline (kinhin: red → green,
  `cargo fmt`/`clippy`/`test`/`doc` gate before finishing), updating
  `parity.yaml`, and opening a PR. It never pushes to `main` and never
  merges its own work.
- **review** — the trust boundary. Re-verifies the PR branch
  independently (fresh checkout, full gate re-run, a second LLM pass
  with NO memory of `apply`'s reasoning, adversarially checking the
  diff against the gap report). Produces a verdict. On approval — which
  requires an explicit human confirmation unless `--yes` is passed —
  rebases the branch onto the latest `main` and merges. On rejection,
  comments with the rationale and leaves the PR for a human.
- **all** — runs `check → analyze → apply` in one command, stopping
  after the PR opens (the safe default). An explicit `--review` flag
  chains into the interactive `review` step for a true single-command
  loop, opted into per-run rather than assumed.

## What It Is Not

- Not a replacement for human code review — `review` is a stronger
  filter before a human looks, not a substitute for one; it still
  requires confirmation to merge by default
- Not a general-purpose "AI upgrades my dependency" tool — it is
  narrowly built for this one pairing (`claude-agent-toolkit` ↔
  `claude-agent-sdk-python`) and its one data contract
  (`docs/sync/parity.yaml`)
- Not part of the `claude-agent-toolkit` crate — it depends on the
  published crate like any other consumer
- Not autonomous by default on the one irreversible action (merging to
  `main`) — every other step can run unattended; this one asks first

## Naming

**catk-sync** — `catk` abbreviates `claude-agent-toolkit`, `sync` says
what it does. Short enough to type daily; the full crate name
(`catk-sync`) doubles as the binary name for the same reason.

## Dependency

`catk-sync` cannot be built before `claude-agent-toolkit` reaches at
least the interactive client (`ClaudeClient`, plan phase 7) — `analyze`
and `apply` are `ClaudeClient` sessions. Its implementation plan
(`docs/plan-sync-cli/`) is written now and executed later, once the SDK
port is far enough along.
