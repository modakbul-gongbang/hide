#!/bin/sh
# Runtime smoke test for a Herdr-managed pane. Reports display metadata through
# the same surface the watcher uses, then proves the settings actions are
# idempotent by invoking each of them twice.
set -eu

source_id="hide.agent-context-labels-runtime-test"
pane_id="${HERDR_PANE_ID:?run inside a Herdr-managed pane}"
settings_path="$HOME/.local/state/hide.agent-context-labels/settings.json"

cleanup() {
  herdr pane report-metadata "$pane_id" --source "$source_id" \
    --clear-token summary \
    --clear-token status_question \
    --clear-token elapsed \
    --clear-token agent_codex >/dev/null 2>&1 || true
}
trap cleanup EXIT INT TERM

herdr plugin list --plugin hide.agent-context-labels --json |
  rg '"startup"|"refresh-active-pane-summary"|"enable-automatic-summaries"|"disable-automatic-summaries"'

herdr pane report-metadata "$pane_id" --source "$source_id" \
  --token summary=fixture \
  --token 'status_question=?' \
  --token elapsed=7s \
  --token agent_codex=codex
herdr pane get "$pane_id" | rg '"summary":"fixture"|"status_question":"\?"|"elapsed":"7s"|"agent_codex":"codex"'

# The same action applied twice must leave the same state.
herdr plugin action invoke disable-automatic-summaries --plugin hide.agent-context-labels >/dev/null
herdr plugin action invoke disable-automatic-summaries --plugin hide.agent-context-labels >/dev/null
sleep 1
rg '"automatic_summaries":false' "$settings_path"

herdr plugin action invoke enable-automatic-summaries --plugin hide.agent-context-labels >/dev/null
herdr plugin action invoke enable-automatic-summaries --plugin hide.agent-context-labels >/dev/null
sleep 1
rg '"automatic_summaries":true' "$settings_path"

echo runtime-verification-pass
