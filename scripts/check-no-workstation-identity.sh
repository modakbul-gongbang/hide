#!/usr/bin/env bash
# The public tree describes no one's workstation. Test fixtures once quoted a
# real home directory and a settings placeholder suggested a real machine name;
# an identity scan passed because it read text and skipped images. This scans
# every tracked text file for the strings that leaked, so the next leak fails
# the pull request instead of shipping.
#
# `/Users/example` is the one home directory allowed, for fixtures that need a
# path shaped like a real one. `macos/Vendor/` is upstream code and carries its
# own authors' examples. `spikes/` is a frozen record and is scanned too: it
# was where the last leak lived.
#
# The text scan is only half of it. The leak that reached the public history
# was a committed browser profile and 184 screenshots, and a text scan passed
# over both. `.gitignore` covers those paths but `git add -f` walks past it, so
# the second half below refuses the paths and the file shapes themselves.
set -euo pipefail

cd "$(dirname "$0")/.."

pattern='/Users/|hoyeonlee|grabs-mac-mini|tail56d7a2'
allowed='/Users/example'

# This script carries the pattern itself, so it is the one file excluded.
hits="$(git grep -nE "$pattern" -- . ':!macos/Vendor' ':!agents' ':!scripts/check-no-workstation-identity.sh' \
    | grep -vF "$allowed" || true)"
if [[ -n "$hits" ]]; then
    printf 'workstation identities are present in tracked files:\n%s\n' "$hits" >&2
    exit 1
fi

# Run artifacts are ignored, but an ignore rule is not a gate: the evidence
# tree that leaked was force-added. Refuse the paths outright.
evidence_paths="$(git ls-files -- 'docs/verification' 'docs/screenshots' 'spikes/*/evidence' || true)"
if [[ -n "$evidence_paths" ]]; then
    printf 'run artifacts are tracked; they belong under agents/runs/<slug>/:\n%s\n' "$evidence_paths" >&2
    exit 1
fi

# A browser profile carries cookies and a session no text scan can see.
profile_files="$(git ls-files \
    | grep -iE '(^|/)(Cookies|History|Login Data|Web Data|Local State)$|\.pma$' || true)"
if [[ -n "$profile_files" ]]; then
    printf 'browser profile artifacts are tracked; never commit a profile:\n%s\n' "$profile_files" >&2
    exit 1
fi

printf 'no workstation identity, tracked run artifacts, or browser profile in tracked files (allowed: %s)\n' "$allowed"
