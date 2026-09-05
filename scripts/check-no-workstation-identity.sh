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
set -euo pipefail

cd "$(dirname "$0")/.."

pattern='/Users/|hoyeonlee|grabs-mac-mini|tail56d7a2'
allowed='/Users/example'

hits="$(git grep -nE "$pattern" -- . ':!macos/Vendor' ':!agents' | grep -vF "$allowed" || true)"
if [[ -n "$hits" ]]; then
    printf 'workstation identities are present in tracked files:\n%s\n' "$hits" >&2
    exit 1
fi

printf 'no workstation identity in tracked files (allowed: %s)\n' "$allowed"
