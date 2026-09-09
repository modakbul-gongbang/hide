#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

rg --fixed-strings -q 'HideBadge(label: "main worktree"' macos/Sources/HerdrMacOS/GitWorktreesView.swift
rg --fixed-strings -q 'notice("No linked worktrees yet")' macos/Sources/HerdrMacOS/GitWorktreesView.swift

echo "The worktree catalog still marks the main worktree and still names the empty state"
