#!/bin/zsh
set -euo pipefail

script_dir=${0:A:h}
spike_root=${script_dir:h}
bundle_path="$spike_root/dist/Herdr IDE Native Spike.app"
temporary_root=$(mktemp -d "${TMPDIR:-/tmp}/herdr-native-bundle.XXXXXX")
temporary_bundle="$temporary_root/Herdr IDE Native Spike.app"

cleanup() {
  rm -rf -- "$temporary_root"
}
trap cleanup EXIT

cd "$spike_root"
cargo build --release --locked

mkdir -p "$temporary_bundle/Contents/MacOS" "$temporary_bundle/Contents/Resources"
cp "$spike_root/target/release/herdr-ide-native-spike" "$temporary_bundle/Contents/MacOS/herdr-ide-native-spike"
cp "$spike_root/resources/Info.plist" "$temporary_bundle/Contents/Info.plist"
chmod 755 "$temporary_bundle/Contents/MacOS/herdr-ide-native-spike"
/usr/bin/plutil -lint "$temporary_bundle/Contents/Info.plist"
/usr/bin/codesign --force --sign - --timestamp=none "$temporary_bundle"
/usr/bin/codesign --verify --deep --strict --verbose=2 "$temporary_bundle"

mkdir -p "$spike_root/dist"
rm -rf -- "$bundle_path"
mv "$temporary_bundle" "$bundle_path"

bundle_hash=$(/usr/bin/shasum -a 256 "$bundle_path/Contents/MacOS/herdr-ide-native-spike" | /usr/bin/awk '{print $1}')
print -r -- "bundle=$bundle_path"
print -r -- "executable_sha256=$bundle_hash"
