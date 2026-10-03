#!/usr/bin/env bash
# The entire root harness namespace is local-only; no PRD/config exception.
# The unanchored form also matched `.claude/agents/`, so this asserts both the
# rule that must hold and the collateral damage that must not return, plus the
# repository note that describes it.
set -euo pipefail

cd "$(dirname "$0")/.."

if ! git check-ignore --no-index -q agents/runs/example; then
    printf 'the harness namespace agents/ is not ignored\n' >&2
    exit 1
fi

if git check-ignore --no-index -q .claude/agents/simplify-scout.md; then
    printf 'the ignore rule still matches .claude/agents/\n' >&2
    exit 1
fi

if git ls-files --error-unmatch -- agents > /dev/null 2>&1; then
    printf 'root agents/ must have zero indexed paths; preserve local files when removing tracking\n' >&2
    exit 1
else
    status=$?
    if [[ "$status" != 1 ]]; then
        printf 'cannot inspect root agents/ tracking\n' >&2
        exit 1
    fi
fi

if ! grep -Fq 'carries one anchored line, `/agents/`' AGENTS.md; then
    printf 'AGENTS.md does not describe the anchored ignore rule\n' >&2
    exit 1
fi

printf 'root agents/ is ignored and has zero indexed paths\n'
