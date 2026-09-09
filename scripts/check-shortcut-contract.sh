#!/usr/bin/env bash
# `⌘⇧B` was the chord the operator pressed and nothing claimed it, while the
# panel sat on `⌘⌥B`. Two things have to hold together: the catalog binds the
# chord, and no menu item, tooltip, or label still advertises the retired one.
# A test can prove the first and only a search can prove the second.
set -euo pipefail
# Searches use `git grep`, never `rg`: ripgrep is not on the CI runner, and
# `if rg ...; then` reads a missing binary's 127 as "no match", which is how
# two required gates passed for weeks without ever running.

cd "$(dirname "$0")/.."

bash scripts/swift-test.sh ShellMenuCommandTests

if git grep -q '⌘⌥B' -- macos/Sources macos/Tests; then
    printf 'the retired ⌘⌥B chord is still advertised:\n' >&2
    git grep -n '⌘⌥B' -- macos/Sources macos/Tests >&2
    exit 1
fi

printf 'the right panel toggle is ⇧⌘B and ⌘⌥B is advertised nowhere\n'
