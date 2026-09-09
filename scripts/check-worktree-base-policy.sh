#!/usr/bin/env bash
set -euo pipefail
# Searches use `git grep`, never `rg`: ripgrep is not on the CI runner.

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

git grep -qF -- 'Button("Set as base branch")' macos/Sources/HerdrMacOS/GitWorktreesView.swift
git grep -qF -- 'if let branch = worktree.branch' macos/Sources/HerdrMacOS/GitWorktreesView.swift

echo "The worktree row still offers base selection and still excludes detached rows"
