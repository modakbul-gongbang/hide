#!/usr/bin/env bash
set -euo pipefail
node scripts/check-hide-components.mjs
node scripts/check-hide-copy.mjs --base 6b45c64
swift test --package-path macos --scratch-path /tmp/herdr-ide-verify/swift-tooltips --filter 'HideHintTests|HideTooltipTests' 2>&1 | tee "${TMPDIR:-/tmp}/hide-tooltips-check.log"
