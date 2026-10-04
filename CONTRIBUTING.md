# Contributing to hide

hide is a macOS app over the [Herdr](https://herdr.dev) runtime.
The Rust core in `herdr-core/` owns every piece of state; the `hided` daemon serves a snapshot of it to the web shell in `web/`, which the Electron host in `desktop/` shows in its own window and dispatches typed events back from.
`AGENTS.md` keeps the rules; [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) owns the architecture and the Herdr wire boundary with their reasons, [docs/BUILD.md](docs/BUILD.md) the build output and worktree rules, and [docs/PERFORMANCE_TESTING.md](docs/PERFORMANCE_TESTING.md) the performance rules that came out of real incidents.
Read `AGENTS.md` and the architecture guide before changing anything under `herdr-core/`, `hided/` or `desktop/`; [docs/UI_BEHAVIOR.md](docs/UI_BEHAVIOR.md) before changing anything a user looks at, and [docs/DESIGN_WORKFLOW.md](docs/DESIGN_WORKFLOW.md) before making a design change.
Use [docs/README.md](docs/README.md) to find current guides and distinguish historical/reference-only material.

## Before you open a pull request

Run the same required lanes CI runs.
These are the local equivalents; the remote `verify` result still depends on the actual CI run.
CI runs the Rust suite, the unit suites, the invariant checks and the whole web end-to-end (six shards) on Linux, where the Herdr schema comparison and the web end-to-end use the pinned release's Linux asset, and keeps two jobs on macOS, the platform hide ships on: the Electron end-to-end and the web end-to-end's `@platform` tests.
`.github/workflows/pr.yml` says why; `nightly.yml` configures full web and desktop suites and package checks on Linux, macOS and Windows, with tracked flaky tests blocking the nightly result.
Read the executed jobs and their skips before claiming an OS is verified; a configured matrix is not execution evidence.

```sh
bash scripts/verify-cargo.sh lint                # cargo fmt --check, then clippy over every target
bash scripts/verify-cargo.sh test                # herdr-core (labels included), hided, hide-ai and hide-agent-hooks
bash scripts/verify-web.sh                       # hcoord typecheck/build/unit/e2e, then web and desktop typecheck, lint, test and build
bash scripts/verify-cargo.sh build -p hided
bash scripts/verify-web.sh web build
bash scripts/verify-web.sh web e2e       # Playwright against a local hided: missing Herdr, and an isolated pinned Herdr (HIDE_E2E_HERDR_BIN, HERDR_BIN_PATH or PATH) for the S2 flows and the S3 Explorer, editor, viewers, attach, watch and reconnect flows (one worker, because each spec starts its own Herdr, hided and browser; CI deals the same tests out to six Linux shards)
bash scripts/verify-cargo.sh cli
bash scripts/verify-web.sh desktop e2e            # Playwright `_electron` against a private hided and the pinned Herdr; windows never activate the app except in `@needs-focus` specs, which `--grep-invert @needs-focus` skips
MEASURE_SCENARIO=multi HIDE_MEASURE_RUN_DIR=agents/runs/<slug>/measure/<attempt> bash scripts/web-shell-measure/run.sh   # echo and frame gates with four splits and five attached tabs; review-required evidence, not a CI check
bash scripts/check-harness-ignore-anchor.sh
bash scripts/check-agent-asset-committed.sh
bash scripts/check-capability-readers-off-lock.sh
python3 -m unittest discover -s scripts/tests -p 'test_*.py'
node --test scripts/tests/nightly-report.test.cjs
bash scripts/check-no-workstation-identity.sh
bash scripts/check-worktree-removal-boundary.sh
zsh scripts/check-herdr-pin-single-source.sh
python3 scripts/check-herdr-schema.py             # needs a Herdr CLI on PATH or --herdr-bin
bash scripts/verify-cargo.sh test-scoped -p hide-platform -p hide-herdr-client # the OS contract and the client's own tests; add HIDE_E2E_HERDR_BIN=<pinned herdr> and `--test real_herdr -- --ignored` to run the client against it
```

Then open the pull request against `main` and answer the template.
`main` accepts pull-request merges, and the `verify` check has to pass; this repository currently uses merge commits, and there is no way around branch protection, including for maintainers.

## CI gates

`verify` is posted on every PR and main push.
`ci-plan.py` records the tested checkout, PR head/base/merge base, version, selected lanes and exclusion reasons before jobs run.
Docs select policy checks; Rust leaves select their actual reverse dependencies; ordinary web and desktop changes select their package checks and end-to-end lanes.
Shared host, wire, terminal, store and shortcut changes include their actual desktop/browser consumers.
Unknown paths, workflow/common-fixture changes, rename/delete and unsafe diff states select full coverage, including the entire Rust workspace.
The aggregate fails on any missing, unknown, failed, cancelled or unexpectedly skipped selected lane, and on an unexpected run of an excluded lane.
Main runs every lane and nightly retains full three-OS coverage.
Run `python3 scripts/ci-plan.py plan --base <base-sha> --head <head-sha>` to inspect the selection locally; `python3 scripts/ci-plan.py rust --plan ci-plan.json` runs its Rust selection through the verification entrypoint.

Every required check exists because something once went wrong without it.
The table says what each one protects, how to run it locally, and what to do when it blocks you.
There is no label or bypass for any of them; when a gate is wrong, change the gate in the same pull request and say why in the description.

| Gate | Protects | Local command | When it blocks you |
| --- | --- | --- | --- |
| `cargo fmt` | Rust formatting stays deterministic across the workspace, so reviews do not accumulate unrelated style drift | `bash scripts/verify-cargo.sh lint` (`cargo fmt --all --check`) | Run the same command without `--check` and commit the machine-generated formatting separately. |
| `cargo clippy` | Every Rust target in the workspace is warning-free, including tests and generated-contract consumers | `bash scripts/verify-cargo.sh lint` (`cargo clippy --locked --workspace --all-targets -- -D warnings`) | Fix a warning when that clarifies the code; use a narrow, explained allowance when the alternative would obscure a generated or performance-sensitive boundary. |
| `cargo test` | The core's behavior including its Herdr fixtures, hided handshake and state-file rules, the `hide-ai` router and codex backend against a fake app server, the core's label worker (`herdr-core/src/labels/tests.rs`), and the agent-hook crate's configuration rules | `bash scripts/verify-cargo.sh test` | Fix the test or the code. A fixture that no longer matches Herdr means the pin moved; see `AGENTS.md`, Herdr API Contract. |
| hcoord typecheck/build/unit/e2e | hcoord (a part of hide, installed by its kit) preserves its CLI and ledger contract, converges daemon ownership, writes portable lineage tokens, and keeps remote operations bounded and recoverable | `bash scripts/verify-web.sh hcoord typecheck`, `build`, `test`, `test:e2e` (also part of `bash scripts/verify-web.sh`) | Fix the plugin or its isolated fixture. Never point the suite at the operator's live hcoord home or Herdr socket. |
| web typecheck/lint/test/build/e2e | The web shell's store merge and structural sharing, modifier-key bytes, connection machine, shortcut registry, close policy, resize math, project projection and registration checks, and Playwright against hided: the missing-socket row, the refused token, the sidebar click -> pane switch -> echo flow, and the S2 flow (two checkouts, three tabs, two splits, zoom, reorder, closes, the ⌘/ sheet, a socket drop, one registration and its refusals) and the S3 flow (the Explorer tree, a Git-decorated row, editor open/edit/save/conflict, Markdown Live, image/PDF/video viewers, create/rename/move/trash, a watch refresh, ⌘P/⌘K, a file drop, a buffer reconnect) on an isolated pinned Herdr | `bash scripts/verify-web.sh` (typecheck, lint, test and build for both `web/` and `desktop/`), then `bash scripts/verify-web.sh web build` and `bash scripts/verify-web.sh web e2e` | Fix the test or the code. Build `web/dist` first; e2e needs `target/debug/hided` (`bash scripts/verify-cargo.sh build -p hided` first; the e2e never rebuilds it), the pinned `herdr` (`HIDE_E2E_HERDR_BIN`, `HERDR_BIN_PATH` or PATH) and `cc` for the fake agent. |
| desktop typecheck/lint/test/e2e | The desktop app's CLI resolution order and answer parsing, window-bounds restore, the menu built from the registry's Electron column, the environment registry and each system's install folders, the paths the page names and is answered in the wire spelling (the Windows rules checked on every system), the one-child spawn helper, and Playwright `_electron` against a private hided and pinned Herdr: attach and the shell, the native chords and a menu click as one `create_tab` each, the ⌘/ sheet's Electron chords, external links, terminal links (a URL, a file at its line and a folder opened in the Workspace, ⌘-click and an outside path handed to macOS with a program there only revealed, OSC 8 links with no dialog, a wrapped path, a drag that selects rather than opens), the OS file manager reveal from the Explorer, History, View tab and sidebar menus with a refused path logged without it, quit leaving hided running, a second launch, a focus guard that fails any test not tagged `@needs-focus` whose app comes forward, the missing-CLI screen and Retry, finding `hide` with PATH lacking it in `~/.local/bin` and then through the remembered path, and re-attaching after the daemon dies | `bash scripts/verify-web.sh`, then `bash scripts/verify-web.sh desktop electron-install` and `bash scripts/verify-web.sh desktop e2e` | Fix the test or the code; an Electron acquisition failure blocks CI once at its five-minute prerequisite, with the upstream error retained. e2e needs `web/dist`, `target/debug/hide`, `hided`, `hide-agent-hooks` and `hide-host-helper`, `plugins/hcoord/dist` (which `verify-web.sh` builds), and the pinned `herdr` as the web e2e does; it never touches the operator's daemon. |
| harness ignore anchor | `/agents/` is ignored and `.claude/agents/` is not | `bash scripts/check-harness-ignore-anchor.sh` | Keep the leading slash on the ignore rule. |
| agent asset committed | The simplification subagent stays a tracked file | `bash scripts/check-agent-asset-committed.sh` | `git add` it; it once became uncommittable through an unanchored ignore rule. |
| capability readers off lock | No production code forks a subprocess while the runtime mutex is held, and every production reader runs from the session-sync coordinator; inline test modules are excluded | `bash scripts/check-capability-readers-off-lock.sh` | Move the subprocess to a reader driven by the coordinator; see `AGENTS.md`, Performance Guide. |
| no workstation identity | No tracked text file names a real home directory or machine, no run-artifact path is tracked, and no browser profile file is tracked | `bash scripts/check-no-workstation-identity.sh` | Use `/Users/example` in fixtures and a neutral placeholder in UI. Move run artifacts under `agents/runs/<slug>/`; a `git add -f` past the ignore rule is what this refuses. |
| script suite | Measurement semantics stay honest, and no gate a workflow runs calls a tool the runner lacks or a script that is untracked or absent | `python3 -m unittest discover -s scripts/tests -p 'test_*.py'` | Fix the measurement semantics; do not trim inconvenient observations. For a portability failure, reach for `git grep` rather than installing the tool on the runner. |
| toolchain reuse | Every script that runs cargo sources `scripts/toolchain-env.sh`, so a runner HOME reuses the machine's toolchain instead of installing a private copy | `python3 -m unittest discover -s scripts/tests -p 'test_*.py'` | Source the resolver rather than recovering the toolchain yourself. rustup auto-installs into an empty `$HOME/.rustup` and still exits 0, which is what made the two earlier failure-guarded workarounds dead code. |
| verification builds | `verify-cargo.sh test` and `release` build inside the checkout, observe a changed core and propagate a failing test or compile error, and `release` leaves every binary the packager ships executable in `target/release` | `python3 -m unittest discover -s scripts/tests -p 'test_*.py'` | Keep the build in-tree; a caller's `CARGO_TARGET_DIR` must not move the binaries `desktop/scripts/package.mjs` reads. |
| worktree removal boundary | The host's confirmed removal (`hide_host::worktrees::remove_confirmed`) forces only what the operator accepted in the delete confirmation: `git worktree remove --force` only with Discard ticked, `git branch -D` only for a branch they were told is unmerged; the Overview cleanup (`worktree_cleanup.rs`) never forces | `bash scripts/check-worktree-removal-boundary.sh` | Tie any force to the operator's recorded choice; a removal nobody confirmed must fail and surface the reason. |
| herdr pin single source | The Herdr version and digest live only in `herdr-bundle.json` | `zsh scripts/check-herdr-pin-single-source.sh` | Derive from the manifest; never restate the value. Bump with `scripts/bump-herdr.sh <version>`. |
| herdr schema contract | The binary schema equals `contracts/herdr-api.schema.json` after canonical JSON normalization and its version matches the manifest, on all three systems | `python3 scripts/check-herdr-schema.py --herdr-bin <pinned herdr>` | The schema moved with a Herdr release; update the contract and every call site it names, then the fixtures. |
| os contract | What differs between macOS, Linux and Windows behaves the same on all three, observed from the caller: the local stream under the Herdr client, owned children and what the kernel says about a pid, private files, locks, atomic replacement, links, file identity and disk space, folder watching (a change in two seconds, no change for a read, a bounded burst that ends in an overflow), where the account's things live, and the client and the default socket against the pinned Herdr for that system (the Windows zip runs only here) | `bash scripts/verify-cargo.sh test-scoped -p hide-platform -p hide-herdr-client`, then `HIDE_E2E_HERDR_BIN=<pinned herdr> bash scripts/verify-cargo.sh test-scoped -p hide-herdr-client --test real_herdr -- --ignored` | A contract failed on one system: fix the code under it, not the test; the Linux and Windows legs are `os-contract.yml`, the macOS leg runs in `desktop-e2e`. |
| windows check | The workspace compiles for Windows and the daemon, kit, hook and web/desktop unit contracts hold there | On Windows: `bash scripts/verify-cargo.sh check --workspace --all-targets`, `bash scripts/verify-cargo.sh test-scoped -p hided --lib pane_auth`, `bash scripts/verify-cargo.sh test-scoped -p hided --test connect_stop`, `bash scripts/verify-cargo.sh test-scoped -p hided --test handshake`, `bash scripts/verify-cargo.sh test-scoped -p hide-kit`, `bash scripts/verify-cargo.sh test-scoped -p hide-agent-hooks`, `HIDE_E2E_HERDR_BIN=<pinned herdr.exe> bash scripts/verify-cargo.sh test-scoped -p hided --test real_herdr -- --ignored`, and `bash scripts/verify-web.sh {web,desktop} {typecheck,test}` | Fix a Windows-only failure under the same assertion; local link tests need Developer Mode or administrator authority. The `windows-check` job in `pr.yml` runs independently of browser provisioning. |
| Windows browser smoke | The real pinned Windows Herdr and browser preserve the existing `@platform` flows | Build hided and `web/dist`, install Chromium, then `HIDE_E2E_HERDR_BIN=<pinned herdr.exe> bash scripts/verify-web.sh web e2e --grep @platform` on Windows | The `windows-e2e` job calls `web-e2e.yml` with one shard. Its compile, fixture and browser failures remain distinct from `windows-check`; both selected lanes must succeed. |

The exact quarantine registry is the only way a web or desktop e2e test leaves a required gate; [docs/TESTING.md](docs/TESTING.md#flaky-tests) owns when a test may be tagged and which lanes still run it.

A web e2e test that exercises what differs by operating system is tagged `{ tag: "@platform" }` with a comment saying what, and a pull request runs those on macOS and Windows as well as in the full Linux shards: Trash, process ownership, file watching and saving, worktree paths, disk cleanup, terminal input and echo through the platform's Herdr, and platform editing chords and browser-composed Korean input.
Every web e2e presses its chords through `web/e2e/chords.ts`, which reads them from the shell's registry for the runner's system, so the Linux shards press the Ctrl+Shift and Alt+Shift chords Windows and Linux use and the macOS run presses the ⌘ ones; a spec spells a chord itself only when it binds one in Settings or presses one the shell must not answer.
The PR runs the remaining web tests on Linux.
The nightly configures the full web suite on Linux (six shards), macOS and Windows (four shards each), the full desktop suite on all three, per-OS schema/runtime contracts, and all three packages.
The Linux desktop runner uses Xvfb; Windows and macOS use the runner desktop.
Every failed, cancelled or skipped dependency reaches the nightly report on main, which opens one `bug` issue or comments once per run attempt on the existing thread.
The reporter's API failures fail its job, and its bounded issue/comment search fails on overflow instead of creating another thread.
Package smoke and simulated hooks prove private fixture behavior; physical IME/candidate windows, real agent hook sessions, first-launch OS security prompts and operator PATH remain device QA.
Run the platform set after the web build with `bash scripts/verify-web.sh web e2e --grep @platform`.
Press an editor or clipboard chord in a spec with `ControlOrMeta`: Playwright binds its editing commands (copy, start of document) to the host system, so a `Meta` press only works on macOS.

The gates that read a running Herdr server, drive the built app, or reach the network are local steps and are not required in CI.
They are listed under "Local gates" below; every script in `scripts/` is either a required gate above, a local gate there, or a generator named in [docs/DESIGN_WORKFLOW.md](docs/DESIGN_WORKFLOW.md).
The separate `design-contract.yml` workflow runs `node scripts/check-design-contract.mjs`, `node --test scripts/tests/pen-gallery.test.mjs`, `node --test scripts/tests/pen-transplant.test.mjs`, `node --test scripts/tests/design-scratch.test.mjs`, `node --test scripts/tests/design-review.test.mjs`, and `node --test scripts/tests/hide-screens.test.mjs`.
The shared entrypoint runs `check-pen.mjs` (the Pen library against what the token generator would write: token values, the Foundations sheet, the `System /`/`Component /` sheet-naming band, and one id per node), `check-pen-gallery.mjs` (the library's `System /` sheets against `web/src/gallery/manifest.ts`, part by part and state by state), `check-web-tokens.mjs` (every web source reaching a color, size, or radius through a token rather than a literal), and `check-hide-screens.mjs` (`design/hide-screens.pen` against the library it imports: `Screen /` sheets with Light and Dark frames, resolving references, locally restated colors, and variables matching `design/tokens.json`); it performs static checks, not desktop interaction. See [docs/DESIGN_WORKFLOW.md](docs/DESIGN_WORKFLOW.md) for what each one refuses.
The gallery test exercises the comparison against fixtures; the screens test exercises each `check-hide-screens.mjs` refusal and a transplanted result; the transplant test exercises `pen-transplant.mjs` against fixture sheets for a clean move, a missing sheet id, and an untouched neighbour sheet.
Scratch tests exercise the creator against a fake third-party Pen CLI: worktree-local linking, overwrite/path guards, failed imports and process cancellation.
They do not prove Pen rendering; import, rendering and reopen verification with the real CLI stays a local step.

### Local design hook

Run `node scripts/check-design-contract.mjs` for immediate feedback on working-tree sources.
The tracked `.githooks/pre-commit` checks staged content through `node scripts/check-design-contract.mjs --staged`.
Use `git -c core.hooksPath=.githooks commit` to enable it for one commit without changing shared Git configuration or other worktrees.
This is opt-in; the hook is not installed automatically and an ordinary commit does not imply it ran.
If an existing hook is already configured, retain it and call the shared staged entrypoint from that hook rather than replacing its hook path.
CI independently runs the same checks even when the local hook was not enabled.
No branch-protection setting is changed by this repository patch.

Stage the checker and affected sources together: staged verification executes the staged checker files and reads staged design and web sources (`design/hide-ui.lib.pen`, `design/tokens.json`, `web/src/**`), ignoring unstaged repairs or new violations.
A missing script, conflict, non-ordinary source input or checker failure blocks the hook with its cause.
The checker does not stage, stash, restore or modify files.
When a check fails, reuse the documented owner (`web/src/components/ui` for a `System /` part, `web/src/components` for a `Component /`) or fix the source; see [docs/DESIGN_WORKFLOW.md](docs/DESIGN_WORKFLOW.md) for the token/System-part/Component procedures.
Keep visual acceptance separate: a passing check proves the Pen library, the gallery, and the tokens agree with each other, not that a composition looks right; human comparison against a gallery or app capture, recorded under `agents/runs/<slug>/`, is what proves that.

## Local gates

None of these run in CI, and a green `verify` says nothing about them.
A script that stops earning its place here is deleted rather than left unreferenced; five gates once rotted silently because nothing named them, and three of those were asserting a symbol the design system had renamed.

| Command | Checks | Needs |
| --- | --- | --- |
| `bash scripts/check-hide-full.sh` | Everything CI requires plus every local gate below that runs unattended | A full build; writes `target/hide-full.log` in the checkout |
| `node scripts/check-hide-design-enforcement.mjs` | `design-contract.yml` still binds the real checkers, so this list cannot drift from CI | - |
| `zsh scripts/check-herdr-contract.sh` | The full contract, including the responses only a live server answers | A running Herdr server |
| `bash scripts/check-typed-contract.sh <stage>` | The typed wire boundary: `generated`, `behavior`, `structure` or `suites` | `suites` builds |
| `bash scripts/check-typed-live-remote.sh <stage>` | The same boundary against a live and a remote server: `structure`, `behavior`, `probe`, `suites` or `attribution` | An authorized remote fixture |
| `python3 scripts/check-no-attribution.py` | No AI tooling attribution in the branch name, the commits, or a prepared PR body | A fetched `origin/main`; `--range` and `--pr-body` override the defaults |
| `python3 scripts/check-herdr-release.py <source\|asset>` | A Herdr release at its public and local source boundaries before the pin moves | A Herdr checkout or a reference binary |
| `python3 scripts/check-worktree-performance-evidence.py [dir]` | A worktree performance run recorded what the guide requires | A completed native run directory |
| `zsh scripts/install-local-runtime.sh --herdr-root PATH` | Not a gate: installs a locally built Herdr for runtime work | A Herdr checkout |
| `node scripts/design-scratch.mjs <task-slug>` | Not a gate: creates an ignored scratch linked to this worktree's design library; see [docs/DESIGN_WORKFLOW.md](docs/DESIGN_WORKFLOW.md) for editing and human review | The verified Pen CLI version and an existing Pen login |
| `node scripts/design-review.mjs review <slug> --baseline <bundle>` | A design change's production screen against its reference bundle: the design contract, the bundle's layout rules measured in Chromium, and reference / current Pen / actual comparisons under matching conditions; see [docs/DESIGN_WORKFLOW.md](docs/DESIGN_WORKFLOW.md#reference-bundle-and-review-run) | `pnpm --dir web install`; Pen and a Pen login for the current Pen column, otherwise the run is INCOMPLETE |

## Performance-sensitive changes

Changes that add resident work or touch a high-frequency path must include a cost review in the PR's Review section, following [Resident work cost review](docs/PERFORMANCE_TESTING.md#resident-work-cost-review).
Explain added work per input or tick, notification fan-out and the pending-work bound, with the code owner and cleanup on every exit path.
Name missing caps explicitly rather than treating a refresh interval, timeout or one active worker as a queue or byte limit.
Update the [state placement guide](docs/ARCHITECTURE.md#state-placement-and-publication), [process ownership table](docs/ARCHITECTURE.md#resident-process-ownership) and [resident work ledger](docs/PERFORMANCE_TESTING.md#resident-ticks-timers-and-watchers) when their contracts change.
Read [PERFORMANCE_TESTING.md](docs/PERFORMANCE_TESTING.md#verification-layers-and-current-ci-coverage) for the three verification layers and review policy.
The Rust and web suites include deterministic performance-related regression tests, and `pr.yml` runs web and Electron desktop end-to-end scenarios against private Herdr/hided fixtures.
Typing, drag, wheel, focus, project Tree/List and destructive cleanup review in the packaged app, and controlled latency/RSS comparisons remain isolated local QA.
Cleanup deletion tests must use a private fixture root; never use an operator project as a cleanup target.
The guide's maintenance policy requires affected app scenarios for input/rendering/lifecycle changes and matched measurements for performance claims; this is review-required evidence, not a branch-protection check today.
Record completed and unrun checks in the PR's Evidence section; a green `verify` result alone does not prove responsiveness in the app.

## Evidence

Screenshots, traces, sample output, and run logs are run artifacts, not source.
They never enter a commit; write them under `agents/runs/<slug>/`, which is ignored, and attach a copy to the pull request when a reviewer needs to see it.
`AGENTS.md` records why: a committed evidence tree once carried a browser profile with cookies and 184 screenshots showing a home directory.

## Releases

A release is a tag on a commit that is already on `main`, never on a branch.
Pushing a stable `vX.Y.Z` tag runs `verify-cargo.sh test` and `verify-web.sh` again on macOS, then packages the app with `HIDE_VERSION=<version> pnpm --dir desktop package` on macOS, Windows and Linux runners.
The last two use `package.yml`, whose `node desktop/scripts/smoke-package.mjs` unpacks each package, installs the kit, resolves the command in a fresh shell and checks daemon replacement with a second complete package fixture.
The [release asset gate](docs/BUILD.md#release-asset-gate) requires that version's `hide-v<version>-macos-arm64.zip`, `hide-v<version>-windows-x64.zip` and `hide-v<version>-linux-x64.tar.gz`, each with its verified `.sha256`, before draft preparation.
Preparation also requires owner-enforced GitHub release immutability and a `RELEASE_POLICY_TOKEN` secret with Administration read access; absent or unverified prerequisites leave the build artifacts available and block release writes.
The writer adds only missing assets to its exact verified draft and never deletes, overwrites, or publishes a release; legacy drafts require separate coverage review.
A maintainer publishes only after reviewing separate installation, native first-launch and terminal input/output evidence on every supported system.
A package checksum or headless daemon smoke check does not provide that native evidence.
The macOS archive is ad-hoc signed, not notarized, so the first launch needs the Gatekeeper step [docs/INSTALL.md](docs/INSTALL.md) describes; the Windows and Linux packages are not signed, and the same guide gives the SmartScreen and sandbox steps.
`package.yml` also runs on a pull request that changes what goes into a package (`desktop/scripts/`, `desktop/package.json`, `desktop/resources/`, the Herdr pin and its fetch scripts, `verify-cargo.sh` and `toolchain-env.sh`, `hided/build.rs`, which embeds the web shell, `hided/src/cli.rs`, `hide-kit/` and `hide-agent-hooks/`, and the package and release workflows) and keeps the packages for a week as the run's artifacts; it is not part of `verify`, because a check that runs on some pull requests only cannot be required.

## Bundled Herdr runtime

The version and digest of the Herdr binary the app ships are pinned in `contracts/herdr-bundle.json` and nowhere else.
`scripts/bump-herdr.sh <version>` moves the pin after verifying the release asset; a weekly workflow proposes that bump as a pull request when a new stable Herdr release appears.
It never merges, because three Herdr behaviors the core relies on are covered by fixtures this repository wrote, not by Herdr's own tests.

## Commit messages and attribution

Write commits as project work: what changed in the product, code, or documentation, and why.
Do not credit an AI agent, model, vendor, or tool in commit messages, trailers, branch names, or pull request text.
When the change's subject is one of the integrated products, name it in full or quote its command in backticks.

The nightly manual input `mode=full` keeps the complete scheduled matrix.
`mode=failure-controls` runs thirty fresh, retry-zero fixtures per selected native/browser failure case with each test's ordinary deadline, plus the original platform and kit suites.
The exact file/title contract in `contracts/ci-failure-controls.json` includes both pane-focus-ordering scenarios on macOS and Windows.
Each required identity must report all thirty distinct repeat indices, with no skipped result or retry; a partially executed grep group fails the receipt check.
Final acceptance also compares those identities with the original full web and desktop suites from `mode=full` on the same SHA and OS.
These diagnostics do not replace required PR lanes or full nightly coverage.
`mode=capacity` compares `max-parallel=4` and `6` on the same pushed SHA while preserving all six Linux shards and their full suites.
The normalized job history distinguishes queue, execution and first attempts; a capacity choice requires measured coverage and first-pass evidence rather than a configured runner count.
