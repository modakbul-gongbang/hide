#!/usr/bin/env bash
set -euo pipefail
# Searches use `git grep`, never `rg`: ripgrep is not on the CI runner.

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

git grep -qF -- 'HideBadge(label: "main worktree"' macos/Sources/HerdrMacOS/GitWorktreesView.swift
git grep -qF -- 'notice("No linked worktrees yet")' macos/Sources/HerdrMacOS/GitWorktreesView.swift

echo "The worktree catalog still marks the main worktree and still names the empty state"
