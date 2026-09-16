#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/.."
node scripts/check-hide-copy.mjs --base "$1"
bash scripts/verify-swift.sh test 2>&1 | tee "${TMPDIR:-/tmp}/hide-copy-swift.log"
