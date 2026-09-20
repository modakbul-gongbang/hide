#!/usr/bin/env bash
# git is available in CI; these assertions follow the retained Overview owner.
set -euo pipefail
cd "$(dirname "$0")/.."
git grep -qF -- 'row.isMain ? "Main checkout" : row.label' macos/Sources/HerdrMacOS/CheckoutOverview.swift
git grep -qF -- 'Text("No agent")' macos/Sources/HerdrMacOS/CheckoutOverview.swift
git grep -qF -- 'static let noMatch = "No matching agents or workspaces"' macos/Sources/HerdrMacOS/OverviewPresentation.swift
echo "Overview retains the main-checkout identity, the empty-group row and the no-match state"
