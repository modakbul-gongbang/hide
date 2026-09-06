#!/usr/bin/env zsh
# Stands up the Scratch verification fixture on an isolated Herdr server, and
# tears it down again.
#
# Everything here runs against a server this script starts on its own socket
# with its own HOME. That isolation is the point, not a convenience: on
# 2026-09-06 an end-to-end run that overrode only HOME joined the operator's
# live Herdr and drove their real panes. `HERDR_SOCKET_PATH` must therefore be
# set, and must not be the default socket; the script refuses to run otherwise.
#
# The fixture is the Scratch folder of that isolated server holding three tabs:
# two that report an agent and carry a chat title, and one plain terminal tab.
# The two agents are reported through `pane report-agent`, not started: the
# sidebar reads `agent.list`, so a reported agent draws the same row as a real
# one and costs no tokens. Exactly one real agent session is spent, by the live
# submission check, and this fixture is not it.
#
# Usage:
#   zsh scripts/scratch-verification-fixture.sh serve      # start the server
#   zsh scripts/scratch-verification-fixture.sh setup      # three Scratch tabs
#   zsh scripts/scratch-verification-fixture.sh status
#   zsh scripts/scratch-verification-fixture.sh teardown   # close what it made
#   zsh scripts/scratch-verification-fixture.sh stop       # stop the server
#
# The dev bundle is pointed at the same server by exporting the same
# HERDR_SOCKET_PATH and HOME before launching it; `serve` prints the exact
# line.

set -eu

# Resolved at the top level: inside a zsh function `$0` is the function's own
# name, so this cannot be read where it is used.
SCRIPT_DIR=${0:A:h}

SOURCE_ID="hide-scratch-fixture"
AGENT_LABEL="claude"
DEFAULT_SOCKET="${HOME}/.config/herdr/herdr.sock"
SOCKET_PATH="${HERDR_SOCKET_PATH:-}"
RUN_ROOT="${HIDE_SCRATCH_FIXTURE_ROOT:-}"

die() {
  print -u2 -- "fixture: $*"
  exit 1
}

# The one guard that matters. A fixture that reached the operator's own server
# would create workspaces in the session they are working in and, on teardown,
# close panes this script never made.
require_isolated_socket() {
  [[ -n "$SOCKET_PATH" ]] || die "set HERDR_SOCKET_PATH to an isolated socket first; this fixture never runs on the default server"
  [[ "$SOCKET_PATH" != "$DEFAULT_SOCKET" ]] || die "HERDR_SOCKET_PATH is the operator's default socket; use a socket under the run directory"
  [[ -n "$RUN_ROOT" ]] || RUN_ROOT="${SOCKET_PATH:h}"
}

require_server() {
  require_isolated_socket
  [[ -S "$SOCKET_PATH" ]] || die "no Herdr server at ${SOCKET_PATH}; run serve first"
}

# Where Scratch lives for this server. The core derives it from HOME, so the
# fixture derives it the same way rather than restating the path.
scratch_root() {
  print -- "${HOME}/Library/Application Support/hide/scratch"
}

scratch_workspace_id() {
  local root
  root="$(scratch_root)"
  herdr pane list \
    | python3 -c '
import json, sys
root = sys.argv[1]
panes = json.load(sys.stdin)["result"]["panes"]
print(next((p["workspace_id"] for p in panes if (p.get("cwd") or "").startswith(root)), ""))
' "$root"
}

panes_of_tab() {
  local tab="$1"
  herdr pane list \
    | python3 -c 'import json,sys; print(" ".join(p["pane_id"] for p in json.load(sys.stdin)["result"]["panes"] if p.get("tab_id") == sys.argv[1]))' "$tab"
}

cmd_serve() {
  require_isolated_socket
  if [[ -S "$SOCKET_PATH" ]]; then
    print -- "a server is already listening at ${SOCKET_PATH}"
    return 0
  fi
  local binary
  binary="$(${SCRIPT_DIR}/fetch-herdr-runtime.sh)"
  mkdir -p "${SOCKET_PATH:h}" "$HOME/.config/herdr"
  "$binary" server >"${SOCKET_PATH:h}/server.log" 2>&1 &
  local attempt
  for attempt in {1..40}; do
    [[ -S "$SOCKET_PATH" ]] && break
    sleep 0.25
  done
  [[ -S "$SOCKET_PATH" ]] || die "the isolated server never opened ${SOCKET_PATH}"
  print -- "server:  ${SOCKET_PATH}"
  print -- "home:    ${HOME}"
  print -- "scratch: $(scratch_root)"
  print -- "point the dev bundle at it with the same HERDR_SOCKET_PATH and HOME"
}

cmd_setup() {
  require_server
  local existing
  existing="$(scratch_workspace_id)"
  # Rule 11: a second run adds tabs to the space that is already there rather
  # than standing up a second one, which is also what the product does.
  [[ -z "$existing" ]] || die "Scratch already holds panes as workspace ${existing}; run teardown first"

  local root
  root="$(scratch_root)"
  mkdir -p "$root"

  herdr workspace create --cwd "$root" --label "hide scratch" --no-focus >/dev/null
  local workspace
  workspace="$(scratch_workspace_id)"
  [[ -n "$workspace" ]] || die "workspace create left no pane in ${root}"

  herdr tab create --workspace "$workspace" --cwd "$root" --label "hide claude" --no-focus >/dev/null
  herdr tab create --workspace "$workspace" --cwd "$root" --label "Tab 3" --no-focus >/dev/null

  local tabs
  tabs=(${(f)"$(herdr tab list --workspace "$workspace" | python3 -c 'import json,sys; print("\n".join(t["tab_id"] for t in json.load(sys.stdin)["result"]["tabs"]))')"})
  [[ ${#tabs} -ge 3 ]] || die "expected three Scratch tabs, found ${#tabs}"

  report_chat "${tabs[1]}" "build me a worktree parser" 0000000000002
  report_chat "${tabs[2]}" "why does the sidebar flicker" 0000000000001

  print -- "workspace: ${workspace}"
  print -- "scratch:   ${root}"
  cmd_status
}

# One reported chat: an agent Herdr will list, and the title token the sidebar
# reads. Reported rather than started, so the fixture spends no agent session.
report_chat() {
  local tab="$1" title="$2" activity="$3" pane
  pane="$(panes_of_tab "$tab")"
  pane="${pane%% *}"
  [[ -n "$pane" ]] || die "tab ${tab} has no pane"
  herdr pane report-agent "$pane" \
    --source "$SOURCE_ID" --agent "$AGENT_LABEL" --state idle >/dev/null
  herdr pane report-metadata "$pane" \
    --source "$SOURCE_ID" \
    --token "hide_chat_title=${title}" \
    --token "status_idle=○" \
    --token "summary=${title}" \
    --token "activity=${activity}" >/dev/null
  print -- "chat ${pane}: ${title}"
}

cmd_status() {
  require_server
  local root
  root="$(scratch_root)"
  print -- "panes with a Scratch working directory:"
  herdr pane list \
    | python3 -c '
import json, sys
root = sys.argv[1]
for pane in json.load(sys.stdin)["result"]["panes"]:
    cwd = pane.get("cwd") or ""
    if not cwd.startswith(root):
        continue
    title = (pane.get("tokens") or {}).get("hide_chat_title", "-")
    print("  {}  tab={}  title={}".format(pane["pane_id"], pane["tab_id"], title))
' "$root"
  print -- "files in ${root}:"
  ls -1 "$root" 2>/dev/null | sed 's/^/  /' || print -- "  (none)"
}

cmd_teardown() {
  require_server
  local workspace before after
  workspace="$(scratch_workspace_id)"
  [[ -n "$workspace" ]] || die "no Scratch workspace to close"

  before="$(herdr workspace list | python3 -c 'import json,sys; print(len(json.load(sys.stdin)["result"]["workspaces"]))')"
  herdr workspace close "$workspace" >/dev/null
  after="$(herdr workspace list | python3 -c 'import json,sys; print(len(json.load(sys.stdin)["result"]["workspaces"]))')"
  print -- "closed ${workspace}; workspaces ${before} -> ${after}"
  # The folder and its files are left exactly where they are. Closing a tab
  # never deletes anything in Scratch, and the check that proves it reads this
  # directory after teardown.
  print -- "left in place: $(scratch_root)"
}

cmd_stop() {
  require_isolated_socket
  [[ -S "$SOCKET_PATH" ]] || { print -- "no server at ${SOCKET_PATH}"; return 0; }
  herdr server stop >/dev/null 2>&1 || true
  print -- "stopped the server at ${SOCKET_PATH}"
}

case "${1:-}" in
  serve) cmd_serve ;;
  setup) cmd_setup ;;
  status) cmd_status ;;
  teardown) cmd_teardown ;;
  stop) cmd_stop ;;
  *) die "usage: $0 <serve|setup|status|teardown|stop>" ;;
esac
