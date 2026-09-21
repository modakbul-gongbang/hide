#!/usr/bin/env bash
# Stop only the S0 private server. Never calls unscoped `herdr server stop`.
set -euo pipefail
measure_dir="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=isolated-env.sh
source "$measure_dir/isolated-env.sh"

pid_file="$S0_PRIVATE/herdr-server.pid"
if [[ -f "$pid_file" ]]; then
  pid="$(cat "$pid_file")"
  if kill -0 "$pid" 2>/dev/null; then
    "$HERDR_BIN" server stop >/dev/null 2>&1 || kill "$pid" 2>/dev/null || true
    deadline=$((SECONDS + 10))
    while kill -0 "$pid" 2>/dev/null && (( SECONDS < deadline )); do
      sleep 0.1
    done
    if kill -0 "$pid" 2>/dev/null; then
      kill -9 "$pid" 2>/dev/null || true
    fi
  fi
  rm -f "$pid_file"
fi
rm -f "$HERDR_SOCKET_PATH" "${HERDR_SOCKET_PATH%.sock}-client.sock"
printf 'stop-server: private socket removed\n'
