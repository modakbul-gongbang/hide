#!/bin/sh
set -eu

# The plugin lives inside the Hide workspace, and the workspace owns the build
# output: plugins/agent-context-labels -> <workspace>/target/release.
root="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
# Summaries come from the user's logged-in Codex CLI; a Herdr server started
# outside a login shell still has to find it. macOS pnpm installs may keep
# global CLI shims directly in Library/pnpm or in its bin subdirectory.
export PATH="$HOME/.local/bin:$HOME/bin:$HOME/Library/pnpm:$HOME/Library/pnpm/bin:/opt/homebrew/bin:/usr/local/bin:$PATH"

exec "$root/../../target/release/hide-agent-context-labels" watch
