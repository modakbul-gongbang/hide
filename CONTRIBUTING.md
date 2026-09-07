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
cargo test --locked --manifest-path herdr-core/Cargo.toml
cargo build --release --locked -p herdr-core      # the shell links target/release/libherdr_core.a
swift test --package-path macos
bash scripts/check-right-panel-sections.sh
bash scripts/check-shortcut-contract.sh
bash scripts/check-harness-ignore-anchor.sh
bash scripts/check-agent-asset-committed.sh
bash scripts/check-capability-readers-off-lock.sh
bash scripts/check-terminal-row-cache.sh
python3 -m unittest discover -s scripts/tests -p 'test_terminal_latency.py'
bash scripts/check-no-workstation-identity.sh
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
| `cargo test` | The core's behavior, including its Herdr fixtures | `cargo test --locked --manifest-path herdr-core/Cargo.toml` | Fix the test or the code. A fixture that no longer matches Herdr means the pin moved; see `AGENTS.md`, Herdr API Contract. |
| `swift test` | The shell's rendering and event contracts | `swift test --package-path macos` after the release core build | Same. `--filter <TestName>` narrows a run. |
| right panel sections | The Workbench name never returns to a user-facing string | `bash scripts/check-right-panel-sections.sh` | The panel presents exactly Explorer, Changes, and Git; rename, do not reintroduce. |
| shortcut contract | The right panel toggle is `⇧⌘B` and `⌘⌥B` is advertised nowhere | `bash scripts/check-shortcut-contract.sh` | Update the catalog and every label together. |
| harness ignore anchor | `/agents/` is ignored and `.claude/agents/` is not | `bash scripts/check-harness-ignore-anchor.sh` | Keep the leading slash on the ignore rule. |
| agent asset committed | The simplification subagent stays a tracked file | `bash scripts/check-agent-asset-committed.sh` | `git add` it; it once became uncommittable through an unanchored ignore rule. |
| capability readers off lock | Nothing forks a subprocess while the runtime mutex is held, and every reader runs from the session-sync coordinator | `bash scripts/check-capability-readers-off-lock.sh` | Move the subprocess to a reader driven by the coordinator; see `AGENTS.md`, Performance Guide. |
| terminal row cache | The terminal draw loop builds a row only when something it is drawn from changed | `bash scripts/check-terminal-row-cache.sh` | Reach text building through `preparedRow`, never from `drawTerminalContents`. This is a cost property, so no test fails when it is lost; see `AGENTS.md`, Performance Guide. |
| no workstation identity | No tracked text file names a real home directory or machine | `bash scripts/check-no-workstation-identity.sh` | Use `/Users/example` in fixtures and a neutral placeholder in UI. |
| latency summary | Missing/excluded observations do not become fast samples and long stalls remain visible | `python3 -m unittest discover -s scripts/tests -p 'test_terminal_latency.py'` | Fix the measurement semantics; do not trim inconvenient observations. |
| herdr pin single source | The Herdr version and digest live only in `herdr-bundle.json` | `zsh scripts/check-herdr-pin-single-source.sh` | Derive from the manifest; never restate the value. Bump with `scripts/bump-herdr.sh <version>`. |
| herdr schema contract | The pinned Herdr CLI's API schema equals `contracts/herdr-api.schema.json` byte for byte | `zsh scripts/check-herdr-contract.sh --schema-only` | The schema moved with a Herdr release; update the contract and every call site it names, then the fixtures. |

The gates that read a running Herdr server (the full `check-herdr-contract.sh` and the workbench evidence scripts under `macos/scripts/`) are local steps and are not required in CI.
The separate `design-contract.yml` workflow runs `check-hide-theme-literals.sh` and `check-hide-components.sh`; it performs static checks, not desktop interaction.

## Performance-sensitive changes

Read [PERFORMANCE_TESTING.md](docs/PERFORMANCE_TESTING.md#verification-layers-and-current-ci-coverage) for the three verification layers and review policy.
The Rust/Swift suites include deterministic performance-related regression tests, including bitmap repaint and cache retention, but CI does not currently launch and drive Hide with a live Herdr server.
Native typing, drag, wheel, focus, compositor, and controlled latency/RSS comparisons remain isolated local QA.
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
