#!/usr/bin/env bash
# The swift entrypoint for verification, as plain argv.
#
# Usage: verify-swift.sh build | test [swift arguments...]
#
# The counterpart to `verify-cargo.sh`, and it exists for the same reason: the
# harness runs each verify command with execvp and no shell, so a binding that
# needs a computed value cannot be a command string.
#
#     "build": "bash scripts/verify-swift.sh test"
#
# Arguments after the mode reach swift unchanged, so a single suite runs as
# `verify-swift.sh test --filter ChangesPresentationTests`.
#
# The shell links `target/release/libherdr_core.a` from a fixed path inside the
# worktree, so the core is built first. Swift output stays at SwiftPM's default
# `macos/.build`, which is ignored and removed with the worktree.
#
# HIDE_CORE_ARCHIVE_PREBUILT=1 skips that Cargo run and links the archive as it
# is. Only CI sets it, after restoring the archive from a cache keyed by every
# source the archive is built from; a missing archive is then an error, never a
# rebuild, because a rebuild there means the cache key and the sources have
# diverged. Locally the variable stays unset and Cargo decides freshness.
set -euo pipefail

cd "$(dirname "$0")/.."

case "${1:-}" in
    build|test) ;;
    *)
        printf 'usage: %s build|test [swift arguments...]\n' "$0" >&2
        exit 2
        ;;
esac
mode="$1"
shift

archive=target/release/libherdr_core.a
if [[ "${HIDE_CORE_ARCHIVE_PREBUILT:-}" == "1" ]]; then
    if [[ ! -f "$archive" ]]; then
        printf 'HIDE_CORE_ARCHIVE_PREBUILT=1 but %s is missing\n' "$archive" >&2
        exit 1
    fi
else
    bash scripts/verify-cargo.sh build
fi

# SwiftPM's -L/-l flags do not declare the external archive as a build input.
# As in build_dev_app.sh, a content digest makes a changed core invalidate the
# Swift build plan, while an identical archive keeps the no-change cache hot.
archive_digest="$(LC_ALL=C shasum -a 256 "$archive")"
archive_digest="${archive_digest%% *}"

# A machine with the Command Line Tools and no Xcode keeps swift-testing's
# `Testing.framework` and its support libraries under the tools' own Developer
# folders, which SwiftPM puts on neither the compiler's framework search path
# nor the test runner's rpath there, so every test target fails to import
# `Testing`. Only on such a machine the test run names those two folders; with
# Xcode selected, as on the CI runners, the command is unchanged. Nothing is
# filtered or skipped either way.
toolchain_flags=()
clt=/Library/Developer/CommandLineTools
if [[ "$mode" == "test" && "$(xcode-select -p 2>/dev/null || true)" == "$clt" \
    && -d "$clt/Library/Developer/Frameworks/Testing.framework" ]]; then
    frameworks="$clt/Library/Developer/Frameworks"
    libraries="$clt/Library/Developer/usr/lib"
    toolchain_flags=(
        -Xswiftc "-F$frameworks" -Xlinker "-F$frameworks"
        -Xlinker -rpath -Xlinker "$frameworks" -Xlinker -rpath -Xlinker "$libraries"
    )
fi

exec swift "$mode" --package-path macos --disable-keychain --disable-sandbox \
    -Xswiftc -D -Xswiftc "HERDR_CORE_${archive_digest}" \
    ${toolchain_flags[@]+"${toolchain_flags[@]}"} "$@"
