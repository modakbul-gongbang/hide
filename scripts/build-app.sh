#!/bin/zsh
set -euo pipefail

export LC_ALL=en_US.UTF-8
export LC_CTYPE=en_US.UTF-8
export LANG=en_US.UTF-8

script_dir=${0:A:h}
project_root=${script_dir:h}
macos_root="$project_root/macos"
dist_root="$project_root/dist"
bundle_path="$dist_root/hide.app"
icon_path="$macos_root/Resources/hide.icns"
archive_path="$dist_root/hide-v${HIDE_VERSION:-0.1.0}-macos-arm64.zip"
temporary_root=$(mktemp -d "${TMPDIR:-/tmp}/hide-bundle.XXXXXX")
temporary_bundle="$temporary_root/hide.app"

cleanup() {
  rm -rf -- "$temporary_root"
}
trap cleanup EXIT

cd "$project_root"
[[ -f "$icon_path" ]] || { print -u2 "app icon is missing: $icon_path"; exit 1; }
cargo build --release --locked -p herdr-core
swift build \
  --package-path "$macos_root" \
  --configuration release \
  --disable-keychain \
  --disable-sandbox \
  -Xswiftc -strict-concurrency=minimal \
  -Xswiftc -Xfrontend \
  -Xswiftc -disable-round-trip-debug-types

mkdir -p \
  "$temporary_bundle/Contents/MacOS" \
  "$temporary_bundle/Contents/Resources/herdr-runtime" \
  "$temporary_bundle/Contents/Resources/THIRD_PARTY_NOTICES"
install -m 755 \
  "$macos_root/.build/arm64-apple-macosx/release/HerdrMacOS" \
  "$temporary_bundle/Contents/MacOS/HerdrMacOS"
install -m 644 \
  "$macos_root/Resources/Info.plist" \
  "$temporary_bundle/Contents/Info.plist"
install -m 644 \
  "$icon_path" \
  "$temporary_bundle/Contents/Resources/hide.icns"

# The Pet resolves its theme from the bundle at runtime, so release builds
# must carry the same resource tree as the development app.
/usr/bin/ditto "$project_root/assets/pet-theme" "$temporary_bundle/Contents/Resources/pet-theme"

for resource_bundle in "$macos_root"/.build/arm64-apple-macosx/release/*.bundle; do
  if [[ -d "$resource_bundle" ]]; then
    /usr/bin/ditto "$resource_bundle" "$temporary_bundle/Contents/Resources/$(basename "$resource_bundle")"
  fi
done

# The runtime the app ships is the pinned asset, verified against the manifest
# by the one script every build shares; nothing on this machine's PATH is
# consulted.
herdr_source=$(zsh "$script_dir/fetch-herdr-runtime.sh")
actual_version=$("$herdr_source" --version | /usr/bin/awk '{print $NF}')
actual_sha=$(/usr/bin/shasum -a 256 "$herdr_source" | /usr/bin/awk '{print $1}')
install -m 755 "$herdr_source" "$temporary_bundle/Contents/Resources/herdr-runtime/herdr"

# Ship every notice, not a named one: a mark added to the app without a
# matching line here would ship unattributed.
/usr/bin/ditto \
  "$macos_root/Resources/THIRD_PARTY_NOTICES" \
  "$temporary_bundle/Contents/Resources/THIRD_PARTY_NOTICES"
install -m 644 \
  "$project_root/docs/INSTALL.md" \
  "$temporary_bundle/Contents/Resources/INSTALL.md"

/usr/bin/plutil -lint "$temporary_bundle/Contents/Info.plist"
/usr/bin/codesign --force --deep --sign - --timestamp=none "$temporary_bundle"
/usr/bin/codesign --verify --deep --strict --verbose=2 "$temporary_bundle"

mkdir -p "$dist_root"
rm -rf -- "$bundle_path" "$archive_path" "$archive_path.sha256"
mv "$temporary_bundle" "$bundle_path"
/usr/bin/ditto -c -k --keepParent "$bundle_path" "$archive_path"
archive_name=$(basename "$archive_path")
archive_digest=$(/usr/bin/shasum -a 256 "$archive_path" | /usr/bin/awk '{print $1}')
printf '%s  %s\n' "$archive_digest" "$archive_name" > "$archive_path.sha256"

print -r -- "bundle=$bundle_path"
print -r -- "archive=$archive_path"
print -r -- "herdr_version=$actual_version"
print -r -- "herdr_sha256=$actual_sha"
print -r -- "archive_sha256=$archive_digest"
