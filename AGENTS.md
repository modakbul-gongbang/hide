# Agent Notes

This file routes and keeps the rules that hold everywhere; the reasons and the procedures live in the documents it names.
Read [docs/README.md](docs/README.md) before choosing supporting documents: it says which are current contracts, who owns them in code and tests, and which are historical.
Do not apply superseded architecture decisions, old milestone reports, or old PRD implementation paths to current code.
Update the owning guide and its active references in the same change as the behavior; keep run evidence outside `docs/`.
Before opening a browser inside Hide, read `docs/BROWSER_PANES.md` for the host entrypoint, installation ownership, and native display verification.

## Repository Layout

- `macos/` - the production macOS application: a SwiftUI shell that renders the core snapshot and dispatches typed events back. Build and sign it with `macos/scripts/build_dev_app.sh`.
- `herdr-core/` - platform-neutral Rust runtime and the six-function C ABI (`herdr-core/include/herdr_core.h`) the shell links against. It projects Herdr-owned pane topology and owns Hide's UI state; the shell owns neither.
- `hide-agent-hooks/` - the only code that writes a configuration file the operator owns (each agent runtime's hook file). A separate crate because a `settings.json` write must never sit behind the render lock; see `docs/agent-hooks.md`.
- `hide-ai/` - the provider boundary for background AI features, backed by the user's own logged-in CLIs; see `docs/AI_PROVIDERS.md`.
- `plugins/` - Herdr plugins shipped from this repository, each installable on its own with `herdr plugin install <owner>/<repo>/plugins/<name>`: `browser/` and `agent-context-labels/`.
- `spikes/swift-shell-pivot/` - the Stage 0 spike and its `VERDICTS.md`, a frozen record; do not edit it to reflect later changes.

## Before Opening A Pull Request

`main` takes squash merges through pull requests only, and the `verify` workflow has to pass; no one, maintainer included, can push around it.
`CONTRIBUTING.md` lists every gate with its local command; run the lanes the diff touches before opening the pull request, and change a gate that is wrong in the same pull request with the reason in the description.
Answer the template's `Risk surface`, `Review focus` and `Breaking change` from the diff, not from intent, and delete the lines that do not apply rather than filling them with "N/A".

## Evidence Belongs Outside The Repository

Screenshots, traces, sample output, run logs, browser profiles, and verification transcripts are run artifacts, not source, and they do not belong in a commit.
They once did: an evidence tree reached 123 MB and carried a browser profile with cookies and screenshots showing the workstation's home directory, and it is still reachable from `origin/main`, so treat everything that was in it as public.

- Write run artifacts under `agents/runs/<slug>/`, which is local-only, so nothing there can reach a commit by accident.
- Never add a path under `docs/verification/`, `docs/screenshots/`, or `spikes/*/evidence/`; `check-no-workstation-identity.sh` refuses them even past a `git add -f`, and refuses a browser profile file by shape.
- When a document needs to cite evidence, state the finding and how it was measured; do not commit the artifact so a path can be linked.
  A verification claim is proven to the person reading the run, in the receipt and the run directory, not to the repository.

## Build Output Belongs To Its Worktree

`docs/BUILD.md` owns the reasons; these are the rules.

- No build directory is ever shared between worktrees: cargo names artifacts by workspace-relative path, so two checkouts sharing one read each other's build as fresh, and its lock serializes the parallel builds worktrees exist for.
- The release archive is `target/release/libherdr_core.a` inside the worktree that built it; `build_dev_app.sh`, `build-app.sh` and `swift-test.sh` read that fixed path, so never redirect a release build with `CARGO_TARGET_DIR`, `--target-dir` or `--build-path`.
- A check that judges the working tree sends its *test* build to the scratch directory `scripts/build-scratch.sh` names; source it rather than writing a path.
- Every script that calls cargo sources `scripts/toolchain-env.sh`, so an isolated HOME reuses the machine's toolchain; without it rustup installs a private 1.4 GB copy and exits 0.
- The PRD harness binds `scripts/verify-cargo.sh`, because a verify command runs with no shell and an `ENV=value cargo ...` binding fails with ENOENT at verify time.
- `[profile.dev] incremental = false` is deliberate: an agent worktree is built a few times and discarded, which never repays an incremental cache.
- `git worktree remove` takes the cache with the work; a worktree kept alive after its branch lands keeps its cache alive too.

## Runtime Architecture

Read `docs/ARCHITECTURE.md` in full before changing anything under `herdr-core/` or `macos/`; it owns the reasons behind these boundaries.

- The core owns all state behind one `Mutex<Runtime>`; the shell dispatches typed events in and pulls one snapshot out when the notifier announces, and holds no authority of its own.
- Herdr owns pane existence, split geometry, zoom, cwd, agent lifecycle and the PTY; the core owns each checkout's visible tab, the keyboard focus pane, panel visibility and text scale, and changes those on the event that asked for it, telling Herdr afterwards.
  While that notification is pending, Herdr's move is read as confirmation; with nothing pending, a move Herdr makes on its own is followed and diagnosed; a refusal or a timeout keeps the core's value and says so.
  Zoom, splits, closes and resizes still wait for Herdr, because their geometry decides the PTY size.
- The notifier announces once per burst, and `herdr_core_snapshot` clears its latch **before** it takes the lock; clearing it after the read would swallow a change that landed during the read.
  Launch creates the core once, after the runtime resolution has finished, and never replaces it.
- A user action is one event, not a sequence: dispatch is fire-and-forget, so four events would arrive as four frames and a refusal partway would leave the screen half moved.
- A path outside every registered checkout never reaches the core; the shell hands it to macOS and reveals rather than opens anything executable.
- A delegated child pane is moved to its own tab, never split into the operator's pane, and a delegated row can only be Working or Seen; `docs/status-model.md` owns the ownership axis and the stall clock.
- An attach lives only while its tab is among the last five shown (`ATTACHED_TAB_LIMIT`); a released pane keeps its projection entry as `released`, because a missing entry reads as a failure.

## Herdr API Contract

Before changing, debugging, or reviewing any Herdr integration, read both official references in full for the Herdr this repository pins: the [CLI reference](https://herdr.dev/docs/cli-reference/) and the [Socket API](https://herdr.dev/docs/socket-api/).
The CLI is a wrapper over the same local socket API: use CLI wrappers for shell scripts, orchestration, debugging and portable plugin commands, and the raw socket only for custom-client control or long-lived subscriptions.

- Do not guess method names, parameters, response fields, or protocol compatibility from existing call sites.
  Check the target binary with `herdr --version` and `herdr api schema --json`, then compare with `contracts/herdr-api.schema.json` through `scripts/check-herdr-contract.sh`.
- The pin lives only in `macos/Sources/HerdrMacOS/Resources/herdr-bundle.json`, and the contract is what that exact binary answers, never a copy from a Herdr checkout; `check-herdr-pin-single-source.sh` fails when anything restates it.
  Move it with `scripts/bump-herdr.sh <release-tag>`.
- `herdr-core/src/wire.rs` is the only place generated wire types are converted into the core's inputs; do not write wire deserialization in `session_sync.rs` or import generated types into domain, runtime or sidebar code.
  `docs/ARCHITECTURE.md` lists the schema gaps the boundary still handwrites and the tests that demand migration when the schema closes them.

<!-- herdr-provenance:start -->
hide distributes a modified Herdr preview from the [modakbul-gongbang/herdr fork](https://github.com/modakbul-gongbang/herdr/releases/tag/preview-2026-09-06-13d8d0b99033), built from commit `13d8d0b99033`.
This fork supplies host-scoped snapshots, ordered event sequences, and agent lineage that the upstream stable release does not yet expose.
The weekly `herdr-update.yml` workflow continues to propose upstream stable releases with `--repo herdrdev/herdr`; return to upstream when the contract field tests and runtime checks pass.
<!-- herdr-provenance:end -->

## Performance Guide

Before diagnosing, changing, reviewing, or verifying terminal responsiveness, rendering, scrolling, selection, resize, tab switching, CPU, memory, snapshots, or attach behavior, read [docs/PERFORMANCE_TESTING.md](docs/PERFORMANCE_TESTING.md) in full.
It owns the reproduction procedure, the isolation checklist, the measurement boundaries, the invariants a fix has to preserve, and the regression ownership table; historical run measurements are not acceptance thresholds.

- Explain the added work per input, the notification fan-out, and the pending-work bound before adding anything to a high-frequency path; publish only actual state transitions, and never drop input to do so.
- No subprocesses, blocking I/O, or large serialization under `Mutex<Runtime>`: `snapshot_delta_payload` takes owned data under the lock and `serialize_snapshot_delta` serializes outside it, and `ChangeNotifier` announces once per burst.
  No per-tick or per-tab git forks: the catalog reads repository facts from the repository's own files (`git_dir.rs`), never from a `git` process.
- Report idle and driven measurements separately, with the load and workload recorded for each.
- Native verification uses exactly one identified app and an isolated Herdr server; never manipulate the operator's panes or server.
- A Browser plugin pane is only for QA of the browser-pane product surface, never a verification surface for the native shell, editor, Git diff, sidebar, build, or installed app.

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

Read `DESIGN.md` before changing any surface a user looks at; it is the design source of truth for the macOS shell, and the Raycast references it records carry an MIT attribution context.
`HideTheme` in `macos/Sources/HerdrMacOS/HideTheme.swift` carries its tokens into the shell: a new color, radius, or spacing value is added there and used from there, never written inline, and a case the system does not cover is raised as a proposed addition rather than settled with a one-off value.
Use the command tooltip modifier and its identical accessibility help for every shell tooltip, preserving the Pet exception.
Run `node scripts/check-design-contract.mjs` before delivery; it is the entrypoint `design-contract.yml` runs.

### The Design Canvas

Screen designs, layout proposals and component sheets live in one pen.dev document, `design/hide.pen`, and it is committed, so a design change shows up in `git diff` beside the code change that answers it.
Put design work there rather than in a new file, an ad-hoc HTML page, or a screenshot pasted into a message: a second `.pen` file cannot use this one's variables (the import trap below) or its components (a `ref` names an id in its own document).

**Three rules cover the whole file; everything else is generated.**

1. A board's name starts with its band prefix. `.pen` has no pages, so the prefix is the whole of a board's classification.
2. Every value is a `$--` variable, never a literal, exactly as the shell designs against `HideTheme`. A value the set does not carry lands in `HideTheme.swift` first.
3. Run `node scripts/gen-pen.mjs` before saving. It writes `HideTheme`'s values into the mapped variables, places every board at its band, and redraws the band labels and `System / Foundations`; `node scripts/check-pen.mjs` refuses a canvas that differs, and rides `check-design-contract.mjs`.

| Band | Prefix | Holds | Lifetime |
|---|---|---|---|
| System | `System /` | the generated Foundations sheet and the primitive sheets (badge, keycap, icon button, panel tab) | kept in step with `HideTheme` |
| Component | `Component /` | one sheet per agreed component | `Screen /` boards reference the masters inside with `ref` nodes |
| Screen | `Screen /<area> /<name>` | what the app draws at this commit | a PRD changes the board first; the code catches up in the same pull request |
| Review | `Review /<date> <topic> / Audit`, `/ Proposal`, `/ As built / ...` | one audit, its proposal, and the as-built evidence beside them | deleted once the proposal lands in code |
| Scratch | `Scratch /<topic>` | exploration, candidates side by side | deleted or redrawn as a `Screen /` or `Review /` board before the pull request opens |

The check fails on a board with no prefix, a mapped variable that drifted from `HideTheme`, a `HideTheme` constant that `scripts/pen-token-map.json` neither maps nor excuses, and a stale label or Foundations sheet.
The unclaimed constant is the one worth having: a token can otherwise reach the shell and never reach the design, and nothing says so.
Where a board sits is not checked; a board dragged elsewhere in the pen app is still in its band by name, and the next `gen-pen.mjs` puts it back.
The canvas also carries variables of its own that the generator never touches - the `--asbuilt-*` family naming the off-scale numbers the app writes directly, the `--proposed-*` family a reconciliation needs, and derived tints pen cannot compute.

**A component is a sheet, one column: title and spec, the `reusable` master, then one row per state.**
A `ref` from a `Screen /` board resolves to the master inside its sheet.
Each state row is a `ref` of the master with `descendants` overrides (`enabled: false` hides a slot, `<refId>/<childId>` reaches into a nested ref), never a redrawn copy, so a change to the master reaches every state.
The states are the ones the code produces and the spec line says where (`ChangedFileRow`, `HideIconButton`); a state the app cannot reach is not drawn.

There is no band for a feature's design, because `main` takes a PRD and its implementation in one squash merge and nothing runs at the merge to move a board.
A PRD draws the screen it changes as the `Screen /` board itself, one frame per state the data can produce; on that branch the board is the target until the code catches up, and on `main` it is what was built.
The target is kept outside the canvas: the PRD commit's `hide.pen`, and the boards exported to `agents/runs/<slug>/design/` when implementation starts.
A component the design needs and does not have is drawn under a `Proposed /` name and recorded in the PRD's Decisions table, so the addition is a decision a reviewer sees rather than a shape that appeared.
A review is adopted the same way: its tokens land in `HideTheme.swift`, its components are renamed into `Component /`, the `Screen /` boards are redrawn on them in the pull request that ships the code, and the `Review /` boards are deleted.

`.pen` is JSON, and pen.dev is a local CLI reached over MCP or headlessly:

    pen interactive --in design/hide.pen --out design/hide.pen                    # drive it yourself
    pen --repo . --in design/hide.pen --out design/hide.pen --prompt "..."        # hand it to pen's own agent

**`--repo .` is not optional.** It is the only thing that makes pen's agent read this file; `--repo` does not fall back to the working directory.
Verified by planting a marker in `AGENTS.md`: with the flag the agent quotes it back, without the flag it reports no project instructions at all.
Anything drawn inside the pen desktop app is scratch by default: its agent cannot reach this file, so assume literal colours and off-scale numbers and clean them up when you promote the board.
Prefer structure a script could have produced over structure only a hand could have clicked, because that is what makes the file reviewable.

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
