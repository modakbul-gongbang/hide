#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

rg -q 'Git section refreshes local worktree state only when repository metadata' "$root/AGENTS.md"
for token in lineageIndent lineageDeepIndent gitRowFontSize gitDetailFontSize; do
  rg -q "$token" "$root/DESIGN.md"
  rg -q "static let $token" "$root/macos/Sources/HerdrMacOS/HideUI.swift"
done
rg -q 'HideTheme\.GitIcon\.(merged|unmerged|dirty|clean|refresh)' \
  "$root/macos/Sources/HerdrMacOS/GitWorktreesView.swift"

if rg --pcre2 -n '(?<![[:alnum:]_])Color\(|\.padding\([0-9]|\.font\(\.system\(size: [0-9]' \
  "$root/macos/Sources/HerdrMacOS/GitWorktreesView.swift"; then
  echo "Git worktree presentation contains an inline color, spacing, or font size" >&2
  exit 1
fi

echo "Git worktree documentation and presentation use the shared theme tokens"
