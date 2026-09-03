#!/usr/bin/env zsh
# Stands up the throwaway Herdr workspace the agent-attention verification runs
# against, fills the four groups, drives the blocked transition, and tears the
# whole thing down again.
#
# The fixture is one workspace holding two tabs in a checkout that is its own
# git repository:
#
#   tab "finished" - three panes side by side, all reported finished. This is
#                    the "one tab, three completions" case: focusing one of
#                    them must clear that one row and leave the other two,
#                    even though Herdr marks every pane in a tab seen at once.
#   tab "mixed"    - three panes, one blocked, one working, one idle. With the
#                    finished tab that fills all four groups at once.
#
# Every workspace, tab and pane this script touches is one it created. It
# refuses to run against a fixture that already exists rather than creating a
# second one, it reports lifecycle state and metadata tokens only to its own
# panes, and teardown closes exactly the workspace whose id it recorded and
# checks the operator's workspace count came back.
#
# No agent session is started. Every state the fixture shows is reported.
#
# Usage:
#   zsh scripts/agent-attention-fixture.sh setup
#   zsh scripts/agent-attention-fixture.sh fill
#   zsh scripts/agent-attention-fixture.sh block | unblock
#   zsh scripts/agent-attention-fixture.sh ask <role>
#   zsh scripts/agent-attention-fixture.sh focus <role>
#   zsh scripts/agent-attention-fixture.sh status
#   zsh scripts/agent-attention-fixture.sh read-records [bundle-id]
#   zsh scripts/agent-attention-fixture.sh procedure
#   zsh scripts/agent-attention-fixture.sh teardown
#
# A role is one of: done-1 done-2 done-3 blocked working idle

set -eu

FIXTURE_LABEL="agent-attention-fixture"
FIXTURE_ROOT="/tmp/herdr-ide-verify/fixtures/agent-attention"
STATE_FILE="${FIXTURE_ROOT}/.fixture-panes"
SOURCE_ID="agent-attention-fixture"
AGENT_LABEL="codex"
SOCKET_PATH="${HERDR_SOCKET_PATH:-${HOME}/.config/herdr/herdr.sock}"
# The dev bundle built from this worktree. Its state file is where Hide's own
# per-pane read records live, and it is separate from the operator's app.
DEV_BUNDLE_ID="me.grab.hide.agent-attention"

ROLES=(done-1 done-2 done-3 blocked working idle)

die() {
  print -u2 -- "fixture: $*"
  exit 1
}

require_socket() {
  [[ -S "$SOCKET_PATH" ]] || die "no Herdr socket at ${SOCKET_PATH}"
}

# One request/response over the Herdr socket, newline-delimited JSON. Used only
# where the CLI has no equivalent command: `herdr pane focus` moves to a
# neighbour by direction, and the fixture needs to focus a named pane.
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

# Pane ids of one tab, in layout order.
panes_of_tab() {
  local workspace="$1" tab="$2"
  herdr pane list --workspace "$workspace" \
    | python3 -c 'import json,sys; print(" ".join(p["pane_id"] for p in json.load(sys.stdin)["result"]["panes"] if p.get("tab_id") == sys.argv[1]))' "$tab"
}

pane_for_role() {
  local role="$1"
  [[ -f "$STATE_FILE" ]] || die "no fixture recorded; run setup first"
  local line
  line="$(grep "^${role}=" "$STATE_FILE" || true)"
  [[ -n "$line" ]] || die "unknown role ${role}; expected one of ${ROLES[*]}"
  print -- "${line#*=}"
}

# One pane's whole reported state: the lifecycle Herdr owns and the display
# tokens the label plugin owns. Reported together so a role's group is decided
# by this script and not by whatever the pane happened to carry before.
report_role() {
  local role="$1" state="$2" token="$3" symbol="$4" summary="$5" activity="$6"
  local pane
  pane="$(pane_for_role "$role")"
  herdr pane report-agent "$pane" \
    --source "$SOURCE_ID" --agent "$AGENT_LABEL" --state "$state" >/dev/null
  # Every status token is cleared first: the demand axis reads whichever one is
  # present, so a leftover token from an earlier fill would decide the group.
  local clear
  for clear in status_question_new status_question status_approval_new status_approval \
               status_error_new status_error status_working status_done_new status_idle; do
    herdr pane report-metadata "$pane" --source "$SOURCE_ID" --clear-token "$clear" >/dev/null
  done
  herdr pane report-metadata "$pane" \
    --source "$SOURCE_ID" \
    --token "${token}=${symbol}" \
    --token "summary=${summary}" \
    --token "activity=${activity}" >/dev/null
  print -- "${role} (${pane}): ${state} / ${token}"
}

cmd_setup() {
  require_socket
  local existing
  existing="$(workspace_id_by_label)"
  # Engineering rule 11: a second run must not stand up a second workspace.
  [[ -z "$existing" ]] || die "workspace ${FIXTURE_LABEL} already exists as ${existing}; run teardown first"

  mkdir -p "$FIXTURE_ROOT"
  # The checkout must be its own git repository, or the catalog keys it under
  # whichever repository encloses the directory instead.
  [[ -d "${FIXTURE_ROOT}/.git" ]] || git -C "$FIXTURE_ROOT" init -q
  printf '# agent attention fixture\n\nA throwaway checkout. Nothing here is real work.\n' \
    > "${FIXTURE_ROOT}/README.md"
  git -C "$FIXTURE_ROOT" add README.md >/dev/null
  git -C "$FIXTURE_ROOT" -c user.email=fixture@local -c user.name=fixture \
    commit -qm 'Add the fixture file' >/dev/null 2>&1 || true

  herdr workspace create --cwd "$FIXTURE_ROOT" --label "$FIXTURE_LABEL" --no-focus >/dev/null
  local workspace
  workspace="$(workspace_id_by_label)"
  [[ -n "$workspace" ]] || die "workspace create reported no ${FIXTURE_LABEL} workspace"

  # The workspace's own first tab becomes the finished tab; the mixed tab is
  # created beside it.
  local finished_tab mixed_tab
  finished_tab="$(herdr tab list --workspace "$workspace" \
    | python3 -c 'import json,sys; print(json.load(sys.stdin)["result"]["tabs"][0]["tab_id"])')"
  herdr tab rename "$finished_tab" finished >/dev/null 2>&1 || true
  mixed_tab="$(herdr tab create --workspace "$workspace" --cwd "$FIXTURE_ROOT" --label mixed --no-focus \
    | python3 -c 'import json,sys; d=json.load(sys.stdin)["result"]; print(d.get("tab_id") or d["tab"]["tab_id"])')"

  local seed
  seed="$(panes_of_tab "$workspace" "$finished_tab")"
  local first="${seed%% *}"
  herdr pane split "$first" --direction right --cwd "$FIXTURE_ROOT" --no-focus >/dev/null
  herdr pane split "$first" --direction down --cwd "$FIXTURE_ROOT" --no-focus >/dev/null
  seed="$(panes_of_tab "$workspace" "$mixed_tab")"
  first="${seed%% *}"
  herdr pane split "$first" --direction right --cwd "$FIXTURE_ROOT" --no-focus >/dev/null
  herdr pane split "$first" --direction down --cwd "$FIXTURE_ROOT" --no-focus >/dev/null

  local finished_panes mixed_panes
  finished_panes=(${=$(panes_of_tab "$workspace" "$finished_tab")})
  mixed_panes=(${=$(panes_of_tab "$workspace" "$mixed_tab")})
  [[ ${#finished_panes[@]} -eq 3 ]] || die "expected three panes in the finished tab, saw ${#finished_panes[@]}"
  [[ ${#mixed_panes[@]} -eq 3 ]] || die "expected three panes in the mixed tab, saw ${#mixed_panes[@]}"

  {
    print -- "workspace=${workspace}"
    print -- "finished_tab=${finished_tab}"
    print -- "mixed_tab=${mixed_tab}"
    print -- "done-1=${finished_panes[1]}"
    print -- "done-2=${finished_panes[2]}"
    print -- "done-3=${finished_panes[3]}"
    print -- "blocked=${mixed_panes[1]}"
    print -- "working=${mixed_panes[2]}"
    print -- "idle=${mixed_panes[3]}"
  } > "$STATE_FILE"

  print -- "workspace: ${workspace}"
  print -- "root:      ${FIXTURE_ROOT}"
  cat "$STATE_FILE"
}

# Fills all four groups at once: three finished panes, one blocked, one
# working, one idle. Nothing is focused, so every reported pane starts unread
# and the idle pane joins Done until something focuses it.
cmd_fill() {
  require_socket
  report_role done-1 idle status_done_new "●" "Ran the suite" 1788300000001
  report_role done-2 idle status_done_new "●" "Wrote the migration" 1788300000002
  report_role done-3 idle status_done_new "●" "Updated the docs" 1788300000003
  report_role blocked blocked status_approval_new "!" "Waiting on approval" 1788300000004
  report_role working working status_working "●" "Building the index" 1788300000005
  report_role idle idle status_idle "○" "Nothing pending" 1788300000006
  print -- "four groups filled; focus the idle pane to move it to Seen"
}

# The blocked half of the transition, on its own so it can be driven twice
# around a focus without re-reporting the other five panes.
cmd_block() {
  require_socket
  report_role blocked blocked status_approval_new "!" "Waiting on approval" 1788300000004
}

# Releasing the block is the recovery path: the pane leaves Needs You only when
# the prompt is answered, never when it is merely looked at.
cmd_unblock() {
  require_socket
  report_role blocked idle status_idle "○" "Approval granted" 1788300000007
}

# Raises a new question on one pane, for the "a question arrives on the pane I
# am already watching" case.
cmd_ask() {
  require_socket
  local role="${1:-}"
  [[ -n "$role" ]] || die "usage: ask <role>"
  report_role "$role" idle status_question_new "?" "Which branch should I use?" "$(date +%s)000"
}

# Focuses one fixture pane. This is the signal Hide reads as "the operator
# looked at it", and it is the same request the sidebar sends when a row is
# clicked. Only panes this fixture created are ever named here.
cmd_focus() {
  require_socket
  local role="${1:-}"
  [[ -n "$role" ]] || die "usage: focus <role>"
  local pane
  pane="$(pane_for_role "$role")"
  socket_call "pane.focus" "{\"pane_id\":\"${pane}\"}" >/dev/null
  print -- "focused ${role} (${pane})"
}

cmd_status() {
  require_socket
  [[ -f "$STATE_FILE" ]] || die "no fixture recorded; run setup first"
  cat "$STATE_FILE"
  print -- "--- reported state ---"
  # The agent list goes to a file rather than a pipe: `python3 -` reads its own
  # program from stdin, so a piped payload would be read as the program.
  local listing="${FIXTURE_ROOT}/.agent-list.json"
  herdr agent list > "$listing"
  python3 - "$STATE_FILE" "$listing" <<'PY'
import json, sys

panes = {}
for line in open(sys.argv[1]):
    key, _, value = line.strip().partition("=")
    if key not in ("workspace", "finished_tab", "mixed_tab"):
        panes[value] = key
rows = json.load(open(sys.argv[2]))["result"]["agents"]
for row in rows:
    role = panes.get(row["pane_id"])
    if role is None:
        continue
    tokens = row.get("tokens", {})
    status = [name for name in tokens if name.startswith("status_")]
    print(f"{role:8} {row['pane_id']:12} lifecycle={row['agent_status']:8} "
          f"seq={row.get('state_change_seq')} focused={row['focused']} tokens={sorted(status)}")
PY
  rm -f "$listing"
}

# Hide's own read record for each fixture pane, read straight out of the dev
# bundle's store. Run it before quitting the app and again after relaunching:
# the two outputs must match, because a restart may not lose what the operator
# already read. Herdr knows nothing about this file.
cmd_read_records() {
  [[ -f "$STATE_FILE" ]] || die "no fixture recorded; run setup first"
  local bundle="${1:-$DEV_BUNDLE_ID}"
  local store="${HOME}/Library/Application Support/hide/instances/${bundle}/state.json"
  [[ -f "$store" ]] || die "no state file for ${bundle} at ${store}"
  python3 - "$STATE_FILE" "$store" <<'PY'
import json, sys

panes = {}
for line in open(sys.argv[1]):
    key, _, value = line.strip().partition("=")
    if key not in ("workspace", "finished_tab", "mixed_tab"):
        panes[value] = key
state = json.load(open(sys.argv[2]))
records = state.get("pane_read_records", {})
for pane_id, role in sorted(panes.items(), key=lambda item: item[1]):
    record = records.get(pane_id)
    print(f"{role:8} {pane_id:12} {'unread' if record is None else json.dumps(record, sort_keys=True)}")
PY
}

# The verification procedure, printed rather than written down somewhere it
# can go stale. Each step names what it proves.
cmd_procedure() {
  cat <<'STEPS'
Setup
  1. zsh scripts/agent-attention-fixture.sh setup
  2. zsh scripts/agent-attention-fixture.sh fill
  3. Launch the dev bundle built from this worktree and focus the fixture
     checkout in its navigator. Hide filters the agent list to the focused
     checkout, so the fixture panes do not appear until it is selected.

Three completions in one tab (SC1, AC7)
  4. Capture the sidebar. All three finished panes are in Done, bright.
  5. Click the first Done row. Capture again: that row leaves Done, the other
     two stay, and the pane it names now has focus. Herdr has marked the whole
     tab seen at this point, which is exactly what must not clear the other two.

A question on the pane being watched (SC2)
  6. zsh scripts/agent-attention-fixture.sh focus idle
  7. zsh scripts/agent-attention-fixture.sh ask idle
     The row stays out of Needs You and reads as a subdued question, because
     the operator is already looking at it.
  8. zsh scripts/agent-attention-fixture.sh focus done-2
     zsh scripts/agent-attention-fixture.sh ask idle
     Now it rises into Needs You with a bright mark.

Blocked outlives being looked at (AC8)
  9. zsh scripts/agent-attention-fixture.sh block
 10. Click the blocked row, then click any other row. Capture: it is still in
     Needs You, because an approval prompt leaves when it is answered, not when
     it is seen.
 11. zsh scripts/agent-attention-fixture.sh unblock
     Capture: it leaves Needs You.

Four groups, three surfaces (SC4, AC6)
 12. zsh scripts/agent-attention-fixture.sh fill
     zsh scripts/agent-attention-fixture.sh focus idle
 13. Capture the Projects view, the Agents view and the pet dashboard. The same
     agent carries the same mark, hue and status word in all three, no view
     shows an underscored state name, and a raised row is not repeated in the
     project tree.

Pet counts (SC5, AC9)
 14. In the same state, the pet's act-now count equals the Needs You section
     count and its done count equals the Done section count.

Restart (SC3)
 15. zsh scripts/agent-attention-fixture.sh read-records > before.txt
 16. Quit only the dev bundle:
       osascript -e 'tell application id "me.grab.hide.agent-attention" to quit'
 17. Relaunch it, focus the fixture checkout again, and capture the sidebar.
 18. zsh scripts/agent-attention-fixture.sh read-records > after.txt
     diff before.txt after.txt must be empty, and the sidebar groups must match
     the capture from step 13. Nothing on the Herdr side changed across the
     restart, so any difference is Hide losing its own record.

Teardown
 19. zsh scripts/agent-attention-fixture.sh teardown
     It closes only the workspace it recorded and fails loudly if the operator's
     workspace count did not come back by exactly one.
STEPS
}

cmd_teardown() {
  require_socket
  local workspace before after
  workspace="$(grep '^workspace=' "$STATE_FILE" 2>/dev/null | cut -d= -f2 || true)"
  [[ -n "$workspace" ]] || workspace="$(workspace_id_by_label)"
  [[ -n "$workspace" ]] || die "no ${FIXTURE_LABEL} workspace recorded or listed; nothing to close"

  before="$(workspace_count)"
  herdr workspace close "$workspace" >/dev/null
  after="$(workspace_count)"
  rm -rf "$FIXTURE_ROOT"
  print -- "closed ${workspace}; workspaces ${before} -> ${after}"
  [[ "$after" -eq $((before - 1)) ]] || die "expected exactly one workspace to close"
  [[ -z "$(workspace_id_by_label)" ]] || die "${FIXTURE_LABEL} is still listed after teardown"
  print -- "no ${FIXTURE_LABEL} workspace remains"
}

case "${1:-}" in
  setup) cmd_setup ;;
  fill) cmd_fill ;;
  block) cmd_block ;;
  unblock) cmd_unblock ;;
  ask) shift; cmd_ask "$@" ;;
  focus) shift; cmd_focus "$@" ;;
  status) cmd_status ;;
  read-records) shift; cmd_read_records "$@" ;;
  procedure) cmd_procedure ;;
  teardown) cmd_teardown ;;
  *) die "usage: $0 <setup|fill|block|unblock|ask|focus|status|read-records|procedure|teardown>" ;;
esac
