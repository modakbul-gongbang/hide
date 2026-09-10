#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/.."
. scripts/build-scratch.sh
node scripts/check-hide-copy.mjs --base "$1"
swift test --package-path macos --scratch-path "$HIDE_SWIFT_SCRATCH" 2>&1 | tee "${TMPDIR:-/tmp}/hide-copy-swift.log"
