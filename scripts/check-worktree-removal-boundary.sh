#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

rg --fixed-strings -q '"branch", "-d", "--", branch' macos/Sources/HerdrMacOS/GitWorktreeRemover.swift
if rg --fixed-strings -q '"branch", "-D"' macos/Sources/HerdrMacOS/GitWorktreeRemover.swift; then
    echo "Worktree removal must never force-delete a branch" >&2
    exit 1
fi

echo "Branch deletion during worktree removal is -d and never -D"
