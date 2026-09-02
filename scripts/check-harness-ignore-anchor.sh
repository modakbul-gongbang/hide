#!/usr/bin/env bash
# The harness namespace `/agents/` is ignored; nothing else named `agents` is.
# The unanchored form also matched `.claude/agents/`, so this asserts both the
# rule that must hold and the collateral damage that must not return, plus the
# repository note that describes it.
set -euo pipefail

cd "$(dirname "$0")/.."

if ! git check-ignore -q agents/runs/example; then
    printf 'the harness namespace agents/ is not ignored\n' >&2
    exit 1
fi

if git check-ignore -q .claude/agents/simplify-scout.md; then
    printf 'the ignore rule still matches .claude/agents/\n' >&2
    exit 1
fi

if ! grep -q 'carries one anchored line, `/agents/`' AGENTS.md; then
    printf 'AGENTS.md does not describe the anchored ignore rule\n' >&2
    exit 1
fi

printf 'harness namespace anchored to /agents/\n'
