#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

cargo test --manifest-path herdr-core/Cargo.toml worktree_rows_sort_main_then_open_then_commit_time
rg --fixed-strings -q 'SidebarBadge(label: "main worktree"' macos/Sources/HerdrMacOS/GitWorktreesView.swift
rg --fixed-strings -q 'notice("No linked worktrees yet")' macos/Sources/HerdrMacOS/GitWorktreesView.swift

echo "Worktree rows sort by main, open pane and commit time, with the empty linked-worktree notice"
