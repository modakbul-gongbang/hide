#!/usr/bin/env bash
# The swift entrypoint the PRD harness binds, as plain argv.
#
# Usage: verify-swift.sh build | test
#
# The counterpart to `verify-cargo.sh`, and it exists for the same reason: the
# harness runs each verify command with execvp and no shell, so a binding that
# needs an environment decision or a computed path cannot be a command string.
#
#     "typecheck": "bash scripts/verify-swift.sh build"
#     "lint":      "bash scripts/verify-swift.sh test"
#
# The shell links `target/release/libherdr_core.a` from a fixed path inside the
# worktree, so the core is built first and is never redirected. SwiftPM's own
# scratch path is redirected out of the tree, because a run that judges the
# working tree must not dirty it by building into `macos/.build`.
set -euo pipefail

cd "$(dirname "$0")/.."

. scripts/toolchain-env.sh

export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-${TMPDIR:-/tmp}/hide-verify/cargo}"
scratch="${HIDE_VERIFY_SWIFT_SCRATCH:-${TMPDIR:-/tmp}/hide-verify/swift}"

case "${1:-}" in
    build|test) ;;
    *)
        printf 'usage: %s build|test\n' "$0" >&2
        exit 2
        ;;
esac

# The archive the package links is read from the worktree at a fixed path, so
# this build alone keeps its in-tree target directory.
CARGO_TARGET_DIR="$PWD/target" cargo build --release --locked -p herdr-core

exec swift "$1" --package-path macos --scratch-path "$scratch" \
    --disable-keychain --disable-sandbox
