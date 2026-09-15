#!/usr/bin/env bash
set -euo pipefail
# git is available in CI; these assertions follow the retained Overview owner.
cd "$(dirname "$0")/.."
git grep -qF -- 'row.isMain ? "Main checkout" : row.label' macos/Sources/HerdrMacOS/CheckoutOverview.swift
git grep -qF -- 'No commits yet' macos/Sources/HerdrMacOS/OverviewGitTree.swift
echo "Overview retains the main-checkout identity and empty Git history state"
