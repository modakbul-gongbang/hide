#!/usr/bin/env bash
set -euo pipefail
# Searches use `git grep`, never `rg`: ripgrep is not on the CI runner.

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

view="macos/Sources/HerdrMacOS/GitWorktreesView.swift"
git grep -qF -- 'Text("Reading worktrees")' "$view"
git grep -qF -- '.disabled(loading || model.isRemoteContext)' "$view"
git grep -qF -- 'Text(worktree.unavailableReason == nil ? "↑\(worktree.ahead) ↓\(worktree.behind)" : "—")' "$view"
git grep -qF -- '.hideTooltip(worktree.unavailableReason ?? "Compared with \(worktree.baseBranch ?? "base")")' "$view"
git grep -qF -- 'notice("Repository unavailable: \(project?.rootPath ?? "")\n\(reason)")' "$view"

echo "Git worktree loading, refresh, failure and unavailable-repository states are explicit"
