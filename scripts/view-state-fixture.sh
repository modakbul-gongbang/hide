#!/usr/bin/env zsh
# Stands up the throwaway Herdr workspace the view-state verification runs
# against, fills every pane with a scrollback the eye can check, drives the
# outside-focus path, stages the missing-runtime launch, and tears the whole
# thing down again.
#
# The fixture is one workspace holding three Herdr tabs in a checkout that is
# its own git repository:
#
#   tab "one"   - two panes side by side. The tab a switch returns to, which is
#                 where a retained terminal view has to still hold its
#                 scrollback and its real split geometry (SC1).
#   tab "two"   - three panes. The unvisited tab, which has to appear split the
#                 way Herdr already split it rather than as an even grid (SC2).
#   tab "three" - one pane. The pane a click has to take keyboard focus to
#                 without waiting for Herdr to confirm it (SC3).
#
# Every pane prints twelve `MARK-<pane>-<n>` lines at setup. A tab switch that
# rebuilt its terminal views instead of keeping them loses those lines, so the
# scrollback is the assertion rather than a description of one.
#
# Every workspace, tab and pane this script touches is one it created. It
# refuses to run against a fixture that already exists rather than creating a
# second one, and teardown closes exactly the workspace whose id it recorded and
# checks the operator's workspace count came back.
#
# Setup creates every object with --no-focus, so standing the fixture up moves
# nothing the operator was looking at. `focus` is the exception and is meant to
# be: it exists to drive the outside-focus path, and Herdr's focused pane is
# session state rather than per-workspace state. So the first `focus` records
# where the focus was and teardown puts it back.
#
# Usage:
#   zsh scripts/view-state-fixture.sh setup
#   zsh scripts/view-state-fixture.sh status
#   zsh scripts/view-state-fixture.sh focus <pane-id|tab-id>   # moves herdr focus
#   zsh scripts/view-state-fixture.sh procedure
#   zsh scripts/view-state-fixture.sh teardown

set -eu

# Captured before any function runs: zsh sets $0 to the function name inside a
# function, so a path derived there points at the caller's directory instead.
SCRIPT_PATH="${0:A}"
REPO_ROOT="${SCRIPT_PATH:h:h}"

FIXTURE_LABEL="view-state-fixture"
FIXTURE_ROOT="/tmp/herdr-ide-verify/fixtures/view-state"
STATE_FILE="${FIXTURE_ROOT}/.fixture-workspace-id"
# Herdr's session-wide focused pane before this script first moved it. Focus is
# session state, not per-workspace state, so a workspace listing cannot see it
# and closing the fixture does not put it back: Herdr picks a new focus on its
# own. The script undoes its own perturbation from this record rather than
# naming a workspace it did not create.
FOCUS_STATE_FILE="${FIXTURE_ROOT}/.fixture-focus-before"
SOCKET_PATH="${HERDR_SOCKET_PATH:-${HOME}/.config/herdr/herdr.sock}"
MARK_COUNT=12

die() {
  print -u2 -- "fixture: $*"
  exit 1
}

require_socket() {
  [[ -S "$SOCKET_PATH" ]] || die "no Herdr socket at ${SOCKET_PATH}"
}

# One request/response over the Herdr socket, newline-delimited JSON. Used only
# where the CLI has no equivalent command: `herdr pane focus` moves to a
# neighbour by direction, and the fixture has to focus a pane by name.
socket_call() {
  local method="$1" params="$2"
  python3 - "$SOCKET_PATH" "$method" "$params" <<'PY'
import json, socket, sys

path, method, params = sys.argv[1], sys.argv[2], sys.argv[3]
request = {"id": "fixture:1", "method": method, "params": json.loads(params)}
client = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
client.connect(path)
client.sendall((json.dumps(request) + "\n").encode())
buffered = b""
while not buffered.endswith(b"\n"):
    chunk = client.recv(65536)
    if not chunk:
        break
    buffered += chunk
client.close()
sys.stdout.write(buffered.decode().strip() + "\n")
PY
}

workspace_id_by_label() {
  herdr workspace list \
    | python3 -c 'import json,sys; print(next((w["workspace_id"] for w in json.load(sys.stdin)["result"]["workspaces"] if w["label"] == sys.argv[1]), ""))' "$FIXTURE_LABEL"
}

workspace_count() {
  herdr workspace list \
    | python3 -c 'import json,sys; print(len(json.load(sys.stdin)["result"]["workspaces"]))'
}

# Pane ids of one tab, in Herdr's own order.
panes_of_tab() {
  herdr pane list --workspace "$1" \
    | python3 -c 'import json,sys
panes = json.load(sys.stdin)["result"]["panes"]
print(" ".join(p["pane_id"] for p in panes if p.get("tab_id") == sys.argv[1]))' "$2"
}

tab_ids() {
  herdr tab list --workspace "$1" \
    | python3 -c 'import json,sys; print(" ".join(t["tab_id"] for t in json.load(sys.stdin)["result"]["tabs"]))'
}

# A scrollback a tab switch either keeps or visibly loses.
fill_pane() {
  local pane="$1"
  herdr pane run "$pane" \
    "for i in \$(seq 1 ${MARK_COUNT}); do printf 'MARK-%s-%s\n' '${pane}' \"\$i\"; done" >/dev/null
}

# A pane split a moment ago may not have a shell yet, so `pane run` accepts the
# command and prints nothing for a beat. Setup waits for the marks it asked for
# rather than reporting a fixture that is not filled yet.
wait_for_marks() {
  local workspace="$1" deadline=$(( SECONDS + 20 )) missing tab pane
  while (( SECONDS < deadline )); do
    missing=""
    for tab in ${=$(tab_ids "$workspace")}; do
      for pane in ${=$(panes_of_tab "$workspace" "$tab")}; do
        herdr pane read "$pane" 2>/dev/null | grep -q "MARK-${pane}-${MARK_COUNT}\b" \
          || missing="${missing} ${pane}"
      done
    done
    [[ -n "$missing" ]] || return 0
    sleep 1
  done
  die "panes never printed their marks:${missing}"
}

cmd_setup() {
  require_socket
  local existing
  existing="$(workspace_id_by_label)"
  # Rule 11: a second run must not stand up a second workspace.
  [[ -z "$existing" ]] || die "workspace ${FIXTURE_LABEL} already exists as ${existing}; run teardown first"

  mkdir -p "$FIXTURE_ROOT"
  # The checkout must be its own git repository, or the catalog keys it under
  # whichever repository encloses the directory instead.
  [[ -d "${FIXTURE_ROOT}/.git" ]] || git -C "$FIXTURE_ROOT" init -q
  printf '# view state fixture\n\nA file the shell can open as a file tab.\n' > "${FIXTURE_ROOT}/notes.md"
  git -C "$FIXTURE_ROOT" add notes.md >/dev/null
  git -C "$FIXTURE_ROOT" -c user.email=fixture@local -c user.name=fixture \
    commit -qm 'Add the fixture file' >/dev/null 2>&1 || true

  herdr workspace create --cwd "$FIXTURE_ROOT" --label "$FIXTURE_LABEL" --no-focus >/dev/null
  local workspace
  workspace="$(workspace_id_by_label)"
  [[ -n "$workspace" ]] || die "workspace create reported no ${FIXTURE_LABEL} workspace"
  printf '%s\n' "$workspace" > "$STATE_FILE"

  herdr tab create --workspace "$workspace" --cwd "$FIXTURE_ROOT" --label two --no-focus >/dev/null
  herdr tab create --workspace "$workspace" --cwd "$FIXTURE_ROOT" --label three --no-focus >/dev/null

  local tabs
  tabs=(${=$(tab_ids "$workspace")})
  [[ "${#tabs[@]}" -eq 3 ]] || die "expected three tabs, got ${#tabs[@]}"

  # Two, three, one. The counts differ so a tab switch that drew an even grid
  # instead of Herdr's layout is visible rather than merely wrong.
  local first
  first="$(panes_of_tab "$workspace" "${tabs[1]}")"
  herdr pane split "${first%% *}" --direction right --cwd "$FIXTURE_ROOT" --no-focus >/dev/null

  first="$(panes_of_tab "$workspace" "${tabs[2]}")"
  herdr pane split "${first%% *}" --direction right --cwd "$FIXTURE_ROOT" --no-focus >/dev/null
  herdr pane split "${first%% *}" --direction down --cwd "$FIXTURE_ROOT" --no-focus >/dev/null

  local tab pane
  for tab in "${tabs[@]}"; do
    for pane in ${=$(panes_of_tab "$workspace" "$tab")}; do
      fill_pane "$pane"
    done
  done
  wait_for_marks "$workspace"

  print -- "workspace: ${workspace}"
  print -- "root:      ${FIXTURE_ROOT}"
  cmd_status
}

cmd_status() {
  require_socket
  local workspace
  workspace="$(cat "$STATE_FILE" 2>/dev/null || true)"
  [[ -n "$workspace" ]] || workspace="$(workspace_id_by_label)"
  [[ -n "$workspace" ]] || die "no ${FIXTURE_LABEL} workspace; run setup first"

  local tab pane last
  for tab in ${=$(tab_ids "$workspace")}; do
    print -- "tab ${tab}"
    for pane in ${=$(panes_of_tab "$workspace" "$tab")}; do
      # The last mark line proves the scrollback survived, and how far it got.
      last="$(herdr pane read "$pane" 2>/dev/null | grep -o "MARK-${pane}-[0-9]*" | tail -1 || true)"
      print -- "  pane ${pane}  last mark: ${last:-none}"
    done
  done
}

# The session's focused pane, which is not what `herdr pane current` answers:
# that reports the pane the calling process runs in, from HERDR_PANE_ID, so a
# script run from inside a pane always reads its own.
current_focused_pane() {
  herdr pane list 2>/dev/null \
    | python3 -c 'import json,sys; print(next((p["pane_id"] for p in json.load(sys.stdin)["result"]["panes"] if p.get("focused")), ""))' 2>/dev/null || true
}

# The outside-focus path (AC8, V6). Hide must follow this on the next event and
# leave a diagnostic saying it followed, rather than fighting it back.
#
# This is the one command here that moves Herdr's session focus, so the first
# call records where the focus was, and teardown puts it back.
cmd_focus() {
  require_socket
  local target="${1:-}"
  [[ -n "$target" ]] || die "usage: focus <pane-id|tab-id>"
  local workspace
  workspace="$(cat "$STATE_FILE" 2>/dev/null || true)"
  [[ -n "$workspace" ]] || workspace="$(workspace_id_by_label)"
  [[ -n "$workspace" ]] || die "no ${FIXTURE_LABEL} workspace; run setup first"
  # Refuse to focus anything outside the workspace this script created.
  [[ "$target" == "${workspace}:"* ]] || \
    die "${target} is not in ${workspace}; this script focuses only what it created"

  if [[ ! -f "$FOCUS_STATE_FILE" ]]; then
    local before
    before="$(current_focused_pane)"
    [[ -z "$before" ]] || printf '%s\n' "$before" > "$FOCUS_STATE_FILE"
  fi

  if [[ "$target" == *":t"* ]]; then
    herdr tab focus "$target"
  else
    socket_call "pane.focus" "{\"pane_id\":\"${target}\"}"
  fi
}

# Puts Herdr's session focus back where the first `focus` call found it, if it
# is not there already and the pane still exists.
restore_focus() {
  local before now
  before="$(cat "$FOCUS_STATE_FILE" 2>/dev/null || true)"
  [[ -n "$before" ]] || return 0
  now="$(current_focused_pane)"
  if [[ "$now" == "$before" ]]; then
    print -- "herdr focus is already on ${before}"
    return 0
  fi
  if ! herdr pane get "$before" >/dev/null 2>&1; then
    print -u2 -- "fixture: pane ${before} is gone; herdr focus left on ${now:-unknown}"
    return 0
  fi
  socket_call "pane.focus" "{\"pane_id\":\"${before}\"}" >/dev/null
  print -- "herdr focus restored: ${now:-unknown} -> ${before}"
}

# The verification procedure, printed rather than written down somewhere it can
# drift from the fixture it describes.
cmd_procedure() {
  local workspace
  workspace="$(cat "$STATE_FILE" 2>/dev/null || true)"
  [[ -n "$workspace" ]] || workspace="$(workspace_id_by_label || true)"
  print -- "fixture workspace: ${workspace:-<not set up>}"
  print -- ""
  print -- "SC1, visited tab switch (AC4, AC5)"
  print -- "  1. Open each of the three tabs once, so every pane has attached."
  print -- "  2. Return to tab one. The strip's active mark and the canvas move"
  print -- "     together, no empty frame and no even grid passes, and both panes"
  print -- "     still show their MARK lines to ${MARK_COUNT}."
  print -- "  3. herdr pane list shows no size change across the switch."
  print -- ""
  print -- "SC2, first visit (AC6)"
  print -- "  1. From a fresh launch, open tab two without opening it first."
  print -- "  2. It appears split three ways the way Herdr split it, then fills"
  print -- "     in one frame per pane. No even grid."
  print -- ""
  print -- "SC3, pane focus (AC7)"
  print -- "  1. On tab one, click the pane that is not focused."
  print -- "  2. The focus ring moves on the click frame; type immediately and the"
  print -- "     characters land in that pane."
  print -- ""
  print -- "AC8, focus from outside"
  print -- "  1. zsh ${SCRIPT_PATH} focus <the other pane id>"
  print -- "  2. Hide follows on the next event and leaves a diagnostic naming the"
  print -- "     pane and the source."
  print -- ""
  print -- "SC4, launch (AC9, AC11)"
  print -- "  1. Launch the dev bundle with the fixture as the last workspace."
  print -- "  2. The launch trace carries one core creation and one"
  print -- "     first_terminal_frame."
  print -- ""
  print -- "AC6's failure clause, the missing runtime, is not staged here."
  print -- "  HideRuntimeEnvironment.resolve tries ~/.local/bin/herdr, then the"
  print -- "  absolute /opt/homebrew/bin/herdr and /usr/local/bin/herdr, then the"
  print -- "  login shell's PATH, then the runtime inside the bundle. Two of those"
  print -- "  ignore HOME and PATH both, and nothing overrides the search from the"
  print -- "  environment, so a staged run finds the operator's installed Herdr"
  print -- "  however it is staged. Reaching that window means removing the"
  print -- "  operator's installed Herdr, which this verification does not do."
}

cmd_teardown() {
  require_socket
  local workspace before after
  workspace="$(cat "$STATE_FILE" 2>/dev/null || true)"
  [[ -n "$workspace" ]] || workspace="$(workspace_id_by_label)"
  [[ -n "$workspace" ]] || die "no ${FIXTURE_LABEL} workspace recorded or listed; nothing to close"

  before="$(workspace_count)"
  # Before the close, while the recorded pane is still reachable to compare.
  restore_focus
  herdr workspace close "$workspace" >/dev/null
  after="$(workspace_count)"
  rm -f "$STATE_FILE"
  rm -rf "$FIXTURE_ROOT"
  print -- "closed ${workspace}; workspaces ${before} -> ${after}"
  [[ "$after" -eq $((before - 1)) ]] || die "expected exactly one workspace to close"
}

case "${1:-}" in
  setup) cmd_setup ;;
  status) cmd_status ;;
  focus) shift; cmd_focus "$@" ;;
  procedure) cmd_procedure ;;
  teardown) cmd_teardown ;;
  *) die "usage: ${SCRIPT_PATH} <setup|status|focus|procedure|teardown>" ;;
esac
