#!/usr/bin/env bash
set -euo pipefail

# SASU_REPOSITORY lets CI or another workstation point this cross-repository
# contract check at its checkout. The default is the sibling of the source
# repository resolved through this worktree's common Git directory, because
# the mechanical runner deliberately supplies an isolated HOME.
project_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
common_git_dir="$(git -C "$project_root" rev-parse --path-format=absolute --git-common-dir)"
default_repository="$(dirname "$(dirname "$common_git_dir")")/sasu"
sasu_repository="${SASU_REPOSITORY:-$default_repository}"
commit="7ce9f5aa11f0ee74486920d414f75d05d80403e3"

git -C "$sasu_repository" cat-file -e "$commit^{commit}"

fixture="$(mktemp -d "${TMPDIR:-/tmp}/hide-sasu-from-pane.XXXXXX")"
trap 'rm -rf -- "$fixture"' EXIT
git -C "$sasu_repository" archive "$commit" | tar -x -C "$fixture"
ln -s "$sasu_repository/cli/node_modules" "$fixture/cli/node_modules"

assert_commit_text() {
  local path="$1"
  local pattern="$2"
  if ! rg --fixed-strings -q -- "$pattern" "$fixture/$path"; then
    echo "$path at $commit is missing: $pattern" >&2
    exit 1
  fi
}

assert_commit_text \
  "skills/implement/references/observer-and-herdr.md" \
  'herdr agent new <unique-name> --from-pane "$HERDR_PANE_ID"'
assert_commit_text \
  "cli/src/implement/herdr.ts" \
  '"agent", "new", input.name, "--from-pane", paneId('
assert_commit_text \
  "cli/src/implement/commands.ts" \
  'spawnImplementor({ name: `${agent}-r${id}`'
assert_commit_text \
  "cli/test/unit/implement-herdr.test.mjs" \
  '["agent", "new", "impl", "--from-pane", "w4G:p12"]'

"$sasu_repository/cli/node_modules/.bin/tsc" -p "$fixture/cli/tsconfig.json"
node --test "$fixture/cli/test/unit/implement-herdr.test.mjs"

echo "Recorded Sasu commit $commit passes the from-pane dispatch contract and focused tests"
