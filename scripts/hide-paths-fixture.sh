#!/usr/bin/env zsh
# Stands up the throwaway Herdr state the path-and-tab verification runs
# against, and tears it down again.
#
# What it builds, and which scenario card needs it:
#
#   - Two checkouts, each its own git repository: one holding
#     `deep/nested/leaf` and a text file inside it, and one holding the pane
#     that prints the paths. SC1 clicks the file and SC2 the folder, both from
#     a pane that starts in the other checkout, which is what gives the click a
#     checkout to switch to.
#   - A directory outside every registered checkout holding a text file, a
#     subfolder and a script with its execute bit set. SC3 clicks all three.
#   - Two Herdr workspaces whose tabs share that one checkout path, which is
#     what makes one Hide checkout hold tabs from two Herdr workspaces. SC6
#     drags in that strip.
#   - Eight tabs, so SC5 can visit more of them than the attach window holds.
#   - A pane that prints the three outside paths and the two inside ones and
#     then several thousand lines, so SC1 to SC4 have something to click and
#     something to scroll.
#   - A pane running only a shell, for the Cmd+W close in SC7.
#   - A pane running a claude agent, for the fork in SC8.
#
# Every workspace, tab and pane this script touches is one it created. It
# refuses to run against a fixture that already exists rather than creating a
# second one, and teardown closes exactly the workspaces whose ids it recorded.
#
# Usage:
#   zsh scripts/hide-paths-fixture.sh setup
#   zsh scripts/hide-paths-fixture.sh status
#   zsh scripts/hide-paths-fixture.sh paths
#   zsh scripts/hide-paths-fixture.sh teardown

set -eu

PRIMARY_LABEL="hide-paths-fixture"
SECONDARY_LABEL="hide-paths-fixture-sibling"
# `/private/tmp` rather than `/tmp` because the catalog keys a checkout by its
# real path, and the reveal has to agree with it through the symlink.
FIXTURE_ROOT="/private/tmp/herdr-ide-verify/fixtures/hide-paths"
OUTSIDE_ROOT="/private/tmp/herdr-ide-verify/fixtures/hide-paths-outside"
# The pane that prints the paths lives in a checkout of its own, because SC1
# and SC2 are about clicking a path that belongs to a checkout other than the
# one on screen: the click has a checkout to switch to only if it starts
# somewhere else.
PRINTER_ROOT="/private/tmp/herdr-ide-verify/fixtures/hide-paths-printer"
STATE_FILE="${FIXTURE_ROOT}/.fixture-workspace-ids"
SCROLL_LINES=4000
SOCKET_PATH="${HERDR_SOCKET_PATH:-${HOME}/.config/herdr/herdr.sock}"

INSIDE_FOLDER="deep/nested/leaf"
INSIDE_FILE="${INSIDE_FOLDER}/target.txt"
OUTSIDE_FILE="notes.txt"
OUTSIDE_FOLDER="reports"
OUTSIDE_SCRIPT="run.sh"
# The script writes this only if something runs it, so an empty directory is
# the proof that a click revealed it rather than executing it.
OUTSIDE_MARKER="executed.marker"

die() {
  print -u2 -- "fixture: $*"
  exit 1
}

require_socket() {
  [[ -S "$SOCKET_PATH" ]] || die "no Herdr socket at ${SOCKET_PATH}"
}

workspace_id_by_label() {
  herdr workspace list \
    | python3 -c 'import json,sys; print(next((w["workspace_id"] for w in json.load(sys.stdin)["result"]["workspaces"] if w["label"] == sys.argv[1]), ""))' "$1"
}

# The pane the given tab was created with. `tab create` reports the tab, and
# the pane is looked up rather than guessed.
first_pane_of_tab() {
  herdr pane list --workspace "$1" \
    | python3 -c 'import json,sys; print(next((p["pane_id"] for p in json.load(sys.stdin)["result"]["panes"] if p.get("tab_id") == sys.argv[1]), ""))' "$2"
}

# The fixture agent's recorded session id, which is what makes its pane
# forkable. Empty until claude has answered its own startup questions.
#
# Herdr reports it as `agent_session`, and only the `id` kind is forkable: a
# session recorded as a path is not something either agent's fork command can
# resume.
fixture_agent_session() {
  herdr agent list | python3 -c '
import json, sys

for agent in json.load(sys.stdin)["result"]["agents"]:
    if agent.get("name") != "hide-paths-fixture-agent":
        continue
    session = agent.get("agent_session") or {}
    if session.get("kind") == "id":
        print(session.get("value") or "")
    break
'
}

create_tab() {
  herdr tab create --workspace "$1" --cwd "${3:-$FIXTURE_ROOT}" --label "$2" --no-focus \
    | python3 -c 'import json,sys; print(json.load(sys.stdin)["result"]["tab"]["tab_id"])'
}

build_directories() {
  rm -rf "$FIXTURE_ROOT" "$OUTSIDE_ROOT" "$PRINTER_ROOT"
  mkdir -p "${FIXTURE_ROOT}/${INSIDE_FOLDER}" "${OUTSIDE_ROOT}/${OUTSIDE_FOLDER}" "$PRINTER_ROOT"
  # Each checkout is its own repository, or the catalog keys it under whichever
  # repository encloses the directory instead.
  git -C "$FIXTURE_ROOT" init -q
  git -C "$PRINTER_ROOT" init -q
  printf 'The checkout the printing pane starts in.\n' > "${PRINTER_ROOT}/README.md"
  printf 'The file SC1 clicks.\n' > "${FIXTURE_ROOT}/${INSIDE_FILE}"
  printf 'The file SC3 opens in its default application.\n' > "${OUTSIDE_ROOT}/${OUTSIDE_FILE}"
  printf 'A file inside the folder SC3 opens in Finder.\n' > "${OUTSIDE_ROOT}/${OUTSIDE_FOLDER}/inside.txt"
  cat > "${OUTSIDE_ROOT}/${OUTSIDE_SCRIPT}" <<'SCRIPT'
#!/bin/sh
# If a click ever runs this instead of revealing it, this file is the evidence.
printf 'executed at %s\n' "$(date)" >> "$(dirname "$0")/executed.marker"
SCRIPT
  chmod +x "${OUTSIDE_ROOT}/${OUTSIDE_SCRIPT}"
  git -C "$FIXTURE_ROOT" add -A >/dev/null
  git -C "$FIXTURE_ROOT" -c user.email=fixture@local -c user.name=fixture \
    -c commit.gpgsign=false commit -qm 'Add the fixture tree' >/dev/null 2>&1 || true
}

cmd_paths() {
  print -- "checkout:        ${FIXTURE_ROOT}"
  print -- "printer checkout: ${PRINTER_ROOT}"
  print -- "inside file:     ${FIXTURE_ROOT}/${INSIDE_FILE}"
  print -- "inside folder:   ${FIXTURE_ROOT}/${INSIDE_FOLDER}"
  print -- "outside file:    ${OUTSIDE_ROOT}/${OUTSIDE_FILE}"
  print -- "outside folder:  ${OUTSIDE_ROOT}/${OUTSIDE_FOLDER}"
  print -- "outside script:  ${OUTSIDE_ROOT}/${OUTSIDE_SCRIPT}"
  print -- "execution mark:  ${OUTSIDE_ROOT}/${OUTSIDE_MARKER} (must never exist)"
}

cmd_setup() {
  require_socket
  for label in "$PRIMARY_LABEL" "$SECONDARY_LABEL"; do
    local existing="$(workspace_id_by_label "$label")"
    # Rule 11: a second run must not stand up a second fixture.
    [[ -z "$existing" ]] || die "workspace ${label} already exists as ${existing}; run teardown first"
  done

  build_directories

  herdr workspace create --cwd "$FIXTURE_ROOT" --label "$PRIMARY_LABEL" --no-focus >/dev/null
  local primary="$(workspace_id_by_label "$PRIMARY_LABEL")"
  [[ -n "$primary" ]] || die "workspace create reported no ${PRIMARY_LABEL} workspace"

  # The second workspace shares the first's cwd, so Hide catalogues one
  # checkout holding tabs from two Herdr workspaces. That is the arrangement
  # every tab drag used to be refused in.
  herdr workspace create --cwd "$FIXTURE_ROOT" --label "$SECONDARY_LABEL" --no-focus >/dev/null
  local secondary="$(workspace_id_by_label "$SECONDARY_LABEL")"
  [[ -n "$secondary" ]] || die "workspace create reported no ${SECONDARY_LABEL} workspace"
  printf '%s\n%s\n' "$primary" "$secondary" > "$STATE_FILE"

  # Eight tabs in all, counting the two the workspaces were created with: five
  # more in the primary workspace and one more in the sibling.
  local printer_tab="" shell_tab="" agent_tab=""
  printer_tab="$(create_tab "$primary" printer "$PRINTER_ROOT")"
  shell_tab="$(create_tab "$primary" plain-shell)"
  agent_tab="$(create_tab "$primary" agent)"
  create_tab "$primary" spare-one >/dev/null
  create_tab "$primary" spare-two >/dev/null
  herdr tab create --workspace "$secondary" --cwd "$FIXTURE_ROOT" --label sibling-two --no-focus >/dev/null

  local printer_pane="" agent_pane=""
  printer_pane="$(first_pane_of_tab "$primary" "$printer_tab")"
  agent_pane="$(first_pane_of_tab "$primary" "$agent_tab")"
  [[ -n "$printer_pane" ]] || die "the printer tab reported no pane"
  [[ -n "$agent_pane" ]] || die "the agent tab reported no pane"

  # The printer prints what SC1 to SC3 click, then enough output for SC4 to
  # scroll through. `pane run` sends the text and the Enter in one call.
  herdr pane run "$printer_pane" \
    "printf '%s\n' '${FIXTURE_ROOT}/${INSIDE_FILE}' '${FIXTURE_ROOT}/${INSIDE_FOLDER}' '${OUTSIDE_ROOT}/${OUTSIDE_FILE}' '${OUTSIDE_ROOT}/${OUTSIDE_FOLDER}' '${OUTSIDE_ROOT}/${OUTSIDE_SCRIPT}'" >/dev/null
  herdr pane run "$printer_pane" \
    "for i in \$(seq 1 ${SCROLL_LINES}); do printf 'fixture scrollback line %d\n' \"\$i\"; done" >/dev/null

  # SC8 forks this one. The name is the fixture's, so teardown can tell it from
  # an agent the operator started.
  # `agent start` returns before the agent is ready, because claude asks
  # whether this folder is trusted before it opens a session, and a pane with
  # no session id is not forkable. The fixture created this directory, so the
  # fixture answers for it.
  herdr agent start "hide-paths-fixture-agent" --kind claude --pane "$agent_pane" >/dev/null 2>&1 || true
  sleep 3
  herdr pane send-keys "$agent_pane" down >/dev/null 2>&1 || true
  herdr pane send-keys "$agent_pane" enter >/dev/null 2>&1 || true
  local waited=0
  while (( waited < 60 )); do
    [[ -z "$(fixture_agent_session)" ]] || break
    sleep 2
    waited=$((waited + 2))
  done
  [[ -n "$(fixture_agent_session)" ]] \
    || print -u2 -- "fixture: the claude agent has no session id yet; SC8 cannot be driven"

  print -- "primary workspace:   ${primary}"
  print -- "sibling workspace:   ${secondary}"
  print -- "printer pane:        ${printer_pane}"
  print -- "plain shell tab:     ${shell_tab}"
  print -- "agent pane:          ${agent_pane}"
  cmd_paths
  cmd_status
}

cmd_status() {
  require_socket
  local primary="$(workspace_id_by_label "$PRIMARY_LABEL")"
  local secondary="$(workspace_id_by_label "$SECONDARY_LABEL")"
  [[ -n "$primary" ]] || die "no ${PRIMARY_LABEL} workspace; run setup first"
  print -- "--- ${PRIMARY_LABEL} (${primary})"
  herdr tab list --workspace "$primary"
  [[ -z "$secondary" ]] || {
    print -- "--- ${SECONDARY_LABEL} (${secondary})"
    herdr tab list --workspace "$secondary"
  }
  print -- "--- execution marker"
  if [[ -e "${OUTSIDE_ROOT}/${OUTSIDE_MARKER}" ]]; then
    print -- "PRESENT: something ran the outside script"
  else
    print -- "absent: the outside script was never executed"
  fi
}

cmd_teardown() {
  require_socket
  local before="" after="" closed=""
  local -a recorded=()
  if [[ -f "$STATE_FILE" ]]; then
    while IFS= read -r line; do
      [[ -z "$line" ]] || recorded+=("$line")
    done < "$STATE_FILE"
  fi
  if (( ${#recorded} == 0 )); then
    for label in "$PRIMARY_LABEL" "$SECONDARY_LABEL"; do
      local found="$(workspace_id_by_label "$label")"
      [[ -z "$found" ]] || recorded+=("$found")
    done
  fi
  (( ${#recorded} > 0 )) || die "no fixture workspace recorded or listed; nothing to close"

  before="$(herdr workspace list | python3 -c 'import json,sys; print(len(json.load(sys.stdin)["result"]["workspaces"]))')"
  closed=0
  for workspace in "${recorded[@]}"; do
    # A workspace Herdr already dropped with its last pane is not an error.
    if herdr workspace close "$workspace" >/dev/null 2>&1; then
      closed=$((closed + 1))
    else
      print -- "already gone: ${workspace}"
    fi
  done
  after="$(herdr workspace list | python3 -c 'import json,sys; print(len(json.load(sys.stdin)["result"]["workspaces"]))')"
  rm -f "$STATE_FILE"
  rm -rf "$FIXTURE_ROOT" "$OUTSIDE_ROOT" "$PRINTER_ROOT"
  print -- "closed ${closed} of ${#recorded} recorded workspaces; workspaces ${before} -> ${after}"
}

case "${1:-}" in
  setup) cmd_setup ;;
  status) cmd_status ;;
  paths) cmd_paths ;;
  teardown) cmd_teardown ;;
  *) die "usage: $0 <setup|status|paths|teardown>" ;;
esac
