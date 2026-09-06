#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

cargo test --manifest-path herdr-core/Cargo.toml idle_twenty_seconds_does_not_reread_but_file_edits_and_manual_refresh_do
cargo test --manifest-path herdr-core/Cargo.toml no_selection_measures_nothing_at_all
cargo test --manifest-path herdr-core/Cargo.toml a_project_is_read_first_then_only_when_its_generation_moves
cargo test --manifest-path herdr-core/Cargo.toml pull_requests_are_requested_only_for_the_visible_git_section

echo "Worktree, disk and pull-request readers run only for visibility, change and explicit refresh triggers"
