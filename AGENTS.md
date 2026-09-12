# Agent Notes

## Documentation Routing

Read [docs/README.md](docs/README.md) before choosing supporting documents.
It identifies current contracts and procedures, their code/test owners, and historical/reference-only material.
Do not apply superseded architecture decisions, old milestone reports, or old PRD implementation paths to current code.
Update the owning guide and its active references in the same change as the behavior; keep run evidence outside `docs/`.
Before opening a browser inside Hide, read `docs/BROWSER_PANES.md` for the host entrypoint, installation ownership, and native display verification.

## Repository Layout

- `macos/` - the production macOS application: a SwiftUI shell that renders the core snapshot and dispatches typed events back. Build and sign it with `macos/scripts/build_dev_app.sh`.
- `herdr-core/` - platform-neutral Rust runtime and the six-function C ABI (`herdr-core/include/herdr_core.h`) the shell links against. It projects Herdr-owned pane topology and owns Hide's UI state; the Swift shell owns neither. See Runtime Architecture for the exact ownership split.
- `hide-agent-hooks/` - the only code path in the product that writes a configuration file the operator owns. It knows where each agent runtime keeps its hook file, how to append one entry without disturbing anybody else's, how to judge what is installed, and how to report that judgement; its `hide-agent-hooks` binary is what the installed hook runs. It is a separate crate because the risk it carries is a file-system risk, and folding it into the crate that owns `Mutex<Runtime>` would put a `settings.json` write behind the render lock.
- `hide-ai/` - the provider boundary for background AI features: a feature submits its own prompt, output schema and parser through `AiRouter`, and the crate owns provider lifecycle, availability, timeouts, retries, sticky failover, queryable provider state and structured errors. Summaries come from the user's own logged-in CLIs: Codex through `codex app-server`, Claude Code through `claude -p` print mode; see `docs/AI_PROVIDERS.md`.
- `plugins/` - Herdr plugins shipped from this repository: `browser/` (the Hide browser pane) and `agent-context-labels/` (pane task labels, a workspace member that consumes `hide-ai`). Each directory is installable on its own with `herdr plugin install <owner>/<repo>/plugins/<name>`.
- `src/` - removed retired Rust-native shell. The SSH/mini runtime is owned by `herdr-core/`; nothing links a root `src/` crate into the application.
- `spikes/swift-shell-pivot/` - the Stage 0 spike source and its `VERDICTS.md`. A frozen record; do not edit it to reflect later changes. Its evidence output is no longer kept in the repository (see `Evidence Belongs Outside The Repository`).

## Before Opening A Pull Request

`main` takes squash merges through pull requests only, and the `verify` workflow (`.github/workflows/pr.yml`) has to pass; no one, maintainer included, can push around it.
Run the lanes it runs before opening the pull request; `CONTRIBUTING.md` lists every gate with its local command, what it protects, and what to do when it blocks.
A gate that is wrong is changed in the same pull request with the reason in the description; there is no bypass label.
The pull request template's `Risk surface` names the places this repository has actually been bitten: the runtime mutex, the snapshot wire, Herdr versus core ownership, the API contract, the failure path, and the high-frequency path.
Answer the ones the diff touches from the diff, not from intent, and delete the rest rather than filling them with "N/A".
The template's other required judgements are `Review focus`, which says what the evidence has already settled and what a person still has to decide, and `Breaking change`, which names what an operator has to do by hand because no commit can do it for them.
Link the PRD or issue the change answers under `Related`.

## Evidence Belongs Outside The Repository

Screenshots, traces, sample output, run logs, browser profiles, and verification transcripts are run artifacts, not source. They do not belong in a commit.

This rule exists because they were: `docs/verification/`, `docs/screenshots/`, and the spike `evidence/` directories grew to 660 files and 123 MB, and a Chrome profile committed under `spikes/integrated-preflight/` carried cookies and a symlink naming the workstation. A later scan for leaked identity passed because it read text and skipped images, while 184 screenshots showed the home directory and hostname in plain sight.

- Write run artifacts under `agents/runs/<slug>/`. That whole namespace is local-only, so nothing there can reach a commit by accident.
- Never add a path under `docs/verification/`, `docs/screenshots/`, or `spikes/*/evidence/`. They are gitignored, and `check-no-workstation-identity.sh` refuses them even when a `git add -f` walks past the ignore rule. It refuses a tracked browser profile file by shape too, because the text scan cannot see one.
- Removing such a path from the working tree does not remove it from a clone. The 2026-08 evidence tree is still reachable from `origin/main`; treat anything that was in it, cookies included, as public.
- When a document needs to cite evidence, state the finding and how it was measured. Do not commit the artifact so a path can be linked.
- A verification claim is proven to the person reading the run, not to the repository. The receipt and the run directory are where it lives.

## Build Output Belongs To Its Worktree

No build directory is ever shared between worktrees, and the release archive the shell links stays inside the worktree that produced it.
Those are two separate rules, and the second is the narrower one.

The release archive is `target/release/libherdr_core.a`, and `macos/scripts/build_dev_app.sh`, `scripts/build-app.sh` and `scripts/swift-test.sh` read it from that fixed path under the worktree.
Redirecting a release build with `CARGO_TARGET_DIR`, `--target-dir` or `--build-path` leaves all three reading a path nothing wrote; `check-typed-live-remote.sh` used to compensate with a worktree `target` symlink, which is the shape this rule exists to prevent.

Sharing is the rule that governs everything else.
Cargo names a workspace member's artifacts by its path relative to the workspace root, so two checkouts sharing one target directory read each other's build as fresh and run the other checkout's test binary.
Cargo also holds an exclusive lock on it, so worktrees sharing one serialize the parallel builds this layout exists to allow.
Local checks resolve their own record tree from the worktree and cannot follow output out of it either.

A check script may still send its *test* build to a scratch directory, because a run that judges the working tree must not dirty it by building into `target/`.
`scripts/build-scratch.sh` is the one place that decides where that goes: a path keyed by checkout, which is isolated from the tree and from every other tree at once.
Source it rather than writing a path; `scripts/rust-test.sh` keys its own default the same way and for the same reason.

The cost of that isolation is one full build cache per worktree, so the cache is removed with the work rather than left behind.
`git worktree remove` takes both directories with it; a worktree kept alive after its branch lands keeps its cache alive too.
On 2026-09-09 one abandoned worktree held 5.3 GB, over half of the 10 GB across all eight.

`[profile.dev] incremental = false` in the workspace manifest is deliberate, not a leftover.
An agent worktree is built a few times and discarded, which never repays an incremental cache; what it does instead is grow one per worktree, and those had reached 1.5 GB.
Debug output is what makes a stale worktree expensive, because nothing strips it: the debug `libherdr_core.a` measured 288 MB against 92 MB for the release archive.

The toolchain itself is not build output and is never copied per run.
`scripts/toolchain-env.sh` is sourced by every script here that calls cargo, and it resolves `CARGO_HOME` and `RUSTUP_HOME` from the cargo shim's own location so an isolated HOME reuses the machine's installed toolchain.

Without it a verification runner pays for a whole toolchain and keeps it.
rustup reads `RUSTUP_HOME` with a default of `$HOME/.rustup`, and a runner HOME makes that an empty directory; rustup does not fail there, it downloads and installs into it and reports the fact as a warning while exiting 0.
That exit 0 is why the two earlier workarounds never ran: `rust-test.sh` and `swift-test.sh` had each diagnosed the missing toolchain correctly, and each guarded its recovery behind a cargo invocation failing.
The cost was 1.3 GB of `.rustup` plus 128 MB of `.cargo` per run, 9.1 GB across eleven run directories, duplicating a toolchain already on the machine.

`scripts/verify-cargo.sh` is the plain-argv entrypoint the PRD harness binds for its `test` and `build` commands.
The harness runs a verify command with no shell, so an `ENV=value cargo ...` binding fails with ENOENT at verify time, when a sealed run can no longer be amended; a script is the only place that environment decision can live.
Its target directory is deliberately shared across runs rather than keyed by checkout, which the sharing rule above permits for the reason it names: every verify run builds the same checkout, so there is one path and sharing is what makes the second run incremental.

## Runtime Architecture

The core (`herdr-core`) owns all state behind one `Mutex<Runtime>`.
The shell dispatches typed JSON events in (`herdr_core_dispatch`) and pulls state out (`herdr_core_snapshot`) when the change notifier announces.
The event sync coordinator (`session_sync.rs`) bootstraps from `session.snapshot`, resumes ordered topology updates through `events.subscribe`, and refreshes agent telemetry with `agent.list` once per second.
A tick whose `agent.list` is unchanged publishes nothing, so an idle session recomputes no projection; the catalog's own refresh window still publishes, because the rebuild can only happen inside `publish_replica`.
The Git section refreshes local worktree state only when repository metadata, tracked paths, or Herdr worktree topology changes; disk usage refreshes when the section opens or its header refresh is pressed, and all three layers run outside the runtime mutex.
Pull requests also load once when a local Git project first appears in the sidebar and refresh from that project's menu or PR popover; these scoped requests reuse the same background reader, cache and generation coalescing.
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

A spawned child does not split the operator's pane.
Herdr owns split geometry and the PTY size, so a delegated child pane is really moved out - `pane.move` to a new tab in the workspace it is already in - rather than left undrawn; a tab holding nothing but delegated children then stays out of the tab strip while remaining in the checkout.
Detection is the same on every pass, so a child that arrives while Hide is running and one already split when Hide started take the same path, and a refusal is retried on a fixed interval rather than assumed to have worked.
Herdr reports a refusal as an unchanged move with a reason rather than as an error, so the decision reads `changed` instead of trusting a successful request.

Ownership is the fourth derived status axis and it is read off the lineage, never stored.
A delegated row can only be Working or Seen, so a child's question or completion never enters the operator's own attention groups; a per-child stall clock is what brings work back when it stops being anybody's problem.
`docs/status-model.md` owns both rules.

What an agent has spawned in-process is not on Herdr's wire at all.
The hook helper reports it through `herdr pane report-metadata`, which Herdr defines as display-only pane metadata, and the core reads it back out of the pane tokens its ordinary snapshot already carries; `herdr-core/src/agent_hooks.rs` is the only place that reads those tokens.
A count Hide cannot read is reported as unknown, never as zero.

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

High-frequency input changes include their shared observable state, tooltips, overlays, and layout dependencies, not only event handlers.
Explain added work per input, notification fan-out, scaling with retained versus visible data, and pending-work bounds before adding a feature to that path.
Publish only actual state transitions; repeated input with no state change must not wake unrelated consumers.
Preserve intentional input order and quantity; suppressing redundant state notifications is not permission to drop input.
Use the action contracts and regression ownership table in the performance guide, with a test that fails when the original failure is restored.

Before diagnosing, changing, reviewing, or verifying terminal responsiveness, rendering, scrolling, selection, resize, tab switching, CPU, memory, snapshots, or attach behavior, read [docs/PERFORMANCE_TESTING.md](docs/PERFORMANCE_TESTING.md) in full.
That guide owns the reproduction procedure, isolation checklist, measurement boundaries, commands, regression coverage, and cleanup/verdict requirements.
Historical run measurements are not acceptance thresholds; compare matched builds and workloads and keep evidence under `agents/runs/<slug>/`.
Report idle and driven measurements separately, with the load and workload recorded for each.

Keep these invariants during implementation:

- No subprocesses, blocking I/O, or large serialization under `Mutex<Runtime>`; precompute outside and keep serialization separate from the locked payload read.
  `snapshot_delta_payload` takes owned data under the lock; `serialize_snapshot_delta` serializes it outside the lock.
- No per-tick/per-tab git forks; reuse `PrecomputedCatalog`, `CatalogCache`, and `RootIndex`.
  The catalog reads repository root, main worktree and branch from the repository's own files (`herdr-core/src/git_dir.rs`), never from a `git` process: it is rebuilt on the session-sync coordinator, the thread that applies Herdr's events, and on 2026-09-10 one `git rev-parse` per fact per pane directory held tab, zoom and focus events for 4-15 s.
  `initialize_git` is the one git spawn left in `workspace.rs`, and `git_calls_on_this_thread` counts it so a test can prove a catalog path ran none.
- Snapshot traffic follows changes, not total retained state; use stream cursors and do not dirty revisioned `rest` with idle timestamps.
- Send every whole-row wheel promptly and coalesce only consecutive requests already waiting in the writer queue; never wait for a terminal frame or timer, and preserve Herdr routing, real geometry, and direct keyboard delivery.
- Resolve the first wheel at its real AppKit target, then reuse that route only while events stay consecutive and stationary; a non-terminal scroll gesture must not repeat SwiftUI hit testing on every tick.
- Parse immediately, settle geometry on display ticks, and submit pending damage once per tick; never reject an AppKit backing-store repair because it already drew that tick.
- Retain only visible rows' latest prepared render state; leave hidden panes undrawn and release unused attaches.
  Keep row preparation behind `preparedRow`; `scripts/check-terminal-row-cache.sh` enforces that the draw loop never bypasses the cache.
- `ChangeNotifier` announces once per burst; clear its latch before taking the snapshot lock.
- Native verification uses exactly one identified app and an isolated Herdr server, including remote-connection checks; never manipulate the operator's panes or server.
- A Browser plugin pane is only for QA of the browser-pane product surface. Never open one as a generic verification surface for the native shell, editor, Git diff, sidebar, build, or installed app.

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

`HideTheme` in `macos/Sources/HerdrMacOS/HideTheme.swift` carries those tokens into the shell, so a new color, radius, or spacing value is added there and used from there rather than written inline.
When the existing system does not cover a case, say so and propose the addition; do not settle it with a one-off value in a view.

`DESIGN.md` also records the Raycast public design references and their MIT attribution context.

The native shell components live in `macos/Sources/HerdrMacOS/`: `HideTheme.swift` defines tokens, `HideKeycap.swift` draws registry-derived shortcuts, `HideBalloon.swift` draws tooltips and hint chips, `HideIconButton.swift` owns icon controls, `HideBadge.swift` owns labels, and `HideOverlay.swift` attaches the shared renderer to window content.
Use the command tooltip modifier and its identical accessibility help for every shell tooltip, preserving the Pet exception; run `node scripts/check-design-contract.mjs` before delivery, which is the same entrypoint `design-contract.yml` runs.

### The Design Canvas

Screen designs, layout proposals and component sheets live in one pen.dev document:

    design/hide.pen

Put design work there rather than in a new file, an ad-hoc HTML page, or a screenshot pasted into a message.
One file is what lets two proposals sit side by side on the same canvas and share one token set; a second file loses both, because a variable does not resolve across an `imports` entry (the trap below) and a `ref` names an id in the document it sits in.
It is committed, so a design change shows up in `git diff` beside the code change that answers it.

**The canvas is organised by band, and the band is read off the board's name.**
`.pen` has no pages, so a top-level frame's name prefix is the whole of its classification, and `scripts/pen-bands.mjs` binds each prefix to a y position:

| Band | Prefix | Holds | Lifetime |
|---|---|---|---|
| System | `System /` | the Foundations sheet and the primitive sheets (badge, keycap, icon button, panel tab) | Foundations is generated; a primitive sheet is kept in step with `HideTheme` |
| Component | `Component /` | one sheet per agreed component | `Screen /` boards reference the masters inside with `ref` nodes |
| Screen | `Screen /<area> /<name>` | what the app draws at this commit | a PRD draws or changes the board first, the code catches up in the same pull request |
| Review | `Review /<date> <topic> / Audit`, `/ Proposal`, `/ As built / ...` | one audit, its proposal, and the as-built evidence beside them | deleted once the proposal lands in code |
| Scratch | `Scratch /<topic>` | exploration, candidates side by side | deleted or redrawn as a `Screen /` or `Review /` board before the pull request opens |

    node scripts/gen-pen.mjs      # tokens from HideTheme, boards placed by band, labels and Foundations redrawn
    node scripts/check-pen.mjs    # refuse a canvas that is not what gen-pen.mjs writes, and say which part

`System / Foundations` is generated too: `scripts/pen-foundations.mjs` draws the surface ladder, inks, semantic colours, type scale, spacing, radius, layout sizes and opacities from the canvas's own variables on every run, so the sheet cannot say something `HideTheme` does not.

**A component is a sheet, and the sheet is one column: title, master, then one row per state.**
The master is the `reusable` frame the rest of the canvas references; it sits inside its sheet, and a `ref` from a `Screen /` board resolves to it there.
Every state row is a `ref` of the master with `descendants` overrides (`enabled: false` hides a slot, `<refId>/<childId>` reaches a node inside a nested ref), never a redrawn copy, so a change to the master reaches every state.
The states are the ones the code produces and the sheet's spec line says where they come from (`ChangedFileRow` for the line row's letters, `HideIconButton` for hover, selected, pressed and disabled); a state the app cannot reach is not drawn.
Components read left to right across the band and states top to bottom within a sheet.

The generator also draws a `Band / <name>` label above each band - the name, what it holds, and a hairline the width of the row - and rebuilds it on every run, so nothing drawn into a label survives and the bands are visible on the canvas, not only in the file.
The check rides `check-design-contract.mjs`, so a board dragged out of its band or named outside the scheme fails the gate rather than disappearing into the file.
Name a board first; the generator decides where it goes.

A review moves upward when it is adopted: the tokens it needs land in `HideTheme.swift`, `gen-pen.mjs` brings them across, the proposal's components are renamed into `Component /`, the `Screen /` boards are redrawn on those components in the same pull request as the code, and the `Review /` boards are deleted.

There is no band for a feature's design, because `main` takes a PRD and its implementation in one squash merge and nothing runs at the merge to move a board.
A PRD draws the screen it changes as the `Screen /` board itself, one frame per state the data can produce, from `Component /` refs and `$--` variables only; on that branch the board is the target until the code catches up, and on `main` it is what was built.
The target is kept for the run outside the canvas: the PRD commit's `hide.pen`, and the boards exported to `agents/runs/<slug>/design/` when implementation starts.
A component the design needs and does not have is drawn inside the board under a `Proposed /` name and recorded in the PRD's Decisions table, so the addition is a decision a reviewer sees rather than a shape that appeared.

`.pen` is JSON, and pen.dev is a local CLI reached over MCP or headlessly:

    pen interactive --in design/hide.pen --out design/hide.pen                    # drive it yourself
    pen --repo . --in design/hide.pen --out design/hide.pen --prompt "..."        # hand it to pen's own agent

**`--repo .` is not optional.** It is the only thing that makes pen's agent read this file.
Without it the agent reads nothing, even when it is launched from inside the checkout - `--repo` does not fall back to the working directory.
Verified by planting a marker in `AGENTS.md`: with the flag the agent quotes it back, without the flag it reports no project instructions at all.

Prefer structure a script could have produced over structure only a hand could have clicked, because that is what makes the file reviewable.

**The direction of truth is one-way, and it is enforced.** `HideTheme.swift` defines a value; the canvas receives it.

    node scripts/gen-pen.mjs      # write HideTheme's values into design/hide.pen, with the layout pass above
    node scripts/check-pen.mjs    # refuse a canvas that disagrees

`scripts/pen-token-map.json` says which HideTheme constant each canvas variable carries, and why each remaining constant stays out.
The check fails on two things: a mapped value that drifted, and a HideTheme constant claimed by neither list.
The second is the one worth having - a token can otherwise reach the shell and never reach the design, and nothing says so.
It runs inside `node scripts/check-design-contract.mjs`, so it rides the existing gate and CI.

Design against those variables (`fill: "$--color-panel"`), never a literal hex, exactly as the shell designs against `HideTheme`.
A proposal that needs a value the token set does not carry names the addition it wants; the addition lands in `HideTheme.swift` first, then the generator brings it across.
The canvas also carries variables of its own that the generator never touches - the `--asbuilt-*` family naming the off-scale numbers the app writes directly, the `--proposed-*` family the reconciliation needs, and derived tints pen cannot compute.

Two substitutions are recorded rather than fixed, because pen cannot express either.
SF Mono is not installed, so the canvas uses **JetBrains Mono**, which `DESIGN.md` accepts as a substitute.
`ss03` cannot ride on a token, so type renders as plain Inter; sizes and spacing are exact, glyph shapes are not.
For the same reason there is no pixel-comparison gate: it would be permanently red or uselessly loose.

**Five traps, each of which cost a run or a wrong answer.**
The last two are the dangerous ones, because they fail silently and look like they worked.

- pen.dev is not HTML and not CSS - `alignItems: baseline`, `alignItems: stretch`, `margin` and percentage sizes all error, so think in the `.pen` schema rather than translating web properties.
- `--enable-preview` crashes the renderer on the second `execute`; use `TakeScreenshot` inside `execute` instead.
- `width` and `height` silently drop a `$variable` reference and leave the node at zero, so those two properties alone carry numeric literals - which is exactly why the generated check earns its keep on everything else.
- **A variable reached through `imports` does not resolve, and renders black.** A second `.pen` file importing this one and filling a frame with `$--color-panel` drew `#000000`; the same variable declared locally drew `#1D1F21`, matching the literal exactly. There is no error. This is why there is one canvas file and not a scratch file beside it.
- **A `context` node does not steer the agent.** A context node carrying a naming rule and "never use a literal hex" was ignored outright: the agent named the frame `Dark Frame` and filled it with `#1A1A1A`. Conventions have to arrive through `--repo .`, or not at all.

### Scratch work on the canvas

Exploration goes in the same file, in the `Scratch /` band:

- name it `Scratch / <topic>`, and let `gen-pen.mjs` place it
- when it earns its place, redraw it as a `Screen /` or `Review /` board and delete the scratch; otherwise just delete it

It goes in `design/hide.pen` rather than a scratch file of its own because that is the only place the tokens resolve; see the import trap above.
Scratch boards cost the contract nothing - the token check reads only the document's `variables`, and the generator preserves every node it does not own.

Anything drawn inside the pen desktop app is scratch by default.
The app's own agent has no way to reach this file, so assume its output carries literal colours and off-scale numbers, and clean it up when you promote it rather than while you are still exploring.
