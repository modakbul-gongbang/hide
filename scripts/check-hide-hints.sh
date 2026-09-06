#!/usr/bin/env bash
set -euo pipefail
swift test --package-path macos --scratch-path /tmp/herdr-ide-verify/swift-hints --filter 'HideHintTests|PaneShortcutSettingsTests|UnifiedTab' 2>&1 | tee "${TMPDIR:-/tmp}/hide-hints-check.log"
