#!/usr/bin/env bash
# Searches use `git grep`, never `rg`: ripgrep is not on the CI runner.
set -euo pipefail
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"
view="macos/Sources/HerdrMacOS/CheckoutOverview.swift"
presentation="macos/Sources/HerdrMacOS/OverviewPresentation.swift"
git grep -qF -- 'HideEmptyState("Loading Overview"' "$view"
git grep -qF -- 'HideEmptyState("Overview is local only"' "$view"
git grep -qF -- 'model.refreshCheckoutCard()' "$view"
git grep -qF -- 'if let reason = model.card.github.unavailableReason' "$view"
git grep -qF -- 'OverviewPresentation.disconnected' "$view"
# The glyph language: `…` while a value is being read, `?` when it cannot be,
# never a measured zero in either place (right-panel-overview D-08).
git grep -qF -- 'value: "… GB"' "$presentation"
git grep -qF -- 'value: "? GB"' "$presentation"
git grep -qF -- 'text: "? files"' "$presentation"
git grep -qF -- 'text: "… files"' "$presentation"
echo "Overview loading, local-only, refresh, disconnected and unreadable states remain explicit"
