#!/bin/zsh
# Produces the pinned Herdr binary for an app build and prints its path.
#
# The asset is kept in a cache keyed by its digest, so the release build, the
# development build and the bump script all read the same verified file and a
# second build on the same machine downloads nothing. A cached file whose
# digest no longer matches the pin is discarded rather than trusted.
set -euo pipefail

export LC_ALL=en_US.UTF-8
export LC_CTYPE=en_US.UTF-8
export LANG=en_US.UTF-8

script_dir=${0:A:h}
project_root=${script_dir:h}
manifest=$project_root/macos/Sources/HerdrMacOS/Resources/herdr-bundle.json
[[ -f "$manifest" ]] || {
  print -u2 -- "error: pinned Herdr runtime manifest is missing: $manifest"
  exit 1
}

version=$(jq -er '.version' "$manifest")
source_url=$(jq -er '.source_url' "$manifest")
sha256=$(jq -er '.sha256' "$manifest")

cache_root=${HIDE_HERDR_CACHE:-${XDG_CACHE_HOME:-$HOME/Library/Caches}/hide/herdr-runtime}
cached=$cache_root/$sha256/herdr

digest_of() {
  /usr/bin/shasum -a 256 "$1" | /usr/bin/awk '{print $1}'
}

if [[ -f "$cached" ]] && [[ "$(digest_of "$cached")" == "$sha256" ]]; then
  print -r -- "$cached"
  exit 0
fi

temporary_root=$(mktemp -d "${TMPDIR:-/tmp}/herdr-fetch.XXXXXX")
cleanup() {
  rm -rf -- "$temporary_root"
}
trap cleanup EXIT
downloaded=$temporary_root/herdr

/usr/bin/curl --fail --location --silent --show-error "$source_url" --output "$downloaded" || {
  print -u2 -- "error: pinned Herdr asset could not be downloaded: $source_url"
  exit 1
}
chmod 755 "$downloaded"

actual_sha256=$(digest_of "$downloaded")
[[ "$actual_sha256" == "$sha256" ]] || {
  print -u2 -- "error: downloaded Herdr asset digest does not match the pin"
  print -u2 -- "pinned_sha256=$sha256 downloaded_sha256=$actual_sha256 url=$source_url"
  exit 1
}
actual_version=$("$downloaded" --version | /usr/bin/awk '{print $NF}')
[[ "$actual_version" == "$version" ]] || {
  print -u2 -- "error: downloaded Herdr asset reports $actual_version, not the pinned $version"
  exit 1
}

mkdir -p "${cached:h}"
install -m 755 "$downloaded" "$cached"
print -r -- "$cached"
