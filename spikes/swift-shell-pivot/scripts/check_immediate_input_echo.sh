#!/usr/bin/env bash
set -euo pipefail

evidence_path="${1:?usage: check_immediate_input_echo.sh <evidence.json>}"

jq -e '
  .window_is_key == true and
  .first_responder_is_terminal == true and
  .terminal_accepts_first_responder == true and
  .terminal_become_first_responder_result == true and
  .app_key_down_events > 0 and
  .terminal_key_down_events > 0 and
  .swiftterm_delegate_events > 0 and
  .transcript_contains_immediate_echo_probe == true
' "$evidence_path" >/dev/null
