# Contributing to hide

hide is a macOS shell over the [Herdr](https://herdr.dev) runtime.
The Rust core in `herdr-core/` owns every piece of state; the SwiftUI shell in `macos/` renders a snapshot of it and dispatches typed events back.
`AGENTS.md` keeps the rules; [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) owns the architecture and the Herdr wire boundary with their reasons, [docs/BUILD.md](docs/BUILD.md) the build output and worktree rules, and [docs/PERFORMANCE_TESTING.md](docs/PERFORMANCE_TESTING.md) the performance rules that came out of real incidents.
Read `AGENTS.md` and the architecture guide before changing anything under `herdr-core/` or `macos/`; [docs/UI_BEHAVIOR.md](docs/UI_BEHAVIOR.md) before changing anything a user looks at, and [docs/DESIGN_WORKFLOW.md](docs/DESIGN_WORKFLOW.md) before making a design change.
Use [docs/README.md](docs/README.md) to find current guides and distinguish historical/reference-only material.

## Before you open a pull request

Run the same three lanes CI runs.
These are the local equivalents; the remote `verify` result still depends on the actual CI run.

```sh
bash scripts/verify-cargo.sh lint                # cargo fmt --check, then clippy over every target
bash scripts/verify-cargo.sh test                # herdr-core, hided, hide-ai and the context-label plugin
bash scripts/verify-swift.sh test                # builds target/release/libherdr_core.a, then the shell
pnpm --dir web typecheck && pnpm --dir web lint && pnpm --dir web test
pnpm --dir web build && pnpm --dir web e2e       # Playwright against a local hided: missing Herdr, and an isolated pinned Herdr (HIDE_E2E_HERDR_BIN, HERDR_BIN_PATH or PATH) for the S2 flows and the S3 Explorer, editor, viewers, attach, watch and reconnect flows (one worker, because each spec starts its own Herdr, hided and browser)
MEASURE_SCENARIO=multi HIDE_MEASURE_RUN_DIR=agents/runs/<slug>/measure/<attempt> bash scripts/web-shell-measure/run.sh   # echo and frame gates with four splits and five attached tabs; review-required evidence, not a CI check
bash scripts/check-right-panel-sections.sh
bash scripts/check-shortcut-contract.sh
bash scripts/check-harness-ignore-anchor.sh
bash scripts/check-agent-asset-committed.sh
bash scripts/check-capability-readers-off-lock.sh
bash scripts/check-terminal-row-cache.sh
bash scripts/check-packaged-resource-access.sh
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
`main` accepts pull-request merges, and the `verify` check has to pass; this repository currently uses merge commits, and there is no way around branch protection, including for maintainers.

## CI gates

Every required check exists because something once went wrong without it.
The table says what each one protects, how to run it locally, and what to do when it blocks you.
There is no label or bypass for any of them; when a gate is wrong, change the gate in the same pull request and say why in the description.

| Gate | Protects | Local command | When it blocks you |
| --- | --- | --- | --- |
| `cargo fmt` | Rust formatting stays deterministic across the workspace, so reviews do not accumulate unrelated style drift | `bash scripts/verify-cargo.sh lint` (`cargo fmt --all --check`) | Run the same command without `--check` and commit the machine-generated formatting separately. |
| `cargo clippy` | Every Rust target in the workspace is warning-free, including tests and generated-contract consumers | `bash scripts/verify-cargo.sh lint` (`cargo clippy --locked --workspace --all-targets -- -D warnings`) | Fix a warning when that clarifies the code; use a narrow, explained allowance when the alternative would obscure a generated or performance-sensitive boundary. |
| `cargo test` | The core's behavior including its Herdr fixtures, hided handshake and state-file rules, the `hide-ai` router and codex backend against a fake app server, the context-label plugin, and the agent-hook crate's configuration rules | `bash scripts/verify-cargo.sh test` | Fix the test or the code. A fixture that no longer matches Herdr means the pin moved; see `AGENTS.md`, Herdr API Contract. |
| web typecheck/lint/test/e2e | The web shell's store merge and structural sharing, modifier-key bytes, connection machine, shortcut registry, close policy, resize math, project projection and registration checks, and Playwright against hided: the missing-socket row, the refused token, the sidebar click -> pane switch -> echo flow, and the S2 flow (two checkouts, three tabs, two splits, zoom, reorder, closes, the ⌘/ sheet, a socket drop, one registration and its refusals) and the S3 flow (the Explorer tree, a Git-decorated row, editor open/edit/save/conflict, Markdown Live, image/PDF/video viewers, create/rename/move/trash, a watch refresh, ⌘P/⌘K, a file drop, a buffer reconnect) on an isolated pinned Herdr | `pnpm --dir web typecheck`, `pnpm --dir web lint`, `pnpm --dir web test`, `pnpm --dir web e2e` | Fix the test or the code. e2e builds `web/dist` and `target/debug/hided`, needs the pinned `herdr` (`HIDE_E2E_HERDR_BIN`, `HERDR_BIN_PATH` or PATH) and `cc` for the fake agent. |
| desktop typecheck/lint/test/e2e | The desktop app's CLI resolution order and answer parsing, window-bounds restore, the menu built from the registry's Electron column, the environment registry, the one-child spawn helper, and Playwright `_electron` against a private hided and pinned Herdr: attach and the shell, the native chords and a menu click as one `create_tab` each, the ⌘/ sheet's Electron chords, external links, quit leaving hided running, a second launch, the missing-CLI screen and Retry, finding `hide` with PATH lacking it in `~/.local/bin` and then through the remembered path, and re-attaching after the daemon dies | `pnpm --dir desktop typecheck`, `pnpm --dir desktop lint`, `pnpm --dir desktop test`, `pnpm --dir desktop e2e` | Fix the test or the code. e2e needs `web/dist`, `target/debug/hide` and `hided`, and the pinned `herdr` as the web e2e does; it never touches the operator's daemon. |
| `swift test` | The shell's rendering and event contracts | `bash scripts/verify-swift.sh test`, which builds the release core first | Same. `--filter <TestName>` after the mode narrows a run. |
| right panel sections | The Workbench name never returns to a user-facing string | `bash scripts/check-right-panel-sections.sh` | The panel presents exactly Overview, Explorer, and History; rename, do not reintroduce. |
| shortcut contract | The right panel toggle is `⇧⌘B` and `⌘⌥B` is advertised nowhere | `bash scripts/check-shortcut-contract.sh` | Update the catalog and every label together. |
| harness ignore anchor | `/agents/` is ignored and `.claude/agents/` is not | `bash scripts/check-harness-ignore-anchor.sh` | Keep the leading slash on the ignore rule. |
| agent asset committed | The simplification subagent stays a tracked file | `bash scripts/check-agent-asset-committed.sh` | `git add` it; it once became uncommittable through an unanchored ignore rule. |
| CoreBridge module ownership | Snapshot DTOs and dispatch policies stay out of the runtime coordinator, with their top-level access levels preserved | `python3 scripts/check-core-bridge-structure.py` | Update the explicit responsibility inventory when a new CoreBridge module or top-level type is intentionally introduced. |
| capability readers off lock | No production code forks a subprocess while the runtime mutex is held, and every production reader runs from the session-sync coordinator; inline test modules are excluded | `bash scripts/check-capability-readers-off-lock.sh` | Move the subprocess to a reader driven by the coordinator; see `AGENTS.md`, Performance Guide. |
| terminal row cache | The terminal draw loop builds a row only when something it is drawn from changed | `bash scripts/check-terminal-row-cache.sh` | Reach text building through `preparedRow`, never from `drawTerminalContents`. This is a cost property, so no test fails when it is lost; see `AGENTS.md`, Performance Guide. |
| packaged resource access | Every shell resource is read through `PackagedResourceBundle.app`, which finds the bundle under `Contents/Resources` | `bash scripts/check-packaged-resource-access.sh` | Replace the `Bundle.module` read. SwiftPM's accessor finds the bundle only through the build tree that produced the binary, so the installed app dies at launch once that tree is gone. |
| no workstation identity | No tracked text file names a real home directory or machine, no run-artifact path is tracked, and no browser profile file is tracked | `bash scripts/check-no-workstation-identity.sh` | Use `/Users/example` in fixtures and a neutral placeholder in UI. Move run artifacts under `agents/runs/<slug>/`; a `git add -f` past the ignore rule is what this refuses. |
| script suite | Measurement semantics stay honest, and no gate a workflow runs calls a tool the runner lacks or a script that is untracked or absent | `python3 -m unittest discover -s scripts/tests -p 'test_*.py'` | Fix the measurement semantics; do not trim inconvenient observations. For a portability failure, reach for `git grep` rather than installing the tool on the runner. |
| toolchain reuse | Every script that runs cargo sources `scripts/toolchain-env.sh`, so a runner HOME reuses the machine's toolchain instead of installing a private copy | `python3 -m unittest discover -s scripts/tests -p 'test_*.py'` | Source the resolver rather than recovering the toolchain yourself. rustup auto-installs into an empty `$HOME/.rustup` and still exits 0, which is what made the two earlier failure-guarded workarounds dead code. |
| git worktree presentation | Overview Git documentation and its view keep using shared theme tokens, with no inline color, spacing or font size | `bash scripts/check-git-worktree-presentation.sh` | Add the token to `design/tokens.json`, then use it; do not write the value in the view. |
| git worktree states | Overview keeps an explicit loading, local-only, refresh, disconnected and unreadable state, and reads an unreadable value as `?` and a pending one as `…`, never as zero | `bash scripts/check-git-worktree-states.sh` | Keep the state visible in `CheckoutOverview.swift`; update the assertion in the same change when the wording moves. |
| worktree base policy | The worktree row still offers base selection and still excludes detached rows | `bash scripts/check-worktree-base-policy.sh` | Restore the control, or move the assertion with it. |
| worktree catalog presentation | Overview keeps the main-checkout identity, the empty-group row and the no-match state | `bash scripts/check-worktree-catalog-presentation.sh` | Same. |
| worktree removal boundary | The core's one removal executor (`worktree_cleanup.rs`) deletes a branch with `-d`, never `-D`, and never forces `git worktree remove` | `bash scripts/check-worktree-removal-boundary.sh` | Never force-delete; an unmerged branch must fail and surface the reason. |
| herdr pin single source | The Herdr version and digest live only in `herdr-bundle.json` | `zsh scripts/check-herdr-pin-single-source.sh` | Derive from the manifest; never restate the value. Bump with `scripts/bump-herdr.sh <version>`. |
| herdr schema contract | The pinned Herdr CLI's API schema equals `contracts/herdr-api.schema.json` byte for byte | `zsh scripts/check-herdr-contract.sh --schema-only` | The schema moved with a Herdr release; update the contract and every call site it names, then the fixtures. |

The gates that read a running Herdr server, drive the built app, or reach the network are local steps and are not required in CI.
They are listed under "Local gates" below; every script in `scripts/` is either a required gate above, a local gate there, or a fixture in [verification-fixtures.md](docs/verification-fixtures.md).
The separate `design-contract.yml` workflow runs `node scripts/check-design-contract.mjs`, `node --test scripts/tests/pen-gallery.test.mjs`, `node --test scripts/tests/pen-transplant.test.mjs`, `node --test scripts/tests/design-scratch.test.mjs`, and `node --test scripts/tests/hide-screens.test.mjs`.
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
| `bash scripts/check-hide-copy.sh <base-sha>` | User-facing copy against the inventory at an immutable pre-change commit | The base commit of the change under review |
| `bash scripts/check-hide-accessibility.sh` | The accessibility tree of the built dev app | Builds and launches the dev bundle |
| `bash scripts/check-typed-contract.sh <stage>` | The typed wire boundary: `generated`, `behavior`, `structure`, `suites` or `e2e` | `suites` and `e2e` build and launch |
| `bash scripts/check-typed-live-remote.sh <stage>` | The same boundary against a live and a remote server | An authorized remote fixture |
| `python3 scripts/check-no-attribution.py` | No AI tooling attribution in the branch name, the commits, or a prepared PR body | A fetched `origin/main`; `--range` and `--pr-body` override the defaults |
| `python3 scripts/check-herdr-release.py <source\|asset>` | A Herdr release at its public and local source boundaries before the pin moves | A Herdr checkout or a reference binary |
| `python3 scripts/check-worktree-performance-evidence.py [dir]` | A worktree performance run recorded what the guide requires | A completed native run directory |
| `bash macos/scripts/check_workbench_native_evidence.sh ...` | A native Workbench run used one identified app and an isolated server | A completed native run |
| `zsh scripts/install-local-runtime.sh --herdr-root PATH` | Not a gate: installs a locally built Herdr for runtime work | A Herdr checkout |
| `node scripts/design-scratch.mjs <task-slug>` | Not a gate: creates an ignored scratch linked to this worktree's design library; see [docs/DESIGN_WORKFLOW.md](docs/DESIGN_WORKFLOW.md) for editing and human review | The verified Pen CLI version and an existing Pen login |

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
The pull request template's `AI tooling` line names how the change was written and what you checked by hand; that is a review input, not an attribution line.
It is one line because the attribution gate rejects a credit phrase wherever it appears, including inside backticks, so the honest prose answer to that question is the thing the gate exists to block.
When the change's subject is one of the integrated products, name it in full or quote its command in backticks.
