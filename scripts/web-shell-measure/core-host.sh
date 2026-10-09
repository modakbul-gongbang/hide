#!/usr/bin/env bash
# The core of a remote-core measurement (PRD core-host-node-remote-core B5-B7,
# B21): a core-role hided on the measured device, in the private HOME the
# device guard admits, on the device's isolated Herdr. Usage:
#   core-host.sh start <hided to upload>   upload the build, start it, wait for its attach socket
#   core-host.sh stop                      end only the hided this script started
# Reads the same MEASURE_DEVICE_* parts as the device scenario and
# MEASURE_CORE_PROGRAM, MEASURE_CORE_STATE and MEASURE_CORE_NODE
# (device-guard.sh refuses anything outside the private HOME first). The
# core is started with MEASURE_CORE_NODE as its node id, so nothing it
# writes names the device's own machine, and with no Tailscale, so it opens
# no listener beyond its loopback port.
set -euo pipefail
export MEASURE_SCENARIO=remote-core
source "$(dirname "$0")/device-guard.sh"
q() { printf %q "$1"; }
on_device() { ssh "${device_guard_ssh[@]}" -- "$MEASURE_DEVICE_ALIAS" "$1"; }
pid_file="$MEASURE_CORE_STATE.pid"
log_file="$MEASURE_CORE_STATE.log"
# The pid file names this script's core only while that process still runs
# the recorded program; anything else is left alone.
ours="pid=\$(cat $(q "$pid_file") 2>/dev/null) && [ \"\$(ps -p \"\$pid\" -o command= 2>/dev/null)\" = $(q "$MEASURE_CORE_PROGRAM") ]"

case "${1:-}" in
  start)
    local_bin=${2:-}
    [[ -x "$local_bin" ]] || { echo "usage: core-host.sh start <hided>" >&2; exit 2; }
    on_device "if $ours; then echo 'a core this script started still runs' >&2; exit 2; fi"
    on_device "mkdir -p $(q "$(dirname "$MEASURE_CORE_PROGRAM")") && cat > $(q "$MEASURE_CORE_PROGRAM.upload") && chmod 755 $(q "$MEASURE_CORE_PROGRAM.upload") && mv -f $(q "$MEASURE_CORE_PROGRAM.upload") $(q "$MEASURE_CORE_PROGRAM")" < "$local_bin"
    on_device "rm -rf $(q "$MEASURE_CORE_STATE") && mkdir -p -m 700 $(q "$MEASURE_CORE_STATE") && \
      nohup env HOME=$(q "$MEASURE_DEVICE_HOME") PATH=$(q "$MEASURE_DEVICE_HOME/.local/bin"):/usr/bin:/bin:/usr/sbin:/sbin SHELL=/bin/zsh \
        XDG_CONFIG_HOME=$(q "$MEASURE_DEVICE_HOME/.config") XDG_STATE_HOME=$(q "$MEASURE_DEVICE_HOME/.state") XDG_DATA_HOME=$(q "$MEASURE_DEVICE_HOME/.data") \
        HERDR_SOCKET_PATH=$(q "$MEASURE_DEVICE_SOCKET") HERDR_BIN_PATH=$(q "$MEASURE_DEVICE_HOME/.local/bin/herdr") \
        HIDE_STATE_DIR=$(q "$MEASURE_CORE_STATE") HIDE_MACHINE_ID=$(q "$MEASURE_CORE_NODE") HIDE_KEEP_ALIVE=1 HIDE_PORT=0 \
        HIDE_OPEN_COMMAND=/usr/bin/true HIDE_TAILSCALE_BIN=$(q "$MEASURE_DEVICE_HOME/absent-tailscale") \
        $(q "$MEASURE_CORE_PROGRAM") </dev/null >$(q "$log_file") 2>&1 & echo \$! > $(q "$pid_file")"
    deadline=$((SECONDS + 60))
    until on_device "test -S $(q "$MEASURE_CORE_STATE/node-attach-socket") || test -e $(q "$MEASURE_CORE_STATE/node-attach-socket")"; do
      if (( SECONDS >= deadline )) || ! on_device "$ours"; then
        on_device "tail -20 $(q "$log_file")" >&2 || true
        echo "the core did not open its attach socket" >&2
        exit 1
      fi
      sleep 1
    done
    on_device "cat $(q "$pid_file")"
    ;;
  stop)
    on_device "if $ours; then kill \"\$pid\"; for _ in \$(seq 50); do ps -p \"\$pid\" >/dev/null || break; sleep 0.1; done; ps -p \"\$pid\" >/dev/null && { echo \"core \$pid did not end\" >&2; exit 1; }; rm -f $(q "$pid_file"); echo stopped; else echo 'no core of this script runs'; fi"
    ;;
  *)
    echo "usage: core-host.sh start <hided>|stop" >&2
    exit 2
    ;;
esac
