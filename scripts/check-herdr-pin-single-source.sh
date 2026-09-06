#!/bin/zsh
# Fails when the pinned Herdr version or digest is restated anywhere outside
# macos/Sources/HerdrMacOS/Resources/herdr-bundle.json. The drift this prevents already happened
# once: 002c2f4 changed one site and dcebff5 had to chase the other three.
set -euo pipefail

export LC_ALL=en_US.UTF-8
export LANG=en_US.UTF-8

script_dir=${0:A:h}
project_root=${script_dir:h}
manifest=$project_root/macos/Sources/HerdrMacOS/Resources/herdr-bundle.json

[[ -f "$manifest" ]] || {
  print -u2 -- "error: pinned Herdr runtime manifest is missing: $manifest"
  exit 1
}

repo=$(jq -er '.repo | strings | select(length > 0)' "$manifest")
version=$(jq -er '.version' "$manifest")
sha256=$(jq -er '.sha256' "$manifest")

derived_sources=(
  scripts/build-app.sh
  scripts/fetch-herdr-runtime.sh
  macos/scripts/build_dev_app.sh
  macos/Sources/HerdrMacOS/RuntimeEnvironment.swift
)

failed=0
for relative in $derived_sources; do
  # Not `path`: in zsh that name is tied to $PATH and assigning it wipes the
  # command search path for the rest of the script.
  source_path=$project_root/$relative
  [[ -f "$source_path" ]] || {
    print -u2 -- "error: derived source is missing: $relative"
    failed=1
    continue
  }
  if grep -Fq -- "$version" "$source_path"; then
    print -u2 -- "error: $relative restates the pinned version $version; read it from the manifest"
    failed=1
  fi
  if grep -Fq -- "$repo" "$source_path"; then
    print -u2 -- "error: $relative restates the pinned repository; read it from the manifest"
    failed=1
  fi
  if grep -Fq -- "$sha256" "$source_path"; then
    print -u2 -- "error: $relative restates the pinned digest; read it from the manifest"
    failed=1
  fi
done

(( failed == 0 )) || exit 1

jq -n \
  --arg version "$version" \
  --arg manifest "$manifest" \
  --argjson checked "${#derived_sources[@]}" \
  '{status: "pass", pin: $version, manifest: $manifest, derived_sources_checked: $checked}'
