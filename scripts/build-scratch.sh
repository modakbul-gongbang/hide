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
# directory. `scripts/rust-test.sh` uses this same test cache.
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

# Resolve the physical checkout, not its basename or a runner's HOME/TMPDIR.
# Equal directory names in different parents must never share build output.
hide_scratch_checkout="$(git rev-parse --show-toplevel)" || return
hide_scratch_checkout="$(cd "$hide_scratch_checkout" && pwd -P)" || return
hide_scratch_key="$(printf '%s' "$hide_scratch_checkout" | LC_ALL=C shasum -a 256)" || return
HIDE_SCRATCH_ROOT="/tmp/hide-verify-$(id -u)/${hide_scratch_key%% *}"
HIDE_CARGO_SCRATCH="$HIDE_SCRATCH_ROOT/cargo"
HIDE_SWIFT_SCRATCH="$HIDE_SCRATCH_ROOT/swift"
unset hide_scratch_checkout hide_scratch_key
export HIDE_SCRATCH_ROOT HIDE_CARGO_SCRATCH HIDE_SWIFT_SCRATCH
