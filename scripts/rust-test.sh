#!/usr/bin/env bash
# Runs the core's Rust tests, optionally filtered to one test binary and name.
#
# This exists for the same reason `swift-test.sh` checks for cargo: a
# verification runner gets its own HOME, and rustup puts cargo on PATH from
# `$HOME/.cargo/env` rather than anywhere standard. Without this the runner
# fails with an empty exit 1 that looks like a failing test rather than a
# missing toolchain.
#
# Usage: rust-test.sh [<test-binary> [<filter>]]
set -euo pipefail

cd "$(dirname "$0")/.."

if ! cargo --version >/dev/null 2>&1; then
    for candidate in "$HOME/.cargo/bin" "${CARGO_HOME:-}/bin" /usr/local/cargo/bin; do
        if [[ -n "$candidate" && -x "$candidate/cargo" ]]; then
            PATH="$candidate:$PATH"
            export PATH
            break
        fi
    done
fi

if ! cargo --version >/dev/null 2>&1; then
    printf 'cargo was not found; install the rust toolchain or put cargo on PATH\n' >&2
    exit 1
fi

if [[ $# -eq 0 ]]; then
    exec cargo test --manifest-path herdr-core/Cargo.toml
fi

if [[ $# -eq 1 ]]; then
    exec cargo test --manifest-path herdr-core/Cargo.toml --test "$1"
fi

exec cargo test --manifest-path herdr-core/Cargo.toml --test "$1" "$2"
