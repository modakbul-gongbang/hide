# Verification fixtures

This guide inventories reusable inputs and test adapters, not completed QA evidence.
For native isolation, process ownership, screenshots, performance measurements, and cleanup, read [PERFORMANCE_TESTING.md](PERFORMANCE_TESTING.md) first.
For CI coverage, read [CONTRIBUTING.md](../CONTRIBUTING.md).

## Deterministic component tests

Rust tests in `herdr-core/` cover session projection, independent demand/activity/read axes, malformed-record exclusion, ambient counts, pet behavior, state persistence, and connection transitions.
Herdr's `_new` suffix is not Hide's unread authority; the current contract is [status-model.md](status-model.md).
Files under `herdr-core/tests/fixtures/` are test inputs, not a writable scratch directory.
A test that persists state must allocate its own temporary path.

Swift tests in `macos/Tests/HerdrMacOSTests/` cover bundled theme loading, physical-key shortcut policy, pet placement and gestures, native rendering, and shell state.
Some renderer tests use AppKit views and bitmap drawing in-process; they do not launch and operate the complete app.

```sh
bash scripts/rust-test.sh
bash scripts/swift-test.sh
```

Both resolve the toolchain through `scripts/toolchain-env.sh`, so they reuse the machine's installed Rust under a verification HOME rather than installing a private copy into it.
The PRD harness binds `scripts/verify-cargo.sh test` and `scripts/verify-cargo.sh build` for the same reason; its verify commands run with no shell, so the environment decision cannot live in the command string.

## Scripted pet server

[pet_scenario_server.py](../macos/scripts/pet_scenario_server.py) provides scripted `session.snapshot`, `events.subscribe`, and `agent.list` responses.
Its protocol/version constants and supported responses are fixture code, not values automatically derived from the bundled Herdr.
Check compatibility with the current wire boundary before using it; it is not evidence of full real-server contract compatibility.

Inspect its arguments with:

```sh
python3 macos/scripts/pet_scenario_server.py --help
```

A minimal scenario is:

```json
{"agents": [{"pane_id": "fixture:p0", "state": "working", "summary": "Fixture agent"}]}
```

The script also maps question, approval, error, done, idle, and acknowledged input states and accepts optional ambient counts.
Its scenario file is re-read on snapshot and agent-list requests.
Use a run-owned scenario, short private socket, explicit app state path, and the performance guide's isolation checks before launch.
Stop only the recorded fixture process to exercise disconnect/recovery; never stop the operator's server.

## Native pet receipt

`--verification-window-receipt <path>` writes a machine-readable record of the pet controller's state, including pose, badges, placement, window flags, and theme/shortcut status.
Write that file under the run directory.
The receipt records the controller's belief; a real screenshot and native interaction must corroborate visible claims.

## Scripted Herdr workspace fixtures

Each script stands up a throwaway Herdr workspace on a server it starts itself, fills it with the state one scenario needs, and tears the whole thing down again.
They are inputs for a native run, not evidence: read the isolation requirements in [PERFORMANCE_TESTING.md](PERFORMANCE_TESTING.md) before running one, and never point one at the operator's server.
None of them run in CI.

| Fixture | Stands up |
| --- | --- |
| [agent-attention-fixture.sh](../scripts/agent-attention-fixture.sh) | Two tabs in a checkout that is its own repository, filled to cover the four attention groups and the blocked transition |
| [tab-verification-fixture.sh](../scripts/tab-verification-fixture.sh) | One workspace holding three Herdr tabs, for the tab strip |
| [view-state-fixture.sh](../scripts/view-state-fixture.sh) | Panes with checkable scrollback, the outside-focus path, and the missing-runtime launch |
| [scratch-verification-fixture.sh](../scripts/scratch-verification-fixture.sh) | The Scratch surface on an isolated server |
| [hide-paths-fixture.sh](../scripts/hide-paths-fixture.sh) | The path-and-tab scenarios, one per scenario card |
| [e2e-fixture.sh](../scripts/e2e-fixture.sh) | A private GitHub repository with the worktrees, branches and pull request state the project panel reads |

## Real Herdr fixtures and terminal inputs

[herdr-ide-fixture.rs](../herdr-core/src/bin/herdr-ide-fixture.rs) offers plan, create, status, and cleanup operations for prefixed verification workspaces.
Its manifest records fixture ownership; inspect its target and current CLI assumptions before executing mutations.
A name prefix alone does not isolate shared focus or prevent connecting to the operator's server.
Pass the same private routing environment to every local command and use only an explicitly authorized remote fixture for remote operations.

[The ANSI fixture](fixtures/t5-terminal-ansi.zsh) exercises colors, OSC links, Unicode, mouse reporting, and alternate-screen transitions.
It waits for Return before restoring terminal modes; run it only in an owned pane.
[The TUI text fixture](fixtures/t5-terminal-tui.txt) is neutral text for a local editor/TUI, not a statement about the renderer's implementation.
Neither file is a measured performance baseline.
