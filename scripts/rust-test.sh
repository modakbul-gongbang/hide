#!/usr/bin/env bash
# Runs the core's Rust tests, optionally filtered to one test binary and name.
#
# This exists for the same reason `swift-test.sh` checks for cargo: a
# verification runner gets its own HOME, and rustup puts cargo on PATH from
# `$HOME/.cargo/env` rather than anywhere standard. Without this the runner
# fails with an empty exit 1 that looks like a failing test rather than a
# missing toolchain.
#
# Usage: rust-test.sh [--lib [<filter>] | <test-binary> [<filter>]]
set -euo pipefail

cd "$(dirname "$0")/.."

# Build output is not source (AGENTS.md, "Evidence Belongs Outside The
# Repository"), and a verification runner that judges the working tree must not
# have `cargo test` writing into it. An explicit CARGO_TARGET_DIR still wins,
# so a caller that wants the in-tree `target/` says so.
#
# The default is keyed by the checkout, because cargo names a workspace
# member's artifacts by its path relative to the workspace root: two worktrees
# of this repository sharing one target dir read each other's build as fresh
# and run the other checkout's test binary.
checkout="$(basename "$(git rev-parse --show-toplevel)")"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-${TMPDIR:-/tmp}/hide-cargo-test-$checkout}"

# `cargo` on PATH is usually rustup's shim, and the shim reads its toolchain
# from RUSTUP_HOME, defaulting to `$HOME/.rustup`. Under a runner HOME that
# holds neither, the shim exits with "could not choose a version of cargo to
# run" - a missing toolchain that reads exactly like a failing test.
if ! cargo --version >/dev/null 2>&1; then
    shim="$(command -v cargo || true)"
    if [[ -n "$shim" ]]; then
        # The shim lives at <cargo home>/bin/cargo, and rustup installs its own
        # home beside that by default.
        cargo_home="$(cd "$(dirname "$shim")/.." && pwd)"
        rustup_home="$(dirname "$cargo_home")/.rustup"
        if [[ -d "$rustup_home" ]]; then
            export CARGO_HOME="${CARGO_HOME:-$cargo_home}"
            export RUSTUP_HOME="${RUSTUP_HOME:-$rustup_home}"
        fi
    fi
fi

if ! cargo --version >/dev/null 2>&1; then
    printf 'cargo could not choose a toolchain under HOME=%s; set RUSTUP_HOME and CARGO_HOME\n' \
        "${HOME:-<unset>}" >&2
    exit 1
fi

if [[ $# -eq 0 ]]; then
    exec cargo test --manifest-path herdr-core/Cargo.toml
fi

# The core's own unit tests live in the library rather than in a test binary,
# so filtering them needs `--lib` rather than `--test <name>`.
if [[ "$1" == "--lib" ]]; then
    shift
    exec cargo test --manifest-path herdr-core/Cargo.toml --lib "$@"
fi

if [[ $# -eq 1 ]]; then
    exec cargo test --manifest-path herdr-core/Cargo.toml --test "$1"
fi

exec cargo test --manifest-path herdr-core/Cargo.toml --test "$1" "$2"
