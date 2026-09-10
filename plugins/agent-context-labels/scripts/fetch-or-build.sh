#!/bin/sh
set -eu

# The plugin lives inside the Hide workspace, and the workspace owns the build
# output: plugins/agent-context-labels -> <workspace>/target/release.
root="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
workspace="$(CDPATH= cd -- "$root/../.." && pwd)"
bin="$workspace/target/release/hide-agent-context-labels"

if [ -x "$bin" ]; then
  exit 0
fi

cd "$workspace"
cargo build --release --locked -p agent-context-labels
