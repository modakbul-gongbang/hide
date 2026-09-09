#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/.."
export PATH="$HOME/.rustup/toolchains/stable-aarch64-apple-darwin/bin:$HOME/.cargo/bin:$PATH"
# Use one canonical cache path, including the dev bundle build. The worktree
# target symlink supplies the relative library path consumed by Swift.
export CARGO_TARGET_DIR=/tmp/herdr-ide-verify/cargo
export SWIFTPM_BUILD_DIR=/tmp/herdr-ide-verify/swift
run=agents/runs/herdr-typed-live-remote
mkdir -p "$run"
case "${1:-}" in
  structure) python3 scripts/check-typed-contract-structure.py ;;
  behavior)
    cargo test --manifest-path herdr-core/Cargo.toml --target-dir /tmp/herdr-ide-verify/cargo live::tests
    cargo test --manifest-path herdr-core/Cargo.toml --target-dir /tmp/herdr-ide-verify/cargo remote::tests
    cargo test --manifest-path herdr-core/Cargo.toml --target-dir /tmp/herdr-ide-verify/cargo wire::tests
    ;;
  probe)
    python3 scripts/probe-typed-live-remote.py --output "$run/probe"
    HERDR_TEST_TYPED_RESPONSES="$PWD/$run/probe" cargo test --manifest-path herdr-core/Cargo.toml --target-dir /tmp/herdr-ide-verify/cargo isolated_live_remote_responses_decode -- --ignored
    ;;
  suites)
    cargo build --release -p herdr-core
    zsh scripts/check-runtime-gates.sh
    bash scripts/check-right-panel-sections.sh
    bash scripts/check-shortcut-contract.sh
    bash scripts/check-harness-ignore-anchor.sh
    bash scripts/check-agent-asset-committed.sh
    bash scripts/check-capability-readers-off-lock.sh
    bash scripts/check-no-workstation-identity.sh
    python3 scripts/check-typed-contract-structure.py
    ;;
  e2e)
    bash scripts/check-typed-live-remote.sh probe
    bash macos/scripts/build_dev_app.sh > "$run/build.log" 2>&1
    python3 scripts/check-herdr-e2e.py --output "$run/e2e"
    ;;
  attribution)
    git diff --check
    python3 scripts/check-no-attribution.py "${@:2}"
    ;;
  *) echo 'usage: check-typed-live-remote.sh structure|behavior|probe|suites|e2e|attribution' >&2; exit 2 ;;
esac
