#!/usr/bin/env bash
# One owner for every isolated fixture, shell, browser and measurement process.
set -euo pipefail
measure_dir="$(cd "$(dirname "$0")" && pwd)"
source "$measure_dir/isolated-env.sh"
. "$S0_WORKTREE/scripts/toolchain-env.sh"
export S0_OWNER_PID=$$
export HIDED_WS_URL="ws://127.0.0.1:${HIDED_SPIKE_PORT}/ws"
chrome_bin="/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"
hided_bin="$S0_SPIKE_ROOT/hided-spike/target/debug/hided-spike"
app="$S0_WORKTREE/macos/build/assembled/hide-$(basename "$S0_WORKTREE").app"
web_dir="$S0_SPIKE_ROOT/web-spike"
pids=()
server_started=false
note() { printf 's0: %s\n' "$*"; }
cleanup() {
  trap - EXIT INT TERM
  set +e
  # End all clients concurrently, stop the server, then reap bounded owners.
  # Waiting for hided first can deadlock while its attach child waits for Herdr.
  for pid in "${pids[@]:-}"; do [[ -n "$pid" ]] && kill "$pid" 2>/dev/null; done
  if $server_started; then bash "$measure_dir/stop-server.sh"; fi
  for pid in "${pids[@]:-}"; do [[ -n "$pid" ]] && wait "$pid" 2>/dev/null; done
  if $server_started && [[ -f "$S0_PRIVATE/herdr-owner.pid" ]]; then kill "$(cat "$S0_PRIVATE/herdr-owner.pid")" 2>/dev/null; fi
  ps -axo pid,ppid,command > "$S0_RUN_DIR/cleanup-processes.txt"
  bash "$measure_dir/operator-counts.sh" "$S0_RUN_DIR/operator-after.json"
}
trap cleanup EXIT
trap 'exit 130' INT TERM
spawn_owned() {
  local name=$1; shift
  (( ${#pids[@]} < 16 )) || { echo 'process owner budget exceeded' >&2; exit 1; }
  python3 "$measure_dir/owned.py" "$$" "$S0_PRIVATE/$name.pid" "$@" >"$S0_RUN_DIR/logs/$name.log" 2>&1 &
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
reset_fixture() {
  "$HERDR_BIN" pane send-keys "$S0_PANE_ID" C-c >/dev/null
  sleep 0.2
  "$HERDR_BIN" pane run "$S0_PANE_ID" 'printf "\033c"; stty -echo -icanon; cat' >/dev/null
}
wait_js() {
  local deadline=$((SECONDS+30))
  until [[ "$(node "$measure_dir/chrome-eval.mjs" "$1")" == true ]]; do
    (( SECONDS < deadline )) || { echo "browser unready: $1" >&2; exit 1; }
    sleep 0.2
  done
}
forget_owned() {
  local remaining=() pid
  for pid in "${pids[@]}"; do [[ "$pid" == "$1" ]] || remaining+=("$pid"); done
  pids=("${remaining[@]}")
}
stop_owned() { kill "$1"; wait "$1" || true; forget_owned "$1"; }

[[ -x "$app/Contents/MacOS/HerdrMacOS" && -x "$hided_bin" ]] || { echo 'build dev bundle and hided first' >&2; exit 1; }
# Never adopt an unrelated server on a measurement port.
python3 - "$HIDED_SPIKE_PORT" "$S0_CDP_PORT" 5173 <<'PY'
import socket,sys
for port in sys.argv[1:]:
    with socket.socket() as s:
        s.setsockopt(socket.SOL_SOCKET,socket.SO_REUSEADDR,1)
        s.bind(('127.0.0.1',int(port)))
PY
bash "$measure_dir/operator-counts.sh" "$S0_RUN_DIR/operator-before.json"
zsh "$S0_WORKTREE/scripts/check-herdr-contract.sh" --herdr-bin "$HERDR_BIN" --schema-only > "$S0_RUN_DIR/herdr-contract.json"
bash "$measure_dir/start-server.sh"
server_started=true
export S0_PANE_ID="$("$HERDR_BIN" api snapshot | python3 "$measure_dir/pane-id.py")"
"$HERDR_BIN" pane run "$S0_PANE_ID" 'stty -echo -icanon; cat' >/dev/null
sleep 1
spawn_owned vite node "$web_dir/node_modules/vite/bin/vite.js" "$web_dir" --host 127.0.0.1
wait_url http://127.0.0.1:5173/
spawn_owned chrome "$chrome_bin" --user-data-dir="$S0_RUN_DIR/chrome-profile" --remote-debugging-port="$S0_CDP_PORT" --remote-debugging-address=127.0.0.1 --no-first-run --no-default-browser-check --disable-sync --disable-background-networking --disable-component-update --disable-background-timer-throttling --disable-renderer-backgrounding --disable-backgrounding-occluded-windows about:blank
wait_url "http://127.0.0.1:$S0_CDP_PORT/json/list"
# The navigation helper identifies the spike by its URL, so create its first target explicitly.
curl -sf -X PUT "http://127.0.0.1:$S0_CDP_PORT/json/new?http://127.0.0.1:5173/?mode=ime" > "$S0_RUN_DIR/chrome-target.json"
python3 - "$S0_CDP_PORT" <<'PY'
import json,sys,urllib.request
base="http://127.0.0.1:"+sys.argv[1]
for tab in json.load(urllib.request.urlopen(base+"/json/list")):
    if tab["type"]=="page" and tab["url"]=="about:blank": urllib.request.urlopen(base+"/json/close/"+tab["id"]).read()
PY

for trial in 1 2 3; do
  reset_fixture
  note "web trial $trial (50 samples)"
  spawn_owned "hided-$trial" env HOME="$S0_PRIVATE/home" "$hided_bin"
  hided_owner=$owned_pid
  wait_url "http://127.0.0.1:$HIDED_SPIKE_PORT/health"
  node "$measure_dir/chrome-navigate.mjs" 'http://127.0.0.1:5173/?mode=live&cols=84&rows=46'
  wait_js 'window.__s0PaneId !== null && typeof window.__s0Arm === "function"'
  sleep 2
  ps -p "$(cat "$S0_PRIVATE/hided-$trial.pid")" -o pid,%cpu,rss,etime,command > "$S0_RUN_DIR/idle-hided-$trial.ps"
  S0_ECHO_REPEATS=50 node "$measure_dir/echo-web.mjs" > "$S0_RUN_DIR/echo-web-$trial.json"
  ps -p "$(cat "$S0_PRIVATE/hided-$trial.pid")" -o pid,%cpu,rss,etime,command > "$S0_RUN_DIR/driven-hided-$trial.ps"
  node "$measure_dir/chrome-navigate.mjs" 'http://127.0.0.1:5173/?mode=ime'
  stop_owned "$hided_owner"
  reset_fixture
  note "Swift trial $trial (50 samples)"
  spawn_owned "swift-$trial" env HOME="$S0_PRIVATE/home" "$app/Contents/MacOS/HerdrMacOS" --state-path "$S0_PRIVATE/swift-state.json" --workspace-root "$S0_FIXTURE" --verification-background
  swift_owner=$owned_pid
  sleep 8
  export S0_SWIFT_PID="$(cat "$S0_PRIVATE/swift-$trial.pid")"
  ps -p "$S0_SWIFT_PID" -o pid,%cpu,rss,etime,command > "$S0_RUN_DIR/idle-swift-$trial.ps"
  /opt/homebrew/bin/peekaboo window list --pid "$S0_SWIFT_PID" --no-remote --json > "$S0_RUN_DIR/swift-windows-$trial.json"
  python3 - "$S0_RUN_DIR/swift-windows-$trial.json" "$S0_RUN_DIR/swift-$trial.png" <<'PY2'
import json,subprocess,sys
raw=json.load(open(sys.argv[1]))
if not raw.get('success'): raise SystemExit(raw)
windows=[w for w in raw['data']['windows'] if w.get('subrole') == 'AXStandardWindow' and w.get('window_title') == 'hide']
if len(windows)!=1: raise SystemExit(f'expected one candidate window: {windows}')
wid=windows[0]['window_id']
subprocess.run(['/usr/sbin/screencapture','-x','-l',str(wid),sys.argv[2]],check=True)
PY2
  S0_ECHO_REPEATS=50 python3 "$measure_dir/echo-swift.py" > "$S0_RUN_DIR/echo-swift-$trial.json"
  python3 - "$S0_RUN_DIR/swift-windows-$trial.json" "$S0_RUN_DIR/swift-echo-$trial.png" <<'PY2'
import json,subprocess,sys
windows=json.load(open(sys.argv[1]))['data']['windows']
wid=next(w['window_id'] for w in windows if w.get('subrole')=='AXStandardWindow' and w.get('window_title')=='hide')
subprocess.run(['/usr/sbin/screencapture','-x','-l',str(wid),sys.argv[2]],check=True)
PY2
  ps -p "$S0_SWIFT_PID" -o pid,%cpu,rss,etime,command > "$S0_RUN_DIR/driven-swift-$trial.ps"
  stop_owned "$swift_owner"
done
note 'capturing 120 seconds of driven deltas'
spawn_owned hided env HOME="$S0_PRIVATE/home" "$hided_bin"
wait_url "http://127.0.0.1:$HIDED_SPIKE_PORT/health"
node "$measure_dir/chrome-navigate.mjs" 'http://127.0.0.1:5173/?mode=live&cols=84&rows=46'
wait_js 'window.__s0PaneId !== null'
"$HERDR_BIN" pane send-keys "$S0_PANE_ID" C-c >/dev/null
sleep 0.5
spawn_owned capture node "$measure_dir/capture.mjs" "$S0_RUN_DIR/capture.raw.jsonl"
capture_owner=$owned_pid
"$HERDR_BIN" pane run "$S0_PANE_ID" '/usr/bin/python3 -c "import time; end=time.monotonic()+122; i=0
while time.monotonic()<end:
 print(f\"s0 {i:05d}\",flush=True); i+=1; time.sleep(0.008)"' >/dev/null
wait "$capture_owner"
forget_owned "$capture_owner"
python3 "$measure_dir/redact.py" < "$S0_RUN_DIR/capture.raw.jsonl" > "$S0_RUN_DIR/capture.jsonl"
python3 "$measure_dir/snapshot-stats.py" "$S0_RUN_DIR/capture.jsonl" > "$S0_RUN_DIR/snapshot-stats.json"
note 'replay: trace starts before the full 120-second rAF window'
node "$measure_dir/chrome-navigate.mjs" 'http://127.0.0.1:5173/?mode=replay&cols=84&rows=46'
wait_js 'window.__s0ReplayReady === true'
node "$measure_dir/chrome-trace.mjs" "$S0_RUN_DIR/chrome-trace.json"
node "$measure_dir/chrome-eval.mjs" '({frames:window.__s0Frames,window:window.__s0ReplayWindow,done:window.__s0ReplayDone})' > "$S0_RUN_DIR/frames.json"
python3 "$measure_dir/frames.py" "$S0_RUN_DIR/frames.json" > "$S0_RUN_DIR/frames-summary.json"
python3 "$measure_dir/rss.py" --trace "$S0_RUN_DIR/chrome-trace.json" "hided=$(cat "$S0_PRIVATE/hided.pid")" > "$S0_RUN_DIR/rss-tab.json"
bash "$measure_dir/operator-counts.sh" "$S0_RUN_DIR/operator-after.json"
python3 "$measure_dir/write-report.py"
note 'measurement complete; cleaning up owned processes'
