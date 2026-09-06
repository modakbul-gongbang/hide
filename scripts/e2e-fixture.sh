#!/usr/bin/env bash
# Builds the private GitHub repository the project panel is verified against.
#
# The panel's whole subject is state that lives outside Hide: which worktrees a
# repository has, what their branches are ahead of, and what GitHub says about
# their pull requests. None of that can be faked into existence locally, so the
# fixture is a real private repository with real pull requests in every state
# the badge mapping distinguishes.
#
# It is idempotent: an existing repository, branch, worktree, or pull request
# is reused rather than recreated, so a re-run costs a few `gh` calls and
# changes nothing. The repository is kept after verification by decision, so
# the next run has the same pull-request history to look at.
#
# Usage: e2e-fixture.sh [--root <directory>]
#   --root  where the clone and its worktrees go (default: $TMPDIR/hide-e2e)
set -euo pipefail

repo_name="hide-e2e-fixture"
root="${HIDE_E2E_ROOT:-${TMPDIR:-/tmp}/hide-e2e}"
while [[ $# -gt 0 ]]; do
    case "$1" in
        --root) root="$2"; shift 2 ;;
        *) printf 'unknown argument: %s\n' "$1" >&2; exit 2 ;;
    esac
done
root="${root%/}"
# Resolve now, and once. `$TMPDIR` carries a trailing slash and `/var` is a
# symlink to `/private/var`, so an unresolved root compares unequal to the
# paths `git worktree list` reports and every idempotence check misses.
mkdir -p "$root"
root="$(cd "$root" && pwd -P)"

if ! command -v gh >/dev/null 2>&1; then
    printf 'gh is not installed; the fixture needs the GitHub CLI\n' >&2
    exit 1
fi
if ! gh auth status >/dev/null 2>&1; then
    printf 'gh is not logged in; run `gh auth login` first\n' >&2
    exit 1
fi

owner="$(gh api user --jq .login)"
slug="${owner}/${repo_name}"
main="${root}/${repo_name}"
worktrees="${root}/${repo_name}.worktrees"

git_q() { git -C "$main" "$@"; }

commit() {
    git_q -c commit.gpgsign=false \
        -c user.email="hide-e2e@example.invalid" \
        -c user.name="hide e2e" \
        commit --quiet "$@"
}

# 1. The repository. Private by decision, and kept afterwards.
if ! gh repo view "$slug" >/dev/null 2>&1; then
    printf 'creating %s\n' "$slug"
    gh repo create "$slug" --private \
        --description "Fixture repository for hide's project panel verification. Safe to keep." \
        >/dev/null
fi

# 2. The clone, with a first commit so branches have somewhere to start.
if [[ ! -d "$main/.git" ]]; then
    printf 'cloning %s\n' "$slug"
    gh repo clone "$slug" "$main" -- --quiet
fi
git_q config commit.gpgsign false
if ! git_q rev-parse --verify --quiet HEAD >/dev/null; then
    git_q symbolic-ref HEAD refs/heads/main
    printf '# hide e2e fixture\n\nA fixture for verifying the project panel.\n' > "$main/README.md"
    git_q add README.md
    commit -m "Add the fixture readme"
    git_q push --quiet -u origin main
fi
git_q fetch --quiet origin

default_branch="$(gh repo view "$slug" --json defaultBranchRef --jq .defaultBranchRef.name)"

# 3. One branch per state the panel has to tell apart.
#
#    A branch is created from the default branch with one commit on it, then
#    pushed or not depending on what it is proving.
make_branch() {
    local branch="$1" file="$2" body="$3"
    if git_q rev-parse --verify --quiet "refs/heads/$branch" >/dev/null; then
        return
    fi
    # `--no-track`: branching off a remote-tracking ref would otherwise set
    # `origin/<default>` as the upstream, and `never-pushed` exists precisely
    # to be a branch with no upstream at all.
    git_q branch --no-track "$branch" "origin/${default_branch}"
    local temp="${root}/.stage-${branch}"
    rm -rf "$temp"
    git_q worktree add --quiet "$temp" "$branch"
    printf '%s\n' "$body" > "${temp}/${file}"
    git -C "$temp" add "$file"
    git -C "$temp" -c commit.gpgsign=false \
        -c user.email="hide-e2e@example.invalid" \
        -c user.name="hide e2e" \
        commit --quiet -m "Add ${file} on ${branch}"
    git_q worktree remove --force "$temp"
}

# `never-pushed` has no upstream, so the card must omit the unpushed row
# entirely rather than show a zero.
make_branch never-pushed never-pushed.md "This branch was never pushed."
# `open-pr`, `merged-pr`, and `closed-pr` carry the three pull-request states.
make_branch open-pr open.md "This branch has an open pull request."
make_branch merged-pr merged.md "This branch's pull request is merged."
make_branch closed-pr closed.md "This branch's pull request was closed unmerged."
# `ahead-one` is pushed and then gains one local commit, so the card must
# show exactly one unpushed commit.
make_branch ahead-one ahead.md "This branch is one commit ahead of its upstream."

for branch in open-pr merged-pr closed-pr ahead-one; do
    if ! git_q rev-parse --verify --quiet "refs/remotes/origin/${branch}" >/dev/null; then
        git_q push --quiet -u origin "$branch"
    fi
done

# 4. The pull requests, one per state.
open_pull_request() {
    local branch="$1" title="$2"
    if [[ -n "$(gh pr list --repo "$slug" --head "$branch" --state all --json number --jq '.[].number')" ]]; then
        return
    fi
    gh pr create --repo "$slug" --base "$default_branch" --head "$branch" \
        --title "$title" --body "Fixture pull request for hide's project panel." >/dev/null
}

open_pull_request open-pr "Open pull request"
open_pull_request merged-pr "Merged pull request"
open_pull_request closed-pr "Closed pull request"

merged_number="$(gh pr list --repo "$slug" --head merged-pr --state all --json number,state --jq '.[0].number')"
merged_state="$(gh pr list --repo "$slug" --head merged-pr --state all --json state --jq '.[0].state')"
if [[ "$merged_state" != "MERGED" ]]; then
    gh pr merge "$merged_number" --repo "$slug" --merge --admin >/dev/null
fi

closed_number="$(gh pr list --repo "$slug" --head closed-pr --state all --json number --jq '.[0].number')"
closed_state="$(gh pr list --repo "$slug" --head closed-pr --state all --json state --jq '.[0].state')"
if [[ "$closed_state" == "OPEN" ]]; then
    gh pr close "$closed_number" --repo "$slug" >/dev/null
fi

# 5. `ahead-one` gains a local commit after its push, so `↑1 origin` has
#    something true to report.
if [[ "$(git_q rev-list --count "origin/ahead-one..ahead-one")" == "0" ]]; then
    temp="${root}/.stage-ahead"
    rm -rf "$temp"
    git_q worktree add --quiet "$temp" ahead-one
    printf 'A second change, committed but not pushed.\n' >> "${temp}/ahead.md"
    git -C "$temp" add ahead.md
    git -C "$temp" -c commit.gpgsign=false \
        -c user.email="hide-e2e@example.invalid" \
        -c user.name="hide e2e" \
        commit --quiet -m "Add an unpushed commit"
    git_q worktree remove --force "$temp"
fi

# 6. The worktrees the sidebar lists. One per branch, plus one whose folder is
#    deleted so the `missing` badge has a subject.
mkdir -p "$worktrees"
add_worktree() {
    local branch="$1" path="${worktrees}/$1"
    if git_q worktree list --porcelain | grep -qx "worktree ${path}"; then
        return
    fi
    git_q worktree add --quiet "$path" "$branch"
}

for branch in never-pushed open-pr merged-pr closed-pr ahead-one; do
    add_worktree "$branch"
done

# The `missing` state: git still lists the worktree, the folder is gone.
if ! git_q worktree list --porcelain | grep -qx "worktree ${worktrees}/gone"; then
    git_q branch --force gone "origin/${default_branch}" >/dev/null 2>&1 || true
    git_q worktree add --quiet "${worktrees}/gone" gone
fi
rm -rf "${worktrees}/gone"

# 7. The states the card's own rows need.
#
#    A large subdirectory, so the biggest-folder row has an obvious answer.
big="${worktrees}/open-pr/vendor"
if [[ ! -d "$big" ]]; then
    mkdir -p "$big"
    # 40 MB is enough to dominate a repository of text files without being
    # slow to write or expensive to keep.
    dd if=/dev/zero of="${big}/bundle.bin" bs=1m count=40 status=none
fi

#    Uncommitted work in the merged worktree, so Remove worktree has a reason
#    to be disabled before it has a reason to be enabled.
if [[ -d "${worktrees}/merged-pr" ]]; then
    printf 'An uncommitted edit.\n' > "${worktrees}/merged-pr/scratch.md"
fi

#    A plain folder that is not a repository, for the card that has no branch,
#    no pull request, and no changes to show.
plain="${root}/plain-folder"
mkdir -p "$plain"
printf 'Not a git repository.\n' > "${plain}/notes.md"

printf '\nfixture ready\n'
printf '  repository   %s\n' "https://github.com/${slug}"
printf '  main clone   %s\n' "$main"
printf '  worktrees    %s\n' "$worktrees"
printf '  plain folder %s\n' "$plain"
printf '\npull requests\n'
gh pr list --repo "$slug" --state all \
    --json number,headRefName,state,isDraft,reviewDecision \
    --jq '.[] | "  #\(.number) \(.headRefName) \(.state)"'
printf '\nworktrees\n'
git_q worktree list
