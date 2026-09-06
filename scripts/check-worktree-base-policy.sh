#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

cargo test --manifest-path herdr-core/Cargo.toml base_override_moves_protection_and_absent_override_reports_fallback
bash scripts/swift-test.sh GitWorktreesPresentationTests
rg --fixed-strings -q 'Button("Set as base branch")' macos/Sources/HerdrMacOS/GitWorktreesView.swift
rg --fixed-strings -q 'if let branch = worktree.branch' macos/Sources/HerdrMacOS/GitWorktreesView.swift

echo "Base selection persists, falls back, moves protection and excludes detached rows"
