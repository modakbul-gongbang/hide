#!/usr/bin/env bash
# The cargo entrypoint for verification, as plain argv.
#
# Usage: verify-cargo.sh test [cargo test arguments...] | lint | release | cli
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
# the caller carries: the desktop packager reads the release binaries from
# `target/release/` at that fixed path, and a build directory shared between
# worktrees reads the other checkout's artifacts as fresh. `git worktree
# remove` takes the cache with the work. See docs/BUILD.md.
set -euo pipefail

cd "$(dirname "$0")/.."

. scripts/toolchain-env.sh

export CARGO_TARGET_DIR="$PWD/target"

# A Herdr pane carries the socket and identity of the Herdr that opened it,
# and the core takes HERDR_SOCKET_PATH over any socket a test hands it, so a
# test daemon started from an agent's pane would follow the operator's live
# Herdr and label its agents from the operator's conversations. No test may
# reach that server; each one that needs Herdr names its own.
unset HERDR_SOCKET_PATH HERDR_BIN_PATH HERDR_ENV HERDR_PANE_ID HERDR_TAB_ID HERDR_WORKSPACE_ID

case "${1:-}" in
    test)
        shift
        exec cargo test --locked --workspace "$@"
        ;;
    lint)
        cargo fmt --all --check
        exec cargo clippy --locked --workspace --all-targets -- -D warnings
        ;;
    release)
        # The binaries the packaged app ships; release hided embeds web/dist,
        # so `pnpm --dir web build` runs first (desktop/scripts/package.mjs).
        exec cargo build --release --locked -p hided --bins -p hide-host --bin hide-host-helper -p hide-agent-hooks --bin hide-agent-hooks
        ;;
    cli)
        exec cargo build --locked -p hided --bins -p hide-host --bin hide-host-helper -p hide-agent-hooks --bin hide-agent-hooks
        ;;
    *)
        printf 'usage: %s test [cargo test arguments...]|lint|release|cli\n' "$0" >&2
        exit 2
        ;;
esac
