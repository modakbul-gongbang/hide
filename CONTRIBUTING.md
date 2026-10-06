# Contributing to hide

hide is a macOS app over the [Herdr](https://herdr.dev) runtime.
The Rust core in `herdr-core/` owns every piece of state; the `hided` daemon serves a snapshot of it to the web shell in `web/`, which the Electron host in `desktop/` shows in its own window and dispatches typed events back from.
`AGENTS.md` keeps the rules; [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) owns the architecture and the Herdr wire boundary with their reasons, [docs/BUILD.md](docs/BUILD.md) the build output and worktree rules, and [docs/PERFORMANCE_TESTING.md](docs/PERFORMANCE_TESTING.md) the performance rules that came out of real incidents.
Read `AGENTS.md` and the architecture guide before changing anything under `herdr-core/`, `hided/` or `desktop/`; [docs/UI_BEHAVIOR.md](docs/UI_BEHAVIOR.md) before changing anything a user looks at, and [docs/DESIGN_WORKFLOW.md](docs/DESIGN_WORKFLOW.md) before making a design change.
Use [docs/README.md](docs/README.md) to find current guides and distinguish historical/reference-only material.

## Before you open a pull request

Run the same required lanes CI runs.
These are the local equivalents; the remote `verify` result still depends on the actual CI run.
CI's `plan` job picks the lanes a pull request needs from the paths it changes, and `verify` requires exactly those; a push to main runs every lane, and [docs/TESTING.md](docs/TESTING.md#which-lanes-a-pull-request-runs) owns the rules.
Across its lanes CI runs the Rust suite, the unit suites, the invariant checks, the whole web end-to-end (six shards) and the remote mailbox lane on Linux, where the Herdr schema comparison and the Linux jobs use the pinned release's Linux asset; two jobs on macOS, the platform hide ships on: the Electron end-to-end and the web end-to-end's `@platform` tests; and on Windows `windows check` (compile, daemon and unit suites) and `windows e2e` (the browser `@platform` tests).
`.github/workflows/pr.yml` says why; `nightly.yml` configures full web and desktop suites and package checks on Linux, macOS and Windows, where a test fails the nightly only when it fails its retry too.
Read the executed jobs and their skips before claiming an OS is verified; a configured matrix is not execution evidence.

```sh
bash scripts/verify-cargo.sh lint                # cargo fmt --check, then clippy over every target
bash scripts/verify-cargo.sh test                # herdr-core (labels included), hided, hide-ai and hide-agent-hooks
bash scripts/verify-web.sh                       # web and desktop typecheck, lint, test and build
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
python3 scripts/ci-plan.py plan --event pull_request --base origin/main --head HEAD   # the lanes CI would plan, only when HEAD is a merge commit; otherwise it plans every lane
node --test scripts/tests/nightly-report.test.cjs
bash scripts/check-no-workstation-identity.sh
bash scripts/check-worktree-removal-boundary.sh
zsh scripts/check-herdr-pin-single-source.sh
python3 scripts/check-herdr-schema.py             # needs a Herdr CLI on PATH or --herdr-bin
bash scripts/verify-cargo.sh test-scoped -p hide-platform -p hide-herdr-client # the OS contract and the client's own tests; add HIDE_E2E_HERDR_BIN=<pinned herdr> and `--test real_herdr -- --ignored` to run the client against it
```

Then open the pull request against `main` and answer the template.
Write the issue it finishes on the `Closes #N` line and nothing else there; a big issue split across several pull requests gets one sub-issue per piece, and each pull request closes its own.
`Related:` names only the PRD path and the pull requests this one follows or depends on, because Hide and GitHub relate a pull request to an issue only through a closing keyword.
`main` accepts pull-request merges, and the `verify` check has to pass; this repository currently uses merge commits, and there is no way around branch protection, including for maintainers.

## CI gates

Every required check exists because something once went wrong without it.
The table says what each one protects, how to run it locally, and what to do when it blocks you.
There is no label or bypass for any of them; when a gate is wrong, change the gate in the same pull request and say why in the description.

| Gate | Protects | Local command | When it blocks you |
| --- | --- | --- | --- |
| `cargo fmt` | Rust formatting stays deterministic across the workspace, so reviews do not accumulate unrelated style drift | `bash scripts/verify-cargo.sh lint` (`cargo fmt --all --check`) | Run the same command without `--check` and commit the machine-generated formatting separately. |
| `cargo clippy` | Every Rust target in the workspace is warning-free, including tests and generated-contract consumers | `bash scripts/verify-cargo.sh lint` (`cargo clippy --locked --workspace --all-targets -- -D warnings`) | Fix a warning when that clarifies the code; use a narrow, explained allowance when the alternative would obscure a generated or performance-sensitive boundary. `clippy.toml` refuses `std::thread::sleep`; see [docs/TESTING.md](docs/TESTING.md#writing-a-rust-test). |
| `cargo test` | The core's behavior including its Herdr fixtures, hided handshake and state-file rules, the `hide-ai` router and codex backend against a fake app server, the core's label worker (`herdr-core/src/labels/tests.rs`), and the agent-hook crate's configuration rules | `bash scripts/verify-cargo.sh test` | Fix the test or the code. A fixture that no longer matches Herdr means the pin moved; see `AGENTS.md`, Herdr API Contract. |
| web typecheck/lint/test/build/e2e | The web shell's store merge and structural sharing, modifier-key bytes, connection machine, shortcut registry, close policy, resize math, project projection and registration checks, and Playwright against hided: the missing-socket row, the refused token, the sidebar click -> pane switch -> echo flow, and the S2 flow (two checkouts, three tabs, two splits, zoom, reorder, closes, the ⌘/ sheet, a socket drop, one registration and its refusals) and the S3 flow (the Explorer tree, a Git-decorated row, editor open/edit/save/conflict, Markdown Live, image/PDF/video viewers, create/rename/move/trash, a watch refresh, ⌘P/⌘K, a file drop, a buffer reconnect) on an isolated pinned Herdr | `bash scripts/verify-web.sh` (typecheck, lint, test and build for both `web/` and `desktop/`), then `bash scripts/verify-web.sh web build` and `bash scripts/verify-web.sh web e2e` | Fix the test or the code. Build `web/dist` first; e2e needs `target/debug/hided` (`bash scripts/verify-cargo.sh build -p hided` first; the e2e never rebuilds it), the pinned `herdr` (`HIDE_E2E_HERDR_BIN`, `HERDR_BIN_PATH` or PATH) and `cc` for the fake agent. Lint also runs the Playwright rules and the e2e test size budget ([docs/TESTING.md](docs/TESTING.md#writing-a-playwright-e2e-test)). |
| desktop typecheck/lint/test/e2e | The desktop app's CLI resolution order and answer parsing, window-bounds restore, the menu built from the registry's Electron column, the environment registry and each system's install folders, the paths the page names and is answered in the wire spelling (the Windows rules checked on every system), the one-child spawn helper, and Playwright `_electron` against a private hided and pinned Herdr: attach and the shell, the native chords and a menu click as one `create_tab` each, the ⌘/ sheet's Electron chords, external links, terminal links (a URL, a file at its line and a folder opened in the Workspace, ⌘-click and an outside path handed to macOS with a program there only revealed, OSC 8 links with no dialog, a wrapped path, a drag that selects rather than opens), the OS file manager reveal from the Explorer, History, View tab and sidebar menus with a refused path logged without it, quit leaving hided running, a second launch, a focus guard that fails any test not tagged `@needs-focus` whose app comes forward, the missing-CLI screen and Retry, finding `hide` with PATH lacking it in `~/.local/bin` and then through the remembered path, and re-attaching after the daemon dies | `bash scripts/verify-web.sh`, then `bash scripts/verify-web.sh desktop electron-install` and `bash scripts/verify-web.sh desktop e2e` | Fix the test or the code; an Electron acquisition failure blocks CI once at its five-minute prerequisite, with the upstream error retained. e2e needs `web/dist`, `target/debug/hide`, `hided`, `hide-agent-hooks` and `hide-host-helper`, and the pinned `herdr` as the web e2e does; it never touches the operator's daemon. Lint also runs the Playwright rules and the e2e test size budget ([docs/TESTING.md](docs/TESTING.md#writing-a-playwright-e2e-test)). |
| harness ignore anchor | All root `/agents/` paths are ignored and absent from the index; `.claude/agents/` is not ignored | `bash scripts/check-harness-ignore-anchor.sh` | Keep the leading slash and remove tracking with `git rm --cached`; preserve local originals. |
| agent asset committed | The repository subagent stays tracked outside the private root harness | `bash scripts/check-agent-asset-committed.sh` | `git add` it; it once became uncommittable through an unanchored ignore rule. |
| capability readers off lock | No production code forks a subprocess while the runtime mutex is held, and every production reader runs from the session-sync coordinator; inline test modules are excluded | `bash scripts/check-capability-readers-off-lock.sh` | Move the subprocess to a reader driven by the coordinator; see `AGENTS.md`, Performance Guide. |
| no workstation identity | Tracked checkout bytes contain no detected home, workstation identity or contact-email shape, run-artifact path or recognizable browser profile artifact | `bash scripts/check-no-workstation-identity.sh`; add `--scope index` to check staged bytes | Use complete neutral account components such as `/Users/example` in fixtures. Use reserved email domains for synthetic contacts. Inspect images, encoded content, arbitrary identities, credentials, history and release payloads separately; a passing shape scan does not prove their absence. |
| script suite | No gate a workflow runs calls a tool the runner lacks or a script that is untracked or absent | `python3 -m unittest discover -s scripts/tests -p 'test_*.py'` | For a portability failure, reach for `git grep` rather than installing the tool on the runner. |
| toolchain reuse | Every script that runs cargo sources `scripts/toolchain-env.sh`, so a runner HOME reuses the machine's toolchain instead of installing a private copy | `python3 -m unittest discover -s scripts/tests -p 'test_*.py'` | Source the resolver rather than recovering the toolchain yourself. rustup auto-installs into an empty `$HOME/.rustup` and still exits 0, which is what made the two earlier failure-guarded workarounds dead code. |
| verification builds | `verify-cargo.sh test` and `release` build inside the checkout, observe a changed core and propagate a failing test or compile error, and `release` leaves every binary the packager ships executable in `target/release` | `python3 -m unittest discover -s scripts/tests -p 'test_*.py'` | Keep the build in-tree; a caller's `CARGO_TARGET_DIR` must not move the binaries `desktop/scripts/package.mjs` reads. |
| worktree removal boundary | The host's confirmed removal (`hide_host::worktrees::remove_confirmed`) forces only what the operator accepted in the delete confirmation: `git worktree remove --force` only with Discard ticked, `git branch -D` only for a branch they were told is unmerged; the Overview cleanup (`worktree_cleanup.rs`) never forces | `bash scripts/check-worktree-removal-boundary.sh` | Tie any force to the operator's recorded choice; a removal nobody confirmed must fail and surface the reason. |
| herdr pin single source | The Herdr version and digest live only in `herdr-bundle.json` | `zsh scripts/check-herdr-pin-single-source.sh` | Derive from the manifest; never restate the value. Bump with `scripts/bump-herdr.sh <version>`. |
| herdr schema contract | The binary schema equals `contracts/herdr-api.schema.json` after canonical JSON normalization and its version matches the manifest, on all three systems | `python3 scripts/check-herdr-schema.py --herdr-bin <pinned herdr>` | The schema moved with a Herdr release; update the contract and every call site it names, then the fixtures. |
| os contract | What differs between macOS, Linux and Windows behaves the same on all three, observed from the caller: the local stream under the Herdr client, owned children and what the kernel says about a pid, private files, locks, atomic replacement, links, file identity and disk space, folder watching (a change in two seconds, no change for a read, a bounded burst that ends in an overflow), where the account's things live, and the client and the default socket against the pinned Herdr for that system (the Windows zip runs only here) | `bash scripts/verify-cargo.sh test-scoped -p hide-platform -p hide-herdr-client`, then `HIDE_E2E_HERDR_BIN=<pinned herdr> bash scripts/verify-cargo.sh test-scoped -p hide-herdr-client --test real_herdr -- --ignored` | A contract failed on one system: fix the code under it, not the test; the Linux and Windows legs are `os-contract.yml`, the macOS leg runs in `desktop-e2e`. |
| windows check | The workspace compiles for Windows, and the daemon and app unit suites hold there with their existing assertions: hided's pane bootstrap and end-to-end tests, `hide connect` starting the `hided.exe` beside it and `hide stop` ending it, the install kit's Windows command shim and junction, the agent hook entries run through PowerShell, hided driving the pinned Herdr, a real listening port attributed to its working directory with native read failures exposed, and checkout cleanup excluded when its live listener starts through another path spelling, and the web and desktop unit suites | On Windows: `bash scripts/verify-cargo.sh check --workspace --all-targets`, `bash scripts/verify-cargo.sh test-scoped -p hided --lib -- pane_auth opener open_command workspace_cli`, `bash scripts/verify-cargo.sh test-scoped -p hided --test opener_lifecycle`, `bash scripts/verify-cargo.sh test-scoped -p hide-host --lib workspace_bridge`, `bash scripts/verify-cargo.sh test-scoped -p hided --test connect_stop`, `bash scripts/verify-cargo.sh test-scoped -p hided --test handshake`, `bash scripts/verify-cargo.sh test-scoped -p hide-kit`, `bash scripts/verify-cargo.sh test-scoped -p hide-agent-hooks`, `HIDE_E2E_HERDR_BIN=<pinned herdr.exe> bash scripts/verify-cargo.sh test-scoped -p hided --test real_herdr -- --ignored`, `bash scripts/verify-cargo.sh test-scoped -p herdr-core --lib ports::tests::a_real_listener_is_found_and_attributed_to_the_directory_it_runs_in -- --exact --nocapture`, `bash scripts/verify-cargo.sh test-scoped -p herdr-core --lib ports::tests::cleanup_keeps_a_checkout_in_use_when_a_listener_starts_through_another_spelling -- --exact --nocapture`, `bash scripts/verify-web.sh web typecheck`, `bash scripts/verify-web.sh web lint`, `bash scripts/verify-web.sh web test`, `bash scripts/verify-web.sh desktop typecheck`, `bash scripts/verify-web.sh desktop lint`, `bash scripts/verify-web.sh desktop test` (local path tests need Developer Mode or an administrator for links) | A Windows-only failure must be fixed under the same assertion; a spec's POSIX assumption is a portability gap, not permission to skip the flow. Unit checks can proceed after an earlier runtime failure, which still fails the required job. The job is in `pr.yml`. |
| windows e2e | The same browser `@platform` tests as on macOS hold on Windows against hided and the pinned Herdr for Windows | On Windows: `bash scripts/verify-cargo.sh build -p hided`, `bash scripts/verify-web.sh web build`, `bash scripts/verify-web.sh web playwright-install`, `HIDE_E2E_HERDR_BIN=<pinned herdr.exe> bash scripts/verify-web.sh web e2e --grep @platform` | Fix it under the same assertion, as for `windows check`. It is `web-e2e.yml` called from `pr.yml` with the Windows runner, and runs when the plan names it. |
| lane plan | Each lane runs exactly when the plan needs it, and `verify` fails a planned lane that did not succeed or an unplanned one that ran | `python3 -m unittest discover -s scripts/tests -p 'test_ci_plan.py'` | A lane left out a change it needed: fix the rule in `scripts/ci-plan.py` and add the path to its test. |

No test leaves a required gate: CI retries a failed test once and files a flaky one as an issue with a deadline, there is no quarantine tag or step, and a Rust test is never ignored for flakiness; [docs/TESTING.md](docs/TESTING.md#flaky-tests) owns the policy.

A web e2e test that exercises what differs by operating system is tagged `{ tag: "@platform" }` with a comment saying what, and a pull request whose plan includes the platform lanes runs those on macOS and Windows as well as in the full Linux shards: Trash, process ownership, file watching and saving, worktree paths, disk cleanup, terminal input and echo through the platform's Herdr, and platform editing chords and browser-composed Korean input.
Every web e2e presses its chords through `web/e2e/chords.ts`, which reads them from the shell's registry for the runner's system, so the Linux shards press the Ctrl+Shift and Alt+Shift chords Windows and Linux use and the macOS run presses the ⌘ ones; a spec spells a chord itself only when it binds one in Settings or presses one the shell must not answer.
The PR runs the remaining web tests on Linux.
The nightly configures the full web suite on Linux (six shards), macOS and Windows (four shards each), the full desktop suite on all three, per-OS schema/runtime contracts, and all three packages.
The Linux desktop runner uses Xvfb; Windows and macOS use the runner desktop.
Every failed, cancelled or skipped dependency reaches the nightly report on main, which opens one `bug` issue or comments once per run attempt on the existing thread.
The reporter's API failures fail its job, and its bounded issue/comment search fails on overflow instead of creating another thread.
Package smoke and simulated hooks prove private fixture behavior; physical IME/candidate windows, real agent hook sessions, first-launch OS security prompts and operator PATH remain device QA.
Run the platform set after the web build with `bash scripts/verify-web.sh web e2e --grep @platform`.
Press an editor or clipboard chord in a spec with `ControlOrMeta`: Playwright binds its editing commands (copy, start of document) to the host system, so a `Meta` press only works on macOS.

The remote mailbox crate-boundary lane is the `remote mailbox` job, on a Linux runner, planned by the crates it builds and tests; no CI job runs it on macOS.
Run it locally with `HIDE_E2E_HERDR_BIN=<pinned-binary> bash scripts/verify-cargo.sh test-scoped -p herdr-core --test remote_delivery -- --ignored`.
It starts only private Herdr servers and a loopback SSH account, with no external device.

The additional gates that read an existing Herdr server, drive an installed app, or reach an external device are local steps and are not required in CI.
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
| `bash scripts/check-typed-live-remote.sh <stage>` | The same boundary against a live and a remote server: `behavior`, `probe`, `suites` or `attribution` | An authorized remote fixture |
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

## Local harness transition

The complete root `agents/` namespace is private, including approved PRDs, configuration, interviews, rules and run evidence.
Reviewers use the pull request body and the owning public guides as the shared contract.
Never force-add a root harness file; `.claude/agents/` remains a separate public repository asset.
Application builds and installation do not require a local harness configuration.

Existing checkouts must save their tracked harness files before pulling the commit that removes them: Git deletes clean tracked files when it applies a deletion.
The following archive retains local edits as well as clean originals, and stays ignored.

```sh
mkdir -p agents/runs/harness-transition
git ls-files -z -- agents/ | tar --null -T - -cf agents/runs/harness-transition/tracked-agents.tar
# Pull the policy change, then restore the saved local originals.
tar -xf agents/runs/harness-transition/tracked-agents.tar
```

When preparing the removal in a checkout, use `git rm -r --cached -- agents/` to leave the originals on disk.
Neither procedure removes anything from public Git history, tags or previously published assets.

## Evidence

Screenshots, traces, sample output, and run logs are run artifacts, not source.
They never enter a commit; write them under `agents/runs/<slug>/`, which is ignored, and attach a copy to the pull request when a reviewer needs to see it.
`AGENTS.md` records why: a committed evidence tree once carried a browser profile with cookies and 184 screenshots showing a home directory.

## Releases

A release is a tag on a commit that is already on `main`, never on a branch.
Pushing a stable `vX.Y.Z` tag runs `verify-cargo.sh test` and `verify-web.sh` again on macOS, then packages the app with `HIDE_VERSION=<version> pnpm --dir desktop package` on macOS, Windows and Linux runners.
The last two use `package.yml`, whose `node desktop/scripts/smoke-package.mjs` unpacks each package, installs the kit, resolves the command in a fresh shell and checks daemon replacement with a second complete package fixture.
The [release asset gate](docs/BUILD.md#release-asset-gate) requires that version's `hide-v<version>-macos-arm64.zip`, `hide-v<version>-windows-x64.zip` and `hide-v<version>-linux-x64.tar.gz`, each with its verified `.sha256`, before draft preparation.
The operator keeps immutable releases turned on in the repository's Settings; the workflow does not check it.
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
