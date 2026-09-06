#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

cargo test --manifest-path herdr-core/Cargo.toml deletion_gate_blocks_before_warning_and_reports_pane_consequence
cargo test --manifest-path herdr-core/Cargo.toml the_card_projects_the_worktrees_shared_deletion_gate

echo "All three deletion surfaces consume the shared blocked, warning and pane-consequence policy"
