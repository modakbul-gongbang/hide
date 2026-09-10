# Contributing to hide

hide is a macOS shell over the [Herdr](https://herdr.dev) runtime.
The Rust core in `herdr-core/` owns every piece of state; the SwiftUI shell in `macos/` renders a snapshot of it and dispatches typed events back.
`AGENTS.md` describes that architecture, the Herdr API contract, and the performance rules that came out of real incidents.
Read it before changing anything under `herdr-core/` or `macos/`; `DESIGN.md` before changing anything a user looks at.
Use [docs/README.md](docs/README.md) to find current guides and distinguish historical/reference-only material.

## Before you open a pull request

Run the same three lanes CI runs.
These are the local equivalents; the remote `verify` result still depends on the actual CI run.

```sh
cargo fmt --all --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace                   # herdr-core, hide-ai and the context-label plugin
cargo build --release --locked -p herdr-core      # the shell links target/release/libherdr_core.a
swift test --package-path macos
bash scripts/check-right-panel-sections.sh
bash scripts/check-shortcut-contract.sh
bash scripts/check-harness-ignore-anchor.sh
bash scripts/check-agent-asset-committed.sh
bash scripts/check-capability-readers-off-lock.sh
bash scripts/check-terminal-row-cache.sh
python3 -m unittest discover -s scripts/tests -p 'test_*.py'
bash scripts/check-no-workstation-identity.sh
bash scripts/check-git-worktree-presentation.sh
bash scripts/check-git-worktree-states.sh
bash scripts/check-worktree-base-policy.sh
bash scripts/check-worktree-catalog-presentation.sh
bash scripts/check-worktree-removal-boundary.sh
zsh scripts/check-herdr-pin-single-source.sh
zsh scripts/check-herdr-contract.sh --schema-only   # needs a Herdr CLI on PATH or --herdr-bin
```

Then open the pull request against `main` and answer the template.
`main` accepts squash merges only, and the `verify` check has to pass; there is no way around it, including for maintainers.

## CI gates

Every required check exists because something once went wrong without it.
The table says what each one protects, how to run it locally, and what to do when it blocks you.
There is no label or bypass for any of them; when a gate is wrong, change the gate in the same pull request and say why in the description.

| Gate | Protects | Local command | When it blocks you |
| --- | --- | --- | --- |
| `cargo fmt` | Rust formatting stays deterministic across the workspace, so reviews do not accumulate unrelated style drift | `cargo fmt --all --check` | Run the same command without `--check` and commit the machine-generated formatting separately. |
| `cargo clippy` | Every Rust target in the workspace is warning-free, including tests and generated-contract consumers | `cargo clippy --locked --workspace --all-targets -- -D warnings` | Fix a warning when that clarifies the code; use a narrow, explained allowance when the alternative would obscure a generated or performance-sensitive boundary. |
| `cargo test` | The core's behavior including its Herdr fixtures, the `hide-ai` router and codex backend against a fake app server, the context-label plugin, and the agent-hook crate's configuration rules | `cargo test --locked --workspace` | Fix the test or the code. A fixture that no longer matches Herdr means the pin moved; see `AGENTS.md`, Herdr API Contract. |
| `swift test` | The shell's rendering and event contracts | `swift test --package-path macos` after the release core build | Same. `--filter <TestName>` narrows a run. |
| right panel sections | The Workbench name never returns to a user-facing string | `bash scripts/check-right-panel-sections.sh` | The panel presents exactly Overview, Explorer, Changes, and Git; rename, do not reintroduce. |
| shortcut contract | The right panel toggle is `⇧⌘B` and `⌘⌥B` is advertised nowhere | `bash scripts/check-shortcut-contract.sh` | Update the catalog and every label together. |
| harness ignore anchor | `/agents/` is ignored and `.claude/agents/` is not | `bash scripts/check-harness-ignore-anchor.sh` | Keep the leading slash on the ignore rule. |
| agent asset committed | The simplification subagent stays a tracked file | `bash scripts/check-agent-asset-committed.sh` | `git add` it; it once became uncommittable through an unanchored ignore rule. |
| capability readers off lock | Nothing forks a subprocess while the runtime mutex is held, and every reader runs from the session-sync coordinator | `bash scripts/check-capability-readers-off-lock.sh` | Move the subprocess to a reader driven by the coordinator; see `AGENTS.md`, Performance Guide. |
| terminal row cache | The terminal draw loop builds a row only when something it is drawn from changed | `bash scripts/check-terminal-row-cache.sh` | Reach text building through `preparedRow`, never from `drawTerminalContents`. This is a cost property, so no test fails when it is lost; see `AGENTS.md`, Performance Guide. |
| no workstation identity | No tracked text file names a real home directory or machine, no run-artifact path is tracked, and no browser profile file is tracked | `bash scripts/check-no-workstation-identity.sh` | Use `/Users/example` in fixtures and a neutral placeholder in UI. Move run artifacts under `agents/runs/<slug>/`; a `git add -f` past the ignore rule is what this refuses. |
| script suite | Measurement semantics stay honest, and no gate a workflow runs calls a tool the runner lacks or a script that is untracked or absent | `python3 -m unittest discover -s scripts/tests -p 'test_*.py'` | Fix the measurement semantics; do not trim inconvenient observations. For a portability failure, reach for `git grep` rather than installing the tool on the runner. |
| toolchain reuse | Every script that runs cargo sources `scripts/toolchain-env.sh`, so a runner HOME reuses the machine's toolchain instead of installing a private copy | `python3 -m unittest discover -s scripts/tests -p 'test_*.py'` | Source the resolver rather than recovering the toolchain yourself. rustup auto-installs into an empty `$HOME/.rustup` and still exits 0, which is what made the two earlier failure-guarded workarounds dead code. |
| git worktree presentation | The Git section's documentation and its view keep using shared theme tokens, with no inline color, spacing or font size | `bash scripts/check-git-worktree-presentation.sh` | Add the token to `HideTheme` and `DESIGN.md`, then use it; do not write the value in the view. |
| git worktree states | The worktree section keeps an explicit loading, refresh, comparison and unavailable-repository state | `bash scripts/check-git-worktree-states.sh` | Keep the state visible in `GitWorktreesView.swift`; update the assertion in the same change when the wording moves. |
| worktree base policy | The worktree row still offers base selection and still excludes detached rows | `bash scripts/check-worktree-base-policy.sh` | Restore the control, or move the assertion with it. |
| worktree catalog presentation | The main worktree stays marked and the empty linked-worktree state stays named | `bash scripts/check-worktree-catalog-presentation.sh` | Same. |
| worktree removal boundary | Branch deletion during worktree removal is `-d` and never `-D` | `bash scripts/check-worktree-removal-boundary.sh` | Never force-delete; an unmerged branch must fail and surface the reason. |
| herdr pin single source | The Herdr version and digest live only in `herdr-bundle.json` | `zsh scripts/check-herdr-pin-single-source.sh` | Derive from the manifest; never restate the value. Bump with `scripts/bump-herdr.sh <version>`. |
| herdr schema contract | The pinned Herdr CLI's API schema equals `contracts/herdr-api.schema.json` byte for byte | `zsh scripts/check-herdr-contract.sh --schema-only` | The schema moved with a Herdr release; update the contract and every call site it names, then the fixtures. |

The gates that read a running Herdr server, drive the built app, or reach the network are local steps and are not required in CI.
They are listed under "Local gates" below; every script in `scripts/` is either a required gate above, a local gate there, or a fixture in [verification-fixtures.md](docs/verification-fixtures.md).
The separate `design-contract.yml` workflow runs `node scripts/check-design-contract.mjs` and `node --test scripts/tests/design-controls.test.mjs`.
The shared entrypoint runs the token, component ownership and counted control-policy checks; it performs static checks, not desktop interaction.
The tests plant default controls, duplicate owners and style literals in nested files and verify staged/unstaged separation in a private Git fixture.

### Local design hook

Run `node scripts/check-design-contract.mjs` for immediate feedback on working-tree sources.
The tracked `.githooks/pre-commit` checks staged content through `node scripts/check-design-contract.mjs --staged`.
Use `git -c core.hooksPath=.githooks commit` to enable it for one commit without changing shared Git configuration or other worktrees.
This is opt-in; the hook is not installed automatically and an ordinary commit does not imply it ran.
If an existing hook is already configured, retain it and call the shared staged entrypoint from that hook rather than replacing its hook path.
CI independently runs the same checks even when the local hook was not enabled.
No branch-protection setting is changed by this repository patch.

Stage the checker, policy and affected sources together: staged verification executes the staged checker files and reads staged Swift sources, ignoring unstaged repairs or new violations.
A missing script, conflict, non-ordinary source input or checker failure blocks the hook with its cause.
The checker does not stage, stash, restore or modify files.
When a check fails, reuse the documented owner or fix the source; if an existing usage was removed, retire its counted allowance in `scripts/design-control-policy.json` in the same change.
Adding an exception requires an explicit design decision and reason in DESIGN.md, not an automatic baseline update.
Keep visual acceptance separate: DESIGN.md owns the future native catalog and human screenshot-review procedure; neither exists as an automated aesthetic approval gate.

## Local gates

None of these run in CI, and a green `verify` says nothing about them.
A script that stops earning its place here is deleted rather than left unreferenced; five gates once rotted silently because nothing named them, and three of those were asserting a symbol the design system had renamed.

| Command | Checks | Needs |
| --- | --- | --- |
| `bash scripts/check-hide-full.sh` | Everything CI requires plus every local gate below that runs unattended | A full build; writes `hide-full.log` under the checkout's scratch root (`scripts/build-scratch.sh`) |
| `zsh scripts/check-herdr-contract.sh` | The full contract, including the responses only a live server answers | A running Herdr server |
| `node scripts/check-hide-design.mjs` | `DESIGN.md` lints clean and still carries the clauses the contract names | Network, for `npx @google/design.md` |
| `node scripts/check-hide-design-enforcement.mjs` | `design-contract.yml` still binds the real checkers, so this list cannot drift from CI | - |
| `bash scripts/check-hide-copy.sh <base-sha>` | User-facing copy against the inventory at an immutable pre-change commit | The base commit of the change under review |
| `bash scripts/check-hide-accessibility.sh` | The accessibility tree of the built dev app | Builds and launches the dev bundle |
| `bash scripts/check-typed-contract.sh <stage>` | The typed wire boundary: `generated`, `behavior`, `structure`, `suites` or `e2e` | `suites` and `e2e` build and launch |
| `bash scripts/check-typed-live-remote.sh <stage>` | The same boundary against a live and a remote server | An authorized remote fixture |
| `python3 scripts/check-no-attribution.py` | No AI tooling attribution in the branch name, the commits, or a prepared PR body | A fetched `origin/main`; `--range` and `--pr-body` override the defaults |
| `python3 scripts/check-herdr-release.py <source\|asset>` | A Herdr release at its public and local source boundaries before the pin moves | A Herdr checkout or a reference binary |
| `python3 scripts/check-worktree-performance-evidence.py [dir]` | A worktree performance run recorded what the guide requires | A completed native run directory |
| `bash macos/scripts/check_workbench_native_evidence.sh ...` | A native Workbench run used one identified app and an isolated server | A completed native run |
| `zsh scripts/install-local-runtime.sh --herdr-root PATH` | Not a gate: installs a locally built Herdr for runtime work | A Herdr checkout |

## Performance-sensitive changes

Read [PERFORMANCE_TESTING.md](docs/PERFORMANCE_TESTING.md#verification-layers-and-current-ci-coverage) for the three verification layers and review policy.
The Rust/Swift suites include deterministic performance-related regression tests, including bitmap repaint and cache retention, but CI does not currently launch and drive Hide with a live Herdr server.
Native typing, drag, wheel, focus, compositor, project Tree/List and destructive cleanup review, and controlled latency/RSS comparisons remain isolated local QA.
Cleanup deletion tests must use a private fixture root; never use an operator project as a cleanup target.
The guide's maintenance policy requires affected native scenarios for input/rendering/lifecycle changes and matched measurements for performance claims; this is review-required evidence, not a branch-protection check today.
Record completed and unrun checks in the PR's Evidence section; a green `verify` result alone does not prove native responsiveness.

## Evidence

Screenshots, traces, sample output, and run logs are run artifacts, not source.
They never enter a commit; write them under `agents/runs/<slug>/`, which is ignored, and attach a copy to the pull request when a reviewer needs to see it.
`AGENTS.md` records why: a committed evidence tree once carried a browser profile with cookies and 184 screenshots showing a home directory.

## Releases

A release is a tag on a commit that is already on `main`, never on a branch.
Pushing `v<version>` runs both suites again and drafts a GitHub release with the signed archive and its checksum; a maintainer publishes the draft after installing the archive once.

## Bundled Herdr runtime

The version and digest of the Herdr binary the app ships are pinned in `macos/Sources/HerdrMacOS/Resources/herdr-bundle.json` and nowhere else.
`scripts/bump-herdr.sh <version>` moves the pin after verifying the release asset; a weekly workflow proposes that bump as a pull request when a new stable Herdr release appears.
It never merges, because three Herdr behaviors the core relies on are covered by fixtures this repository wrote, not by Herdr's own tests.

## Commit messages and attribution

Write commits as project work: what changed in the product, code, or documentation, and why.
Do not add AI agent, model, or tool names to commit messages, trailers, branch names, or pull request text.
Say in the pull request template whether AI tooling produced part of the change; that is a review input, not an attribution line.
