#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/.."
# Reuse the machine's installed toolchain; rustup installs a private copy into an
# empty $HOME/.rustup and still exits 0.
. scripts/toolchain-env.sh
export PATH="$HOME/.rustup/toolchains/stable-aarch64-apple-darwin/bin:$HOME/.cargo/bin:$PATH"
case "${1:-}" in
  generated)
    cargo test --manifest-path herdr-core/Cargo.toml wire::tests
    ;;
  behavior)
    cargo test --manifest-path herdr-core/Cargo.toml session_sync
    cargo test --manifest-path herdr-core/Cargo.toml wire::tests
    ;;
  structure)
    python3 scripts/check-typed-contract-structure.py
    ;;
  suites)
    zsh scripts/check-runtime-gates.sh
    ;;
  *) echo 'usage: check-typed-contract.sh generated|behavior|structure|suites' >&2; exit 2 ;;
esac
