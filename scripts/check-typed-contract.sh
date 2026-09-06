#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/.."
export PATH="$HOME/.rustup/toolchains/stable-aarch64-apple-darwin/bin:$HOME/.cargo/bin:$PATH"
case "${1:-}" in
  generated)
    cargo test --manifest-path herdr-core/Cargo.toml --target-dir /tmp/herdr-ide-verify/cargo wire::tests
    ;;
  behavior)
    cargo test --manifest-path herdr-core/Cargo.toml --target-dir /tmp/herdr-ide-verify/cargo session_sync
    cargo test --manifest-path herdr-core/Cargo.toml --target-dir /tmp/herdr-ide-verify/cargo wire::tests
    ;;
  structure)
    python3 scripts/check-typed-contract-structure.py
    ;;
  suites)
    cargo build --release -p herdr-core
    zsh scripts/check-runtime-gates.sh
    ;;
  e2e)
    mkdir -p agents/runs/herdr-typed-contract/e2e
    bash macos/scripts/build_dev_app.sh 2>&1 | tee agents/runs/herdr-typed-contract/e2e/build.log
    python3 scripts/check-herdr-e2e.py --output agents/runs/herdr-typed-contract/e2e
    ;;
  *) echo 'usage: check-typed-contract.sh generated|behavior|structure|suites|e2e' >&2; exit 2 ;;
esac
