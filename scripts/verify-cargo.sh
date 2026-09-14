#!/usr/bin/env bash
# The cargo entrypoint the PRD harness binds, as plain argv.
#
# Usage: verify-cargo.sh test | build
#
# The harness runs each verify command with execvp and no shell, so an
# `ENV=value cargo ...` binding fails with ENOENT at verify time rather than at
# configuration time, and a sealed run cannot be amended to fix it. A script is
# the only place an environment decision can live, which is why this file exists
# rather than a longer command string in `agents/config.json`:
#
#     "test":  "bash scripts/verify-cargo.sh test"
#     "build": "bash scripts/verify-cargo.sh build"
#
# Tests use the checkout's scratch cache; release output stays at the fixed
# archive path SwiftPM links. Cargo still checks freshness on every invocation.
set -euo pipefail

cd "$(dirname "$0")/.."

. scripts/toolchain-env.sh

. scripts/build-scratch.sh

case "${1:-}" in
    test)
        export CARGO_TARGET_DIR="$HIDE_CARGO_SCRATCH"
        exec cargo test --locked --workspace
        ;;
    build)
        export CARGO_TARGET_DIR="$PWD/target"
        exec cargo build --release --locked -p herdr-core
        ;;
    *)
        printf 'usage: %s test|build\n' "$0" >&2
        exit 2
        ;;
esac
