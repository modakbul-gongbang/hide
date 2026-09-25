#!/usr/bin/env bash
set -euo pipefail
# Searches use `git grep`, never `rg`: ripgrep is not on the CI runner.

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

git grep -q 'Git context refreshes local worktree state only when repository metadata' -- docs/ARCHITECTURE.md
for token in lineageIndent lineageElbowY gitRowFontSize gitDetailFontSize; do
  git grep -q "static let $token" -- macos/Sources/HerdrMacOS/HideTheme.swift
done
git grep -Eq 'HideTheme\.(GitIcon|Overview)' \
  -- macos/Sources/HerdrMacOS/CheckoutOverview.swift

if git grep -nE '(^|[^A-Za-z0-9_])Color\(|\.padding\([0-9]|\.font\(\.system\(size: [0-9]' \
  -- macos/Sources/HerdrMacOS/CheckoutOverview.swift; then
  echo "Git worktree presentation contains an inline color, spacing, or font size" >&2
  exit 1
fi

echo "Git worktree documentation and presentation use the shared theme tokens"
