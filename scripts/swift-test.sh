#!/usr/bin/env bash
# Runs the Swift shell's test suite against a current Rust core.
#
# The shell links the core's static archive, so a stale archive would link
# silently and test yesterday's core. Where cargo is available the archive is
# rebuilt; where it is not the archive must already be newer than every core
# source, and the run fails rather than testing an unknown binary.
#
# A verification runner with its own HOME used to take the unavailable branch
# for the wrong reason, and pay a full toolchain download on the way: rustup
# auto-installs into an empty `$HOME/.rustup` and still exits 0.
# `toolchain-env.sh` resolves the machine's toolchain first, so the branch below
# now separates a machine with no Rust from a runner that merely has its own
# HOME.
set -euo pipefail

cd "$(dirname "$0")/.."

. scripts/toolchain-env.sh

archive="target/release/libherdr_core.a"

if cargo --version >/dev/null 2>&1; then
    cargo build --manifest-path herdr-core/Cargo.toml --release
else
    if [[ ! -f "$archive" ]]; then
        printf 'cargo is unavailable and %s has not been built\n' "$archive" >&2
        exit 1
    fi
    stale="$(find herdr-core/src herdr-core/Cargo.toml Cargo.lock -newer "$archive" 2>/dev/null | head -5)"
    if [[ -n "$stale" ]]; then
        printf 'cargo is unavailable and the core archive is older than:\n%s\n' "$stale" >&2
        exit 1
    fi
    printf 'cargo is unavailable here; the linked core archive is current\n'
fi

if [[ $# -gt 0 ]]; then
    swift test --package-path macos --disable-keychain --disable-sandbox --filter "$1"
else
    swift test --package-path macos --disable-keychain --disable-sandbox
fi
