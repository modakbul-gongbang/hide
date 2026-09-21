#!/usr/bin/env bash
# B5: hided-spike refuses to start without an isolated HERDR_SOCKET_PATH.
set -euo pipefail
measure_dir="$(cd "$(dirname "$0")" && pwd)"
worktree="$(cd "$measure_dir/../../.." && pwd)"
. "$worktree/scripts/toolchain-env.sh"
bin="$measure_dir/../hided-spike/target/debug/hided-spike"
if [[ ! -x "$bin" ]]; then
  cargo build --manifest-path "$measure_dir/../hided-spike/Cargo.toml"
fi

unset_out="$(env -u HERDR_SOCKET_PATH "$bin" 2>&1 || true)"
printf '%s\n' "$unset_out"
printf '%s\n' "$unset_out" | grep -q 'HERDR_SOCKET_PATH is unset'

operator_out="$(HERDR_SOCKET_PATH="$HOME/.config/herdr/herdr.sock" "$bin" 2>&1 || true)"
printf '%s\n' "$operator_out"
printf '%s\n' "$operator_out" | grep -q 'operator socket'
printf 'check-refuse: ok\n'
