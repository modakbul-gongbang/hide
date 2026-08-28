#!/bin/zsh
set -euo pipefail

export LC_ALL=en_US.UTF-8
export LC_CTYPE=en_US.UTF-8
export LANG=en_US.UTF-8

script_dir=${0:A:h}
project_root=${script_dir:h}
bundle_path="$project_root/dist/Herdr IDE.app"
temporary_root=$(mktemp -d "${TMPDIR:-/tmp}/herdr-ide-bundle.XXXXXX")
temporary_bundle="$temporary_root/Herdr IDE.app"

cleanup() {
  rm -rf -- "$temporary_root"
}
trap cleanup EXIT

cd "$project_root"
cargo build --release --locked
build_target=${CARGO_TARGET_DIR:-"$project_root/target"}

mkdir -p "$temporary_bundle/Contents/MacOS" "$temporary_bundle/Contents/Resources"
cp "$build_target/release/herdr-ide" "$temporary_bundle/Contents/MacOS/herdr-ide"
cp "$project_root/resources/Info.plist" "$temporary_bundle/Contents/Info.plist"
chmod 755 "$temporary_bundle/Contents/MacOS/herdr-ide"
/usr/bin/plutil -lint "$temporary_bundle/Contents/Info.plist"
/usr/bin/codesign --force --sign - --timestamp=none "$temporary_bundle"
/usr/bin/codesign --verify --deep --strict --verbose=2 "$temporary_bundle"

mkdir -p "$project_root/dist"
rm -rf -- "$bundle_path"
mv "$temporary_bundle" "$bundle_path"

bundle_hash=$(/usr/bin/shasum -a 256 "$bundle_path/Contents/MacOS/herdr-ide" | /usr/bin/awk '{print $1}')
print -r -- "bundle=$bundle_path"
print -r -- "executable_sha256=$bundle_hash"
