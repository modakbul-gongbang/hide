# Agent Notes

## Repository Layout

- `macos/` - the production macOS application: a SwiftUI shell that renders the core snapshot and dispatches typed events back. Build and sign it with `macos/scripts/build_dev_app.sh`.
- `herdr-core/` - platform-neutral Rust runtime and the six-function C ABI (`herdr-core/include/herdr_core.h`) the shell links against. All authority (pane layout, focus, zoom, persisted state) lives here.
- `src/` - removed retired Rust-native shell. The SSH/mini runtime is owned by `herdr-core/`; nothing links a root `src/` crate into the application.
- `spikes/swift-shell-pivot/` - the Stage 0 spike, its evidence, and `VERDICTS.md`. A frozen record; do not edit it to reflect later changes.

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
