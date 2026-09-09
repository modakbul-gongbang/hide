#!/usr/bin/env bash
set -euo pipefail
# Searches use `git grep`, never `rg`: ripgrep is not on the CI runner.

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

git grep -qF -- '"branch", "-d", "--", branch' macos/Sources/HerdrMacOS/GitWorktreeRemover.swift
if git grep -qF -- '"branch", "-D"' macos/Sources/HerdrMacOS/GitWorktreeRemover.swift; then
    echo "Worktree removal must never force-delete a branch" >&2
    exit 1
fi

echo "Branch deletion during worktree removal is -d and never -D"
