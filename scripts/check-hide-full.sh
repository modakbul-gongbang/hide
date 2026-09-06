#!/usr/bin/env bash
# The four full-suite commands are the repository verification contract.
# Keep the static boundary checks here so AC16 has one fail-closed entrypoint.
set -euo pipefail
mkdir -p /tmp/herdr-ide-verify
exec > >(tee /tmp/herdr-ide-verify/hide-full.log) 2>&1
cargo test --manifest-path herdr-core/Cargo.toml --target-dir /tmp/herdr-ide-verify/cargo
cargo build --release --manifest-path herdr-core/Cargo.toml --target-dir /tmp/herdr-ide-verify/cargo
swift build --package-path macos --scratch-path /tmp/herdr-ide-verify/swift
swift test --package-path macos --scratch-path /tmp/herdr-ide-verify/swift
bash scripts/check-capability-readers-off-lock.sh
bash scripts/check-agent-asset-committed.sh
bash scripts/check-harness-ignore-anchor.sh
zsh scripts/check-herdr-pin-single-source.sh
zsh scripts/check-herdr-contract.sh --schema-only
bash scripts/check-right-panel-sections.sh
bash scripts/check-shortcut-contract.sh
bash scripts/check-hide-theme-literals.sh
bash scripts/check-hide-components.sh
bash scripts/check-hide-copy.sh 6b45c64
node scripts/check-hide-design.mjs
node scripts/check-hide-design-enforcement.mjs
