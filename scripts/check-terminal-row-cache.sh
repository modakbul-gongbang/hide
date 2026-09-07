#!/usr/bin/env bash
# The terminal's per-draw cost stays bounded by what changed, not by how many
# rows are on screen.
#
# `drawTerminalContents` used to call `buildAttributedString` for every visible
# row on every draw, and only segments of eight UTF-16 units or fewer reached
# the CTLine cache, so a frame that changed one cell relaid out the whole
# viewport and re-interned its attribute dictionaries. On this workstation a
# ten-second window with no interaction at all spent 15.3% of the sampled main
# thread under `_NSViewDrawRect`, which was 63% of everything the main thread
# did while idle.
#
# Rows are now prepared once per change behind `preparedRow`. That is a cost
# property, not a correctness one, so no test fails when it is removed; this
# gate is what notices. It is asserted structurally, in the style of
# `check-capability-readers-off-lock.sh`: the draw loop must reach text
# building only through the cache.
set -euo pipefail

cd "$(dirname "$0")/.."

source="macos/Vendor/SwiftTerm/Sources/SwiftTerm/Apple/AppleTerminalView.swift"

if [[ ! -f "$source" ]]; then
    printf 'the shared Apple terminal view is missing: %s\n' "$source" >&2
    exit 1
fi

# The cache entry point and its invalidation both have to exist. Without the
# second, a font or palette change would keep drawing rows built from the old
# one, which is the failure this gate's own fix could otherwise introduce.
for symbol in 'func preparedRow ' 'func invalidatePreparedRows ' 'struct PreparedRowKey'; do
    if ! grep -q "$symbol" "$source"; then
        printf '%s no longer defines `%s`; the terminal row cache is gone.\n' "$source" "$symbol" >&2
        exit 1
    fi
done

# The body of the CoreGraphics draw loop, from its own `func` line to the next
# method at the same indentation.
body="$(awk '
    /^    func drawTerminalContents /  { inside = 1; next }
    inside && /^    (func|@|\/\/\/) /  { exit }
    inside                             { print NR ": " $0 }
' "$source")"

if [[ -z "$body" ]]; then
    printf '%s no longer defines `drawTerminalContents`; this gate cannot read it.\n' "$source" >&2
    exit 1
fi

# Building an attributed string interns its attribute dictionaries into a
# process-wide weak table, and laying one out runs the CoreText font cascade.
# Neither belongs on a path that runs once per visible row per frame.
for forbidden in 'buildAttributedString(' 'cachedCTLine(' 'CTLineCreateWithAttributedString('; do
    hits="$(printf '%s\n' "$body" | grep -F "$forbidden" || true)"
    if [[ -n "$hits" ]]; then
        printf 'drawTerminalContents calls `%s` directly, so every visible row pays it on every draw:\n%s\n' \
            "$forbidden" "$hits" >&2
        printf 'Reach it through `preparedRow`, which builds a row only when something it is drawn from has changed.\n' >&2
        exit 1
    fi
done

if [[ "$body" != *'preparedRow('* ]]; then
    printf 'drawTerminalContents no longer calls `preparedRow`; the row cache is bypassed.\n' >&2
    exit 1
fi

printf 'terminal row cache: draw loop builds rows only through preparedRow\n'
