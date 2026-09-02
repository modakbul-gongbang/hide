#!/usr/bin/env bash
# `⌘⇧B` was the chord the operator pressed and nothing claimed it, while the
# panel sat on `⌘⌥B`. Two things have to hold together: the catalog binds the
# chord, and no menu item, tooltip, or label still advertises the retired one.
# A test can prove the first and only a search can prove the second.
set -euo pipefail

cd "$(dirname "$0")/.."

bash scripts/swift-test.sh ShellMenuCommandTests

if rg --quiet '⌘⌥B' macos/Sources macos/Tests; then
    printf 'the retired ⌘⌥B chord is still advertised:\n' >&2
    rg --line-number '⌘⌥B' macos/Sources macos/Tests >&2
    exit 1
fi

printf 'the right panel toggle is ⇧⌘B and ⌘⌥B is advertised nowhere\n'
