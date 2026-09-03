#!/usr/bin/env zsh
# Stands up the throwaway Herdr workspace the tab-strip verification runs against,
# and tears it down again.
#
# The fixture is one workspace, labelled below, holding three Herdr tabs in a
# checkout that is its own git repository, plus a file the shell can open as a
# file tab. It also drives the move-refusal path, which has no CLI command:
# `herdr tab move` does not exist, so the request goes over the socket directly.
#
# Every workspace, tab and pane this script touches is one it created. It
# refuses to run against a fixture that already exists rather than creating a
# second one, and teardown closes exactly the workspace whose id it recorded.
#
# `setup --split` builds the same workspace with its tabs split across two
# checkouts: the workspace's own first tab sits in a linked worktree of the
# fixture repository, and the three tabs after it sit in the repository. That
# is the arrangement where the strip a drag happens in does not start at the
# workspace's first tab, which is what makes the insertion index a workspace
# index rather than a strip index.
#
# Usage:
#   zsh scripts/tab-verification-fixture.sh setup [--split]
#   zsh scripts/tab-verification-fixture.sh status
#   zsh scripts/tab-verification-fixture.sh refuse [tab-id]
#   zsh scripts/tab-verification-fixture.sh teardown

set -eu

FIXTURE_LABEL="tab-verify-fixture"
FIXTURE_ROOT="/tmp/herdr-ide-verify/fixtures/tab-verify"
# A sibling directory, not one inside the repository, so the worktree is a
# second checkout of the project rather than a path within the first.
FIXTURE_WORKTREE="/tmp/herdr-ide-verify/fixtures/tab-verify-feature"
FIXTURE_FILE="notes.md"
STATE_FILE="${FIXTURE_ROOT}/.fixture-workspace-id"
SOCKET_PATH="${HERDR_SOCKET_PATH:-${HOME}/.config/herdr/herdr.sock}"

die() {
  print -u2 -- "fixture: $*"
  exit 1
}

# One request/response over the Herdr socket, newline-delimited JSON.
# Used only where the CLI has no equivalent command.
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

require_socket() {
  [[ -S "$SOCKET_PATH" ]] || die "no Herdr socket at ${SOCKET_PATH}"
}

cmd_setup() {
  require_socket
  local split="no"
  [[ "${1:-}" != "--split" ]] || split="yes"
  local existing
  existing="$(workspace_id_by_label)"
  # Rule 11: a second run must not stand up a second workspace.
  [[ -z "$existing" ]] || die "workspace ${FIXTURE_LABEL} already exists as ${existing}; run teardown first"

  mkdir -p "$FIXTURE_ROOT"
  # The checkout must be its own git repository, or the catalog keys it under
  # whichever repository encloses the directory instead.
  [[ -d "${FIXTURE_ROOT}/.git" ]] || git -C "$FIXTURE_ROOT" init -q
  printf '# tab verification fixture\n\nA file the shell opens as a file tab.\n' > "${FIXTURE_ROOT}/${FIXTURE_FILE}"
  git -C "$FIXTURE_ROOT" add "$FIXTURE_FILE" >/dev/null
  git -C "$FIXTURE_ROOT" -c user.email=fixture@local -c user.name=fixture commit -qm 'Add the fixture file' >/dev/null 2>&1 || true

  # The workspace's first tab decides where the split checkout's tab sits in
  # Herdr's list, and Herdr appends every tab after it, so the worktree has to
  # be created and named as the workspace's cwd before the other three tabs.
  local first_cwd="$FIXTURE_ROOT"
  if [[ "$split" == "yes" ]]; then
    rm -rf "$FIXTURE_WORKTREE"
    git -C "$FIXTURE_ROOT" worktree add -q -b tab-verify-feature "$FIXTURE_WORKTREE" >/dev/null
    first_cwd="$FIXTURE_WORKTREE"
  fi

  herdr workspace create --cwd "$first_cwd" --label "$FIXTURE_LABEL" --no-focus >/dev/null
  local workspace
  workspace="$(workspace_id_by_label)"
  [[ -n "$workspace" ]] || die "workspace create reported no ${FIXTURE_LABEL} workspace"
  printf '%s\n' "$workspace" > "$STATE_FILE"

  herdr tab create --workspace "$workspace" --cwd "$FIXTURE_ROOT" --label two --no-focus >/dev/null
  herdr tab create --workspace "$workspace" --cwd "$FIXTURE_ROOT" --label three --no-focus >/dev/null
  [[ "$split" != "yes" ]] || \
    herdr tab create --workspace "$workspace" --cwd "$FIXTURE_ROOT" --label four --no-focus >/dev/null

  print -- "workspace: ${workspace}"
  print -- "root:      ${FIXTURE_ROOT}"
  print -- "file:      ${FIXTURE_ROOT}/${FIXTURE_FILE}"
  [[ "$split" != "yes" ]] || print -- "worktree:  ${FIXTURE_WORKTREE} (holds the workspace's first tab)"
  cmd_status
}

cmd_status() {
  require_socket
  local workspace
  workspace="$(workspace_id_by_label)"
  [[ -n "$workspace" ]] || die "no ${FIXTURE_LABEL} workspace; run setup first"
  herdr tab list --workspace "$workspace"
}

# The refusal path. `tab.move` has no CLI command, so the request goes over the
# socket. With no argument it names a tab id no workspace holds, which is the
# refusal the acceptance criterion names; pass a tab id to refuse a real tab a
# different way.
cmd_refuse() {
  require_socket
  local workspace target
  workspace="$(workspace_id_by_label)"
  [[ -n "$workspace" ]] || die "no ${FIXTURE_LABEL} workspace; run setup first"
  target="${1:-${workspace}:tZZ}"
  print -- "requesting tab.move for a tab Herdr does not hold: ${target}"
  socket_call "tab.move" "{\"tab_id\":\"${target}\",\"insert_index\":0}"
  print -- "tab order after the refused request:"
  herdr tab list --workspace "$workspace"
}

cmd_teardown() {
  require_socket
  local workspace before after
  workspace="$(cat "$STATE_FILE" 2>/dev/null || true)"
  [[ -n "$workspace" ]] || workspace="$(workspace_id_by_label)"
  [[ -n "$workspace" ]] || die "no ${FIXTURE_LABEL} workspace recorded or listed; nothing to close"

  before="$(herdr workspace list | python3 -c 'import json,sys; print(len(json.load(sys.stdin)["result"]["workspaces"]))')"
  herdr workspace close "$workspace" >/dev/null
  after="$(herdr workspace list | python3 -c 'import json,sys; print(len(json.load(sys.stdin)["result"]["workspaces"]))')"
  rm -f "$STATE_FILE"
  # The worktree is registered inside the repository, so it goes before the
  # repository does; a `setup --split` that left one behind would make the next
  # `git worktree add` refuse the same path.
  [[ ! -d "$FIXTURE_WORKTREE" ]] || \
    git -C "$FIXTURE_ROOT" worktree remove --force "$FIXTURE_WORKTREE" >/dev/null 2>&1 || true
  rm -rf "$FIXTURE_WORKTREE"
  rm -rf "$FIXTURE_ROOT"
  print -- "closed ${workspace}; workspaces ${before} -> ${after}"
  [[ "$after" -eq $((before - 1)) ]] || die "expected exactly one workspace to close"
}

case "${1:-}" in
  setup) shift; cmd_setup "$@" ;;
  status) cmd_status ;;
  refuse) shift; cmd_refuse "$@" ;;
  teardown) cmd_teardown ;;
  *) die "usage: $0 <setup|status|refuse|teardown>" ;;
esac
