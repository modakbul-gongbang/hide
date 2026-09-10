#!/usr/bin/env bash
# One fail-closed entrypoint that runs everything CI requires plus the local
# gates CI cannot run. It is a superset of `verify` and `design-contract`, so a
# green run here predicts both; the reverse is not true.
#
# Keep this list equal to `.github/workflows/pr.yml` and `design-contract.yml`.
# `scripts/check-hide-design-enforcement.mjs` below fails when the workflow and
# the checker binding drift apart.
set -euo pipefail
export LC_ALL=en_US.UTF-8
export LC_CTYPE=en_US.UTF-8
export LANG=en_US.UTF-8
cd "$(dirname "$0")/.."
mkdir -p /tmp/herdr-ide-verify
exec > >(tee /tmp/herdr-ide-verify/hide-full.log) 2>&1

# verify / rust and swift lanes
cargo fmt --manifest-path herdr-core/Cargo.toml --check
cargo clippy --locked --manifest-path herdr-core/Cargo.toml --all-targets -- -D warnings
cargo test --locked --manifest-path herdr-core/Cargo.toml --target-dir /tmp/herdr-ide-verify/cargo
# SwiftPM links this archive from the repository target directory.
cargo build --release --locked -p herdr-core
swift build --package-path macos --scratch-path /tmp/herdr-ide-verify/swift
swift test --package-path macos --scratch-path /tmp/herdr-ide-verify/swift
bash scripts/check-right-panel-sections.sh
bash scripts/check-shortcut-contract.sh

# verify / repository invariants lane
python3 -m unittest discover -s scripts/tests -p 'test_*.py'
bash scripts/check-harness-ignore-anchor.sh
bash scripts/check-agent-asset-committed.sh
bash scripts/check-capability-readers-off-lock.sh
bash scripts/check-terminal-row-cache.sh
bash scripts/check-no-workstation-identity.sh
bash scripts/check-git-worktree-presentation.sh
bash scripts/check-git-worktree-states.sh
bash scripts/check-worktree-base-policy.sh
bash scripts/check-worktree-catalog-presentation.sh
bash scripts/check-worktree-removal-boundary.sh
zsh scripts/check-herdr-pin-single-source.sh
zsh scripts/check-herdr-contract.sh --schema-only

# design-contract workflow
node scripts/check-design-contract.mjs
node --test scripts/tests/design-controls.test.mjs

# local only: DESIGN.md lint needs the network, and the enforcement checker
# asserts the workflow still binds the commands above.
node scripts/check-hide-design.mjs
node scripts/check-hide-design-enforcement.mjs
