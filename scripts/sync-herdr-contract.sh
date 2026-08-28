#!/bin/zsh
set -euo pipefail

export LC_ALL=en_US.UTF-8
export LC_CTYPE=en_US.UTF-8
export LANG=en_US.UTF-8

usage() {
  print -u2 -- "usage: $0 --herdr-root PATH"
}

herdr_root=""
while (( $# > 0 )); do
  case "$1" in
    --herdr-root)
      (( $# >= 2 )) || { usage; exit 2; }
      herdr_root=$2
      shift 2
      ;;
    *)
      usage
      exit 2
      ;;
  esac
done

[[ -n "$herdr_root" ]] || { usage; exit 2; }

script_dir=${0:A:h}
project_root=${script_dir:h}
source_schema=${herdr_root:A}/docs/next/api/herdr-api.schema.json
target_schema=$project_root/contracts/herdr-api.schema.json

[[ -f "$source_schema" ]] || {
  print -u2 -- "error: canonical Herdr API schema is missing: $source_schema"
  exit 1
}

protocol=$(jq -er '.protocol | select(type == "number" and . > 0)' "$source_schema")
schema_version=$(jq -er '.schema_version | select(type == "number" and . > 0)' "$source_schema")

temporary_schema=$(mktemp "${TMPDIR:-/tmp}/herdr-api.schema.XXXXXX")
cleanup() {
  rm -f -- "$temporary_schema"
}
trap cleanup EXIT

cp "$source_schema" "$temporary_schema"
mkdir -p "$project_root/contracts"
if [[ -f "$target_schema" ]] && cmp -s "$temporary_schema" "$target_schema"; then
  sync_status=unchanged
else
  mv "$temporary_schema" "$target_schema"
  temporary_schema=""
  sync_status=updated
fi

schema_sha256=$(/usr/bin/shasum -a 256 "$target_schema" | awk '{print $1}')
jq -n \
  --arg status "$sync_status" \
  --arg path "$target_schema" \
  --arg sha256 "$schema_sha256" \
  --argjson protocol "$protocol" \
  --argjson schema_version "$schema_version" \
  '{status: $status, contract: $path, protocol: $protocol, schema_version: $schema_version, sha256: $sha256}'
