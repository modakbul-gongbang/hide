#!/usr/bin/env bash
# The right panel presents exactly Explorer, Changes and Git, and
# the Workbench name it used to carry appears in no string the operator can
# read. The section set is asserted by the Swift suite; this asserts the name
# is gone from every user-facing surface, which no unit test can see.
set -euo pipefail

cd "$(dirname "$0")/.."

if rg -ni 'workbench' macos/Sources/HerdrMacOS >/dev/null; then
    printf 'the Workbench name is still present in the shell sources:\n' >&2
    rg -ni 'workbench' macos/Sources/HerdrMacOS >&2
    exit 1
fi

bash scripts/swift-test.sh ChangesPresentationTests

printf 'right panel presents exactly Explorer, Changes and Git; no Workbench string remains\n'
