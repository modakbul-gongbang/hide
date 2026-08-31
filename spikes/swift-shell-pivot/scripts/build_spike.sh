#!/usr/bin/env bash
set -euo pipefail

spike_root="$(cd "$(dirname "$0")/.." && pwd)"
app_root="$spike_root/build/SwiftShellPivotSpike.app"

cd "$spike_root/rust-core"
cargo build --release

cd "$spike_root"
rust_archive="$spike_root/rust-core/target/release/libherdr_core_spike.a"
rust_archive_hash="$(LC_ALL=C LANG=C /usr/bin/shasum -a 256 "$rust_archive" | /usr/bin/awk '{print $1}')"
swift build -c release --arch arm64 \
    -Xswiftc -D \
    -Xswiftc "HERDR_CORE_${rust_archive_hash}"

mkdir -p "$app_root/Contents/MacOS"
install -m 755 ".build/arm64-apple-macosx/release/SwiftShellSpike" "$app_root/Contents/MacOS/SwiftShellSpike"
install -m 644 "resources/Info.plist" "$app_root/Contents/Info.plist"

codesign --force --deep --sign - "$app_root"
codesign --verify --deep --strict --verbose=2 "$app_root"

printf '%s\n' "$app_root"
