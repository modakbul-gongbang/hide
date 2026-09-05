#!/bin/zsh
# Moves the Herdr pin to a new release: downloads the asset, refuses to write
# anything unless the binary reports the version asked for, then updates the
# manifest and every document that quotes the pin. Re-running it for the
# version already pinned converges and writes nothing.
set -euo pipefail

export LC_ALL=en_US.UTF-8
export LC_CTYPE=en_US.UTF-8
export LANG=en_US.UTF-8

usage() {
  print -u2 -- "usage: $0 <version> [--dry-run] [--keep-binary PATH]"
  print -u2 -- "       version is the release number without a leading v, e.g. 0.8.3"
  print -u2 -- "       --keep-binary saves the verified asset so a caller need not"
  print -u2 -- "       download and re-verify it"
}

target_version=""
dry_run=false
keep_binary=""
while (( $# > 0 )); do
  case "$1" in
    --dry-run)
      dry_run=true
      shift
      ;;
    --keep-binary)
      (( $# >= 2 )) || { usage; exit 2; }
      keep_binary=$2
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    -*)
      usage
      exit 2
      ;;
    *)
      [[ -z "$target_version" ]] || { usage; exit 2; }
      target_version=${1#v}
      shift
      ;;
  esac
done

[[ -n "$target_version" ]] || { usage; exit 2; }
[[ "$target_version" =~ '^[0-9]+(\.[0-9]+)+$' ]] || {
  print -u2 -- "error: version must be dotted digits without a leading v: $target_version"
  exit 2
}

script_dir=${0:A:h}
project_root=${script_dir:h}
manifest=$project_root/macos/Sources/HerdrMacOS/Resources/herdr-bundle.json
[[ -f "$manifest" ]] || {
  print -u2 -- "error: pinned Herdr runtime manifest is missing: $manifest"
  exit 1
}

current_version=$(jq -er '.version' "$manifest")
current_sha256=$(jq -er '.sha256' "$manifest")
target_url="https://github.com/herdrdev/herdr/releases/download/v${target_version}/herdr-macos-aarch64"

temporary_root=$(mktemp -d "${TMPDIR:-/tmp}/herdr-bump.XXXXXX")
cleanup() {
  rm -rf -- "$temporary_root"
}
trap cleanup EXIT
downloaded=$temporary_root/herdr

/usr/bin/curl --fail --location --silent --show-error "$target_url" --output "$downloaded" || {
  print -u2 -- "error: release asset could not be downloaded: $target_url"
  exit 1
}
chmod 755 "$downloaded"

# The asset is trusted only after it says who it is. A release whose binary
# reports a different version is a mislabelled upload, not a bump.
reported_version=$("$downloaded" --version | /usr/bin/awk '{print $NF}')
[[ "$reported_version" == "$target_version" ]] || {
  print -u2 -- "error: downloaded binary reports $reported_version, not $target_version"
  exit 1
}
target_sha256=$(/usr/bin/shasum -a 256 "$downloaded" | /usr/bin/awk '{print $1}')

outcome=""
if [[ "$current_version" == "$target_version" ]]; then
  if [[ "$current_sha256" != "$target_sha256" ]]; then
    print -u2 -- "error: v$target_version is already pinned but its asset digest changed"
    print -u2 -- "pinned_sha256=$current_sha256 downloaded_sha256=$target_sha256"
    print -u2 -- "the upstream release was replaced; confirm the new asset before repinning"
    exit 1
  fi
  outcome=unchanged
elif [[ "$dry_run" == true ]]; then
  outcome=planned
else
  outcome=bumped
fi

# Documents quote the pin inside prose and inside a licence notice. Only the
# version and digest tokens are substituted; the sentences around them are not
# generated and must survive untouched.
derived_documents=(
  README.md
  docs/INSTALL.md
  macos/Resources/THIRD_PARTY_NOTICES/herdr-APACHE-2.0.txt
)

replacements_json=$temporary_root/replacements.json
print -r -- '[]' > "$replacements_json"
[[ "$outcome" == unchanged ]] || for relative in $derived_documents; do
  document=$project_root/$relative
  [[ -f "$document" ]] || {
    print -u2 -- "error: document quoting the pin is missing: $relative"
    exit 1
  }
  version_hits=$(grep -Fc -- "$current_version" "$document" || true)
  digest_hits=$(grep -Fc -- "$current_sha256" "$document" || true)
  if (( version_hits == 0 && digest_hits == 0 )); then
    print -u2 -- "error: $relative quotes neither the pinned version nor its digest"
    print -u2 -- "the document moved away from the pin; update the script's document list"
    exit 1
  fi
  if [[ "$dry_run" == false ]]; then
    /usr/bin/sed -i '' \
      -e "s/${current_version}/${target_version}/g" \
      -e "s/${current_sha256}/${target_sha256}/g" \
      "$document"
  fi
  jq --arg document "$relative" \
     --argjson version_lines "$version_hits" \
     --argjson digest_lines "$digest_hits" \
     '. + [{document: $document, version_lines: $version_lines, digest_lines: $digest_lines}]' \
     "$replacements_json" > "$replacements_json.next"
  mv "$replacements_json.next" "$replacements_json"
done

if [[ "$outcome" == bumped ]]; then
  jq --arg version "$target_version" \
     --arg source_url "$target_url" \
     --arg sha256 "$target_sha256" \
     '.version = $version | .source_url = $source_url | .sha256 = $sha256' \
     "$manifest" > "$manifest.next"
  mv "$manifest.next" "$manifest"
fi

# The verified binary is handed over last, so a failure anywhere above leaves
# nothing behind at the caller's path.
if [[ -n "$keep_binary" ]]; then
  install -m 755 "$downloaded" "$keep_binary"
fi

jq -n \
  --arg outcome "$outcome" \
  --arg from "$current_version" \
  --arg version "$target_version" \
  --arg sha256 "$target_sha256" \
  --arg source_url "$target_url" \
  --argjson documents "$(cat "$replacements_json")" \
  '{status: "pass", outcome: $outcome, from: $from, version: $version,
    sha256: $sha256, source_url: $source_url, documents: $documents}'
