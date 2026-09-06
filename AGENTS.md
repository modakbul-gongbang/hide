# Agent Notes

## Repository Layout

- `macos/` - the production macOS application: a SwiftUI shell that renders the core snapshot and dispatches typed events back. Build and sign it with `macos/scripts/build_dev_app.sh`.
- `herdr-core/` - platform-neutral Rust runtime and the six-function C ABI (`herdr-core/include/herdr_core.h`) the shell links against. All authority (pane layout, focus, zoom, persisted state) lives here.
- `src/` - removed retired Rust-native shell. The SSH/mini runtime is owned by `herdr-core/`; nothing links a root `src/` crate into the application.
- `spikes/swift-shell-pivot/` - the Stage 0 spike source and its `VERDICTS.md`. A frozen record; do not edit it to reflect later changes. Its evidence output is no longer kept in the repository (see `Evidence Belongs Outside The Repository`).

## Before Opening A Pull Request

`main` takes squash merges through pull requests only, and the `verify` workflow (`.github/workflows/pr.yml`) has to pass; no one, maintainer included, can push around it.
Run the lanes it runs before opening the pull request; `CONTRIBUTING.md` lists every gate with its local command, what it protects, and what to do when it blocks.
A gate that is wrong is changed in the same pull request with the reason in the description; there is no bypass label.
The pull request template asks five questions about the runtime mutex, the snapshot wire, Herdr versus core ownership, the API contract, and the failure path; answer them from the diff, not from intent.

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
The Git section refreshes local worktree state only when repository metadata, tracked paths, or Herdr worktree topology changes; disk usage and pull requests refresh only when the section opens or its header refresh is pressed, and all three layers run outside the runtime mutex.
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

A clicked path is one event, not a sequence.
The shell resolves the token on the filesystem, decides which registered checkout owns it by the longest symlink-resolved prefix, and sends `reveal_path`; the core then decides the focused checkout, the right panel's visibility and section, the tree's expanded set and selection, and the editor tab together.
Dispatch is fire-and-forget, so four separate events would arrive as four frames and a refusal partway would leave the screen half moved.
A path outside every checkout never reaches the core: the shell hands it to macOS, opening a file in its default application and a folder as a Finder window, and revealing rather than opening anything whose default application is the operating system running it - an executable file, an application bundle, an installer package - because link detection is a guess over arbitrary agent output and one wrong click must not start a program.

An attach lives only while its tab is in the last five shown.
Herdr renders a pane for every attached client, so an attach nobody is looking at costs a child process here and a render there for the life of the process; visiting eight tabs used to leave eight attaches alive.
The core keeps the most recently shown tabs (`ATTACHED_TAB_LIMIT`) and releases the rest, which is the ordinary session drop, not a new path.
A released pane keeps its projection entry carrying the transport state `released`, because the sidebar and the pane header read their state from there and a missing entry reads as a failure; the shell drops that pane's canvas and its held bytes on that state, so the tab redraws from Herdr's own frame on the next visit.
Nothing re-attaches it until it is shown again: an idle tick attaches only the visible tab's panes.

Tab reorder ownership is decided per drag, not per checkout.
Herdr orders the tabs inside one of its workspaces and has no order that spans two of them, so a strip slot is refilled from the workspace that slot already belongs to and Hide owns how the workspaces and the file tabs interleave.
A drag that changes the moved tab's own workspace subsequence sends one `tab.move` with an index counted in that workspace; a drag that only steps over another workspace's tabs settles locally with no Herdr call.
Deciding this for the whole checkout is what refused every drag in a checkout two Herdr workspaces share, which is the ordinary arrangement for a repository opened twice.

A pane that is going away ends its attach quietly.
Herdr closes the PTY before it reports the pane gone, so the attach child ends while the pane is still drawn; projecting that as `ended` is what flashed "terminal attach ended" over a pane the operator had just closed.
A close Hide asked for, or a pane Herdr has already stopped listing, projects `closing` with no notice chunk and keeps the pane's last frame until it is removed. Every other reason still reports `ended` with its message.

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
`herdr-core/build.rs` turns the five sub-schemas into Rust modules under `herdr_contract::wire` at build time; generated source stays in `OUT_DIR` and is never committed.
`herdr-core/src/wire.rs` is the only boundary that converts generated values into the core's projection and event inputs and builds generated subscription parameters.
Do not write new wire deserialization structs in `session_sync.rs` or import generated types into domain, runtime or sidebar code.
The pinned event schema currently omits protocol, host and sequence: only the boundary's minimal metadata envelope is handwritten, and its schema-gap test requires deletion when the fork declares those fields.
Request envelopes still name their method explicitly because generation does not discriminate method constants; use generated parameter types inside them.
`live.rs` and `remote.rs` also use this boundary for response decoding and generated request parameters.
The boundary preserves remote protocol diagnostics before decoding the complete generated snapshot, and the isolated pinned-server probe checks the control responses and CLI-created agent envelope.
Terminal input, scroll, resize and release messages and the parameterless snapshot request remain boundary-owned schema gaps, with tests that require migration when their parameter types appear.

The app runs the Herdr it bundles: `HerdrRuntimeResolver` verifies the bundled binary against the manifest digest and starts it on the default socket when no server is running there; a server that is already running is joined as it is when its protocol matches, and refused with the two revisions and the `herdr server stop` remedy when it does not.
There is no installed-CLI candidate list and no version floor; the pin is exact.
The Swift shell reads the manifest at launch, and `scripts/fetch-herdr-runtime.sh` downloads and verifies the asset against it for both `scripts/build-app.sh` and `macos/scripts/build_dev_app.sh`; `scripts/check-herdr-pin-single-source.sh` fails when any of those restates the value.
Move the pin with `scripts/bump-herdr.sh <release-tag>` (a stable `v0.8.3` or a `preview-...` tag), which verifies the asset, writes the contract that binary reports, and rewrites the tag, version and digest tokens in the README, install guide and third-party notice.
`.github/workflows/herdr-update.yml` polls for a new stable release weekly and opens a PR with that bump after running both test suites; it never merges, because the core's Herdr behavior assumptions are only asserted against fixtures this repository wrote.

<!-- herdr-provenance:start -->
hide distributes a modified Herdr preview from the [modakbul-gongbang/herdr fork](https://github.com/modakbul-gongbang/herdr/releases/tag/preview-2026-09-06-13d8d0b99033), built from commit `13d8d0b99033`.
This fork supplies host-scoped snapshots, ordered event sequences, and agent lineage that the upstream stable release does not yet expose.
The weekly `herdr-update.yml` workflow continues to propose upstream stable releases with `--repo herdrdev/herdr`; return to upstream when the contract field tests and runtime checks pass.
<!-- herdr-provenance:end -->

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
  The catalog was not the only fork: placing each tab into a checkout resolved the pane directory's repository root with `git rev-parse` inside `reconcile_session_catalog`, once per tab per publish, under the runtime mutex.
  On 2026-09-06 with 18 agents and load 7 to 11 that held the mutex for 60% of a five-second window; the main thread waited on it for 33% of its samples and every attach reader for 25% to 35%, which is what "everything is slower than the herdr TUI" felt like.
  The roots now ride the precomputed catalog (`RootIndex`), a stale precomputation keeps the last accepted catalog instead of rebuilding under the lock, and `reconciling_with_a_precomputed_catalog_runs_no_git` counts the forks.
- The snapshot wire is sized by what changed, not by total state.
  Terminal chunks ride a sequence cursor; do not re-send retained state wholesale.
  When adding a snapshot field, decide its channel: rarely-changing sections belong in the revisioned `rest`, per-event scalars ride top-level, high-volume streams need their own cursor.
  A field on the revisioned `rest` section that no reader reads still costs a full-state re-send on every tick that writes it.
  Two `last_checked_at_unix_ms` fields nobody decoded restamped `rest` on every session heartbeat, which re-sent the whole navigator, ui state, status and pet about once a second; deleting them took an idle twenty-second window from 38 snapshot reads to one.
- Send the first wheel immediately and coalesce only while a response is pending.
  The old fixed 16 ms window delayed even one wheel before the server round trip.
  `PendingScroll` now writes the first movement immediately and sums later signed rows until a frame arrives; a cancelling sum writes nothing.
  The stream has no request acknowledgement, so the next frame releases the accumulated movement.
  A 100 ms response timeout prevents a boundary or an ignored wheel from holding the next movement forever; it never delays the first wheel or keyboard input.
  Herdr 0.8.2 routes `terminal.scroll` between application mouse input and host history, so include the actual zero-based pointer cell and modifiers.
  Rendered frames do not carry the application's mouse-tracking mode.
  Ordinary clicks use the explicitly accepted matching-pane `agent_kind == claude` policy, record the detection basis, and send an SGR press/release without Enter; other or undetected panes retain local selection.
  Do not recreate local scrollback from viewport frames: rows can disappear between server renders, and application mouse state is unavailable in this contract.
  A scroll response already publishes a frame, so do not append a same-size resize to force a repaint.
  A wheel without a reported view size writes nothing and emits its diagnostic once; never invent fallback geometry.
- Settle geometry and pace rendering with the view's display link.
  Two stable display ticks publish the final grid; transient reports only update the frame guard.
  Attach sends the current settled size, and only matching full frames replace a held canvas after a geometry or control transition.
  Parse incoming data immediately, draw each visible pane at most once per display tick, and leave hidden panes undrawn.
  Keyboard bytes go directly from the main-actor delegate to the core writer; do not add an asynchronous main-actor hop.
- Announce changes once per burst, not once per change.
  `ChangeNotifier` latches on the false-to-true flip and `herdr_core_snapshot` clears the latch before it takes the lock.
  Clear-then-read costs at most one read for nothing; read-then-clear loses a change that lands during the read.
- Verify performance claims with `/usr/bin/sample <pid>` on the running app and `herdr server`, not by reading code.
  Before sampling, confirm exactly one app instance is running and know whether it is the dev build or an installed bundle (rule FACT-dev-runtime-instances).
  Ambient load (Screen Sharing, WindowServer, a stale second instance) routinely masquerades as app slowness; rule it out first.
  Above roughly load 14 this machine stops symbolicating a `sample` window longer than about five seconds and returns every frame as `???`, which a summing script reads as zero time rather than as no answer.
  Take the observation as several short windows and check how many failed to symbolicate before believing any ratio.
  A latency claim about the operator's own input cannot come from a synthetic keystroke: the `osascript` call alone costs about 123 ms before the app is involved, so read the interval between the shell's own trace marks instead.

Run native verification against an isolated Herdr server when the operator's instance is running.
A shared server also shares focus, so a separate app state file and fixture-only intent do not stop Hide from following the operator into a protected workspace.
Use the documented `HERDR_SESSION` and `HERDR_SOCKET_PATH` routing, a separate `HERDR_CONFIG_PATH`, and private `XDG_CONFIG_HOME` and `XDG_STATE_HOME` roots.
In Herdr 0.8.2, session data is under `<XDG_CONFIG_HOME>/herdr/sessions/<HERDR_SESSION>`; `HERDR_CONFIG_PATH` alone changes only the config file.
The client socket is derived from the API socket by inserting `-client` before `.sock`, so use a short absolute socket path that fits the platform limit.
Pass the same environment to the server, dev bundle, CLI and reference TUI, and clear inherited pane, workspace and tab identifiers.
Before creating fixtures, prove that the private server has zero workspaces and that the operator server gained no connection; keep the socket and process evidence in the run directory.
After verification, stop only that private server and remove only its recorded state directory and sockets.
The machine still carries the operator's load; record shared agent count separately from private fixture count.
References: [named sessions](https://herdr.dev/docs/persistence-remote/#named-sessions), [CLI environment](https://herdr.dev/docs/cli-reference/#environment-variables), and the pinned [path implementation](https://github.com/herdrdev/herdr/blob/v0.8.2/src/config/io.rs).

On 2026-09-06, the isolated 120 Hz verification retained ten attached panes with one private and 22 to 23 shared agents.
The final plain-shell painted-wheel result was p50 30.83 ms and p95 38.06 ms, 1.98 ms above the same-pane Herdr TUI p95; Claude was p50 43.54 ms and p95 53.75 ms, 9.55 ms below its TUI p95.
Key-to-transport-flush p95 was 0.62 ms and frame-receive-to-draw p95 was 6.26 ms.
Across 11 one-minute RSS samples, the baseline p50 was 148000 KiB, two fresh changed-build p50 values were 133904 and 127680 KiB, and a later final-source warm-window p50 was 135360 KiB; keep the raw ranges, endpoints, load and pane count with any comparison because macOS memory pressure moved individual endpoints across the baseline.

Capture terminal intervals from the exact app PID with `/usr/bin/log stream --process <pid> --level debug --style ndjson --predicate 'subsystem == "me.grab.hide" AND category == "TerminalLatency"'`, redirecting to the run's evidence directory.
Run `python3 scripts/summarize-terminal-latency.py <trace> --started-after <unix-seconds>` to report nearest-rank p50, p95, maximum and sample count for key-to-send, receive-to-draw, wheel-to-draw and tab-to-first-draw.
The debug mirror contains the same end values as the signposts and works without Instruments.
Key-to-send ends at the actual transport flush; receive-to-draw starts at delivery to the registered view and ends after software drawing.
Hidden, released, consumed, capacity-limited and pre-window intervals are reported separately, never as zero latency.
The display link records its measured refresh rate, with zero treated as unknown.
The wheel signpost ends at the next draw, which can be unrelated output; use a content-region change and the same window-server display timestamp method in both clients for a causal wheel comparison.
Record each PID and bundle, selected pane, terminal grid, attached-child count, shared agent count, refresh rate, load and foreground interruptions.
For RSS, record eleven one-minute samples across ten minutes and retain the process lists; memory pressure and occlusion can change RSS without an allocation improvement.
Build an archived baseline with an isolated target directory.
Sharing a release output directory can leave a fresh package fingerprint beside another checkout's `libherdr_core.a`; verify the linked archive hash and expected runtime diagnostics before treating a bundle as current.
Core diagnostics are also mirrored as identical JSON lines in the state directory's `Logs/core.jsonl`, with one previous 1 MiB file, bounded queued writes and I/O outside the runtime lock.

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

The native shell components live in `macos/Sources/HerdrMacOS/`: `HideTheme.swift` defines tokens, `HideKeycap.swift` draws registry-derived shortcuts, `HideBalloon.swift` draws tooltips and hint chips, `HideIconButton.swift` owns icon controls, `HideBadge.swift` owns labels, and `HideOverlay.swift` attaches the shared renderer to window content.
Use the command tooltip modifier and its identical accessibility help for every shell tooltip, preserving the Pet exception; run `scripts/check-hide-theme-literals.sh` and `scripts/check-hide-components.sh` before delivery.
