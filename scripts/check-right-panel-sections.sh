#!/usr/bin/env bash
# The right panel presents exactly Overview, Explorer and History, and
# the Workbench name it used to carry appears in no string the operator can
# read. The section set is asserted by the Swift suite; this asserts the name
# is gone from every user-facing surface, which no unit test can see.
set -euo pipefail
# Searches use `git grep`, never `rg`: ripgrep is not on the CI runner, and
# `if rg ...; then` reads a missing binary's 127 as "no match", which is how
# two required gates passed for weeks without ever running.

cd "$(dirname "$0")/.."

if git grep -qi 'workbench' -- macos/Sources/HerdrMacOS; then
    printf 'the Workbench name is still present in the shell sources:\n' >&2
    git grep -ni 'workbench' -- macos/Sources/HerdrMacOS >&2
    exit 1
fi

bash scripts/verify-swift.sh test --filter ChangesPresentationTests

printf 'right panel presents exactly Overview, Explorer and History; no Workbench string remains\n'
