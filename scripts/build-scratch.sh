#!/usr/bin/env bash
# Scratch build directories for the local check scripts, keyed by checkout.
#
# Source this, do not execute it:
#
#     . "$(dirname "$0")/build-scratch.sh"
#     cargo test --target-dir "$HIDE_CARGO_SCRATCH"
#     swift test --package-path macos --scratch-path "$HIDE_SWIFT_SCRATCH"
#
# Two rules decide these paths, and they pull in opposite directions.
#
# A check script must not write into the tree it is judging: `cargo test`
# building into the worktree's own `target/` makes the working tree dirty while
# a verification run is reading it. So the test build goes to a scratch
# directory. `scripts/rust-test.sh` reached the same conclusion for the same
# reason and keys its default the same way.
#
# But that directory must never be shared between worktrees. Cargo names a
# workspace member's artifacts by its path relative to the workspace root, so
# two checkouts of this repository sharing one target directory read each
# other's build as fresh and run the other checkout's test binary. Cargo also
# holds an exclusive lock on it, which serializes the parallel builds the
# worktree layout exists to allow. Keying by checkout is what keeps both
# properties: isolated from the tree, and isolated from every other tree.
#
# The release archive is the exception and never comes here. SwiftPM links
# `target/release/libherdr_core.a` from a fixed path inside the worktree, and
# `macos/scripts/build_dev_app.sh`, `scripts/build-app.sh` and
# `scripts/swift-test.sh` read it there. A redirected release build leaves all
# three reading a path nothing wrote, which is what the worktree `target`
# symlink in `check-typed-live-remote.sh` used to paper over.
#
# See docs/BUILD.md.

hide_scratch_checkout="$(basename "$(git rev-parse --show-toplevel)")"
# TMPDIR carries a trailing slash on macOS; a doubled separator survives into
# every path a check script prints, so strip it once here.
hide_scratch_tmp="${TMPDIR:-/tmp}"
hide_scratch_tmp="${hide_scratch_tmp%/}"
HIDE_SCRATCH_ROOT="$hide_scratch_tmp/hide-verify-$hide_scratch_checkout"
HIDE_CARGO_SCRATCH="$HIDE_SCRATCH_ROOT/cargo"
HIDE_SWIFT_SCRATCH="$HIDE_SCRATCH_ROOT/swift"
unset hide_scratch_checkout hide_scratch_tmp
export HIDE_SCRATCH_ROOT HIDE_CARGO_SCRATCH HIDE_SWIFT_SCRATCH
