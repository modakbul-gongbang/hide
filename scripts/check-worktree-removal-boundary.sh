#!/usr/bin/env bash
set -euo pipefail
# Searches use `git grep`, never `rg`: ripgrep is not on the CI runner.

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

# The core's confirmed-removal worker is the only code that removes a worktree
# the operator asked to delete, for the native and the web shell alike.
executor=herdr-core/src/worktree_cleanup.rs
git grep -qF -- '&["branch", "-d", "--", branch]' "$executor"
if git grep -qE -- '"branch", "-D"|"worktree", "remove", "--force"|"worktree", "remove", "-f"' "$executor"; then
    echo "Worktree removal must never force-delete a branch or a worktree" >&2
    exit 1
fi

echo "Branch deletion during worktree removal is -d and never -D"
