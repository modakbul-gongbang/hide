#!/usr/bin/env bash
# Isolation boundary for S0 measurements.
# Source from a subshell that owns the private server. Never export these
# values back into the operator pane.
#
# Scrubs inherited Herdr routing (this pane carries the operator socket) and
# points every child at a short private socket under the run directory.
set -euo pipefail

measure_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
spike_root="$(cd "$measure_dir/.." && pwd)"
worktree_root="$(cd "$spike_root/../.." && pwd)"
run_dir="${S0_RUN_DIR:-$worktree_root/agents/runs/web-shell-pivot-s0}"
mkdir -p "$run_dir" "$run_dir/logs" "$run_dir/fixture" "$run_dir/chrome-profile"

operator_socket="${HOME}/.config/herdr/herdr.sock"
manifest="$worktree_root/macos/Sources/HerdrMacOS/Resources/herdr-bundle.json"
pin_sha="$(jq -er '.sha256' "$manifest")"
pin_version="$(jq -er '.version' "$manifest")"
cached_herdr="${HIDE_HERDR_CACHE:-${XDG_CACHE_HOME:-$HOME/Library/Caches}/hide/herdr-runtime}/$pin_sha/herdr"
if [[ -x "$cached_herdr" ]]; then
  herdr_bin="$cached_herdr"
else
  herdr_bin="$(command -v herdr)"
fi
herdr_version="$("$herdr_bin" --version | awk '{print $2}')"
if [[ "$herdr_version" != "$pin_version" ]]; then
  printf 'isolated-env: herdr %s is not pin %s (%s)\n' "$herdr_version" "$pin_version" "$herdr_bin" >&2
  exit 2
fi

# Unix socket paths are short. Keep the node under /tmp, state under the run dir.
socket_path="/tmp/h-s0-$(printf %s "$run_dir" | shasum | cut -c1-10).sock"
private="$run_dir/isolated"
mkdir -p "$private/xdg-config" "$private/xdg-state" "$private/home"
config_path="$private/herdr-config.toml"
if [[ ! -f "$config_path" ]]; then
  printf '[update]\nversion_check = false\nmanifest_check = false\n' > "$config_path"
fi

unset HERDR_PANE_ID HERDR_TAB_ID HERDR_WORKSPACE_ID HERDR_ENV
export HERDR_SESSION="web-shell-s0"
export HERDR_DISABLE_SOUND=1
export HERDR_SOCKET_PATH="$socket_path"
export HERDR_CONFIG_PATH="$config_path"
export XDG_CONFIG_HOME="$private/xdg-config"
export XDG_STATE_HOME="$private/xdg-state"
export HERDR_BIN="$herdr_bin"
export HIDED_SPIKE_STATE_PATH="$private/hided-state.json"
export HIDED_SPIKE_PORT="${HIDED_SPIKE_PORT:-9876}"
export S0_CDP_PORT="${S0_CDP_PORT:-9333}"
export S0_RUN_DIR="$run_dir"
export S0_WORKTREE="$worktree_root"
export S0_SPIKE_ROOT="$spike_root"
export S0_OPERATOR_SOCKET="$operator_socket"
export S0_FIXTURE="$run_dir/fixture"
export S0_PRIVATE="$private"

if [[ "$HERDR_SOCKET_PATH" == "$operator_socket" ]]; then
  printf 'isolated-env: refusing to reuse the operator socket\n' >&2
  exit 2
fi
