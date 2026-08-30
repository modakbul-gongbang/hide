# Agent Notes

## Repository Layout

- `macos/` - the production macOS application: a SwiftUI shell that renders the core snapshot and dispatches typed events back. Build and sign it with `macos/scripts/build_dev_app.sh`.
- `herdr-core/` - platform-neutral Rust runtime and the six-function C ABI (`herdr-core/include/herdr_core.h`) the shell links against. All authority (pane layout, focus, zoom, persisted state) lives here.
- `src/` - removed retired Rust-native shell. The SSH/mini runtime is owned by `herdr-core/`; nothing links a root `src/` crate into the application.
- `spikes/swift-shell-pivot/` - the Stage 0 spike, its evidence, and `VERDICTS.md`. A frozen record; do not edit it to reflect later changes.

## Runtime Architecture

The core (`herdr-core`) owns all state behind one `Mutex<Runtime>`.
The shell dispatches typed JSON events in (`herdr_core_dispatch`) and pulls state out (`herdr_core_snapshot`) whenever the change notifier fires.
A background session poller (`live.rs`) polls the herdr server socket once per second and ingests the result; per-pane attach threads stream PTY bytes into the runtime as terminal chunks.
Everything the shell renders comes from that one snapshot pull; the shell holds no authority.

## Performance Guide

These rules exist because each one was violated and diagnosed in a real incident (2026-08-30 typing-lag session: main thread spent 47% of wall time waiting on the runtime mutex).

- Never hold the runtime mutex across a subprocess, blocking I/O, or a large serialization.
  Every shell snapshot read and every attach thread blocks on that mutex; whatever you hold it through becomes UI latency.
  Precompute outside the lock and pass results in (see the session poller's `PrecomputedCatalog` in `live.rs`).
- Never fork subprocesses (git especially) in a per-tick or per-event path.
  The workspace catalog caches by input equality plus a refresh window (`CatalogCache` in `live.rs`); extend that cache rather than adding a new per-tick invocation.
- The snapshot wire is sized by what changed, not by total state.
  Terminal chunks ride a sequence cursor; do not re-send retained state wholesale.
  When adding a snapshot field, decide its channel: rarely-changing sections belong in the revisioned `rest`, per-event scalars ride top-level, high-volume streams need their own cursor.
- Verify performance claims with `/usr/bin/sample <pid>` on the running app and `herdr server`, not by reading code.
  Before sampling, confirm exactly one app instance is running and know whether it is the dev build or an installed bundle (rule FACT-dev-runtime-instances).
  Ambient load (Screen Sharing, WindowServer, a stale second instance) routinely masquerades as app slowness; rule it out first.

<!-- harness:agents-namespace:start -->
## Harness Namespace (`agents/`)

This project uses the engineering-harness PRD pipeline. Agent-facing assets live in one visible namespace:

- `agents/prd/` - PRD contracts, committed and human-approved before implementation.
- `agents/rules/` - learned rules: `INDEX.md` is the ledger, `invariants/` hold machine-checked rules (trigger globs + executable check) that gate delivery, `pending/` holds lessons that have not landed yet.
- `agents/runs/` - per-run state and evidence (gate verdicts + implement state under one `agents/runs/<slug>/`), gitignored (policy: one line `agents/runs/`), never hand-edited.
- `agents/quick/` - quick lane state and evidence (generated contract, receipt, verify verdict, evidence blobs), gitignored (policy: one line `agents/quick/`), never hand-edited.
- `agents/config.json` - pipeline configuration, committed.

Conventions:

- AGENTS.md is the main agent context file; CLAUDE.md is always a symlink to it.
- Before planning work that touches files matched by a rule trigger, consult `node ~/projects/sasu/cli/dist/cli.js rules relevant --paths <files>` or read `agents/rules/INDEX.md`.
- Rules are added through `rules add` (never hand-edit the ledger); every rule cites evidence from a real run or incident.
<!-- harness:agents-namespace:end -->

## Design Reference

- `DESIGN.md` records the Raycast public design references, MIT attribution context, and the `HideTheme` token source used by the macOS shell.
