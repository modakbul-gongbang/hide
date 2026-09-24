# Build output and worktrees

This document owns where build output goes and why: every build inside the worktree that asked for it, the release archive at its fixed path, and the machine's toolchain reused rather than reinstalled.
The scripts named here are the executable authority; `scripts/tests/test_toolchain_reuse.py`, `test_ci_gate_portability.py` and `test_verification_builds.py` assert the parts a workflow depends on.

## One rule: build output lives in the worktree

Cargo writes to `target/`, SwiftPM to `macos/.build/`, and the web shell to `web/node_modules/` and `web/dist/`, all at their default locations inside the checkout and all ignored by Git.
Nothing a checkout builds is written anywhere else, so `git worktree remove` is the whole cleanup, and no cache can outlive the work that produced it.

Two facts make the default location the only correct one.

Cargo names a workspace member's artifacts by its path relative to the workspace root, so two checkouts sharing one target directory read each other's build as fresh and run the other checkout's test binary; Cargo also holds an exclusive lock on the directory, so sharing serializes the parallel builds the worktree layout exists to allow.
SwiftPM's build database is keyed by absolute source path, so it cannot be shared either.

The release archive is `target/release/libherdr_core.a`, and `macos/Package.swift` links it from that fixed relative path; `macos/scripts/build_dev_app.sh`, `scripts/build-app.sh` and `scripts/verify-swift.sh` read it there too.
Redirecting a release build with `CARGO_TARGET_DIR`, `--target-dir` or `--build-path` leaves all of them reading a path nothing wrote.
`scripts/verify-cargo.sh` therefore sets `CARGO_TARGET_DIR` to the worktree's own `target/` on every invocation, whatever the caller carried.

The cost of this layout is one full build cache per worktree, which is why a worktree is removed when its branch lands rather than kept around.

A release `hided` carries `web/dist` inside the binary: `hided/build.rs` embeds every file under it when the profile is `release` and fails the build when `web/dist/index.html` is missing, so a release build is always `pnpm --dir web build` first, then `cargo build --release -p hided`.
A debug `hided` embeds nothing and reads `web/dist` from disk at run time (`HIDED_UI_DIR` overrides the lookup), so a rebuilt web shell shows up without a cargo rebuild.
The web measurement (`MEASURE_SCENARIO=multi HIDE_MEASURE_RUN_DIR=agents/runs/<slug>/measure/<attempt> bash scripts/web-shell-measure/run.sh`, `docs/PERFORMANCE_TESTING.md`) needs that release `hided` at `target/release/hided` inside the worktree it measures; it is never redirected, for the same reason as the release archive.
On 2026-09-09 one abandoned worktree held 5.3 GB, over half of the 10 GB across all eight.
An earlier design sent test builds to `/tmp/hide-verify-<uid>/<hash>/`, keyed by checkout path so a run judging the tree would not dirty it; the reason had lapsed once the harness scored only tracked files, and the caches it left behind reached 7.5 GB with no worktree owning them.
Hide's own merged-worktree cleanup had grown a step that forked `bash` to compute and delete that path, which this layout makes unnecessary.

`[profile.dev] incremental = false` in the workspace manifest is deliberate, not a leftover.
An agent worktree is built a few times and discarded, which never repays an incremental cache; what it does instead is grow one per worktree, and those had reached 1.5 GB.
Debug output is what makes a stale worktree expensive, because nothing strips it: the debug `libherdr_core.a` measured 288 MB against 92 MB for the release archive.

## The toolchain is never copied

`scripts/toolchain-env.sh` is sourced by every script that calls cargo, and it resolves `CARGO_HOME` and `RUSTUP_HOME` from the cargo shim's own location so an isolated HOME reuses the machine's installed toolchain.

Without it a verification runner pays for a whole toolchain and keeps it.
rustup reads `RUSTUP_HOME` with a default of `$HOME/.rustup`, and a runner HOME makes that an empty directory; rustup does not fail there, it downloads and installs into it and reports the fact as a warning while exiting 0.
That exit 0 is why two earlier workarounds never ran: each had diagnosed the missing toolchain correctly, and each guarded its recovery behind a cargo invocation failing.
The cost was 1.3 GB of `.rustup` plus 128 MB of `.cargo` per run, 9.1 GB across eleven run directories, duplicating a toolchain already on the machine.

## Two entrypoints

`scripts/verify-cargo.sh` and `scripts/verify-swift.sh` are the only way a check script or the PRD harness runs cargo or swift.
The harness runs a verify command with no shell, so an `ENV=value cargo ...` binding fails with ENOENT at verify time, when a sealed run can no longer be amended; a script is the only place that environment decision can live.
A check script calls the same two scripts rather than cargo or swift directly, so the target directory and toolchain decisions are made once.

| Binding | Command | Output and proof |
| --- | --- | --- |
| test | `bash scripts/verify-cargo.sh test` | `target/debug`; locked workspace tests |
| lint | `bash scripts/verify-cargo.sh lint` | `cargo fmt --check` then `cargo clippy -D warnings` over every target |
| build | `bash scripts/verify-swift.sh test` | `target/release/libherdr_core.a`, then `macos/.build`; the executable, resources and test targets compile and the tests execute |

`verify-swift.sh` first invokes `verify-cargo.sh build`, so a Swift run never links a stale core.
Cargo checks freshness each time; a warm cycle does not recompile the release archive.
Cargo decides freshness by mtime, which a fresh CI checkout always fails, so the `swift shell` lane caches the archive itself, keyed by the rustc version and a content hash of every source the archive is built from, and sets `HIDE_CORE_ARCHIVE_PREBUILT=1` on a hit; the wrapper then links the restored archive without a Cargo run and treats a missing archive as an error rather than a rebuild.
On a miss the lane builds through `verify-cargo.sh build` and saves the result, so a Swift-only change restores main's archive.
SwiftPM's `-L`/`-l` linkage does not declare the Rust archive as a build input, so, like `build_dev_app.sh`, the wrapper passes the archive's SHA-256 digest as a Swift compilation condition: a changed archive rebuilds the Swift targets, an identical one keeps the cache hot.
On a machine with only the Command Line Tools selected (`xcode-select -p` is `/Library/Developer/CommandLineTools`, no Xcode), SwiftPM finds neither swift-testing's `Testing.framework` nor its support libraries under the tools' `Library/Developer` folders, and every test target fails with `no such module 'Testing'`; the `test` mode then adds those two folders as compiler and linker framework paths and test-runner rpaths.
With Xcode selected, as on the CI runners, the command is unchanged, and neither case filters or skips a test.
Arguments after the mode reach swift unchanged, so `verify-swift.sh test --filter <TestName>` runs one suite.
The Cargo `test` mode likewise forwards trailing test arguments, so an explicitly configured live probe can run as `verify-cargo.sh test <test-name> -- --ignored` without bypassing worktree isolation or toolchain ownership.
The no-argument `test` mode remains the full locked workspace gate.
Neither wrapper assembles, signs or verifies an application bundle; that remains the responsibility of `build_dev_app.sh` and `build-app.sh`.
A failing Cargo prerequisite stops Swift, and each compiler or test process's failure reaches the caller.

The build regression tests in `test_verification_builds.py` use tiny real Cargo and SwiftPM packages, not compiler mocks.
They check that a caller's `CARGO_TARGET_DIR` cannot move the archive, that output stays in the checkout, warm archive reuse, Rust and Swift value changes, changed failing tests, and compiler and prerequisite failure propagation.
They require macOS with Cargo and Swift.
