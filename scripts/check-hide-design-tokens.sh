#!/bin/bash
set -euo pipefail
swift test --package-path macos --scratch-path /tmp/herdr-ide-verify/swift-design-tokens --filter HideDesignContractTests
