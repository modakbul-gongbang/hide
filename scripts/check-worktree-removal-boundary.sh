#!/usr/bin/env bash
set -euo pipefail
# Searches use `git grep`, never `rg`: ripgrep is not on the CI runner.

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

# The host's confirmed removal (`hide_host::worktrees::remove_confirmed`) is the
# only code that removes a worktree the operator asked to delete, for the native
# and the web shell and on every device; the core's cleanup calls into it.
executor=hide-host/src/worktrees.rs
git grep -qF -- '&["branch", "-d", "--", branch]' "$executor"
if git grep -qE -- '"branch", "-D"|"worktree", "remove", "--force"|"worktree", "remove", "-f"' "$executor" herdr-core/src/worktree_cleanup.rs; then
    echo "Worktree removal must never force-delete a branch or a worktree" >&2
    exit 1
fi

echo "Branch deletion during worktree removal is -d and never -D"
