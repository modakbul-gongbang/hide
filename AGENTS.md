# Agent Notes

## Repository Layout

- `macos/` - the production macOS application: a SwiftUI shell that renders the core snapshot and dispatches typed events back. Build and sign it with `macos/scripts/build_dev_app.sh`.
- `herdr-core/` - platform-neutral Rust runtime and the six-function C ABI (`herdr-core/include/herdr_core.h`) the shell links against. All authority (pane layout, focus, zoom, persisted state) lives here.
- `src/` - removed retired Rust-native shell. The SSH/mini runtime is owned by `herdr-core/`; nothing links a root `src/` crate into the application.
- `spikes/swift-shell-pivot/` - the Stage 0 spike source and its `VERDICTS.md`. A frozen record; do not edit it to reflect later changes. Its evidence output is no longer kept in the repository (see `Evidence Belongs Outside The Repository`).

## Evidence Belongs Outside The Repository

Screenshots, traces, sample output, run logs, browser profiles, and verification transcripts are run artifacts, not source. They do not belong in a commit.

This rule exists because they were: `docs/verification/`, `docs/screenshots/`, and the spike `evidence/` directories grew to 660 files and 123 MB, and a Chrome profile committed under `spikes/integrated-preflight/` carried cookies and a symlink naming the workstation. A later scan for leaked identity passed because it read text and skipped images, while 184 screenshots showed the home directory and hostname in plain sight.

- Write run artifacts under `agents/runs/<slug>/`. That whole namespace is local-only, so nothing there can reach a commit by accident.
- Never add a path under `docs/verification/`, `docs/screenshots/`, or `spikes/*/evidence/`. They are gitignored; do not force past it.
- When a document needs to cite evidence, state the finding and how it was measured. Do not commit the artifact so a path can be linked.
- A verification claim is proven to the person reading the run, not to the repository. The receipt and the run directory are where it lives.

## Runtime Architecture

The core (`herdr-core`) owns all state behind one `Mutex<Runtime>`.
The shell dispatches typed JSON events in (`herdr_core_dispatch`) and pulls state out (`herdr_core_snapshot`) whenever the change notifier fires.
The event sync coordinator (`session_sync.rs`) bootstraps from `session.snapshot`, resumes ordered topology updates through `events.subscribe`, and refreshes agent telemetry with `agent.list` once per second.
Per-pane attach threads stream PTY bytes into the runtime as terminal chunks.
Everything the shell renders comes from that one snapshot pull; the shell holds no authority.

## Herdr API Contract

Before changing, debugging, or reviewing any Herdr integration, read both current official references in full for the Herdr version this repository ships or targets:

- [CLI reference](https://herdr.dev/docs/cli-reference/)
- [Socket API](https://herdr.dev/docs/socket-api/)

This repository uses both layers, and they are not separate backends: the Herdr CLI is a wrapper over the same local socket API.
The local live runtime is primarily a raw socket client: `herdr-core/src/herdr_api.rs`, `herdr-core/src/session_sync.rs`, and `herdr-core/src/live.rs` send newline-delimited JSON methods such as `session.snapshot`, `events.subscribe`, `agent.list`, `pane.layout`, `pane.focus`, and `pane.resize` over the Unix socket.
CLI wrappers are used where Herdr owns higher-level or streaming behavior, including terminal control/observe sessions, CLI-owned pane commands, remote SSH snapshot/attach commands, and contract diagnostics.

Follow the official layer boundary when adding behavior:

- Use CLI wrappers for shell scripts, simple orchestration, human debugging, and portable plugin commands.
- Use the raw socket API only for custom-client request/response control or long-lived event subscriptions.
- Do not guess method names, parameters, response fields, or protocol compatibility from existing call sites alone.
  Check the target binary with `herdr --version` and `herdr api schema --json`, then compare it with `contracts/herdr-api.schema.json` through `scripts/check-herdr-contract.sh` before relying on new behavior.

## Performance Guide

These rules exist because each one was violated and diagnosed in a real incident (2026-08-30 typing-lag session: main thread spent 47% of wall time waiting on the runtime mutex).

- Never hold the runtime mutex across a subprocess, blocking I/O, or a large serialization.
  Every shell snapshot read and every attach thread blocks on that mutex; whatever you hold it through becomes UI latency.
  Precompute outside the lock and pass results in (see `PrecomputedCatalog` in `session_sync.rs`).
- Never fork subprocesses (git especially) in a per-tick or per-event path.
  The workspace catalog caches by input equality plus a refresh window (`CatalogCache` in `session_sync.rs`); extend that cache rather than adding a new per-tick invocation.
- The snapshot wire is sized by what changed, not by total state.
  Terminal chunks ride a sequence cursor; do not re-send retained state wholesale.
  When adding a snapshot field, decide its channel: rarely-changing sections belong in the revisioned `rest`, per-event scalars ride top-level, high-volume streams need their own cursor.
- Verify performance claims with `/usr/bin/sample <pid>` on the running app and `herdr server`, not by reading code.
  Before sampling, confirm exactly one app instance is running and know whether it is the dev build or an installed bundle (rule FACT-dev-runtime-instances).
  Ambient load (Screen Sharing, WindowServer, a stale second instance) routinely masquerades as app slowness; rule it out first.

<!-- harness:agents-namespace:start -->
## Harness Namespace (`agents/`)

This project uses the engineering-harness PRD pipeline. Agent-facing assets live in one visible namespace.

**`agents/` is local-only by default, and `agents/prd/<slug>/prd.md` is the one exception.** `.gitignore` carries one anchored line, `/agents/`, which ignores the top-level harness namespace and nothing else, so nothing under it is committed unless someone adds it deliberately. The anchor matters: the unanchored form also matched `.claude/agents/`, which silently made a committed subagent definition uncommittable. A PRD is the approved contract a reviewer reads to judge the change, so it is committed with `git add -f`; interview logs, rules, run state, and every run artifact stay on the machine that produced them.

Because the ignore rule does not know about the exception, a new PRD is committed only when someone remembers the `-f`. Check `git ls-files agents/` before claiming a PRD is shared.

Nothing else under `agents/` may be force-added. In particular, never force-add a run directory to make an evidence path linkable; see `Evidence Belongs Outside The Repository`.

- `agents/prd/` - PRD contracts, human-approved before implementation. Committed.
- `agents/interview/` - interview sources (`qa-log.md`), the canonical record behind a PRD.
- `agents/rules/` - learned rules: `INDEX.md` is the ledger, `invariants/` hold machine-checked rules (trigger globs + executable check) that gate delivery, `pending/` holds lessons that have not landed yet.
- `agents/runs/` - per-run state and evidence (gate verdicts + implement state under one `agents/runs/<slug>/`), never hand-edited. This is also where run artifacts go; see `Evidence Belongs Outside The Repository`.
- `agents/quick/` - quick lane state and evidence (generated contract, receipt, verify verdict, evidence blobs), never hand-edited.
- `agents/config.json` - pipeline configuration.

Conventions:

- AGENTS.md is the main agent context file; CLAUDE.md is always a symlink to it.
- Before planning work that touches files matched by a rule trigger, consult `node ~/projects/sasu/cli/dist/cli.js rules relevant --paths <files>` or read `agents/rules/INDEX.md`.
- Rules are added through `rules add` (never hand-edit the ledger); every rule cites evidence from a real run or incident.
<!-- harness:agents-namespace:end -->

## Design Reference

Read `DESIGN.md` before changing any surface a user looks at, and design against its tokens instead of choosing values at the call site.

It is the design source of truth for the macOS shell.
The system is a single dark mode with a four-step surface ladder, hairline 1px borders and no drop shadows, Inter with the `ss03` stylistic set, a radius scale running from 6px keycaps to 16px containers, and a spacing system the layout follows.
Saturated accent colors belong to category illustration, never to chrome.

`HideTheme` in `macos/Sources/HerdrMacOS/HideUI.swift` carries those tokens into the shell, so a new color, radius, or spacing value is added there and used from there rather than written inline.
When the existing system does not cover a case, say so and propose the addition; do not settle it with a one-off value in a view.

`DESIGN.md` also records the Raycast public design references and their MIT attribution context.
