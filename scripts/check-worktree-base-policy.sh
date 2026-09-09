#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

rg --fixed-strings -q 'Button("Set as base branch")' macos/Sources/HerdrMacOS/GitWorktreesView.swift
rg --fixed-strings -q 'if let branch = worktree.branch' macos/Sources/HerdrMacOS/GitWorktreesView.swift

echo "The worktree row still offers base selection and still excludes detached rows"
