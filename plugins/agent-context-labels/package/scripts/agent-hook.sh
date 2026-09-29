#!/bin/sh
set -eu

# The install kit ships the release binary beside this plugin's manifest.
root="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
exec "$root/hide-agent-context-labels" hook
