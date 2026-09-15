#!/usr/bin/env bash
set -euo pipefail
# Searches use `git grep`, never `rg`: ripgrep is not on the CI runner.

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

view="macos/Sources/HerdrMacOS/CheckoutOverview.swift"
git grep -qF -- 'HideEmptyState("Loading Overview"' "$view"
git grep -qF -- 'model.refreshCheckoutCard()' "$view"
git grep -qF -- 'Base unavailable' "$view"
git grep -qF -- 'if let reason = model.card.github.unavailableReason' "$view"
git grep -qF -- 'Git context unavailable' "$view"

echo "Overview Git loading, refresh, comparison and unavailable states remain explicit"
