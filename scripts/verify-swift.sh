#!/usr/bin/env bash
# The swift entrypoint the PRD harness binds, as plain argv.
#
# Usage: verify-swift.sh build | test
#
# The counterpart to `verify-cargo.sh`, and it exists for the same reason: the
# harness runs each verify command with execvp and no shell, so a binding that
# needs an environment decision or a computed path cannot be a command string.
#
#     "typecheck": "bash scripts/verify-swift.sh build"
#     "lint":      "bash scripts/verify-swift.sh test"
#
# The shell links `target/release/libherdr_core.a` from a fixed path inside the
# worktree, so the core is built first and is never redirected. SwiftPM's own
# scratch path is redirected out of the tree, because a run that judges the
# working tree must not dirty it by building into `macos/.build`.
set -euo pipefail

cd "$(dirname "$0")/.."

. scripts/toolchain-env.sh

. scripts/build-scratch.sh

case "${1:-}" in
    build|test) ;;
    *)
        printf 'usage: %s build|test\n' "$0" >&2
        exit 2
        ;;
esac

# The archive the package links is read from the worktree at a fixed path, so
# every verification release build uses the same in-tree target directory.
bash scripts/verify-cargo.sh build

# SwiftPM's -L/-l flags do not declare the external archive as a build input.
# As in build_dev_app.sh, a content digest makes a changed core invalidate the
# Swift build plan, while an identical archive keeps the no-change cache hot.
archive_digest="$(LC_ALL=C shasum -a 256 target/release/libherdr_core.a)"
archive_digest="${archive_digest%% *}"

exec swift "$1" --package-path macos --scratch-path "$HIDE_SWIFT_SCRATCH" \
    --disable-keychain --disable-sandbox \
    -Xswiftc -D -Xswiftc "HERDR_CORE_${archive_digest}"
