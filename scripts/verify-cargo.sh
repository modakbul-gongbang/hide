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
# Two decisions are made here and nowhere else.
#
# The toolchain comes from the machine, through `toolchain-env.sh`. A verify run
# gets its own HOME, and rustup answers an empty `$HOME/.rustup` by installing a
# private toolchain into it: 1.4 GB per run, 9.1 GB across eleven runs,
# duplicating one that was already installed.
#
# The target directory is shared across runs on purpose, which is the opposite
# of the per-worktree rule in AGENTS.md and for a reason that rule names. Cargo
# keys a workspace member's artifacts by its path relative to the workspace
# root, so sharing is unsafe between two checkouts. Every verify run builds the
# same checkout, so there is one path here and sharing is what makes the second
# run incremental. Removing this directory is always safe and costs one full
# build.
set -euo pipefail

cd "$(dirname "$0")/.."

. scripts/toolchain-env.sh

export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-${TMPDIR:-/tmp}/hide-verify/cargo}"

case "${1:-}" in
    test)
        exec cargo test --locked --workspace
        ;;
    build)
        exec cargo build --release --locked -p herdr-core
        ;;
    *)
        printf 'usage: %s test|build\n' "$0" >&2
        exit 2
        ;;
esac
