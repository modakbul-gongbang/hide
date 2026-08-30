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
downloaded_herdr="$temporary_root/herdr"

herdr_version="0.8.2"
herdr_url="https://github.com/herdrdev/herdr/releases/download/v${herdr_version}/herdr-macos-aarch64"
herdr_sha256="bba6c79874689d5c8ec45811518ecf5cef9b521e61b081a9f56ddd406a482328"

cleanup() {
  rm -rf -- "$temporary_root"
}
trap cleanup EXIT

cd "$project_root"
[[ -f "$icon_path" ]] || { print -u2 "app icon is missing: $icon_path"; exit 1; }
cargo build --release --locked -p herdr-core
swift build --package-path "$macos_root" --configuration release --disable-keychain --disable-sandbox

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

herdr_input="${HERDR_BINARY_PATH:-$HOME/.local/bin/herdr}"
if [[ -x "$herdr_input" ]]; then
  actual_input_version=$("$herdr_input" --version 2>/dev/null | /usr/bin/awk '{print $NF}' || true)
  actual_input_sha=$(/usr/bin/shasum -a 256 "$herdr_input" | /usr/bin/awk '{print $1}')
else
  actual_input_version=""
  actual_input_sha=""
fi
if [[ "$actual_input_version" == "$herdr_version" && "$actual_input_sha" == "$herdr_sha256" ]]; then
  herdr_source="$herdr_input"
else
  /usr/bin/curl --fail --location --silent --show-error "$herdr_url" --output "$downloaded_herdr"
  chmod 755 "$downloaded_herdr"
  herdr_source="$downloaded_herdr"
fi

actual_version=$("$herdr_source" --version | /usr/bin/awk '{print $NF}')
actual_sha=$(/usr/bin/shasum -a 256 "$herdr_source" | /usr/bin/awk '{print $1}')
[[ "$actual_version" == "$herdr_version" ]] || { print -u2 "bundled herdr version mismatch: $actual_version"; exit 1; }
[[ "$actual_sha" == "$herdr_sha256" ]] || { print -u2 "bundled herdr SHA-256 mismatch: $actual_sha"; exit 1; }
install -m 755 "$herdr_source" "$temporary_bundle/Contents/Resources/herdr-runtime/herdr"

install -m 644 \
  "$macos_root/Resources/herdr-bundle.json" \
  "$temporary_bundle/Contents/Resources/herdr-bundle.json"
install -m 644 \
  "$macos_root/Resources/THIRD_PARTY_NOTICES/herdr-APACHE-2.0.txt" \
  "$temporary_bundle/Contents/Resources/THIRD_PARTY_NOTICES/herdr-APACHE-2.0.txt"
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
/usr/bin/shasum -a 256 "$archive_path" > "$archive_path.sha256"

print -r -- "bundle=$bundle_path"
print -r -- "archive=$archive_path"
print -r -- "herdr_version=$actual_version"
print -r -- "herdr_sha256=$actual_sha"
print -r -- "archive_sha256=$(/usr/bin/awk '{print $1}' "$archive_path.sha256")"
