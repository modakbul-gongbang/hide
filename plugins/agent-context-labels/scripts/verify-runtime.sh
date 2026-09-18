#!/bin/sh
# Runtime smoke test for a Herdr-managed pane. Confirms the contract-shaped
# event subscription, reports display metadata through the same surface the
# watcher uses, proves the settings actions are idempotent, and measures the
# watcher's Herdr child-process count while it is idle.
set -eu

source_id="hide.agent-context-labels-runtime-test"
pane_id="${HERDR_PANE_ID:?run inside a Herdr-managed pane}"
settings_path="$HOME/.local/state/hide.agent-context-labels/settings.json"
socket_path="${HERDR_SOCKET_PATH:-$HOME/.config/herdr/herdr.sock}"
idle_seconds="${VERIFY_IDLE_SECONDS:-60}"

cleanup() {
  herdr pane report-metadata "$pane_id" --source "$source_id" \
    --clear-token task \
    --clear-token summary \
    --clear-token status_question \
    --clear-token status_question_new \
    --clear-token status_approval \
    --clear-token status_approval_new \
    --clear-token status_error \
    --clear-token status_error_new \
    --clear-token status_working \
    --clear-token status_done \
    --clear-token status_interrupted \
    --clear-token status_idle \
    --clear-token status_stale \
    --clear-token sort_rank \
    --clear-token activity \
    --clear-token elapsed \
    --clear-token agent_codex >/dev/null 2>&1 || true
}
trap cleanup EXIT INT TERM

herdr plugin list --plugin hide.agent-context-labels --json |
  rg '"startup"|"refresh-active-pane-task"|"enable-automatic-summaries"|"disable-automatic-summaries"'

# The status-changed subscription is pane-scoped in the pinned contract. The
# watcher expands that filter from its bootstrap pane list; this probe checks
# the same request shape against the live server without using a CLI fallback.
python3 - "$socket_path" "$pane_id" <<'PY'
import json
import socket
import sys

socket_path, pane_id = sys.argv[1:]
request = {
    "id": "hide.agent-context-labels:verify-subscription",
    "method": "events.subscribe",
    "params": {
        "subscriptions": [
            {"type": "pane.created"},
            {"type": "pane.updated"},
            {"type": "pane.closed"},
            {"type": "pane.exited"},
            {"type": "pane.focused"},
            {"type": "pane.agent_detected"},
            {"type": "pane.agent_status_changed", "pane_id": pane_id},
        ],
    },
}
with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as stream:
    stream.settimeout(5)
    stream.connect(socket_path)
    stream.sendall((json.dumps(request) + "\n").encode())
    line = b""
    while not line.endswith(b"\n"):
        chunk = stream.recv(4096)
        if not chunk:
            raise RuntimeError("events.subscribe closed before its acknowledgement")
        line += chunk
ack = json.loads(line)
if ack.get("id") != request["id"]:
    raise RuntimeError(f"events.subscribe response id mismatch: {ack.get('id')!r}")
if "error" in ack:
    raise RuntimeError(f"events.subscribe failed: {ack['error']}")
result = ack.get("result", {})
if result.get("type") != "subscription_started":
    raise RuntimeError(f"unexpected events.subscribe result: {result}")
print("subscription-started")
PY

herdr pane report-metadata "$pane_id" --source "$source_id" \
  --token task=fixture \
  --token 'status_question=?' \
  --token elapsed=7s \
  --token agent_codex=codex
herdr pane get "$pane_id" | rg '"task":"fixture"|"status_question":"\?"|"elapsed":"7s"|"agent_codex":"codex"'

# The same action applied twice must leave the same state.
herdr plugin action invoke disable-automatic-summaries --plugin hide.agent-context-labels >/dev/null
herdr plugin action invoke disable-automatic-summaries --plugin hide.agent-context-labels >/dev/null
sleep 1
rg '"automatic_summaries":false' "$settings_path"

herdr plugin action invoke enable-automatic-summaries --plugin hide.agent-context-labels >/dev/null
herdr plugin action invoke enable-automatic-summaries --plugin hide.agent-context-labels >/dev/null
sleep 1
rg '"automatic_summaries":true' "$settings_path"

watcher_pid="$(ps -axo pid=,command= | awk '/hide-agent-context-labels[[:space:]]+watch([[:space:]]|$)/ {print $1; exit}')"
if [ -z "$watcher_pid" ]; then
  echo "cannot find the running hide-agent-context-labels watcher" >&2
  exit 1
fi

max_herdr_children=0
started_at="$(date +%s)"
while :; do
  now="$(date +%s)"
  elapsed=$((now - started_at))
  children="$(ps -axo pid=,ppid=,command= | awk -v parent="$watcher_pid" '
    $2 == parent && $0 ~ /(^|[[:space:]\/])herdr([[:space:]]|$)/ {count++}
    END {print count + 0}
  ')"
  if [ "$children" -gt "$max_herdr_children" ]; then
    max_herdr_children="$children"
  fi
  if [ "$elapsed" -ge "$idle_seconds" ]; then
    break
  fi
  sleep 1
done
if [ "$max_herdr_children" -ne 0 ]; then
  echo "idle Herdr child-process count was $max_herdr_children" >&2
  exit 1
fi
echo "idle-herdr-children=0 duration=${idle_seconds}s"

echo runtime-verification-pass
