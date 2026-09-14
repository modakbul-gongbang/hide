#!/usr/bin/env bash
# Runs the core's Rust tests, optionally filtered to one test binary and name.
#
# This exists for the same reason `swift-test.sh` checks for cargo: a
# verification runner gets its own HOME, and rustup resolves neither its
# toolchain nor its cache from anywhere standard under one.
# `toolchain-env.sh` owns that resolution for every script here.
#
# Usage: rust-test.sh [--lib [<filter>] | <test-binary> [<filter>]]
set -euo pipefail

cd "$(dirname "$0")/.."

# Reuse the machine's installed toolchain rather than letting rustup install a
# private copy under a runner HOME.
. scripts/toolchain-env.sh

# Use the same checkout-isolated test cache as the verification entrypoint.
. scripts/build-scratch.sh
export CARGO_TARGET_DIR="$HIDE_CARGO_SCRATCH"

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
