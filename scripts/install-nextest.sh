#!/usr/bin/env bash
# Installs the pinned cargo-nextest release binary into $CARGO_HOME/bin on a
# CI runner, with no compile: the Rust lanes run `scripts/verify-cargo.sh
# nextest` (.config/nextest.toml, docs/TESTING.md "Flaky tests"). The version
# lives here only, so a bump is one line.
set -euo pipefail

version=0.9.143
case "$(uname -s)" in
    Linux) platform=linux ;;
    Darwin) platform=mac ;;
    MINGW*|MSYS*|CYGWIN*) platform=windows-tar ;;
    *) printf 'no cargo-nextest release for %s\n' "$(uname -s)" >&2; exit 1 ;;
esac

bin=${CARGO_HOME:-$HOME/.cargo}/bin
if command -v cygpath >/dev/null; then bin=$(cygpath -u "$bin"); fi
mkdir -p "$bin"
curl -LsSf "https://get.nexte.st/$version/$platform" | tar zxf - -C "$bin"
# The release must be the one pinned above, whatever an image already carried.
"$bin/cargo-nextest" nextest --version | grep -F -- "$version" >/dev/null || {
    printf 'cargo-nextest is not %s\n' "$version" >&2
    exit 1
}
