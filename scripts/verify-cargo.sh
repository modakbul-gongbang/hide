#!/usr/bin/env bash
# The cargo entrypoint for verification, as plain argv.
#
# Usage: verify-cargo.sh test [cargo test arguments...] | lint | build | cli
#
# The PRD harness runs each verify command with execvp and no shell, so an
# `ENV=value cargo ...` binding fails with ENOENT at verify time rather than at
# configuration time, and a sealed run cannot be amended to fix it. A script is
# the only place an environment decision can live, which is why this file exists
# rather than a longer command string in `agents/config.json`:
#
#     "test": "bash scripts/verify-cargo.sh test"
#     "lint": "bash scripts/verify-cargo.sh lint"
#
# Every build lands in the worktree's own `target/`, whatever CARGO_TARGET_DIR
# the caller carries: SwiftPM links `target/release/libherdr_core.a` from that
# fixed path, and a build directory shared between worktrees reads the other
# checkout's artifacts as fresh. `git worktree remove` takes the cache with the
# work. See docs/BUILD.md.
set -euo pipefail

cd "$(dirname "$0")/.."

. scripts/toolchain-env.sh

export CARGO_TARGET_DIR="$PWD/target"

case "${1:-}" in
    test)
        shift
        exec cargo test --locked --workspace "$@"
        ;;
    lint)
        cargo fmt --all --check
        exec cargo clippy --locked --workspace --all-targets -- -D warnings
        ;;
    build)
        exec cargo build --release --locked -p herdr-core
        ;;
    cli)
        exec cargo build --locked -p hided --bins
        ;;
    *)
        printf 'usage: %s test [cargo test arguments...]|lint|build|cli\n' "$0" >&2
        exit 2
        ;;
esac
