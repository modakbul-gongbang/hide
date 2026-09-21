#!/usr/bin/env bash
# Orchestrate S0 measurements against an isolated Herdr server.
# The operator app and its socket are never mutated. Cleanup is owned here.
set -euo pipefail
measure_dir="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=isolated-env.sh
source "$measure_dir/isolated-env.sh"
. "$S0_WORKTREE/scripts/toolchain-env.sh"

chrome_bin="/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"
hided_bin="$S0_SPIKE_ROOT/hided-spike/target/debug/hided-spike"
web_dir="$S0_SPIKE_ROOT/web-spike"
export S0_CDP_PORT="${S0_CDP_PORT:-9333}"
export HIDED_WS_URL="ws://127.0.0.1:${HIDED_SPIKE_PORT}/ws"
pids=()

note() { printf 's0: %s\n' "$*"; }

kill_owned() {
  local pid
  for pid in "${pids[@]:-}"; do
    if [[ -n "$pid" ]] && kill -0 "$pid" 2>/dev/null; then
      kill "$pid" 2>/dev/null || true
      wait "$pid" 2>/dev/null || true
    fi
  done
  bash "$measure_dir/stop-server.sh" >/dev/null 2>&1 || true
}

trap kill_owned EXIT

operator_before="$S0_RUN_DIR/operator-before.json"
bash "$measure_dir/operator-counts.sh" "$operator_before"

bash "$measure_dir/start-server.sh"
pane_id="$("$HERDR_BIN" api snapshot | python3 "$measure_dir/pane-id.py")"
export S0_PANE_ID="$pane_id"
note "isolated pane $pane_id"
"$HERDR_BIN" pane run "$pane_id" "cat" >/dev/null || true
sleep 0.4

if [[ ! -x "$hided_bin" ]]; then
  note "building hided-spike"
  cargo build --manifest-path "$S0_SPIKE_ROOT/hided-spike/Cargo.toml"
fi
"$hided_bin" >"$S0_RUN_DIR/logs/hided.log" 2>&1 &
pids+=($!)
hided_pid=$!
deadline=$((SECONDS + 20))
until curl -sf "http://127.0.0.1:${HIDED_SPIKE_PORT}/health" >/dev/null; do
  if (( SECONDS >= deadline )); then
    note "hided did not become healthy"; exit 1
  fi
  sleep 0.2
done

if [[ ! -d "$web_dir/node_modules" ]]; then
  note "pnpm install web-spike"
  (cd "$web_dir" && pnpm install --ignore-scripts)
fi
(cd "$web_dir" && pnpm dev) >"$S0_RUN_DIR/logs/vite.log" 2>&1 &
pids+=($!)
deadline=$((SECONDS + 30))
until curl -sf "http://127.0.0.1:5173/" >/dev/null; do
  if (( SECONDS >= deadline )); then
    note "vite did not start"; exit 1
  fi
  sleep 0.2
done

"$chrome_bin" \
  --user-data-dir="$S0_RUN_DIR/chrome-profile" \
  --remote-debugging-port="$S0_CDP_PORT" \
  --remote-debugging-address=127.0.0.1 \
  --no-first-run \
  --no-default-browser-check \
  --disable-sync \
  --disable-background-timer-throttling \
  --disable-renderer-backgrounding \
  "http://127.0.0.1:5173/?mode=live" \
  >"$S0_RUN_DIR/logs/chrome.log" 2>&1 &
pids+=($!)
deadline=$((SECONDS + 40))
until curl -sf "http://127.0.0.1:${S0_CDP_PORT}/json/list" >/dev/null; do
  if (( SECONDS >= deadline )); then
    note "chrome CDP did not start"; exit 1
  fi
  sleep 0.3
done
sleep 2

ps -p "$hided_pid" -o pid,%cpu,rss,etime,command > "$S0_RUN_DIR/idle-hided.ps"
uptime > "$S0_RUN_DIR/idle-uptime.txt"

note "web echo trial 1"
S0_ECHO_REPEATS=50 node "$measure_dir/echo-web.mjs" > "$S0_RUN_DIR/echo-web-1.json"
note "web echo trial 2"
S0_ECHO_REPEATS=50 node "$measure_dir/echo-web.mjs" > "$S0_RUN_DIR/echo-web-2.json"
note "web echo trial 3"
S0_ECHO_REPEATS=50 node "$measure_dir/echo-web.mjs" > "$S0_RUN_DIR/echo-web-3.json"

note "capturing 120s driven deltas"
"$HERDR_BIN" pane send-keys "$pane_id" C-c >/dev/null || true
sleep 0.3
S0_CAPTURE_MS=120000 node "$measure_dir/capture.mjs" "$S0_RUN_DIR/capture.raw.jsonl" &
capture_pid=$!
pids+=($capture_pid)
"$HERDR_BIN" pane run "$pane_id" '/usr/bin/python3 -c "import time,sys
for i in range(15000):
    print(f\"s0 {i:05d}\")
    sys.stdout.flush()
    time.sleep(0.008)"' >/dev/null || true
wait "$capture_pid"
python3 "$measure_dir/redact.py" < "$S0_RUN_DIR/capture.raw.jsonl" > "$S0_RUN_DIR/capture.jsonl"
python3 "$measure_dir/snapshot-stats.py" "$S0_RUN_DIR/capture.jsonl" > "$S0_RUN_DIR/snapshot-stats.json"
ps -p "$hided_pid" -o pid,%cpu,rss,etime,command > "$S0_RUN_DIR/driven-hided.ps"

note "replay + frames + chrome RSS"
node "$measure_dir/chrome-navigate.mjs" "http://127.0.0.1:5173/?mode=replay"
sleep 2
S0_TRACE_MS=125000 node "$measure_dir/chrome-trace.mjs" "$S0_RUN_DIR/chrome-trace.json" &
trace_pid=$!
deadline=$((SECONDS + 140))
while (( SECONDS < deadline )); do
  done_flag="$(node "$measure_dir/chrome-eval.mjs" 'window.__s0ReplayDone === true' 2>/dev/null || true)"
  if [[ "$done_flag" == "true" ]]; then
    break
  fi
  sleep 2
done
wait "$trace_pid" || true
node "$measure_dir/chrome-eval.mjs" 'window.__s0Frames' > "$S0_RUN_DIR/frames.json"
python3 "$measure_dir/frames.py" "$S0_RUN_DIR/frames.json" > "$S0_RUN_DIR/frames-summary.json"

chrome_pids="$(pgrep -f "$S0_RUN_DIR/chrome-profile" || true)"
rss_args=("hided=$hided_pid")
idx=0
for cpid in $chrome_pids; do
  rss_args+=("chrome$idx=$cpid")
  idx=$((idx + 1))
done
python3 "$measure_dir/rss.py" "${rss_args[@]}" > "$S0_RUN_DIR/rss-driven.json"

bash "$measure_dir/operator-counts.sh" "$S0_RUN_DIR/operator-after.json"
python3 "$measure_dir/write-report.py" | tee "$S0_RUN_DIR/logs/report-write.txt"

note "owned processes will be stopped by EXIT trap"
