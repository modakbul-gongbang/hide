# Agent Notes

This file routes and keeps the rules that hold everywhere; the reasons and the procedures live in the documents it names.
Read [docs/README.md](docs/README.md) before choosing supporting documents: it says which are current contracts, who owns them in code and tests, and which are historical.
Do not apply superseded architecture decisions, old milestone reports, or old PRD implementation paths to current code.
Update the owning guide and its active references in the same change as the behavior; keep run evidence outside `docs/`.
Before opening a browser inside Hide, read `docs/BROWSER_PANES.md` for the host entrypoint, installation ownership, and native display verification.

## Repository Layout

- `macos/` - the production macOS application: a SwiftUI shell that renders the core snapshot and dispatches typed events back. Build and sign it with `macos/scripts/build_dev_app.sh`. It coexists with the web shell until S6.
- `hided/` - the product daemon and `hide` CLI. It links `herdr-core` on an owner thread, serves loopback HTTP (`/`, `/assets`, `/health`) and a token-gated WebSocket for dispatch and snapshot deltas.
- `web/` - the React web shell (Vite, zustand, xterm.js). Build output is `web/dist/` inside this worktree and is gitignored.
- `herdr-core/` - platform-neutral Rust runtime and the six-function C ABI (`herdr-core/include/herdr_core.h`) the shell links against, plus the pub Rust API `hided` uses. It projects Herdr-owned pane topology and owns Hide's UI state; the shell owns neither.
- `hide-agent-hooks/` - the only code that writes a configuration file the operator owns (each agent runtime's hook file). A separate crate because a `settings.json` write must never sit behind the render lock; see `docs/agent-hooks.md`.
- `hide-ai/` - the provider boundary for background AI features, backed by the user's own logged-in CLIs; see `docs/AI_PROVIDERS.md`.
- `hide-session/` - shared local Claude and Codex session location, incremental reading, and conversation parsing used by the plugin and core usage fallback.
- `plugins/` - Herdr plugins shipped from this repository, each installable on its own with `herdr plugin install <owner>/<repo>/plugins/<name>`: `browser/` and `agent-context-labels/`.
- `spikes/swift-shell-pivot/` - the Stage 0 spike and its `VERDICTS.md`, a frozen record; do not edit it to reflect later changes.

## Before Opening A Pull Request

`main` takes pull-request merges only, and the `verify` workflow has to pass; this repository currently uses merge commits, and no one, maintainer included, can push around branch protection.
`CONTRIBUTING.md` lists every gate with its local command; run the lanes the diff touches before opening the pull request, and change a gate that is wrong in the same pull request with the reason in the description.
Write the template in the order a reviewer reads it: `Summary` with screenshots, `Review` (what needs judgment, which files to watch and why, questions), `Evidence` (what was and was not confirmed), then `Breaking change` only when something breaks; answer from the diff, not from intent, and delete the lines that do not apply rather than filling them with "N/A".
The reasons this repository has been burned by stay behind `Review`'s file list (runtime mutex, snapshot wire, ownership, Herdr contract, failure path, high-frequency path); machine facts such as SHAs, suite output and evidence hashes go in the folded `Verification record` block at the end, which `/ship` fills.

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
- The release archive is `target/release/libherdr_core.a` inside the worktree that built it; `build_dev_app.sh`, `build-app.sh` and `verify-swift.sh` read that fixed path, so never redirect a release build with `CARGO_TARGET_DIR`, `--target-dir` or `--build-path`.
- Every build lands inside the worktree, cargo in `target/` and SwiftPM in `macos/.build/`, both ignored; `git worktree remove` is the whole cleanup, and nothing under `/tmp` belongs to a checkout.
- `scripts/verify-cargo.sh` and `scripts/verify-swift.sh` are the only Rust and Swift verification entrypoints; a check script calls them rather than cargo or swift directly.
- Every script that calls cargo sources `scripts/toolchain-env.sh`, so an isolated HOME reuses the machine's toolchain; without it rustup installs a private 1.4 GB copy and exits 0.
- The PRD harness binds `scripts/verify-cargo.sh`, because a verify command runs with no shell and an `ENV=value cargo ...` binding fails with ENOENT at verify time.
- `[profile.dev] incremental = false` is deliberate: an agent worktree is built a few times and discarded, which never repays an incremental cache.
- `git worktree remove` takes the cache with the work; a worktree kept alive after its branch lands keeps its cache alive too.
- The root checkout stays on `main`; every branch is worked on in its own worktree under `../herdr-ide.worktrees/`.
  `scripts/hooks/root-worktree-main-only.sh` is a `PreToolUse` hook (registered for Claude Code in `.claude/settings.json`, for Codex in `~/.codex/hooks.json`) that refuses a `git checkout`/`git switch` off `main` in the root worktree and answers with the `git worktree add` form to use instead.

## Runtime Architecture

Read `docs/ARCHITECTURE.md` in full before changing anything under `herdr-core/` or `macos/`; it owns the reasons behind these boundaries.

- The core owns all state behind one `Mutex<Runtime>`; the shell dispatches typed events in and pulls one snapshot out when the notifier announces, and holds no authority of its own.
- Herdr owns pane existence, split geometry, zoom, cwd, agent lifecycle and the PTY; the core owns each checkout's visible tab, the keyboard focus pane, panel visibility and text scale, and changes those on the event that asked for it, telling Herdr afterwards.
  While that notification is pending, Herdr's move is read as confirmation; with nothing pending, a move Herdr makes on its own is followed and a diagnostic records it; a refusal or a timeout keeps the core's value and records a diagnostic.
  Zoom, splits, closes and resizes still wait for Herdr, because their geometry decides the PTY size.
- The notifier announces once per burst, and `herdr_core_snapshot` clears its latch **before** it takes the lock; clearing it after the read would swallow a change that landed during the read.
  Launch creates the core once, after the runtime resolution has finished, and never replaces it.
- A user action is one event, not a sequence: dispatch is fire-and-forget, so four events would arrive as four frames and a refusal partway would leave the screen half moved.
- A path outside every registered checkout never reaches the core; the shell hands it to macOS and reveals rather than opens anything executable.
- A delegated child pane is moved to its own tab, never split into the operator's pane, and a delegated row can only be Working or Seen; a descendant's demand or completion turns its ancestors unread and nothing else on the parent moves; `docs/status-model.md` owns the ownership axis, the descendant badge and that rule.
- An attach lives only while its tab is among the last five shown (`ATTACHED_TAB_LIMIT`); a released pane keeps its projection entry as `released`, because a missing entry reads as a failure.
- Design principle #13 governs what reaches the screen: a failure the operator cannot act on goes to the diagnostic log, and an alert, a banner, or a sheet is a PRD decision, not a default.

## Herdr API Contract

Before changing, debugging, or reviewing any Herdr integration, read both official references in full for the Herdr this repository pins: the [CLI reference](https://herdr.dev/docs/cli-reference/) and the [Socket API](https://herdr.dev/docs/socket-api/).
The CLI is a wrapper over the same local socket API: use CLI wrappers for shell scripts, orchestration, debugging and portable plugin commands, and the raw socket only for custom-client control or long-lived subscriptions.

- Do not guess method names, parameters, response fields, or protocol compatibility from existing call sites.
  Check the target binary with `herdr --version` and `herdr api schema --json`, then compare with `contracts/herdr-api.schema.json` through `scripts/check-herdr-contract.sh`.
- The pin lives only in `macos/Sources/HerdrMacOS/Resources/herdr-bundle.json`, and the contract is what that exact binary answers, never a copy from a Herdr checkout; `check-herdr-pin-single-source.sh` fails when anything restates it.
  Move it with `scripts/bump-herdr.sh <release-tag>`.
- `herdr-core/src/wire.rs` is the only place generated wire types are converted into the core's inputs; do not write wire deserialization in `session_sync/{projection,replica}.rs` or import generated types into domain, runtime or sidebar code.
  `docs/ARCHITECTURE.md` lists the schema gaps the boundary still handwrites and the tests that demand migration when the schema closes them.

<!-- herdr-provenance:start -->
hide distributes the [upstream Herdr release v0.9.1](https://github.com/herdrdev/herdr/releases/tag/v0.9.1).
The bundled binary is not modified by hide.
The weekly `herdr-update.yml` workflow proposes upstream stable releases with `--repo herdrdev/herdr`; updates must pass contract and runtime checks.
<!-- herdr-provenance:end -->

## Performance Guide

Before diagnosing, changing, reviewing, or verifying terminal responsiveness, rendering, scrolling, selection, resize, tab switching, CPU, memory, snapshots, or attach behavior, read [docs/PERFORMANCE_TESTING.md](docs/PERFORMANCE_TESTING.md) in full.
It owns the reproduction procedure, the isolation checklist, the measurement boundaries, the invariants a fix has to preserve, and the regression ownership table; historical run measurements are not acceptance thresholds.

- Explain the added work per input, the notification fan-out, and the pending-work bound before adding anything to a high-frequency path; publish only actual state transitions, and never drop input to do so.
- No subprocesses, blocking I/O, or large serialization under `Mutex<Runtime>`: `snapshot_delta_payload` takes owned data under the lock and `serialize_snapshot_delta` serializes outside it, and `ChangeNotifier` announces once per burst.
  No per-tick or per-tab git forks: the catalog reads repository facts from the repository's own files (`git_dir.rs`), never from a `git` process.
- Report idle and driven measurements separately, with the load and workload recorded for each.
- Native verification targets one precisely identified candidate PID/window and an isolated Herdr server; the operator app may remain running. Never quit, restart, focus, or manipulate the operator's app, panes, or server for QA without explicit coordination. Prefer background exact-window capture; see `docs/PERFORMANCE_TESTING.md` for isolation and foreground-interaction boundaries.
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

Read [docs/UI_BEHAVIOR.md](docs/UI_BEHAVIOR.md) before changing any surface a user looks at; it owns what the UI does.
Read [docs/DESIGN_WORKFLOW.md](docs/DESIGN_WORKFLOW.md) before making a design change; it owns how a change moves from scratch to a shipped PR, including the token/System-part/Component procedures and the screen transplant procedure.
Visual authority is the Pen library (`design/hide-ui.lib.pen`), numeric authority is `design/tokens.json`, and code authority is `web/src/components/ui` and `web/src/components`.
`design/tokens.json` is web-only: `scripts/gen-tokens.mjs` writes `web/src/tokens.css`, and it no longer generates or updates `HideTheme.swift`, which is frozen for the Swift shell's remaining coexistence period (see `macos/AGENTS.md`).
Run `node scripts/check-design-contract.mjs` before delivery; it is the entrypoint `design-contract.yml` runs.

### The Design Library

`design/hide-ui.lib.pen` is the committed design-system library: tokens, `System /` shadcn-part sheets, and `Component /` hide-composite sheets with their state sheets.
Only `System /` and `Component /` top-level sheets belong there; product screens live in `design/hide-screens.pen` instead, and proposals, audits, and scratch never enter either committed file.
Read [DESIGN_WORKFLOW.md](docs/DESIGN_WORKFLOW.md) in full before editing it: it owns the scratch-to-PR flow, human review, local scratch, Pen toolchain limits, the screen transplant procedure, and how to add a token, a System part, or a Component.
