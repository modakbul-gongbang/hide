#!/usr/bin/env bash
# Runs the measured device's isolated Herdr CLI over its private sshd:
# `device-herdr.sh <herdr arguments...>`. The binary is the pinned Herdr in
# the private HOME's ~/.local/bin, on the socket MEASURE_DEVICE_SOCKET names.
set -euo pipefail
source "$(dirname "$0")/device-guard.sh"
remote="HERDR_SOCKET_PATH=$(printf %q "$MEASURE_DEVICE_SOCKET") $(printf %q "$MEASURE_DEVICE_HOME/.local/bin/herdr")"
(( $# == 0 )) || remote+=" $(printf '%q ' "$@")"
exec ssh "${device_guard_ssh[@]}" -- "$MEASURE_DEVICE_ALIAS" "$remote"
