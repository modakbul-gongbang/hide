#!/usr/bin/env bash
# Existing CI entrypoint. Default: tracked checkout bytes; --scope index reads
# the exact staged blobs instead. See the Python command's --help for limits.
set -euo pipefail
cd "$(dirname "$0")/.."
exec python3 scripts/check-no-workstation-identity.py "$@"
