#!/usr/bin/env bash
# Owner of every isolated fixture, daemon, browser and driver for the web
# shell echo (PRD B8) and frame (PRD B12) measurements against the product
# hided. Usage:
#   HIDE_MEASURE_RUN_DIR=agents/runs/<slug>/measure bash scripts/web-shell-measure/run.sh
#   MEASURE_SCENARIO=multi HIDE_MEASURE_RUN_DIR=... bash scripts/web-shell-measure/run.sh
# `single` (default) is the S1 shape: one pane in one tab. `multi` is the S2
# shape (PRD web-shell-pivot-s2 D-08): the measured pane shares its tab with
# four splits, and four more tabs are shown once each so the core holds five
# attached tabs, before the shell returns to the measured tab. The driver and
# the marker still go to the one measured pane; the other panes are idle
# shells with mounted xterm instances.
# `topology` (PRD instant-pane-topology D-15) adds a second tab to the
# measured workspace and, with MEASURE_SCALE=operator or double, fills the
# server to that scale (scale.sh); it then records idle resources, one echo
# trial under the scale's agent churn, and the split, zoom, close, new tab
# and tab switch latencies (topology.mjs), and skips the frame window.
# `keys` (PRD core-host-node-terminal D-07, D-08, B2, B4, B5, B19, B21) is
# the measured pane and four splits in one tab, five attached panes: idle
# resources for MEASURE_IDLE_SECONDS (60), the screen key echo idle
# (key-echo.mjs), then every pane printing a line per 8 ms while resources
# are sampled for MEASURE_DRIVEN_SECONDS (120), then the key echo again on
# the measured pane while the other four keep printing,
# then MEASURE_KEY_COUNT (0: skipped) distinct keys counted back from the
# pane (key-count.mjs), then the window closed while the panes print for
# MEASURE_WINDOWLESS_SECONDS (60) and reopened.
# `device` (B3, B4) is the same key echo and key count on a device's pane:
# the shell registers the device with consent (device-front.mjs), brings
# its fixture workspace to the front, and types into the device's one
# pane. The device is an isolated account the caller set up: its SSH alias
# in MEASURE_DEVICE_SSH_CONFIG and its recorded host key in
# MEASURE_DEVICE_KNOWN_HOSTS (both copied into the private HOME), its Herdr
# socket in MEASURE_DEVICE_SOCKET, MEASURE_DEVICE_HERDR the command that
# runs its Herdr from here, and MEASURE_DEVICE_HELPER_ROOT and
# MEASURE_DEVICE_CLI_DIR the consent folders inside that account.
# MEASURE_HIDED_BIN measures another hided build, such as a baseline, with
# the same fixture.
# Needs: the pinned herdr (HERDR_BIN_PATH or PATH), Google Chrome,
# target/release/hided built after `pnpm --dir web build` (docs/BUILD.md: a release hided embeds web/dist).
set -euo pipefail
# Optional unattended mode: no native window and no operator socket read.
isolated_headless=false
memory_series=false
case "${1:-}" in
  '') ;;
  --isolated-headless) isolated_headless=true ;;
  *) echo 'usage: run.sh [--isolated-headless]' >&2; exit 2 ;;
esac
case "${2:-}" in
  '') ;;
  --memory-series) memory_series=true; export MEASURE_FRAME_WINDOW_MS=600000 ;;
  *) echo 'second argument must be --memory-series' >&2; exit 2 ;;
esac
measure_dir="$(cd "$(dirname "$0")" && pwd)"
source "$measure_dir/isolated-env.sh"
trap 'rmdir "$MEASURE_SOCKET_DIR"' EXIT
scenario="${MEASURE_SCENARIO:-single}"
case "$scenario" in single|multi|keys|device|areas2|areas3|topology) ;; *) echo "MEASURE_SCENARIO must be single, multi, keys, device, areas2, areas3 or topology" >&2; exit 2;; esac
scale="${MEASURE_SCALE:-none}"
case "$scale" in none|operator|double) ;; *) echo "MEASURE_SCALE must be none, operator or double" >&2; exit 2;; esac
[[ "$scale" == none || "$scenario" == topology ]] || { echo "MEASURE_SCALE needs MEASURE_SCENARIO=topology" >&2; exit 2; }
chrome_bin="${MEASURE_CHROME_BIN:-/Applications/Google Chrome.app/Contents/MacOS/Google Chrome}"
hided_bin="${MEASURE_HIDED_BIN:-$MEASURE_WORKTREE/target/release/hided}"
[[ -x "$hided_bin" ]] || { echo "build target/release/hided first: pnpm --dir web build, then the release build described in docs/BUILD.md" >&2; exit 1; }
[[ -x "$chrome_bin" ]] || { echo "Chrome not found at $chrome_bin" >&2; exit 1; }
pids=()
server_started=false
note() { printf 'measure: %s\n' "$*"; }
operator_counts() {
  if $isolated_headless; then
    printf '{"observed":false,"reason":"isolated-headless mode never reads the operator socket"}\n' > "$MEASURE_RUN_DIR/operator-$1.json"
  else
    python3 "$measure_dir/operator-counts.py" "$HERDR_BIN_PATH" "$MEASURE_OPERATOR_SOCKET" "$MEASURE_RUN_DIR/operator-$1.json"
  fi
}
cleanup() {
  trap - EXIT INT TERM
  set +e
  kill_owned() {
    local pid=$1
    [[ -n "$pid" ]] || return 0
    [[ "$(ps -p "$pid" -o ppid= | tr -d ' ')" == "$$" ]] && kill "$pid" 2>/dev/null
  }
  for pid in "${pids[@]:-}"; do kill_owned "$pid"; done
  if $server_started; then
    kill_owned "${server_pid:-}"
    [[ -n "${server_pid:-}" ]] && wait "$server_pid" 2>/dev/null
    rm -f "$HERDR_SOCKET_PATH" "$HERDR_SOCKET_PATH.agent" "${HERDR_SOCKET_PATH%.sock}-client.sock" "$HERDR_SOCKET_PATH.hide-label-generator.lock"
  fi
  for pid in "${pids[@]:-}"; do [[ -n "$pid" ]] && wait "$pid" 2>/dev/null; done
  rmdir "$MEASURE_SOCKET_DIR"
  {
    for pid in "${pids[@]:-}" "${server_pid:-}"; do
      [[ -n "$pid" ]] || continue
      ps -p "$pid" -o pid=,ppid=,comm= || printf '%s exited\n' "$pid"
    done
  } > "$MEASURE_RUN_DIR/cleanup-processes.txt"
  operator_counts after
}
trap cleanup EXIT
trap 'exit 130' INT TERM
spawn_owned() {
  local name=$1; shift
  (( ${#pids[@]} < 8 )) || { echo 'process owner budget exceeded' >&2; exit 1; }
  "$@" >"$MEASURE_RUN_DIR/logs/$name.log" 2>"$MEASURE_RUN_DIR/logs/$name.stderr.log" &
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
    if (( SECONDS >= deadline )); then
      node --input-type=module - "$measure_dir/cdp.mjs" "$MEASURE_CDP_PORT" <<'JS' >&2
const { connectPage } = await import(process.argv[2]);
const page = await connectPage(process.argv[3]);
try {
  console.log(JSON.stringify(await page.evaluate(`({
    screen: document.querySelector('main')?.getAttribute('aria-label'),
    mainView: document.querySelector('[data-main-screen]')?.getAttribute('data-main-view'),
    overviewState: document.querySelector('[data-overview-screen]')?.getAttribute('data-overview-state'),
    projectRows: document.querySelectorAll('[data-main-project]').length,
    workspaceRows: document.querySelectorAll('[data-overview-workspace]').length,
    sidebarMode: document.querySelector('[data-sidebar]')?.getAttribute('data-sidebar'),
    sidebarProjects: document.querySelectorAll('[data-project-row]').length,
    sidebarCheckouts: document.querySelectorAll('[data-checkout]').length,
    checkoutKinds: [...document.querySelectorAll('[data-checkout-kind]')].map((row) => row.getAttribute('data-checkout-kind')),
    workspaceVisible: Boolean(document.querySelector('[data-workspace-screen]'))
  })`)));
} finally {
  page.close();
}
JS
      echo "browser unready: $1" >&2
      exit 1
    fi
    sleep 0.3
  done
}
reset_fixture() {
  "$HERDR_BIN_PATH" pane send-keys "$MEASURE_PANE_ID" ctrl+c >/dev/null
  sleep 0.3
  "$HERDR_BIN_PATH" pane run "$MEASURE_PANE_ID" 'printf "\033c"; stty -echo -icanon; cat' >/dev/null
  sleep 0.5
}

{
  echo "worktree_head=$(git -C "$MEASURE_WORKTREE" rev-parse HEAD)"
  echo "worktree_dirty=$(git -C "$MEASURE_WORKTREE" status --porcelain | wc -l | tr -d ' ')"
  echo "hided_bin=$hided_bin"
  echo "hided_sha256=$(shasum -a 256 "$hided_bin" | awk '{print $1}')"
  echo "herdr_bin=$HERDR_BIN_PATH"
  echo "herdr_version=$("$HERDR_BIN_PATH" --version)"
  echo "chrome_version=$("$chrome_bin" --version 2>/dev/null)"
  echo "socket=$HERDR_SOCKET_PATH"
  echo "scenario=$scenario"
  echo "scale=$scale"
  echo "isolated_headless=$isolated_headless"
  echo "memory_series=$memory_series"
  echo "started_at=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  uptime
} > "$MEASURE_RUN_DIR/identity.txt"
operator_counts before

# Private server and one cat pane.
[[ -S "$HERDR_SOCKET_PATH" ]] && { echo "socket already exists: $HERDR_SOCKET_PATH" >&2; exit 2; }
if [[ ! -d "$MEASURE_FIXTURE/.git" ]]; then
  git -C "$MEASURE_FIXTURE" init --quiet
  printf 'measure fixture\n' > "$MEASURE_FIXTURE/README"
  git -C "$MEASURE_FIXTURE" add README
  git -C "$MEASURE_FIXTURE" -c commit.gpgsign=false -c user.email="measure@example.invalid" -c user.name="measure" commit --quiet -m "measure fixture"
fi
measure_checkout="$(cd "$MEASURE_RUN_DIR" && pwd)/checkout"
if [[ ! -f "$measure_checkout/.git" ]]; then
  git -C "$MEASURE_FIXTURE" worktree add --quiet -b measure-worktree "$measure_checkout"
fi
export MEASURE_FIXTURE="$measure_checkout"
env HOME="$MEASURE_PRIVATE/home" "$HERDR_BIN_PATH" server >"$MEASURE_RUN_DIR/logs/herdr-server.log" 2>&1 &
server_pid=$!
server_started=true
deadline=$((SECONDS+20)); until [[ -S "$HERDR_SOCKET_PATH" ]]; do (( SECONDS < deadline )) || { echo 'socket did not appear' >&2; exit 1; }; sleep 0.1; done
workspaces="$("$HERDR_BIN_PATH" api snapshot | python3 -c 'import json,sys; d=json.load(sys.stdin); r=d.get("result") or d; s=r.get("snapshot") or r; print(len(s.get("workspaces") or []))')"
[[ "$workspaces" == "0" ]] || { echo "private server already has $workspaces workspaces" >&2; exit 1; }
created="$("$HERDR_BIN_PATH" workspace create --cwd "$MEASURE_FIXTURE" --label measure --focus)"
export MEASURE_PANE_ID="$("$HERDR_BIN_PATH" api snapshot | python3 "$measure_dir/pane-id.py")"
measure_workspace="$(printf %s "$created" | python3 -c 'import json,sys; print(json.load(sys.stdin)["result"]["workspace"]["workspace_id"])')"
measure_tab="$(printf %s "$created" | python3 -c 'import json,sys; print(json.load(sys.stdin)["result"]["tab"]["tab_id"])')"
# The current checkout-owner contract requires an explicit owner before
# Hide can reuse this fixture's existing workspace and cat pane.
measure_owner="$(python3 -c 'import hashlib,os; print(hashlib.sha256(("local\0"+os.path.realpath(os.environ["MEASURE_FIXTURE"])).encode()).hexdigest()[:32])')"
"$HERDR_BIN_PATH" workspace report-metadata "$measure_workspace" --source performance-fixture --token "hide_owner=$measure_owner" --token purpose="Isolated performance fixture" >/dev/null
wait_prompt() {
  # The pane shell has printed its fixed prompt before it takes a command.
  local deadline=$((SECONDS+20))
  until "$HERDR_BIN_PATH" pane read "$1" --source visible --format text 2>/dev/null | grep -q 'fixture %'; do
    (( SECONDS < deadline )) || { echo "pane $1 prompt did not appear" >&2; exit 1; }
    sleep 0.1
  done
}
wait_prompt "$MEASURE_PANE_ID"
"$HERDR_BIN_PATH" pane run "$MEASURE_PANE_ID" 'stty -echo -icanon; cat' >/dev/null
extra_panes=()
extra_tabs=()
if [[ "$scenario" == multi || "$scenario" == keys ]]; then
  # Four splits beside the measured pane, alternating direction so the tree
  # nests, and (multi) four more tabs; none of it takes focus from the
  # measured pane.
  split_from="$MEASURE_PANE_ID"
  for direction in right down right down; do
    split_pane="$("$HERDR_BIN_PATH" pane split "$split_from" --direction "$direction" --no-focus | python3 -c 'import json,sys; print(json.load(sys.stdin)["result"]["pane"]["pane_id"])')"
    extra_panes+=("$split_pane")
    split_from="$split_pane"
  done
fi
if [[ "$scenario" == multi ]]; then
  for label in two three four five; do
    extra_tab="$("$HERDR_BIN_PATH" tab create --workspace "$measure_workspace" --cwd "$MEASURE_FIXTURE" --label "$label" --no-focus | python3 -c 'import json,sys; print(json.load(sys.stdin)["result"]["tab"]["tab_id"])')"
    extra_tabs+=("$extra_tab")
  done
  for pane in "${extra_panes[@]}"; do wait_prompt "$pane"; done
fi

if [[ "$scenario" == areas* ]]; then
  for ((area=1; area<${scenario#areas}; area++)); do
    created="$("$HERDR_BIN_PATH" tab create --workspace "$measure_workspace" --cwd "$MEASURE_FIXTURE" --label "area-$area" --no-focus)"
    extra_tabs+=("$(printf %s "$created" | python3 -c 'import json,sys; print(json.load(sys.stdin)["result"]["tab"]["tab_id"])')")
    extra_panes+=("$(printf %s "$created" | python3 -c 'import json,sys; print(json.load(sys.stdin)["result"]["root_pane"]["pane_id"])')")
  done
  for pane in "${extra_panes[@]}"; do wait_prompt "$pane"; done
fi

if [[ "$scenario" == topology ]]; then
  created="$("$HERDR_BIN_PATH" tab create --workspace "$measure_workspace" --cwd "$MEASURE_FIXTURE" --label switch --no-focus)"
  export MEASURE_SWITCH_TAB="$(printf %s "$created" | python3 -c 'import json,sys; print(json.load(sys.stdin)["result"]["tab"]["tab_id"])')"
  wait_prompt "$(printf %s "$created" | python3 -c 'import json,sys; print(json.load(sys.stdin)["result"]["root_pane"]["pane_id"])')"
  if [[ "$scale" != none ]]; then
    note "filling the private server to the $scale scale"
    bash "$measure_dir/scale.sh" build "$scale"
  fi
fi

hided_env=()
if [[ "$scenario" == device ]]; then
  for name in MEASURE_DEVICE_ID MEASURE_DEVICE_ALIAS MEASURE_DEVICE_SSH_CONFIG MEASURE_DEVICE_SOCKET MEASURE_DEVICE_HERDR MEASURE_DEVICE_HELPER_ROOT MEASURE_DEVICE_CLI_DIR; do
    [[ -n "${!name:-}" ]] || { echo "the device scenario needs $name" >&2; exit 2; }
  done
  mkdir -p "$MEASURE_PRIVATE/home/.ssh"
  cp "$MEASURE_DEVICE_SSH_CONFIG" "$MEASURE_PRIVATE/home/.ssh/config"
  chmod 600 "$MEASURE_PRIVATE/home/.ssh/config"
  # Hide checks the device's host key against ~/.ssh/known_hosts only.
  [[ -z "${MEASURE_DEVICE_KNOWN_HOSTS:-}" ]] || cp "$MEASURE_DEVICE_KNOWN_HOSTS" "$MEASURE_PRIVATE/home/.ssh/known_hosts"
  hided_env=(HIDE_HOST_HELPER_ROOT="$MEASURE_DEVICE_HELPER_ROOT" HIDE_HOST_CLI_DIR="$MEASURE_DEVICE_CLI_DIR")
  device_herdr() { eval "$MEASURE_DEVICE_HERDR" '"$@"'; }
  device_pane="$(device_herdr api snapshot | python3 "$measure_dir/pane-id.py")"
  device_herdr pane send-keys "$device_pane" ctrl+c >/dev/null
  sleep 0.3
  device_herdr pane run "$device_pane" "printf '\033c'; stty -echo -icanon; cat" >/dev/null
  echo "device_pane=$device_pane" >> "$MEASURE_RUN_DIR/identity.txt"
fi

# Product hided (release, embedded web/dist) on the private socket.
spawn_owned hided env HOME="$MEASURE_PRIVATE/home" HIDE_STATE_DIR="$MEASURE_PRIVATE/hide-state" HIDE_KEEP_ALIVE=1 HIDE_PORT=0 ${hided_env[@]+"${hided_env[@]}"} "$hided_bin"
hided_pid=$owned_pid
deadline=$((SECONDS+20)); until [[ -f "$MEASURE_PRIVATE/hide-state/hided.json" ]]; do (( SECONDS < deadline )) || { echo 'hided state file missing' >&2; exit 1; }; sleep 0.1; done
hided_port="$(python3 -c "import json;print(json.load(open('$MEASURE_PRIVATE/hide-state/hided.json'))['port'])")"
hided_token="$(python3 -c "import json;print(json.load(open('$MEASURE_PRIVATE/hide-state/hided.json'))['token'])")"
wait_url "http://127.0.0.1:$hided_port/health"
page_url="http://127.0.0.1:$hided_port/?probe=1#token=$hided_token"

port_file="$MEASURE_RUN_DIR/chrome-profile/DevToolsActivePort"
[[ ! -e "$port_file" ]] || { echo 'stale CDP port file; use a new run directory' >&2; exit 2; }
chrome_flags=()
if $isolated_headless; then chrome_flags+=(--headless=new); fi
spawn_owned chrome "$chrome_bin" "${chrome_flags[@]}" --user-data-dir="$MEASURE_RUN_DIR/chrome-profile" --remote-debugging-port=0 --remote-debugging-address=127.0.0.1 --no-first-run --no-default-browser-check --disable-sync --disable-background-networking --disable-component-update --disable-background-timer-throttling --disable-renderer-backgrounding --disable-backgrounding-occluded-windows --window-size=1280,900 about:blank
chrome_pid=$owned_pid
deadline=$((SECONDS+20)); until [[ -s "$port_file" ]]; do
  [[ "$(ps -p "$chrome_pid" -o ppid= | tr -d ' ')" == "$$" ]] || { echo 'owned Chrome exited before CDP became ready' >&2; exit 1; }
  (( SECONDS < deadline )) || { echo 'owned Chrome CDP port did not appear' >&2; exit 1; }
  sleep 0.1
done
export MEASURE_CDP_PORT="$(head -n 1 "$port_file")"
[[ "$MEASURE_CDP_PORT" =~ ^[0-9]+$ ]] || { echo 'owned Chrome CDP port is invalid' >&2; exit 1; }
wait_url "http://127.0.0.1:$MEASURE_CDP_PORT/json/list"
printf '%s' "$page_url" | node "$measure_dir/navigate.mjs" "$MEASURE_CDP_PORT"
# A first run opens on Main (PRD S6 D-11). Open the one fixture checkout
# through the Projects sidebar once its checkout row appears.
wait_js "(() => { if (document.querySelector('[data-workspace-screen]')) return true; const projects = document.querySelector('[data-sidebar-mode=\"projects\"]'); if (projects?.getAttribute('aria-selected') !== 'true') { projects?.click(); return false; } const checkout = document.querySelector('[data-checkout-kind=\"branch\"]:not([disabled])'); if (checkout) { if (!window.__measureCheckoutOpened) { window.__measureCheckoutOpened = true; checkout.click(); } return false; } document.querySelector('[data-project-toggle][aria-expanded=\"false\"]')?.click(); return false; })()"
wait_js "window.__hideProbe?.paneId() === '$MEASURE_PANE_ID'"
sleep 2
if [[ "$scenario" == areas* ]]; then
  for tab in "${extra_tabs[@]}"; do
    node "$measure_dir/split-agent.mjs" "$MEASURE_CDP_PORT" "$tab"
  done
  wait_js "(() => { document.querySelector('[data-agent-tab-bar] [data-tab=\"$measure_tab\"]')?.click(); return document.querySelectorAll('[data-agent-area-id]').length === ${scenario#areas} && window.__hideProbe.paneId() === '$MEASURE_PANE_ID'; })()"
fi
if [[ "$scenario" == multi ]]; then
  # Show each extra tab once so the core attaches it, then return.
  for tab in "${extra_tabs[@]}" "$measure_tab"; do
    wait_js "(() => { const el = document.querySelector('[data-tab=\"$tab\"]'); if (!el) return false; if (document.querySelector('[data-canvas]')?.dataset.canvas !== '$tab') el.click(); return true; })()"
    wait_js "document.querySelector('[data-canvas]')?.dataset.canvas === '$tab' && document.querySelectorAll('[data-pane-view]').length > 0"
    sleep 1
  done
  wait_js "document.querySelectorAll('[data-pane-view]').length === 5 && window.__hideProbe.paneId() === '$MEASURE_PANE_ID'"
  wait_js "window.__hideProbe.attachedPanes().length >= 9"
  sleep 2
fi
if [[ "$scenario" == keys ]]; then
  for pane in "${extra_panes[@]}"; do wait_prompt "$pane"; done
  wait_js "document.querySelectorAll('[data-pane-view]').length === 5 && window.__hideProbe.paneId() === '$MEASURE_PANE_ID'"
  wait_js "window.__hideProbe.attachedPanes().length >= 5"
  sleep 2
fi
node -e "
import('$measure_dir/cdp.mjs').then(async ({connectPage}) => { const p = await connectPage('$MEASURE_CDP_PORT'); console.log(JSON.stringify(await p.evaluate('({scenario: \"$scenario\", pane: window.__hideProbe.paneId(), pane_views: document.querySelectorAll(\"[data-pane-view]\").length, splits: document.querySelectorAll(\"[data-split]\").length, tabs: document.querySelectorAll(\"[role=tab]\").length, attached_panes: window.__hideProbe.attachedPanes(), live_terminals: window.__hideProbe.liveTerminals(), ua: navigator.userAgent, dpr: devicePixelRatio, inner: [innerWidth, innerHeight]})'))); p.close(); })" > "$MEASURE_RUN_DIR/page.json"
cat "$MEASURE_RUN_DIR/page.json"
# Resident memory with every attached pane's terminal alive (D-05): sampled
# before the echo trials and again after the driven window.
python3 "$measure_dir/memory.py" settled "$chrome_pid" "$hided_pid" "$MEASURE_CDP_PORT" > "$MEASURE_RUN_DIR/memory-settled.json"
cat "$MEASURE_RUN_DIR/memory-settled.json"
python3 "$measure_dir/resources.py" "$hided_pid" "$server_pid" "$chrome_pid" > "$MEASURE_RUN_DIR/resources-idle.json"

if [[ "$scenario" == topology ]]; then
  if [[ "$scale" != none ]]; then
    spawn_owned churn bash "$measure_dir/scale.sh" churn
  fi
  reset_fixture
  note "echo under the scale's agent churn (50 samples)"
  MEASURE_ECHO_REPEATS=50 node "$measure_dir/echo.mjs" > "$MEASURE_RUN_DIR/echo-1.json"
  python3 "$measure_dir/summarize.py" echo "$MEASURE_RUN_DIR/echo-1.json" > "$MEASURE_RUN_DIR/echo-summary.json"
  note "topology: ${MEASURE_TOPOLOGY_ROUNDS:-20} rounds of split, zoom, unzoom, close, new tab and tab switch"
  node "$measure_dir/topology.mjs" > "$MEASURE_RUN_DIR/topology.json"
  python3 "$measure_dir/summarize.py" topology "$MEASURE_RUN_DIR/topology.json" > "$MEASURE_RUN_DIR/topology-summary.json"
  # Only a build that records stage times writes these lines.
  python3 "$measure_dir/summarize.py" timing "$MEASURE_PRIVATE/hide-state/Logs/core.jsonl" > "$MEASURE_RUN_DIR/timing-summary.json"
  python3 "$measure_dir/memory.py" after-topology "$chrome_pid" "$hided_pid" "$MEASURE_CDP_PORT" > "$MEASURE_RUN_DIR/memory-after-topology.json"
  cat "$MEASURE_RUN_DIR/echo-summary.json" "$MEASURE_RUN_DIR/topology-summary.json"
  note 'measurement complete; cleaning up owned processes'
  exit 0
fi

if [[ "$scenario" == device ]]; then
  export MEASURE_HIDED_PORT="$hided_port" MEASURE_HIDED_TOKEN="$hided_token"
  note "device: registering $MEASURE_DEVICE_ID and bringing it to the front"
  node "$measure_dir/device-front.mjs" register
  node "$measure_dir/device-front.mjs" front
  scoped_pane="remote:$MEASURE_DEVICE_ID:pane:$device_pane"
  wait_js "window.__hideProbe?.paneId() === '$scoped_pane' && window.__hideProbe.attachedPanes().includes('$scoped_pane')"
  sleep 2
  note "device: screen key echo on $scoped_pane (50 samples)"
  MEASURE_PANE_ID="$scoped_pane" MEASURE_ECHO_REPEATS=50 node "$measure_dir/key-echo.mjs" > "$MEASURE_RUN_DIR/key-echo-device.json"
  python3 "$measure_dir/summarize.py" echo "$MEASURE_RUN_DIR/key-echo-device.json" > "$MEASURE_RUN_DIR/key-echo-device-summary.json"
  cat "$MEASURE_RUN_DIR/key-echo-device-summary.json"
  if (( ${MEASURE_KEY_COUNT:-0} > 0 )); then
    device_herdr pane send-keys "$device_pane" ctrl+c >/dev/null
    sleep 0.3
    device_herdr pane run "$device_pane" "printf '\033c'; stty -echo -icanon; cat" >/dev/null
    sleep 0.5
    note "device: $MEASURE_KEY_COUNT distinct keys counted back from the device's pane"
    MEASURE_PANE_ID="$device_pane" MEASURE_SCREEN_PANE_ID="$scoped_pane" \
      MEASURE_PANE_READ="$MEASURE_DEVICE_HERDR pane read '$device_pane' --source recent-unwrapped --lines 4000" \
      node "$measure_dir/key-count.mjs" > "$MEASURE_RUN_DIR/key-count-device.json"
    cat "$MEASURE_RUN_DIR/key-count-device.json"
  fi
  device_herdr pane send-keys "$device_pane" ctrl+c >/dev/null
  note 'measurement complete; cleaning up owned processes'
  exit 0
fi

if [[ "$scenario" == keys ]]; then
  all_panes=("$MEASURE_PANE_ID" "${extra_panes[@]}")
  # One line per 8 ms for the given seconds, beside the cat that echoes keys.
  printf '%s\n' 'import sys, time' 'end = time.monotonic() + float(sys.argv[1]); i = 0' \
    'while time.monotonic() < end:' "    print(f'drive {i:05d}', flush=True); i += 1; time.sleep(0.008)" > "$MEASURE_RUN_DIR/drive.py"
  # A pane's printer runs in the background of its shell, where Ctrl+C does
  # not reach it; it carries its pane id so only that one is ended.
  reset_pane() {
    pkill -f "$MEASURE_RUN_DIR/drive\.py [0-9]+ $1\$" || true
    "$HERDR_BIN_PATH" pane send-keys "$1" ctrl+c >/dev/null
    sleep 0.3
    "$HERDR_BIN_PATH" pane run "$1" "printf '\033c'; stty -echo -icanon; ${2:+/usr/bin/python3 $MEASURE_RUN_DIR/drive.py $2 $1 & }cat" >/dev/null
  }
  drive_all() { for pane in "${all_panes[@]}"; do reset_pane "$pane" "$1"; done; sleep 1; }
  quiet_all() { for pane in "${all_panes[@]}"; do reset_pane "$pane"; done; sleep 0.5; }
  idle_seconds="${MEASURE_IDLE_SECONDS:-60}"
  driven_seconds="${MEASURE_DRIVEN_SECONDS:-120}"
  windowless_seconds="${MEASURE_WINDOWLESS_SECONDS:-60}"
  quiet_all
  note "keys: idle resources, $idle_seconds s, five attached panes"
  MEASURE_RESOURCE_SECONDS="$idle_seconds" python3 "$measure_dir/resources.py" "$hided_pid" "$server_pid" "$chrome_pid" > "$MEASURE_RUN_DIR/resources-idle.json"
  note "keys: screen key echo, idle (50 samples)"
  MEASURE_ECHO_REPEATS=50 node "$measure_dir/key-echo.mjs" > "$MEASURE_RUN_DIR/key-echo-idle.json"
  note "keys: driven, $driven_seconds s at one line per 8 ms per pane"
  drive_all $((driven_seconds + 120))
  export MEASURE_RESOURCE_SECONDS="$driven_seconds"
  spawn_owned resources-driven python3 "$measure_dir/resources.py" "$hided_pid" "$server_pid" "$chrome_pid"
  resource_pid=$owned_pid
  unset MEASURE_RESOURCE_SECONDS
  python3 "$measure_dir/diag-counts.py" "$MEASURE_PRIVATE/hide-state/Logs/core.jsonl" > "$MEASURE_RUN_DIR/driven-diagnostics-before.json"
  MEASURE_WS_COUNT_SECONDS="$driven_seconds" spawn_owned ws-driven node "$measure_dir/ws-count.mjs"
  ws_pid=$owned_pid
  wait "$resource_pid" "$ws_pid"
  cp "$MEASURE_RUN_DIR/logs/resources-driven.log" "$MEASURE_RUN_DIR/resources-driven.json"
  cp "$MEASURE_RUN_DIR/logs/ws-driven.log" "$MEASURE_RUN_DIR/ws-driven.json"
  python3 "$measure_dir/diag-counts.py" "$MEASURE_PRIVATE/hide-state/Logs/core.jsonl" > "$MEASURE_RUN_DIR/driven-diagnostics-after.json"
  # The echoed marker has to arrive whole, so the measured pane goes back
  # to a plain cat while the other four keep printing.
  reset_pane "$MEASURE_PANE_ID"
  sleep 0.5
  note "keys: screen key echo, driven (50 samples, the other four panes printing)"
  MEASURE_ECHO_REPEATS=50 node "$measure_dir/key-echo.mjs" > "$MEASURE_RUN_DIR/key-echo-driven.json"
  quiet_all
  if (( ${MEASURE_KEY_COUNT:-0} > 0 )); then
    note "keys: $MEASURE_KEY_COUNT distinct keys counted back from the pane"
    node "$measure_dir/key-count.mjs" > "$MEASURE_RUN_DIR/key-count.json"
    cat "$MEASURE_RUN_DIR/key-count.json"
    quiet_all
  fi
  note "keys: windowless, $windowless_seconds s with every pane printing"
  MEASURE_WS_COUNT_SECONDS=$((windowless_seconds + 30)) spawn_owned ws-reopen node "$measure_dir/ws-count.mjs"
  ws_pid=$owned_pid
  sleep 1
  printf 'about:blank' | node "$measure_dir/navigate.mjs" "$MEASURE_CDP_PORT"
  sleep 2
  drive_all $((windowless_seconds + 10))
  MEASURE_RESOURCE_SECONDS="$windowless_seconds" python3 "$measure_dir/resources.py" "$hided_pid" "$server_pid" 0 > "$MEASURE_RUN_DIR/resources-windowless.json"
  printf '%s' "$page_url" | node "$measure_dir/navigate.mjs" "$MEASURE_CDP_PORT"
  wait_js "window.__hideProbe?.paneId() === '$MEASURE_PANE_ID' && document.querySelectorAll('[data-pane-view]').length === 5"
  sleep 5
  node -e "
import('$measure_dir/cdp.mjs').then(async ({connectPage}) => { const p = await connectPage('$MEASURE_CDP_PORT'); const panes = JSON.parse(process.argv[1]); console.log(JSON.stringify(await p.evaluate('(' + JSON.stringify(panes) + ').map((id) => ({pane: id, text_chars: window.__hideProbe.paneText(id).trim().length}))'))); p.close(); })" "$(printf '%s\n' "${all_panes[@]}" | python3 -c 'import json,sys; print(json.dumps(sys.stdin.read().split()))')" > "$MEASURE_RUN_DIR/reopen-panes.json"
  python3 "$measure_dir/diag-counts.py" "$MEASURE_PRIVATE/hide-state/Logs/core.jsonl" > "$MEASURE_RUN_DIR/reopen-diagnostics.json"
  wait "$ws_pid"
  cp "$MEASURE_RUN_DIR/logs/ws-reopen.log" "$MEASURE_RUN_DIR/ws-reopen.json"
  cat "$MEASURE_RUN_DIR/reopen-panes.json" "$MEASURE_RUN_DIR/reopen-diagnostics.json" "$MEASURE_RUN_DIR/ws-driven.json" "$MEASURE_RUN_DIR/ws-reopen.json"
  for name in idle driven; do
    python3 "$measure_dir/summarize.py" echo "$MEASURE_RUN_DIR/key-echo-$name.json" > "$MEASURE_RUN_DIR/key-echo-$name-summary.json"
  done
  for name in idle driven windowless; do
    python3 "$measure_dir/summarize.py" resources "$MEASURE_RUN_DIR/resources-$name.json" > "$MEASURE_RUN_DIR/resources-$name-summary.json"
  done
  cat "$MEASURE_RUN_DIR"/key-echo-*-summary.json "$MEASURE_RUN_DIR"/resources-*-summary.json
  note 'measurement complete; cleaning up owned processes'
  exit 0
fi

for trial in 1 2 3; do
  reset_fixture
  note "echo trial $trial (50 samples)"
  ps -p "$hided_pid" -o pid,%cpu,rss,etime,command > "$MEASURE_RUN_DIR/idle-hided-$trial.ps"
  MEASURE_ECHO_REPEATS=50 node "$measure_dir/echo.mjs" > "$MEASURE_RUN_DIR/echo-$trial.json"
  ps -p "$hided_pid" -o pid,%cpu,rss,etime,command > "$MEASURE_RUN_DIR/driven-hided-$trial.ps"
done
python3 "$measure_dir/summarize.py" echo "$MEASURE_RUN_DIR"/echo-{1,2,3}.json > "$MEASURE_RUN_DIR/echo-summary.json"

export MEASURE_DRIVEN_PANES="$MEASURE_PANE_ID"
if [[ "$scenario" == areas* ]]; then
  for pane in "${extra_panes[@]}"; do MEASURE_DRIVEN_PANES+=",$pane"; done
fi
note "frames: ${MEASURE_FRAME_WINDOW_MS:-120000} ms driven window, every shown Agent area"
reset_fixture
"$HERDR_BIN_PATH" pane send-keys "$MEASURE_PANE_ID" ctrl+c >/dev/null
sleep 0.5
uptime > "$MEASURE_RUN_DIR/frames-uptime-before.txt"
spawn_owned resources-driven python3 "$measure_dir/resources.py" "$hided_pid" "$server_pid" "$chrome_pid"
resource_pid=$owned_pid
if $memory_series; then
  spawn_owned memory-series python3 "$measure_dir/resources.py" "$hided_pid" "$server_pid" "$chrome_pid" --memory-series
  memory_pid=$owned_pid
fi
node "$measure_dir/frames.mjs" > "$MEASURE_RUN_DIR/frames.json"
wait "$resource_pid"
cp "$MEASURE_RUN_DIR/logs/resources-driven.log" "$MEASURE_RUN_DIR/resources-driven.json"
if $memory_series; then
  wait "$memory_pid"
  cp "$MEASURE_RUN_DIR/logs/memory-series.log" "$MEASURE_RUN_DIR/memory-series.json"
fi
uptime > "$MEASURE_RUN_DIR/frames-uptime-after.txt"
"$HERDR_BIN_PATH" pane read "$MEASURE_PANE_ID" --source recent-unwrapped --lines 5 > "$MEASURE_RUN_DIR/frames-pane-tail.txt" || true
ps -p "$hided_pid" -o pid,%cpu,rss,etime,command > "$MEASURE_RUN_DIR/driven-hided-frames.ps"
python3 - "$MEASURE_RUN_DIR" <<'PYJSON'
import json, pathlib, sys
root = pathlib.Path(sys.argv[1])
data = json.loads((root / 'frames.json').read_text())
(root / 'echo-driven.json').write_text(json.dumps(data['driven_echo']))
PYJSON
python3 "$measure_dir/summarize.py" echo "$MEASURE_RUN_DIR/echo-driven.json" > "$MEASURE_RUN_DIR/echo-driven-summary.json"
python3 "$measure_dir/summarize.py" frames "$MEASURE_RUN_DIR/frames.json" > "$MEASURE_RUN_DIR/frames-summary.json"
python3 "$measure_dir/memory.py" after-frames "$chrome_pid" "$hided_pid" "$MEASURE_CDP_PORT" > "$MEASURE_RUN_DIR/memory-after-frames.json"
cat "$MEASURE_RUN_DIR/memory-after-frames.json"
cat "$MEASURE_RUN_DIR/echo-summary.json" "$MEASURE_RUN_DIR/frames-summary.json"
note 'measurement complete; cleaning up owned processes'
