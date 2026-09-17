#!/usr/bin/env bash
set -euo pipefail
# Searches use `git grep`, never `rg`: ripgrep is not on the CI runner.

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

git grep -qF -- 'Button(WorktreeMenuPolicy.setBaseBranch' macos/Sources/HerdrMacOS/HideSidebar.swift
git grep -qF -- 'if let branch = checkout.branch' macos/Sources/HerdrMacOS/HideSidebar.swift

echo "The worktree row still offers base selection and still excludes detached rows"
