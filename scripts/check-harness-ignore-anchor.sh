#!/usr/bin/env bash
# The entire root harness namespace is local-only; no PRD/config exception.
# The unanchored form also matched `.claude/agents/`, so this asserts both the
# rule that must hold and the collateral damage that must not return, plus the
# repository note that describes it.
set -euo pipefail

cd "$(dirname "$0")/.."

check_ignore() {
    local path="$1" expected="$2" failure="$3" status
    if git check-ignore --no-index -q -- "$path" > /dev/null 2>&1; then
        status=0
    else
        status=$?
    fi
    if [[ "$status" != 0 && "$status" != 1 ]]; then
        printf 'cannot inspect the harness ignore boundary\n' >&2
        exit 1
    fi
    if [[ "$status" != "$expected" ]]; then
        printf '%s\n' "$failure" >&2
        exit 1
    fi
}

for path in agents/ agents/config.json agents/prd/example/prd.md; do
    check_ignore "$path" 0 'the harness namespace agents/ is not ignored'
done
check_ignore .claude/agents/simplify-scout.md 1 'the ignore rule still matches .claude/agents/'

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
