# Build output and worktrees

This document owns where build output goes and why: one build cache per worktree, the release archive at its fixed path, test builds in a scratch directory, and the machine's toolchain reused rather than reinstalled.
The scripts named here are the executable authority; `scripts/tests/test_toolchain_reuse.py`, `test_ci_gate_portability.py` and `test_verification_builds.py` assert the parts a workflow depends on.

No build directory is ever shared between worktrees, and the release archive the shell links stays inside the worktree that produced it.
Those are two separate rules, and the second is the narrower one.

The release archive is `target/release/libherdr_core.a`, and `macos/scripts/build_dev_app.sh`, `scripts/build-app.sh` and `scripts/swift-test.sh` read it from that fixed path under the worktree.
Redirecting a release build with `CARGO_TARGET_DIR`, `--target-dir` or `--build-path` leaves all three reading a path nothing wrote; `check-typed-live-remote.sh` used to compensate with a worktree `target` symlink, which is the shape this rule exists to prevent.

Sharing is the rule that governs everything else.
Cargo names a workspace member's artifacts by its path relative to the workspace root, so two checkouts sharing one target directory read each other's build as fresh and run the other checkout's test binary.
Cargo also holds an exclusive lock on it, so worktrees sharing one serialize the parallel builds this layout exists to allow.
Local checks resolve their own record tree from the worktree and cannot follow output out of it either.

A check script may still send its *test* build to a scratch directory, because a run that judges the working tree must not dirty it by building into `target/`.
`scripts/build-scratch.sh` is the one place that decides where that goes: a path keyed by checkout, which is isolated from the tree and from every other tree at once.
Source it rather than writing a path; `scripts/rust-test.sh`, `verify-cargo.sh test` and `verify-swift.sh` source that same helper.

The cost of that isolation is one full build cache per worktree, so the cache is removed with the work rather than left behind.
`git worktree remove` takes the in-tree build output with it.
Before removing an owned worktree, source `scripts/build-scratch.sh` there and remove its `$HIDE_SCRATCH_ROOT` after all checks using it finish; scratch output is external and Git does not remove it.
A worktree kept alive after its branch lands keeps its in-tree cache alive too.
On 2026-09-09 one abandoned worktree held 5.3 GB, over half of the 10 GB across all eight.

`[profile.dev] incremental = false` in the workspace manifest is deliberate, not a leftover.
An agent worktree is built a few times and discarded, which never repays an incremental cache; what it does instead is grow one per worktree, and those had reached 1.5 GB.
Debug output is what makes a stale worktree expensive, because nothing strips it: the debug `libherdr_core.a` measured 288 MB against 92 MB for the release archive.

The toolchain itself is not build output and is never copied per run.
`scripts/toolchain-env.sh` is sourced by every script here that calls cargo, and it resolves `CARGO_HOME` and `RUSTUP_HOME` from the cargo shim's own location so an isolated HOME reuses the machine's installed toolchain.

Without it a verification runner pays for a whole toolchain and keeps it.
rustup reads `RUSTUP_HOME` with a default of `$HOME/.rustup`, and a runner HOME makes that an empty directory; rustup does not fail there, it downloads and installs into it and reports the fact as a warning while exiting 0.
That exit 0 is why the two earlier workarounds never ran: `rust-test.sh` and `swift-test.sh` had each diagnosed the missing toolchain correctly, and each guarded its recovery behind a cargo invocation failing.
The cost was 1.3 GB of `.rustup` plus 128 MB of `.cargo` per run, 9.1 GB across eleven run directories, duplicating a toolchain already on the machine.

`scripts/verify-cargo.sh` is the plain-argv entrypoint the PRD harness binds for its `test` and `build` commands.
The harness runs a verify command with no shell, so an `ENV=value cargo ...` binding fails with ENOENT at verify time, when a sealed run can no longer be amended; a script is the only place that environment decision can live.

## Verification cache ownership

`build-scratch.sh` names `/tmp/hide-verify-<uid>/<sha256-of-physical-checkout-path>/cargo` and its sibling `swift`.
The physical absolute path distinguishes worktrees even with equal basenames and makes symlink aliases converge.
The path is independent of a runner's `HOME` and `TMPDIR`, so another run of the same checkout reuses its cache.
No build cache is shared between different checkouts; toolchain and downloaded dependencies remain machine-wide.
The verification wrappers and `rust-test.sh` select these paths themselves rather than accepting `CARGO_TARGET_DIR` or the retired `HIDE_VERIFY_SWIFT_SCRATCH` override.

The four harness bindings remain separate:

| Binding | Command | Output and proof |
| --- | --- | --- |
| test | `bash scripts/verify-cargo.sh test` | Scratch Cargo debug output; locked workspace tests |
| build | `bash scripts/verify-cargo.sh build` | Worktree `target/release/libherdr_core.a`; locked optimized core build |
| typecheck | `bash scripts/verify-swift.sh build` | Scratch Swift output; package executable and resources compile |
| lint | `bash scripts/verify-swift.sh test` | Same Swift scratch; test targets compile and tests execute |

Both Swift commands first invoke the same release entrypoint, preserving standalone use and failure propagation.
Cargo checks freshness each time; three release invocations in a four-command cycle do not mean three compilations.
A cold cycle compiles release dependencies and core once, then Swift's prerequisites reuse that fixed archive.
Rust tests still compile their own debug artifacts, since tests and a release build prove different things.
SwiftPM's `-L`/`-l` linkage does not declare the Rust archive as a build input.
Like `build_dev_app.sh`, the Swift verification wrapper passes its SHA-256 digest as a Swift compilation condition so a changed archive cannot leave a stale executable or test bundle.
The digest is content-based: a Cargo freshness check or an identical archive does not invalidate Swift output.
An archive content change rebuilds Swift targets, including dependencies; that correctness cost must be included when measuring Rust changes.
Swift build and test are retained: neither wrapper assembles, signs or verifies an application bundle, which remains the responsibility of the dev and release package scripts.
A failing Cargo prerequisite stops Swift, and each compiler/test process's failure reaches the caller.

The build regression tests use tiny real Cargo and SwiftPM packages, not compiler mocks.
They check equal-basename checkout isolation, runner path changes, warm archive reuse, Rust and Swift value changes, changed failing tests, and compiler/prerequisite failure propagation.
The linking tests require macOS with Cargo and Swift; the scratch identity tests run independently of those compilers.
