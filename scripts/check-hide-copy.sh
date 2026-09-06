#!/bin/bash
set -euo pipefail
node scripts/check-hide-copy.mjs --base "$1"
swift test --package-path macos --scratch-path /tmp/herdr-ide-verify/swift 2>&1 | tee "${TMPDIR:-/tmp}/hide-copy-swift.log"
