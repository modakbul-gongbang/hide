#!/usr/bin/env bash
set -euo pipefail

macos_root="$(cd "$(dirname "$0")/.." && pwd)"
worktree_root="$(cd "$macos_root/.." && pwd)"
app_root="$macos_root/build/assembled/hide.app"
icon_path="$macos_root/Resources/hide.icns"

if [[ ! -f "$icon_path" ]]; then
    printf 'app icon is missing: %s\n' "$icon_path" >&2
    exit 1
fi

cargo build --manifest-path "$worktree_root/herdr-core/Cargo.toml" --release
rust_archive="$worktree_root/target/release/libherdr_core.a"
rust_archive_hash="$(LC_ALL=C LANG=C /usr/bin/shasum -a 256 "$rust_archive" | /usr/bin/awk '{print $1}')"
swift build --package-path "$macos_root" --disable-keychain --disable-sandbox \
    -Xswiftc -D \
    -Xswiftc "HERDR_CORE_${rust_archive_hash}"

mkdir -p "$app_root/Contents/MacOS" "$app_root/Contents/Resources"
install -m 755 \
    "$macos_root/.build/arm64-apple-macosx/debug/HerdrMacOS" \
    "$app_root/Contents/MacOS/HerdrMacOS"
install -m 644 \
    "$macos_root/Resources/Info.plist" \
    "$app_root/Contents/Info.plist"
install -m 644 \
    "$icon_path" \
    "$app_root/Contents/Resources/hide.icns"

# The pet resolves its theme from the bundle's own resources, so the art has
# to travel with the app rather than being read out of the checkout.
/usr/bin/ditto "$worktree_root/assets/pet-theme" "$app_root/Contents/Resources/pet-theme"

for resource_bundle in "$macos_root"/.build/arm64-apple-macosx/debug/*.bundle; do
    if [[ -d "$resource_bundle" ]]; then
        /usr/bin/ditto "$resource_bundle" "$app_root/Contents/Resources/$(basename "$resource_bundle")"
    fi
done

codesign --force --deep --sign - "$app_root"
codesign --verify --deep --strict --verbose=2 "$app_root"

printf '%s\n' "$app_root"
