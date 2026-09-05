#!/bin/zsh
set -euo pipefail

export LC_ALL=en_US.UTF-8
export LC_CTYPE=en_US.UTF-8
export LANG=en_US.UTF-8

usage() {
  print -u2 -- "usage: $0 [--herdr-bin PATH] [--schema-only]"
  print -u2 -- "       --schema-only compares the CLI schema against the contract and skips"
  print -u2 -- "       every check that needs a running server, so it runs on a bare machine"
}

herdr_bin=""
schema_only=false
while (( $# > 0 )); do
  case "$1" in
    --herdr-bin)
      (( $# >= 2 )) || { usage; exit 2; }
      herdr_bin=$2
      shift 2
      ;;
    --schema-only)
      schema_only=true
      shift
      ;;
    *)
      usage
      exit 2
      ;;
  esac
done

if [[ -z "$herdr_bin" ]]; then
  herdr_bin=$(command -v herdr || true)
fi
[[ -n "$herdr_bin" && -x "$herdr_bin" ]] || {
  print -u2 -- "error: executable Herdr CLI was not found"
  exit 1
}

script_dir=${0:A:h}
project_root=${script_dir:h}
contract_schema=$project_root/contracts/herdr-api.schema.json
[[ -f "$contract_schema" ]] || {
  print -u2 -- "error: bundled Herdr API contract is missing: $contract_schema"
  exit 1
}

temporary_root=$(mktemp -d "${TMPDIR:-/tmp}/herdr-contract-check.XXXXXX")
cleanup() {
  rm -rf -- "$temporary_root"
}
trap cleanup EXIT

contract_normalized=$temporary_root/contract.json
cli_normalized=$temporary_root/cli.json
jq -S . "$contract_schema" > "$contract_normalized"
"$herdr_bin" api schema --json | jq -S . > "$cli_normalized"

contract_protocol=$(jq -er '.protocol' "$contract_normalized")
cli_protocol=$(jq -er '.protocol' "$cli_normalized")
if ! cmp -s "$contract_normalized" "$cli_normalized"; then
  print -u2 -- "error: installed Herdr CLI schema does not match the IDE contract"
  print -u2 -- "expected_protocol=$contract_protocol received_protocol=$cli_protocol cli=$herdr_bin"
  exit 1
fi

cli_version=$("$herdr_bin" --version | awk '{print $2}')
schema_sha256=$(/usr/bin/shasum -a 256 "$contract_normalized" | awk '{print $1}')

# `herdr api schema --json` is answered by the binary itself, so the comparison
# above is the whole of what a machine with no Herdr session can verify. The
# remaining checks read live server state and are a local step, not a CI one.
if [[ "$schema_only" == true ]]; then
  jq -n \
    --arg herdr_bin "$herdr_bin" \
    --arg version "$cli_version" \
    --arg schema_sha256 "$schema_sha256" \
    --argjson protocol "$contract_protocol" \
    '{status: "pass", scope: "schema-only", herdr_bin: $herdr_bin, version: $version,
      protocol: $protocol, schema_sha256: $schema_sha256}'
  exit 0
fi

snapshot=$("$herdr_bin" api snapshot)
server_protocol=$(print -r -- "$snapshot" | jq -er '.result.snapshot.protocol')
if [[ "$server_protocol" != "$contract_protocol" ]]; then
  print -u2 -- "error: running Herdr server protocol does not match the IDE contract"
  print -u2 -- "expected_protocol=$contract_protocol received_protocol=$server_protocol"
  exit 1
fi

server_status=$("$herdr_bin" status server)
server_version=$(print -r -- "$server_status" | awk '/^version:/ {print $2}')
if [[ -z "$server_version" || "$server_version" != "$cli_version" ]]; then
  print -u2 -- "error: installed Herdr CLI and running server versions differ"
  print -u2 -- "cli_version=$cli_version server_version=${server_version:-unknown}"
  exit 1
fi

workspace_count=$(print -r -- "$snapshot" | jq -er '.result.snapshot.workspaces | length')
pane_count=$(print -r -- "$snapshot" | jq -er '.result.snapshot.panes | length')
jq -n \
  --arg herdr_bin "$herdr_bin" \
  --arg version "$cli_version" \
  --arg schema_sha256 "$schema_sha256" \
  --argjson protocol "$contract_protocol" \
  --argjson workspaces "$workspace_count" \
  --argjson panes "$pane_count" \
  '{status: "pass", scope: "full", herdr_bin: $herdr_bin, version: $version, protocol: $protocol, schema_sha256: $schema_sha256, workspaces: $workspaces, panes: $panes}'
