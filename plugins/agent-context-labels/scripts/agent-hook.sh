#!/bin/sh
set -eu

# The plugin lives inside the Hide workspace, and the workspace owns the build
# output: plugins/agent-context-labels -> <workspace>/target/release.
root="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
exec "$root/../../target/release/hide-agent-context-labels" hook
