#!/usr/bin/env bash
# Owner of every isolated fixture, daemon, browser and driver for the web
# shell echo (PRD B8) and frame (PRD B12) measurements against the product
# hided. Usage:
#   HIDE_MEASURE_RUN_DIR=agents/runs/<slug>/measure bash scripts/web-shell-measure/run.sh
# Needs: the pinned herdr (HERDR_BIN_PATH or PATH), Google Chrome,
# target/release/hided built after `pnpm --dir web build` (docs/BUILD.md: a release hided embeds web/dist).
set -euo pipefail
measure_dir="$(cd "$(dirname "$0")" && pwd)"
source "$measure_dir/isolated-env.sh"
chrome_bin="${MEASURE_CHROME_BIN:-/Applications/Google Chrome.app/Contents/MacOS/Google Chrome}"
hided_bin="$MEASURE_WORKTREE/target/release/hided"
[[ -x "$hided_bin" ]] || { echo "build target/release/hided first: pnpm --dir web build, then the release build described in docs/BUILD.md" >&2; exit 1; }
[[ -x "$chrome_bin" ]] || { echo "Chrome not found at $chrome_bin" >&2; exit 1; }
pids=()
server_started=false
note() { printf 'measure: %s\n' "$*"; }
cleanup() {
  trap - EXIT INT TERM
  set +e
  for pid in "${pids[@]:-}"; do [[ -n "$pid" ]] && kill "$pid" 2>/dev/null; done
  if $server_started; then
    "$HERDR_BIN_PATH" server stop >/dev/null 2>&1 || true
    sleep 1
    [[ -n "${server_pid:-}" ]] && kill "$server_pid" 2>/dev/null
    rm -f "$HERDR_SOCKET_PATH" "${HERDR_SOCKET_PATH%.sock}-client.sock"
  fi
  for pid in "${pids[@]:-}"; do [[ -n "$pid" ]] && wait "$pid" 2>/dev/null; done
  ps -axo pid,ppid,command > "$MEASURE_RUN_DIR/cleanup-processes.txt"
  python3 "$measure_dir/operator-counts.py" "$HERDR_BIN_PATH" "$MEASURE_OPERATOR_SOCKET" "$MEASURE_RUN_DIR/operator-after.json"
}
trap cleanup EXIT
trap 'exit 130' INT TERM
spawn_owned() {
  local name=$1; shift
  (( ${#pids[@]} < 8 )) || { echo 'process owner budget exceeded' >&2; exit 1; }
  "$@" >"$MEASURE_RUN_DIR/logs/$name.log" 2>&1 &
  owned_pid=$!
  pids+=("$owned_pid")
}
wait_url() {
  local deadline=$((SECONDS+30))
  until curl -sf "$1" >/dev/null; do
    (( SECONDS < deadline )) || { echo "unready: $1" >&2; exit 1; }
    sleep 0.2
  done
}
wait_js() {
  local deadline=$((SECONDS+30))
  until [[ "$(node -e "
import('$measure_dir/cdp.mjs').then(async ({connectPage}) => { const p = await connectPage('$MEASURE_CDP_PORT'); console.log(await p.evaluate(process.argv[1])); p.close(); }).catch(() => console.log('unready'))" "$1")" == true ]]; do
    (( SECONDS < deadline )) || { echo "browser unready: $1" >&2; exit 1; }
    sleep 0.3
  done
}
reset_fixture() {
  "$HERDR_BIN_PATH" pane send-keys "$MEASURE_PANE_ID" ctrl+c >/dev/null
  sleep 0.3
  "$HERDR_BIN_PATH" pane run "$MEASURE_PANE_ID" 'printf "\033c"; stty -echo -icanon; cat' >/dev/null
  sleep 0.5
}

# Never adopt an unrelated server on the CDP port.
python3 - "$MEASURE_CDP_PORT" <<'PY'
import socket,sys
with socket.socket() as s:
    s.setsockopt(socket.SOL_SOCKET,socket.SO_REUSEADDR,1)
    s.bind(('127.0.0.1',int(sys.argv[1])))
PY
{
  echo "worktree_head=$(git -C "$MEASURE_WORKTREE" rev-parse HEAD)"
  echo "worktree_dirty=$(git -C "$MEASURE_WORKTREE" status --porcelain | wc -l | tr -d ' ')"
  echo "hided_bin=$hided_bin"
  echo "hided_sha256=$(shasum -a 256 "$hided_bin" | awk '{print $1}')"
  echo "herdr_bin=$HERDR_BIN_PATH"
  echo "herdr_version=$("$HERDR_BIN_PATH" --version)"
  echo "chrome_version=$("$chrome_bin" --version 2>/dev/null)"
  echo "socket=$HERDR_SOCKET_PATH"
  echo "started_at=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  uptime
} > "$MEASURE_RUN_DIR/identity.txt"
python3 "$measure_dir/operator-counts.py" "$HERDR_BIN_PATH" "$MEASURE_OPERATOR_SOCKET" "$MEASURE_RUN_DIR/operator-before.json"

# Private server and one cat pane.
[[ -S "$HERDR_SOCKET_PATH" ]] && { echo "socket already exists: $HERDR_SOCKET_PATH" >&2; exit 2; }
if [[ ! -d "$MEASURE_FIXTURE/.git" ]]; then
  git -C "$MEASURE_FIXTURE" init --quiet
  printf 'measure fixture\n' > "$MEASURE_FIXTURE/README"
  git -C "$MEASURE_FIXTURE" add README
  git -C "$MEASURE_FIXTURE" -c commit.gpgsign=false -c user.email="measure@example.invalid" -c user.name="measure" commit --quiet -m "measure fixture"
fi
env HOME="$MEASURE_PRIVATE/home" "$HERDR_BIN_PATH" server >"$MEASURE_RUN_DIR/logs/herdr-server.log" 2>&1 &
server_pid=$!
server_started=true
deadline=$((SECONDS+20)); until [[ -S "$HERDR_SOCKET_PATH" ]]; do (( SECONDS < deadline )) || { echo 'socket did not appear' >&2; exit 1; }; sleep 0.1; done
workspaces="$("$HERDR_BIN_PATH" api snapshot | python3 -c 'import json,sys; d=json.load(sys.stdin); r=d.get("result") or d; s=r.get("snapshot") or r; print(len(s.get("workspaces") or []))')"
[[ "$workspaces" == "0" ]] || { echo "private server already has $workspaces workspaces" >&2; exit 1; }
"$HERDR_BIN_PATH" workspace create --cwd "$MEASURE_FIXTURE" --label measure --focus >/dev/null
export MEASURE_PANE_ID="$("$HERDR_BIN_PATH" api snapshot | python3 "$measure_dir/pane-id.py")"
sleep 1
"$HERDR_BIN_PATH" pane run "$MEASURE_PANE_ID" 'stty -echo -icanon; cat' >/dev/null
sleep 1

# Product hided (release, embedded web/dist) on the private socket.
spawn_owned hided env HOME="$MEASURE_PRIVATE/home" HIDE_STATE_DIR="$MEASURE_PRIVATE/hide-state" HIDE_KEEP_ALIVE=1 HIDE_PORT=0 "$hided_bin"
hided_pid=$owned_pid
deadline=$((SECONDS+20)); until [[ -f "$MEASURE_PRIVATE/hide-state/hided.json" ]]; do (( SECONDS < deadline )) || { echo 'hided state file missing' >&2; exit 1; }; sleep 0.1; done
hided_port="$(python3 -c "import json;print(json.load(open('$MEASURE_PRIVATE/hide-state/hided.json'))['port'])")"
hided_token="$(python3 -c "import json;print(json.load(open('$MEASURE_PRIVATE/hide-state/hided.json'))['token'])")"
wait_url "http://127.0.0.1:$hided_port/health"
page_url="http://127.0.0.1:$hided_port/?probe=1#token=$hided_token"

spawn_owned chrome "$chrome_bin" --user-data-dir="$MEASURE_RUN_DIR/chrome-profile" --remote-debugging-port="$MEASURE_CDP_PORT" --remote-debugging-address=127.0.0.1 --no-first-run --no-default-browser-check --disable-sync --disable-background-networking --disable-component-update --disable-background-timer-throttling --disable-renderer-backgrounding --disable-backgrounding-occluded-windows --window-size=1280,900 "$page_url"
wait_url "http://127.0.0.1:$MEASURE_CDP_PORT/json/list"
wait_js 'Boolean(window.__hideProbe && window.__hideProbe.paneId())'
sleep 2
node -e "
import('$measure_dir/cdp.mjs').then(async ({connectPage}) => { const p = await connectPage('$MEASURE_CDP_PORT'); console.log(JSON.stringify(await p.evaluate('({pane: window.__hideProbe.paneId(), cols: document.querySelector(\".xterm\") ? undefined : null, ua: navigator.userAgent, dpr: devicePixelRatio, inner: [innerWidth, innerHeight]})'))); p.close(); })" > "$MEASURE_RUN_DIR/page.json"

for trial in 1 2 3; do
  reset_fixture
  note "echo trial $trial (50 samples)"
  ps -p "$hided_pid" -o pid,%cpu,rss,etime,command > "$MEASURE_RUN_DIR/idle-hided-$trial.ps"
  MEASURE_ECHO_REPEATS=50 node "$measure_dir/echo.mjs" > "$MEASURE_RUN_DIR/echo-$trial.json"
  ps -p "$hided_pid" -o pid,%cpu,rss,etime,command > "$MEASURE_RUN_DIR/driven-hided-$trial.ps"
done
python3 "$measure_dir/summarize.py" echo "$MEASURE_RUN_DIR"/echo-{1,2,3}.json > "$MEASURE_RUN_DIR/echo-summary.json"

note 'frames: 120 s driven window'
reset_fixture
"$HERDR_BIN_PATH" pane send-keys "$MEASURE_PANE_ID" ctrl+c >/dev/null
sleep 0.5
uptime > "$MEASURE_RUN_DIR/frames-uptime-before.txt"
node "$measure_dir/frames.mjs" > "$MEASURE_RUN_DIR/frames.json"
uptime > "$MEASURE_RUN_DIR/frames-uptime-after.txt"
"$HERDR_BIN_PATH" pane read "$MEASURE_PANE_ID" --source recent-unwrapped --lines 5 > "$MEASURE_RUN_DIR/frames-pane-tail.txt" || true
ps -p "$hided_pid" -o pid,%cpu,rss,etime,command > "$MEASURE_RUN_DIR/driven-hided-frames.ps"
python3 "$measure_dir/summarize.py" frames "$MEASURE_RUN_DIR/frames.json" > "$MEASURE_RUN_DIR/frames-summary.json"
cat "$MEASURE_RUN_DIR/echo-summary.json" "$MEASURE_RUN_DIR/frames-summary.json"
note 'measurement complete; cleaning up owned processes'
