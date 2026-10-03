# Build output and worktrees

This document owns where build output goes and why: every build inside the worktree that asked for it, the release binaries at their fixed path, and the machine's toolchain reused rather than reinstalled.
The scripts named here are the executable authority; `scripts/tests/test_toolchain_reuse.py`, `test_ci_gate_portability.py` and `test_verification_builds.py` assert the parts a workflow depends on.

## One rule: build output lives in the worktree

Cargo writes to `target/`, the web shell to `web/node_modules/` and `web/dist/`, and the desktop app to `desktop/node_modules/`, `desktop/dist/` and `desktop/out/`, all at their default locations inside the checkout and all ignored by Git.
Nothing a checkout builds is written anywhere else, so `git worktree remove` is the whole cleanup, and no cache can outlive the work that produced it.

Two facts make the default location the only correct one.

Cargo names a workspace member's artifacts by its path relative to the workspace root, so two checkouts sharing one target directory read each other's build as fresh and run the other checkout's test binary.
Cargo also holds an exclusive lock on the directory, so sharing serializes the parallel builds the worktree layout exists to allow.

The release binaries are `target/release/{hided,hide,hide-host-helper,hide-agent-hooks}`, and `desktop/scripts/package.mjs` reads every one of them from that fixed relative path.
Redirecting a release build with `CARGO_TARGET_DIR`, `--target-dir` or `--build-path` leaves the packager reading a path nothing wrote.
`scripts/verify-cargo.sh` therefore sets `CARGO_TARGET_DIR` to the worktree's own `target/` on every invocation, whatever the caller carried.

The cost of this layout is one full build cache per worktree, which is why a worktree is removed when its branch lands rather than kept around.

A release `hided` carries `web/dist` inside the binary: `hided/build.rs` embeds every file under it when the profile is `release` and fails the build when `web/dist/index.html` is missing, so a release build is always `pnpm --dir web build` first, then `cargo build --release --locked -p hided --bins -p hide-host --bin hide-host-helper -p hide-agent-hooks --bin hide-agent-hooks` (`scripts/verify-cargo.sh release`).
`desktop/scripts/package.mjs` performs that build before staging the release binaries into the app bundle, so a packaged app's SessionStart probe has a matching `hide-agent-hooks` executable even without a separate CLI on `PATH`.
A debug `hided` embeds nothing and reads `web/dist` from disk at run time (`HIDED_UI_DIR` overrides the lookup), so a rebuilt web shell shows up without a cargo rebuild.
The web measurement (`MEASURE_SCENARIO=multi HIDE_MEASURE_RUN_DIR=agents/runs/<slug>/measure/<attempt> bash scripts/web-shell-measure/run.sh`, `docs/PERFORMANCE_TESTING.md`) needs that release `hided` at `target/release/hided` inside the worktree it measures; it is never redirected, for the same reason as the release binaries.
On 2026-09-09 one abandoned worktree held 5.3 GB, over half of the 10 GB across all eight.
An earlier design sent test builds to `/tmp/hide-verify-<uid>/<hash>/`, keyed by checkout path so a run judging the tree would not dirty it; the reason had lapsed once the harness scored only tracked files, and the caches it left behind reached 7.5 GB with no worktree owning them.
Hide's own merged-worktree cleanup had grown a step that forked `bash` to compute and delete that path, which this layout makes unnecessary.

`[profile.dev] incremental = false` in the workspace manifest is deliberate, not a leftover.
An agent worktree is built a few times and discarded, which never repays an incremental cache; what it does instead is grow one per worktree, and those had reached 1.5 GB.
Debug output is what makes a stale worktree expensive, because nothing strips it: a debug `hided` measures around 115 MB against 24 MB for the release binary.

## The toolchain is never copied

`scripts/toolchain-env.sh` is sourced by every script that calls cargo, and it resolves `CARGO_HOME` and `RUSTUP_HOME` from the cargo shim's own location so an isolated HOME reuses the machine's installed toolchain.

Without it a verification runner pays for a whole toolchain and keeps it.
rustup reads `RUSTUP_HOME` with a default of `$HOME/.rustup`, and a runner HOME makes that an empty directory; rustup does not fail there, it downloads and installs into it and reports the fact as a warning while exiting 0.
That exit 0 is why two earlier workarounds never ran: each had diagnosed the missing toolchain correctly, and each guarded its recovery behind a cargo invocation failing.
The cost was 1.3 GB of `.rustup` plus 128 MB of `.cargo` per run, 9.1 GB across eleven run directories, duplicating a toolchain already on the machine.

## Two entrypoints

`scripts/verify-cargo.sh` and `scripts/verify-web.sh` are the only way a check script or the PRD harness builds this repository.
The harness runs a verify command with no shell, so an `ENV=value cargo ...` binding fails with ENOENT at verify time, when a sealed run can no longer be amended; a script is the only place that environment decision can live.
A check script calls these scripts rather than cargo or pnpm directly, so the target directory and toolchain decisions are made once.

| Binding | Command | Output and proof |
| --- | --- | --- |
| test | `bash scripts/verify-cargo.sh test` | `target/debug`; locked workspace tests |
| lint | `bash scripts/verify-cargo.sh lint` | `cargo fmt --check` then `cargo clippy -D warnings` over every target |
| release | `bash scripts/verify-cargo.sh release` | `target/release/{hided,hide,hide-host-helper,hide-agent-hooks}`, the binaries the desktop packager ships |
| cli | `bash scripts/verify-cargo.sh cli` | `target/debug/{hide,hided,hide-host-helper,hide-agent-hooks}` for isolated CLI, daemon, SessionStart and install kit checks |
| web | `bash scripts/verify-web.sh` | `pnpm install --frozen-lockfile`, then typecheck, lint, test and build for both `web` and `desktop`: `web/dist` and `desktop/dist` |

The Cargo `test` mode forwards trailing test arguments, so an explicitly configured live probe can run as `verify-cargo.sh test <test-name> -- --ignored` without bypassing worktree isolation or toolchain ownership.
The no-argument `test` mode remains the full locked workspace gate.
Focused delivery and session-activity filters and their nonzero-test prerequisite are listed in [delivery.md: Verification](delivery.md#verification).
Each compiler or test process's failure reaches the caller.

The build regression tests in `test_verification_builds.py` use a tiny real Cargo workspace, not compiler mocks.
They check that a caller's `CARGO_TARGET_DIR` cannot move the release binaries, that output stays in the checkout, warm build reuse, a core value change, a changed failing test, and compiler and prerequisite failure propagation.
They require macOS with Cargo.

## The desktop app

`desktop/` is a pnpm workspace member; `pnpm install` at the root installs it with the web shell.
Electron downloads its runtime into `desktop/node_modules/electron/dist/` on the first launch rather than at install, so a lane that only typechecks never fetches it.

| Command | Does |
| --- | --- |
| `pnpm --dir desktop dev` | Bundles `desktop/dist/` with esbuild and launches the app unpackaged; it finds this worktree's `target/{debug,release}/hide` itself |
| `pnpm --dir desktop typecheck`, `lint`, `test` | The desktop CI lane |
| `pnpm --dir desktop e2e` | Playwright `_electron` against a private hided and pinned Herdr; needs `web/dist`, `target/debug/hide` and `hided`, and the pinned `herdr` as the web e2e does |
| `pnpm --dir desktop package` | `desktop/scripts/package.mjs`: builds the release binaries and fetches the pinned Herdr, bundles `hided`, `hide`, `hide-agent-hooks`, `hide-host-helper-macos-<arch>` and `herdr` into `Contents/Resources` with the install kit's `hcoord/`, ad-hoc signs `desktop/out/hide-darwin-<arch>/hide.app` (bundle id `me.grab.hide.desktop`), and archives it to `desktop/out/hide-v<version>-macos-<arch>.zip` beside a `.sha256` checksum; nothing is notarized or installed. On Windows x64 and Linux x64 the same command builds that system's package, unsigned: the folder `desktop/out/hide-win32-x64/` or `hide-linux-x64/` with the same binaries (`.exe` on Windows, with Herdr's `conpty/` beside `herdr.exe`) and `hide-host-helper-<windows\|linux>-x86_64` in its `resources/`, archived to `hide-v<version>-windows-x64.zip` (the system's `tar.exe`) or `hide-v<version>-linux-x64.tar.gz` beside a `.sha256`. Each system packages only itself, and a machine of another architecture than its pinned Herdr asset is refused |
| `node desktop/scripts/smoke-package.mjs <archive>` | On Windows or Linux only: checks a package against its `.sha256`, unpacks it into a temporary folder, and with a private home and state folder runs the bundled `herdr --version`, the device helper, `hide-agent-hooks doctor`, the bundled hcoord through Electron's Node, and `hide connect`, `/health`, the embedded shell and `hide stop`; `.github/workflows/package.yml` runs it after each package. It refuses macOS, where a daemon inside `hide.app` installs the kit and its LaunchAgent into the account |

The app attaches to whatever daemon the environment names: without `HIDE_STATE_DIR` it is the operator's own at `~/.hide/state`.
For QA, set `HIDE_STATE_DIR`, `HOME`, `HERDR_SOCKET_PATH` and `HIDE_DESKTOP_USER_DATA_DIR` to private paths, as `desktop/e2e/fixture.ts` does and refuses to launch without.
A packaged app does not need `hide` on `PATH`: it ships its own CLI and Herdr, and only falls back to a login-shell PATH search and the well-known install directories when its own bundled CLI is somehow missing (see `docs/ARCHITECTURE.md`, The desktop host).
macOS may refuse the unsigned app's first launch until it is opened once with Open from the context menu.

The desktop e2e runs beside the operator's own work on the same Mac (issue 232).
Every launch through `desktop/e2e/fixture.ts` passes `--hide-show-inactive`, so the host shows its window behind every other window without activating the app and a second launch never focuses it, and `--disable-backgrounding-occluded-windows`, so the window keeps painting behind the operator's and a capture by window id is current.
The fixture also preloads `desktop/e2e/focus-guard.cjs` into the app, which records into a per-test file each time this app becomes active or a window takes the keyboard, from launch to quit, and fails any test not tagged `@needs-focus` that recorded one; specs take `test` from the fixture, and `launch` refuses a test that did not.
The guard sees only this app, so a spec that could reach another program (a browser, Finder, the folder picker) stubs it, as the existing ones do.
A spec that needs the key window or native input (a page holding the keyboard, a pinch, a native drag) carries the `@needs-focus` tag (`NEEDS_FOCUS` in the fixture) and focuses the window itself; `pnpm --dir desktop e2e --grep-invert @needs-focus` runs everything that leaves the operator's keyboard alone, and CI runs the whole suite.
The suite runs the focus tests after every other one (`desktop/playwright.config.ts` puts them in a project that depends on the rest): a focus test brings its app forward and quits it, and on a machine with no other app in front, a CI runner, macOS then activates the next app that opens, which failed every background test after the first focus test.
