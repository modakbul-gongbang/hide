# Agent Notes

This file routes and keeps the rules that hold everywhere; the reasons and the procedures live in the documents it names.
Read [docs/README.md](docs/README.md) before choosing supporting documents: it says which are current contracts, who owns them in code and tests, and which are historical.
Do not apply superseded architecture decisions, old milestone reports, or old PRD implementation paths to current code.
Update the owning guide and its active references in the same change as the behavior; keep run evidence outside `docs/`.
Before changing browser displays, read `docs/BROWSER_DISPLAYS.md` for who owns a page, the `file:` address boundary, and native display verification.
Before calling a change verified, read `docs/VERIFICATION.md` for which check proves the claim, the traps that made a check prove nothing, and the native QA tools that can address a candidate app without reaching the operator's.

## Repository Layout

- `hided/` - the product daemon and `hide` CLI. It links `herdr-core` on an owner thread, serves loopback HTTP (`/`, `/assets`, `/health`) and a token-gated WebSocket for dispatch and snapshot deltas.
- `web/` - the React web shell (Vite, zustand, xterm.js). Build output is `web/dist/` inside this worktree and is gitignored.
- `desktop/` - the Electron desktop host and the only shipped app: the web shell hided serves, in its own macOS window, attached through `hide connect`. The packaged `hide.app` carries `hided`, `hide`, `hide-agent-hooks`, the device helper and the pinned Herdr flat in `Contents/Resources`, ad-hoc signed by `pnpm --dir desktop package`; it never stops a daemon it did not start. Build output is `desktop/dist/` and the app and archive under `desktop/out/`, all gitignored; see `docs/ARCHITECTURE.md`, The desktop host.
- `herdr-core/` - platform-neutral Rust runtime, an rlib whose owner-thread handle (`herdr-core/src/handle.rs`) `hided` drives. It projects Herdr-owned pane topology and owns Hide's UI state; the shell owns neither.
- `hide-agent-hooks/` - the only code that writes a configuration file the operator owns (each agent runtime's hook file). A separate crate because a `settings.json` write must never sit behind the render lock; see `docs/agent-hooks.md`.
- `hide-platform/` - the operating-system layer under every other crate: what differs between macOS, Linux and Windows is written here once and checked by contract tests that run on all three; today the local stream (`ipc`) the Herdr client uses, the processes (`process`: the one child-start helper and what the kernel says about a pid) and the files (`fs`: private files and folders, locks, atomic replacement, links, file identity). It has no hide dependencies and no state; see `docs/ARCHITECTURE.md`, The platform layer.
- `hide-ai/` - the provider boundary for background AI features, backed by the user's own logged-in CLIs; see `docs/AI_PROVIDERS.md`.
- `hide-session/` - shared local Claude and Codex session location, incremental and backwards page reading, and conversation parsing used by the core's label worker, the core usage fallback and the phone's conversation.
- `plugins/` - Herdr plugins shipped from this repository, each installable on its own with `herdr plugin install <owner>/<repo>/plugins/<name>`: `hcoord/` only.
  Agent labels are made by the core, in `herdr-core/src/labels/`; see `docs/status-model.md`, Task identity.

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
- The release binaries are `target/release/{hided,hide,hide-agent-hooks,hide-host-helper}` inside the worktree that built them; `desktop/scripts/package.mjs` reads that fixed path, so never redirect a release build with `CARGO_TARGET_DIR` or `--target-dir`.
- Every build lands inside the worktree, cargo in `target/`, the web shell in `web/dist/`, the desktop host in `desktop/dist/` and the packaged app in `desktop/out/`, all ignored; `git worktree remove` is the whole cleanup, and nothing under `/tmp` belongs to a checkout.
- `scripts/verify-cargo.sh` and `scripts/verify-web.sh` are the only Rust and web verification entrypoints; a check script calls them rather than cargo or pnpm directly.
- Every script that calls cargo sources `scripts/toolchain-env.sh`, so an isolated HOME reuses the machine's toolchain; without it rustup installs a private 1.4 GB copy and exits 0.
- The PRD harness binds `scripts/verify-cargo.sh`, because a verify command runs with no shell and an `ENV=value cargo ...` binding fails with ENOENT at verify time.
- `[profile.dev] incremental = false` is deliberate: an agent worktree is built a few times and discarded, which never repays an incremental cache.
- `git worktree remove` takes the cache with the work; a worktree kept alive after its branch lands keeps its cache alive too.
- The root checkout stays on `main`; every branch is worked on in its own worktree under `../herdr-ide.worktrees/`.
  `scripts/hooks/root-worktree-main-only.sh` is a `PreToolUse` hook (registered for Claude Code in `.claude/settings.json`, for Codex in `~/.codex/hooks.json`) that refuses a `git checkout`/`git switch` off `main` in the root worktree and answers with the `git worktree add` form to use instead.

## The Installed App Is The Operator's

`/Applications/hide.app` is the copy the operator works in, and a live check against it is only as good as knowing which build it is.
On 2026-09-27 one session replaced it with a build that predated a merged fix, deleted the backup bundles beside it, and a second session spent an hour proving the fix was gone.

- Replace `/Applications/hide.app` only when the operator asked for it in the session doing the replacing; another session's approval, a PRD, or a verification plan is not that authority.
- Keep the bundle you replace as `/Applications/hide-previous-<version>.app.bak`, and never delete a `.bak` another session left; the version is `CFBundleShortVersionString` in its `Contents/Info.plist`.
- Before a live check, read the installed `Contents/Info.plist` version and `hide --help` from its `Contents/Resources`, and say in the run record which build answered.
- Launch it with the `HERDR_*` and `HIDE_*` variables stripped from the launching shell (`env -u HERDR_BIN_PATH -u HERDR_SOCKET_PATH … open /Applications/hide.app`): a Herdr pane carries the socket, identity and binary path of the Herdr that opened it, and the app would attach to that pane's server rather than the one a Dock launch finds.
- Package with `HIDE_VERSION=<version>` set, because no release tag describes `main` (`docs/BUILD.md`).

## Runtime Architecture

Read `docs/ARCHITECTURE.md` in full before changing anything under `herdr-core/`, `hided/` or `desktop/`; it owns the reasons behind these boundaries.

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
- The pin lives only in `contracts/herdr-bundle.json`, and the contract is what that exact binary answers, never a copy from a Herdr checkout; `check-herdr-pin-single-source.sh` fails when anything restates it.
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
`scripts/gen-tokens.mjs` writes `design/tokens.json` to `web/src/tokens.css`; nothing else consumes the tokens.
Run `node scripts/check-design-contract.mjs` before delivery; it is the entrypoint `design-contract.yml` runs.

### The Design Library

`design/hide-ui.lib.pen` is the committed design-system library: tokens, `System /` shadcn-part sheets, and `Component /` hide-composite sheets with their state sheets.
Only `System /` and `Component /` top-level sheets belong there; product screens live in `design/hide-screens.pen` instead, and proposals, audits, and scratch never enter either committed file.
Read [DESIGN_WORKFLOW.md](docs/DESIGN_WORKFLOW.md) in full before editing it: it owns the scratch-to-PR flow, human review, local scratch, Pen toolchain limits, the screen transplant procedure, and how to add a token, a System part, or a Component.
