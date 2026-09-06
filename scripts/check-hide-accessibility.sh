#!/usr/bin/env bash
set -euo pipefail
# Run in a linked checkout so verification cannot use the installed identity.
test "$(git rev-parse --path-format=absolute --git-dir)" != "$(git rev-parse --path-format=absolute --git-common-dir)"
app_path="$(bash macos/scripts/build_dev_app.sh | tail -1)"
swiftc scripts/hide-accessibility-probe.swift -o /tmp/herdr-ide-verify/hide-accessibility-probe
node scripts/check-hide-accessibility.mjs "$app_path"
