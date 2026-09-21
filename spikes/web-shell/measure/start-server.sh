#!/usr/bin/env bash
# Start the private Herdr server for S0. Owns the child; writes a pid file.
set -euo pipefail
measure_dir="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=isolated-env.sh
source "$measure_dir/isolated-env.sh"

pid_file="$S0_PRIVATE/herdr-server.pid"
log_file="$S0_RUN_DIR/logs/herdr-server.log"
if [[ -S "$HERDR_SOCKET_PATH" ]]; then
  printf 'start-server: socket already exists: %s\n' "$HERDR_SOCKET_PATH" >&2
  exit 2
fi
if [[ -f "$pid_file" ]] && kill -0 "$(cat "$pid_file")" 2>/dev/null; then
  printf 'start-server: already running pid %s\n' "$(cat "$pid_file")" >&2
  exit 2
fi

mkdir -p "$S0_FIXTURE"
if [[ ! -d "$S0_FIXTURE/.git" ]]; then
  git -C "$S0_FIXTURE" init --quiet
  printf 's0 fixture\n' > "$S0_FIXTURE/README"
  git -C "$S0_FIXTURE" add README
  git -C "$S0_FIXTURE" \
    -c commit.gpgsign=false \
    -c user.email="s0@example.invalid" \
    -c user.name="s0" \
    commit --quiet -m "s0 fixture"
fi

: > "$log_file"
"$HERDR_BIN" server >>"$log_file" 2>&1 &
server_pid=$!
printf '%s\n' "$server_pid" > "$pid_file"
cleanup_on_fail() {
  if kill -0 "$server_pid" 2>/dev/null; then
    kill "$server_pid" 2>/dev/null || true
    wait "$server_pid" 2>/dev/null || true
  fi
  rm -f "$pid_file" "$HERDR_SOCKET_PATH" "${HERDR_SOCKET_PATH%.sock}-client.sock"
}
trap cleanup_on_fail EXIT

deadline=$((SECONDS + 20))
while (( SECONDS < deadline )); do
  if [[ -S "$HERDR_SOCKET_PATH" ]]; then
    break
  fi
  if ! kill -0 "$server_pid" 2>/dev/null; then
    printf 'start-server: herdr server exited; see %s\n' "$log_file" >&2
    exit 1
  fi
  sleep 0.1
done
if [[ ! -S "$HERDR_SOCKET_PATH" ]]; then
  printf 'start-server: socket did not appear: %s\n' "$HERDR_SOCKET_PATH" >&2
  exit 1
fi

snap="$("$HERDR_BIN" api snapshot)"
workspaces="$(printf '%s\n' "$snap" | python3 -c 'import json,sys; d=json.load(sys.stdin); s=(d.get("result") or d).get("snapshot") or (d.get("result") or d); print(len(s.get("workspaces") or []))')"
if [[ "$workspaces" != "0" ]]; then
  printf 'start-server: private server already has %s workspaces; aborting\n' "$workspaces" >&2
  exit 1
fi

"$HERDR_BIN" workspace create --cwd "$S0_FIXTURE" --label s0 --focus >/dev/null
trap - EXIT
printf 'start-server: pid %s socket %s\n' "$server_pid" "$HERDR_SOCKET_PATH"
