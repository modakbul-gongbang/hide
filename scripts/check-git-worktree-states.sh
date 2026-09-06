#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

bash scripts/swift-test.sh GitWorktreesPresentationTests
view="macos/Sources/HerdrMacOS/GitWorktreesView.swift"
rg --fixed-strings -q 'Text("Reading worktrees")' "$view"
rg --fixed-strings -q '.disabled(loading || model.isRemoteContext)' "$view"
rg --fixed-strings -q 'Text(worktree.unavailableReason == nil ? "↑\(worktree.ahead) ↓\(worktree.behind)" : "—")' "$view"
rg --fixed-strings -q '.help(worktree.unavailableReason ?? "Compared with \(worktree.baseBranch ?? "base")")' "$view"
rg --fixed-strings -q 'notice("Repository unavailable: \(project?.rootPath ?? "")\n\(reason)")' "$view"

echo "Git worktree loading, refresh, failure and unavailable-repository states are explicit"
