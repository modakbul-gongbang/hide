#!/bin/zsh
# Moves the Herdr pin to a release tag: downloads the asset, takes the version
# the binary reports as the pinned version, writes the API schema that same
# binary reports as the contract, then updates the manifest and every document
# that quotes the pin. Nothing is written unless the asset verified.
# Re-running it for the tag already pinned converges and writes nothing.
set -euo pipefail

export LC_ALL=en_US.UTF-8
export LC_CTYPE=en_US.UTF-8
export LANG=en_US.UTF-8

usage() {
  print -u2 -- "usage: $0 <release-tag> [--repo OWNER/NAME] [--dry-run] [--keep-binary PATH]"
  print -u2 -- "       release-tag is the GitHub release tag, e.g. v0.8.3 or"
  print -u2 -- "       preview-2026-08-31-b1ff4582e968; a bare 0.8.3 means v0.8.3"
  print -u2 -- "       --keep-binary saves the verified asset so a caller need not"
  print -u2 -- "       download and re-verify it"
}

target_tag=""
target_repo=""
dry_run=false
keep_binary=""
while (( $# > 0 )); do
  case "$1" in
    --repo)
      (( $# >= 2 )) || { usage; exit 2; }
      [[ -n "$2" ]] || { usage; exit 2; }
      target_repo=$2
      shift 2
      ;;
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
      [[ -z "$target_tag" ]] || { usage; exit 2; }
      target_tag=$1
      shift
      ;;
  esac
done

[[ -n "$target_tag" ]] || { usage; exit 2; }
if [[ "$target_tag" =~ '^[0-9]+(\.[0-9]+)+$' ]]; then
  target_tag=v$target_tag
fi
[[ "$target_tag" =~ '^[A-Za-z0-9][A-Za-z0-9._-]*$' ]] || {
  print -u2 -- "error: release tag has characters a GitHub tag cannot carry: $target_tag"
  exit 2
}

script_dir=${0:A:h}
project_root=${script_dir:h}
manifest=$project_root/macos/Sources/HerdrMacOS/Resources/herdr-bundle.json
contract=$project_root/contracts/herdr-api.schema.json
[[ -f "$manifest" ]] || {
  print -u2 -- "error: pinned Herdr runtime manifest is missing: $manifest"
  exit 1
}

current_repo=$(jq -er '.repo | strings | select(test("^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$"))' "$manifest")
[[ -n "$target_repo" ]] || target_repo=$current_repo
[[ "$target_repo" =~ '^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$' ]] || { print -u2 -- "error: invalid release repository: $target_repo"; exit 2; }
current_tag=$(jq -er '.tag | strings | select(test("^[A-Za-z0-9][A-Za-z0-9._-]*$"))' "$manifest")
current_version=$(jq -er '.version' "$manifest")
current_sha256=$(jq -er '.sha256' "$manifest")
target_url="https://github.com/${target_repo}/releases/download/${target_tag}/herdr-macos-aarch64"

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

# The binary names its own version; a stable tag must agree with it, and a
# preview tag carries a build id the version string repeats. An asset whose
# report contradicts its tag is a mislabelled upload, not a bump.
target_version=$("$downloaded" --version | /usr/bin/awk '{print $NF}')
[[ -n "$target_version" ]] || {
  print -u2 -- "error: downloaded binary reports no version"
  exit 1
}
case "$target_tag" in
  v*)
    [[ "$target_version" == "${target_tag#v}" ]] || {
      print -u2 -- "error: downloaded binary reports $target_version, not ${target_tag#v}"
      exit 1
    }
    ;;
  *)
    [[ "$target_version" == *"${target_tag##*-}"* ]] || {
      print -u2 -- "error: downloaded binary reports $target_version, which does not name the build in $target_tag"
      exit 1
    }
    ;;
esac
target_sha256=$(/usr/bin/shasum -a 256 "$downloaded" | /usr/bin/awk '{print $1}')

# The contract is whatever this exact binary answers; nothing is fetched from
# a second place that could disagree with the asset that ships.
target_contract=$temporary_root/herdr-api.schema.json
"$downloaded" api schema --json | jq . > "$target_contract" || {
  print -u2 -- "error: downloaded binary did not answer 'api schema --json'"
  exit 1
}
contract_protocol=$(jq -er '.protocol' "$target_contract")

outcome=""
if [[ "$current_repo" == "$target_repo" && "$current_tag" == "$target_tag" ]]; then
  if [[ "$current_sha256" != "$target_sha256" ]]; then
    print -u2 -- "error: $target_tag is already pinned but its asset digest changed"
    print -u2 -- "pinned_sha256=$current_sha256 downloaded_sha256=$target_sha256"
    print -u2 -- "the upstream release was replaced; confirm the new asset before repinning"
    exit 1
  fi
  if cmp -s <(jq -S . "$contract") <(jq -S . "$target_contract"); then
    outcome=unchanged
  elif [[ "$dry_run" == true ]]; then
    print -u2 -- "error: $target_tag is already pinned but contracts/herdr-api.schema.json is not what its binary reports"
    print -u2 -- "the contract drifted from the pin; rerun without --dry-run to regenerate it"
    exit 1
  else
    outcome=contract_regenerated
  fi
elif [[ "$dry_run" == true ]]; then
  outcome=planned
else
  outcome=bumped
fi

# Documents quote the pin inside prose and inside a licence notice. Only the
# release URL, version and digest tokens are substituted; the sentences around them
# are not generated and must survive untouched. The tag goes first because a
# stable tag contains the version, and replacing the version inside a tag
# would corrupt the release URL.
derived_documents=(
  README.md
  docs/INSTALL.md
  macos/Resources/THIRD_PARTY_NOTICES/herdr-APACHE-2.0.txt
)

replacements_json=$temporary_root/replacements.json
print -r -- '[]' > "$replacements_json"
[[ "$outcome" == unchanged || "$outcome" == contract_regenerated ]] || for relative in $derived_documents; do
  document=$project_root/$relative
  [[ -f "$document" ]] || {
    print -u2 -- "error: document quoting the pin is missing: $relative"
    exit 1
  }
  tag_hits=$(grep -Fc -- "releases/tag/$current_tag" "$document" || true)
  version_hits=$(grep -Fc -- "$current_version" "$document" || true)
  digest_hits=$(grep -Fc -- "$current_sha256" "$document" || true)
  if (( tag_hits == 0 && version_hits == 0 && digest_hits == 0 )); then
    print -u2 -- "error: $relative quotes neither the pinned tag, version nor digest"
    print -u2 -- "the document moved away from the pin; update the script's document list"
    exit 1
  fi
  if [[ "$dry_run" == false ]]; then
    /usr/bin/python3 - "$document" "$current_repo" "$target_repo" "$current_tag" "$target_tag" "$current_version" "$target_version" "$current_sha256" "$target_sha256" <<'PYDOC'
import sys
from pathlib import Path
p = Path(sys.argv[1])
old_repo, repo, old_tag, tag, old_version, version, old_digest, digest = sys.argv[2:]
text = p.read_text()
for prefix in ("releases/tag/", "releases/download/"):
    text = text.replace(f"https://github.com/{old_repo}/{prefix}{old_tag}",
                        f"https://github.com/{repo}/{prefix}{tag}")
text = text.replace(old_version, version).replace(old_digest, digest)
p.write_text(text)
PYDOC
  fi
  jq --arg document "$relative" \
     --argjson tag_lines "$tag_hits" \
     --argjson version_lines "$version_hits" \
     --argjson digest_lines "$digest_hits" \
     '. + [{document: $document, tag_lines: $tag_lines, version_lines: $version_lines, digest_lines: $digest_lines}]' \
     "$replacements_json" > "$replacements_json.next"
  mv "$replacements_json.next" "$replacements_json"
done

if [[ "$outcome" == bumped ]]; then
  jq --arg repo "$target_repo" --arg tag "$target_tag" \
     --arg version "$target_version" \
     --arg source_url "$target_url" \
     --arg sha256 "$target_sha256" \
     '.repo = $repo | .tag = $tag | .version = $version | .source_url = $source_url | .sha256 = $sha256' \
     "$manifest" > "$manifest.next"
  mv "$manifest.next" "$manifest"
fi
if [[ "$outcome" == bumped || "$outcome" == contract_regenerated ]]; then
  install -m 644 "$target_contract" "$contract"
fi

# The verified binary is handed over last, so a failure anywhere above leaves
# nothing behind at the caller's path.
if [[ -n "$keep_binary" ]]; then
  install -m 755 "$downloaded" "$keep_binary"
fi

jq -n \
  --arg outcome "$outcome" \
  --arg from "$current_tag" \
  --arg tag "$target_tag" \
  --arg version "$target_version" \
  --arg sha256 "$target_sha256" \
  --arg source_url "$target_url" \
  --argjson protocol "$contract_protocol" \
  --argjson documents "$(cat "$replacements_json")" \
  '{status: "pass", outcome: $outcome, from: $from, tag: $tag, version: $version,
    sha256: $sha256, source_url: $source_url, protocol: $protocol, documents: $documents}'
