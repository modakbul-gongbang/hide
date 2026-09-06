#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

cargo test --manifest-path herdr-core/Cargo.toml worktree_control::tests
cargo test --manifest-path herdr-core/Cargo.toml ancestry_does_not_mistake_a_squash_merge_for_a_merged_tip
bash scripts/swift-test.sh GitWorktreeRemoverTests
rg --fixed-strings -q '"branch", "-d", "--", branch' macos/Sources/HerdrMacOS/GitWorktreeRemover.swift
if rg --fixed-strings -q '"branch", "-D"' macos/Sources/HerdrMacOS/GitWorktreeRemover.swift; then
    echo "Worktree removal must never force-delete a branch" >&2
    exit 1
fi

echo "Pane closure confirmation precedes removable worktree and safe branch deletion"
