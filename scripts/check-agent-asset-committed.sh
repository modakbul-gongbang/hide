#!/usr/bin/env bash
# The simplification subagent is a committed repository asset, not a local
# convenience. It became uncommittable once already, because the harness
# ignore rule was unanchored and also matched `.claude/agents/`. This check
# fails if the asset ever leaves the committed file list again.
# Root agents/ is entirely private; this asset belongs to .claude/agents/.
set -euo pipefail

cd "$(dirname "$0")/.."

asset=".claude/agents/simplify-scout.md"

if ! git ls-files --error-unmatch -- "$asset" >/dev/null 2>&1; then
    printf 'not in the committed file list: %s\n' "$asset" >&2
    exit 1
fi

printf 'committed: %s\n' "$asset"
