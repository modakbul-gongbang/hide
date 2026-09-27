# Contributing to hide

hide is a macOS app over the [Herdr](https://herdr.dev) runtime.
The Rust core in `herdr-core/` owns every piece of state; the `hided` daemon serves a snapshot of it to the web shell in `web/`, which the Electron host in `desktop/` shows in its own window and dispatches typed events back from.
`AGENTS.md` keeps the rules; [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) owns the architecture and the Herdr wire boundary with their reasons, [docs/BUILD.md](docs/BUILD.md) the build output and worktree rules, and [docs/PERFORMANCE_TESTING.md](docs/PERFORMANCE_TESTING.md) the performance rules that came out of real incidents.
Read `AGENTS.md` and the architecture guide before changing anything under `herdr-core/`, `hided/` or `desktop/`; [docs/UI_BEHAVIOR.md](docs/UI_BEHAVIOR.md) before changing anything a user looks at, and [docs/DESIGN_WORKFLOW.md](docs/DESIGN_WORKFLOW.md) before making a design change.
Use [docs/README.md](docs/README.md) to find current guides and distinguish historical/reference-only material.

## Before you open a pull request

Run the same required lanes CI runs.
These are the local equivalents; the remote `verify` result still depends on the actual CI run.

```sh
bash scripts/verify-cargo.sh lint                # cargo fmt --check, then clippy over every target
bash scripts/verify-cargo.sh test                # herdr-core, hided, hide-ai, hide-agent-hooks and the context-label plugin
bash scripts/verify-web.sh                       # hcoord typecheck/build/unit/e2e, then web and desktop typecheck, lint, test and build
cargo build -p hided && pnpm --dir web e2e       # Playwright against a local hided: missing Herdr, and an isolated pinned Herdr (HIDE_E2E_HERDR_BIN, HERDR_BIN_PATH or PATH) for the S2 flows and the S3 Explorer, editor, viewers, attach, watch and reconnect flows (one worker, because each spec starts its own Herdr, hided and browser)
pnpm --dir desktop e2e                           # Playwright `_electron` against a private hided and the pinned Herdr
MEASURE_SCENARIO=multi HIDE_MEASURE_RUN_DIR=agents/runs/<slug>/measure/<attempt> bash scripts/web-shell-measure/run.sh   # echo and frame gates with four splits and five attached tabs; review-required evidence, not a CI check
bash scripts/check-harness-ignore-anchor.sh
bash scripts/check-agent-asset-committed.sh
bash scripts/check-capability-readers-off-lock.sh
python3 -m unittest discover -s scripts/tests -p 'test_*.py'
bash scripts/check-no-workstation-identity.sh
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
| hcoord typecheck/build/unit/e2e | The standalone plugin preserves its CLI and ledger contract, converges daemon ownership, writes portable lineage tokens, and keeps remote operations bounded and recoverable | `pnpm --dir plugins/hcoord typecheck && pnpm --dir plugins/hcoord build && pnpm --dir plugins/hcoord test && pnpm --dir plugins/hcoord test:e2e` (also part of `bash scripts/verify-web.sh`) | Fix the plugin or its isolated fixture. Never point the suite at the operator's live hcoord home or Herdr socket. |
| web typecheck/lint/test/build/e2e | The web shell's store merge and structural sharing, modifier-key bytes, connection machine, shortcut registry, close policy, resize math, project projection and registration checks, and Playwright against hided: the missing-socket row, the refused token, the sidebar click -> pane switch -> echo flow, and the S2 flow (two checkouts, three tabs, two splits, zoom, reorder, closes, the ⌘/ sheet, a socket drop, one registration and its refusals) and the S3 flow (the Explorer tree, a Git-decorated row, editor open/edit/save/conflict, Markdown Live, image/PDF/video viewers, create/rename/move/trash, a watch refresh, ⌘P/⌘K, a file drop, a buffer reconnect) on an isolated pinned Herdr | `bash scripts/verify-web.sh` (typecheck, lint, test and build for both `web/` and `desktop/`), then `pnpm --dir web e2e` | Fix the test or the code. e2e builds `web/dist`, needs `target/debug/hided` (`cargo build -p hided` first; the e2e never rebuilds it), the pinned `herdr` (`HIDE_E2E_HERDR_BIN`, `HERDR_BIN_PATH` or PATH) and `cc` for the fake agent. |
| desktop typecheck/lint/test/e2e | The desktop app's CLI resolution order and answer parsing, window-bounds restore, the menu built from the registry's Electron column, the environment registry, the one-child spawn helper, and Playwright `_electron` against a private hided and pinned Herdr: attach and the shell, the native chords and a menu click as one `create_tab` each, the ⌘/ sheet's Electron chords, external links, quit leaving hided running, a second launch, the missing-CLI screen and Retry, finding `hide` with PATH lacking it in `~/.local/bin` and then through the remembered path, and re-attaching after the daemon dies | `bash scripts/verify-web.sh`, then `pnpm --dir desktop e2e` | Fix the test or the code. e2e needs `web/dist`, `target/debug/hide` and `hided`, and the pinned `herdr` as the web e2e does; it never touches the operator's daemon. |
| harness ignore anchor | `/agents/` is ignored and `.claude/agents/` is not | `bash scripts/check-harness-ignore-anchor.sh` | Keep the leading slash on the ignore rule. |
| agent asset committed | The simplification subagent stays a tracked file | `bash scripts/check-agent-asset-committed.sh` | `git add` it; it once became uncommittable through an unanchored ignore rule. |
| capability readers off lock | No production code forks a subprocess while the runtime mutex is held, and every production reader runs from the session-sync coordinator; inline test modules are excluded | `bash scripts/check-capability-readers-off-lock.sh` | Move the subprocess to a reader driven by the coordinator; see `AGENTS.md`, Performance Guide. |
| no workstation identity | No tracked text file names a real home directory or machine, no run-artifact path is tracked, and no browser profile file is tracked | `bash scripts/check-no-workstation-identity.sh` | Use `/Users/example` in fixtures and a neutral placeholder in UI. Move run artifacts under `agents/runs/<slug>/`; a `git add -f` past the ignore rule is what this refuses. |
| script suite | Measurement semantics stay honest, and no gate a workflow runs calls a tool the runner lacks or a script that is untracked or absent | `python3 -m unittest discover -s scripts/tests -p 'test_*.py'` | Fix the measurement semantics; do not trim inconvenient observations. For a portability failure, reach for `git grep` rather than installing the tool on the runner. |
| toolchain reuse | Every script that runs cargo sources `scripts/toolchain-env.sh`, so a runner HOME reuses the machine's toolchain instead of installing a private copy | `python3 -m unittest discover -s scripts/tests -p 'test_*.py'` | Source the resolver rather than recovering the toolchain yourself. rustup auto-installs into an empty `$HOME/.rustup` and still exits 0, which is what made the two earlier failure-guarded workarounds dead code. |
| verification builds | `verify-cargo.sh test` and `release` build inside the checkout, observe a changed core and propagate a failing test or compile error, and `release` leaves every binary the packager ships executable in `target/release` | `python3 -m unittest discover -s scripts/tests -p 'test_*.py'` | Keep the build in-tree; a caller's `CARGO_TARGET_DIR` must not move the binaries `desktop/scripts/package.mjs` reads. |
| worktree removal boundary | The core's one removal executor (`worktree_cleanup.rs`) deletes a branch with `-d`, never `-D`, and never forces `git worktree remove` | `bash scripts/check-worktree-removal-boundary.sh` | Never force-delete; an unmerged branch must fail and surface the reason. |
| herdr pin single source | The Herdr version and digest live only in `herdr-bundle.json` | `zsh scripts/check-herdr-pin-single-source.sh` | Derive from the manifest; never restate the value. Bump with `scripts/bump-herdr.sh <version>`. |
| herdr schema contract | The pinned Herdr CLI's API schema equals `contracts/herdr-api.schema.json` byte for byte | `zsh scripts/check-herdr-contract.sh --schema-only` | The schema moved with a Herdr release; update the contract and every call site it names, then the fixtures. |

The gates that read a running Herdr server, drive the built app, or reach the network are local steps and are not required in CI.
They are listed under "Local gates" below; every script in `scripts/` is either a required gate above, a local gate there, or a generator named in [docs/DESIGN_WORKFLOW.md](docs/DESIGN_WORKFLOW.md).
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
| `bash scripts/check-typed-contract.sh <stage>` | The typed wire boundary: `generated`, `behavior`, `structure` or `suites` | `suites` builds |
| `bash scripts/check-typed-live-remote.sh <stage>` | The same boundary against a live and a remote server: `structure`, `behavior`, `probe`, `suites` or `attribution` | An authorized remote fixture |
| `python3 scripts/check-no-attribution.py` | No AI tooling attribution in the branch name, the commits, or a prepared PR body | A fetched `origin/main`; `--range` and `--pr-body` override the defaults |
| `python3 scripts/check-herdr-release.py <source\|asset>` | A Herdr release at its public and local source boundaries before the pin moves | A Herdr checkout or a reference binary |
| `python3 scripts/check-worktree-performance-evidence.py [dir]` | A worktree performance run recorded what the guide requires | A completed native run directory |
| `zsh scripts/install-local-runtime.sh --herdr-root PATH` | Not a gate: installs a locally built Herdr for runtime work | A Herdr checkout |
| `node scripts/design-scratch.mjs <task-slug>` | Not a gate: creates an ignored scratch linked to this worktree's design library; see [docs/DESIGN_WORKFLOW.md](docs/DESIGN_WORKFLOW.md) for editing and human review | The verified Pen CLI version and an existing Pen login |

## Performance-sensitive changes

Read [PERFORMANCE_TESTING.md](docs/PERFORMANCE_TESTING.md#verification-layers-and-current-ci-coverage) for the three verification layers and review policy.
The Rust and web suites include deterministic performance-related regression tests, and the web echo and frame measurement runs against a real hided, but CI does not currently launch and drive Hide with a live Herdr server.
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
Pushing `v<version>` runs `verify-cargo.sh test` and `verify-web.sh` again, packages the app with `HIDE_VERSION=<version> pnpm --dir desktop package`, and drafts a GitHub release with `hide-v<version>-macos-arm64.zip` and its `.sha256`; a maintainer publishes the draft after installing the archive once.
The archive is ad-hoc signed, not notarized, so the first launch needs the Gatekeeper step [docs/INSTALL.md](docs/INSTALL.md) describes.

## Bundled Herdr runtime

The version and digest of the Herdr binary the app ships are pinned in `contracts/herdr-bundle.json` and nowhere else.
`scripts/bump-herdr.sh <version>` moves the pin after verifying the release asset; a weekly workflow proposes that bump as a pull request when a new stable Herdr release appears.
It never merges, because three Herdr behaviors the core relies on are covered by fixtures this repository wrote, not by Herdr's own tests.

## Commit messages and attribution

Write commits as project work: what changed in the product, code, or documentation, and why.
Do not add AI agent, model, or tool names to commit messages, trailers, branch names, or pull request text.
The pull request template's `AI tooling` line names how the change was written and what you checked by hand; that is a review input, not an attribution line.
It is one line because the attribution gate rejects a credit phrase wherever it appears, including inside backticks, so the honest prose answer to that question is the thing the gate exists to block.
When the change's subject is one of the integrated products, name it in full or quote its command in backticks.
