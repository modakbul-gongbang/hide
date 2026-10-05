#!/usr/bin/env bash
# The cargo entrypoint for verification, as plain argv.
#
# Usage: verify-cargo.sh test [args...] | lint | release | cli
# CI-scoped lanes: test-scoped | check | build | clippy | metadata [cargo arguments...]
#                  nextest [cargo nextest run arguments...] (the CI Rust lane; .config/nextest.toml)
#                  fmt-check (cargo fmt --all --check alone)
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
# and a test daemon that reads them from its environment follows the
# operator's live Herdr and labels its agents from the operator's
# conversations. No test may reach that server; each one that needs Herdr
# names its own. Retirement fixtures must also ignore an inherited legacy
# coordination-home override so no private kit pass follows operator state.
for name in $(compgen -e); do
    case "$name" in HERDR_*|HCOORD_*) unset "$name" ;; esac
done

# Scoped modes cannot move the checkout or its artifacts through cargo flags.
# Keep the legacy modes unchanged for sealed verification commands.
case "${1:-}" in
    test-scoped|check|build|clippy|metadata|nextest)
        mode=$1
        shift
        (( $# <= 128 )) || { printf 'too many cargo arguments\n' >&2; exit 2; }
        for argument in "$@"; do
            case "$argument" in
                --target-dir|--target-dir=*|--manifest-path|--manifest-path=*|--config|--config=*)
                    printf 'cargo argument may not redirect verification: %s\n' "$argument" >&2
                    exit 2
                    ;;
            esac
        done
        [[ "$mode" != nextest ]] || exec cargo nextest run --locked "$@"
        [[ "$mode" != test-scoped ]] || mode=test
        exec cargo "$mode" --locked "$@"
        ;;
    test)
        shift
        exec cargo test --locked --workspace "$@"
        ;;
    fmt-check)
        exec cargo fmt --all --check
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
        printf 'usage: %s test [args...]|lint|fmt-check|release|cli|test-scoped|check|build|clippy|metadata|nextest [args...]\n' "$0" >&2
        exit 2
        ;;
esac
