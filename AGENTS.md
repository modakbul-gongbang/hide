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
The shell dispatches typed JSON events in (`herdr_core_dispatch`) and pulls state out (`herdr_core_snapshot`) when the change notifier announces.
The event sync coordinator (`session_sync.rs`) bootstraps from `session.snapshot`, resumes ordered topology updates through `events.subscribe`, and refreshes agent telemetry with `agent.list` once per second.
A tick whose `agent.list` is unchanged publishes nothing, so an idle session recomputes no projection; the catalog's own refresh window still publishes, because the rebuild can only happen inside `publish_replica`.
Per-pane attach threads stream PTY bytes into the runtime as terminal chunks.
Everything the shell renders comes from that one snapshot pull.

The shell holds no authority, but the core does not hand all of it to Herdr either.
Herdr owns pane existence, split geometry, zoom, cwd, agent lifecycle and the PTY; the core owns each checkout's visible tab, the keyboard focus pane, panel visibility and text scale.
A core-owned value changes on the event that asked for it and Herdr is told afterwards, so the canvas and the focus ring never wait for a round trip.
While that notification is pending, the Herdr workspace that owns the target showing it is read as its confirmation, whichever workspace holds Herdr's keyboard, because a checkout is keyed by path and can hold tabs from several Herdr workspaces; with nothing pending, a move of Herdr's focused tab or pane to another value is followed and a diagnostic records the ids and the origin; a refusal or a timeout keeps the core's value and says so.
A non-focused workspace's active tab is that workspace's memory, never a focus to follow: folding every workspace's active tab into one value per checkout let the last one overwrite the rest, and every tab focus on the other workspace timed out and snapped back.
The focused checkout always draws the tab that holds the selected pane: a tab action moves the pane into the tab, and a pane action, a restore, or a retirement moves the tab to the pane (`align_visible_tab_with_selected_pane`).
The pending model covers the visible tab and the focused pane and nothing else: zoom, splits, closes and resizes still wait for Herdr, because their geometry decides the PTY size (commit 9570a2a).

The notifier announces once per burst rather than once per change.
`herdr_core_snapshot` clears the announcement flag **before** it takes the lock; clearing it after the read would swallow a change that landed during the read.
Launch creates the core once, after the runtime resolution (login-shell PATH, binary, version) has finished, and the first window is presented before that resolution completes.
The first attach uses the size the view reported, or the size persisted from the last launch, and never a placeholder; a pane with no known size is held back and says it is waiting.

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

The bundled Herdr release is pinned in one place, `macos/Sources/HerdrMacOS/Resources/herdr-bundle.json`, and `contracts/herdr-api.schema.json` is derived from it: it is what that exact binary answers to `api schema --json`, never a copy from a Herdr checkout.
The app runs the Herdr it bundles: `HerdrRuntimeResolver` verifies the bundled binary against the manifest digest and starts it on the default socket when no server is running there; a server that is already running is joined as it is when its protocol matches, and refused with the two revisions and the `herdr server stop` remedy when it does not.
There is no installed-CLI candidate list and no version floor; the pin is exact.
The Swift shell reads the manifest at launch, and `scripts/fetch-herdr-runtime.sh` downloads and verifies the asset against it for both `scripts/build-app.sh` and `macos/scripts/build_dev_app.sh`; `scripts/check-herdr-pin-single-source.sh` fails when any of those restates the value.
Move the pin with `scripts/bump-herdr.sh <release-tag>` (a stable `v0.8.3` or a `preview-...` tag), which verifies the asset, writes the contract that binary reports, and rewrites the tag, version and digest tokens in the README, install guide and third-party notice.
`.github/workflows/herdr-update.yml` polls for a new stable release weekly and opens a PR with that bump after running both test suites; it never merges, because the core's Herdr behavior assumptions are only asserted against fixtures this repository wrote.

## Performance Guide

These rules exist because each one was violated and diagnosed in a real incident.

The figure to measure against is not one number.
On 2026-09-04, on the assembled dev bundle against a live server with 26 panes and 12 agents, the main thread spent 0.28% of its samples waiting on the runtime mutex while idle at load 4.9, and 1.52% while driven at load 11.4.
The same measurements read 0.89% idle before any of that day's work and 4.31% driven at `fc80f6c`, before the notifier and the delta boundary changed.
Quote a mutex-wait figure with the load and the drive it was taken under or it means nothing; the 47% an earlier note carried was measured while typing, on a build three rounds of work ago, and comparing anything to it is a mistake.

- Never hold the runtime mutex across a subprocess, blocking I/O, or a large serialization.
  Every shell snapshot read and every attach thread blocks on that mutex; whatever you hold it through becomes UI latency.
  Precompute outside the lock and pass results in (see `PrecomputedCatalog` in `session_sync.rs`).
  The snapshot delta is the worked example: `Runtime::snapshot_delta_payload` takes an owned payload under the lock and the free `runtime::serialize_snapshot_delta` writes the bytes outside it.
  The signature is the enforcement, because the serializing half has no runtime in scope to lock; keep it that way rather than adding a convenience method that does both.
- Never fork subprocesses (git especially) in a per-tick or per-event path.
  The workspace catalog caches by input equality plus a refresh window (`CatalogCache` in `session_sync.rs`); extend that cache rather than adding a new per-tick invocation.
- The snapshot wire is sized by what changed, not by total state.
  Terminal chunks ride a sequence cursor; do not re-send retained state wholesale.
  When adding a snapshot field, decide its channel: rarely-changing sections belong in the revisioned `rest`, per-event scalars ride top-level, high-volume streams need their own cursor.
  A field on the revisioned `rest` section that no reader reads still costs a full-state re-send on every tick that writes it.
  Two `last_checked_at_unix_ms` fields nobody decoded restamped `rest` on every session heartbeat, which re-sent the whole navigator, ui state, status and pet about once a second; deleting them took an idle twenty-second window from 38 snapshot reads to one.
- Announce changes once per burst, not once per change.
  `ChangeNotifier` latches on the false-to-true flip and `herdr_core_snapshot` clears the latch before it takes the lock.
  Clear-then-read costs at most one read for nothing; read-then-clear loses a change that lands during the read.
- Verify performance claims with `/usr/bin/sample <pid>` on the running app and `herdr server`, not by reading code.
  Before sampling, confirm exactly one app instance is running and know whether it is the dev build or an installed bundle (rule FACT-dev-runtime-instances).
  Ambient load (Screen Sharing, WindowServer, a stale second instance) routinely masquerades as app slowness; rule it out first.
  Above roughly load 14 this machine stops symbolicating a `sample` window longer than about five seconds and returns every frame as `???`, which a summing script reads as zero time rather than as no answer.
  Take the observation as several short windows and check how many failed to symbolicate before believing any ratio.
  A latency claim about the operator's own input cannot come from a synthetic keystroke: the `osascript` call alone costs about 123 ms before the app is involved, so read the interval between the shell's own trace marks instead.

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
