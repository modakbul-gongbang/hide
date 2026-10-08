#!/usr/bin/env bash
# Refuses a device measurement that could reach an operator's account, before
# anything dials the device; run.sh and device-herdr.sh source it.
#
# hided installs its node with folders it spells `~/` from the SFTP home,
# which is the account's own home even where a private sshd gives its
# commands another HOME: on 2026-10-09 a probe with the `~/` defaults
# uploaded its node into a real account's ~/.hide/host-helper. So the alias
# in MEASURE_DEVICE_SSH_CONFIG must resolve to the private sshd's port, named
# in MEASURE_DEVICE_PORT and never 22; the private HOME (MEASURE_DEVICE_HOME)
# lives under /tmp, where no account's own home does; the consent folders
# are absolute and inside it; and the device's Herdr socket is named under
# /tmp, so nothing asks for the account's default Herdr.

device_guard_refuse() { echo "device guard: $*" >&2; exit 2; }

# An absolute path strictly inside a folder, with no empty, `.` or `..` part.
device_guard_inside() {
  local path=$1 folder=$2 rest part
  [[ "$path" != *[[:cntrl:]]* && "$path" == "$folder"/?* ]] || return 1
  rest=${path#"$folder"/}
  IFS=/ read -r -a parts <<< "$rest"
  [[ "$rest" != */ ]] || return 1
  for part in "${parts[@]}"; do
    [[ -n "$part" && "$part" != . && "$part" != .. ]] || return 1
  done
}

device_guard_under_tmp() {
  device_guard_inside "$1" /tmp || device_guard_inside "$1" /private/tmp
}

for name in MEASURE_DEVICE_ALIAS MEASURE_DEVICE_SSH_CONFIG MEASURE_DEVICE_PORT MEASURE_DEVICE_HOME MEASURE_DEVICE_SOCKET MEASURE_DEVICE_HELPER_ROOT MEASURE_DEVICE_CLI_DIR; do
  [[ -n "${!name:-}" ]] || device_guard_refuse "the device scenario needs $name"
done
[[ "$MEASURE_DEVICE_PORT" =~ ^[0-9]+$ ]] || device_guard_refuse "MEASURE_DEVICE_PORT $MEASURE_DEVICE_PORT is not a port"
(( 10#$MEASURE_DEVICE_PORT != 22 )) || device_guard_refuse "MEASURE_DEVICE_PORT is 22, an account's own sshd"
device_guard_under_tmp "$MEASURE_DEVICE_HOME" || device_guard_refuse "MEASURE_DEVICE_HOME $MEASURE_DEVICE_HOME is not a private path under /tmp"
for name in MEASURE_DEVICE_HELPER_ROOT MEASURE_DEVICE_CLI_DIR; do
  device_guard_inside "${!name}" "$MEASURE_DEVICE_HOME" || device_guard_refuse "$name ${!name} is not an absolute folder inside MEASURE_DEVICE_HOME"
done
device_guard_under_tmp "$MEASURE_DEVICE_SOCKET" || device_guard_refuse "MEASURE_DEVICE_SOCKET $MEASURE_DEVICE_SOCKET is not a private path under /tmp"
# `ssh -G` prints the resolved configuration without connecting.
device_guard_port="$(ssh -G -F "$MEASURE_DEVICE_SSH_CONFIG" "$MEASURE_DEVICE_ALIAS" 2>/dev/null | awk '$1 == "port" { print $2; exit }')" || true
[[ "$device_guard_port" == "$((10#$MEASURE_DEVICE_PORT))" ]] || device_guard_refuse "SSH alias $MEASURE_DEVICE_ALIAS resolves to port ${device_guard_port:-none}, not the private sshd's $MEASURE_DEVICE_PORT"
