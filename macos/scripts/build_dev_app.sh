#!/usr/bin/env bash
set -euo pipefail

macos_root="$(cd "$(dirname "$0")/.." && pwd)"
worktree_root="$(cd "$macos_root/.." && pwd)"
icon_path="$macos_root/Resources/hide.icns"

# A build from a linked worktree becomes its own instance: its own bundle
# identifier so macOS will run it alongside the main build instead of
# activating that one, and its own name so the two are distinguishable in the
# Dock and the app switcher. The app derives its state file from the bundle
# identifier, so this is also what keeps the two from overwriting each other's
# selected pane and window layout.
#
# `--git-dir` and `--git-common-dir` differ only in a linked worktree, so the
# main checkout keeps building the release identity with no flag to remember.
git_dir="$(git -C "$worktree_root" rev-parse --path-format=absolute --git-dir)"
git_common_dir="$(git -C "$worktree_root" rev-parse --path-format=absolute --git-common-dir)"
if [[ "$git_dir" == "$git_common_dir" ]]; then
    instance_suffix=""
else
    instance_suffix="$(basename "$worktree_root")"
fi

if [[ -n "$instance_suffix" ]]; then
    bundle_identifier="me.grab.hide.$instance_suffix"
    bundle_name="hide ($instance_suffix)"
    app_root="$macos_root/build/assembled/hide-$instance_suffix.app"
else
    bundle_identifier="me.grab.hide"
    bundle_name="hide"
    app_root="$macos_root/build/assembled/hide.app"
fi

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
/usr/libexec/PlistBuddy \
    -c "Set :CFBundleIdentifier $bundle_identifier" \
    -c "Set :CFBundleName $bundle_name" \
    -c "Set :CFBundleDisplayName $bundle_name" \
    "$app_root/Contents/Info.plist" >/dev/null
install -m 644 \
    "$icon_path" \
    "$app_root/Contents/Resources/hide.icns"

# The pet resolves its theme from the bundle's own resources, so the art has
# to travel with the app rather than being read out of the checkout.
/usr/bin/ditto "$worktree_root/assets/pet-theme" "$app_root/Contents/Resources/pet-theme"

# Third-party resources and their notices ship as one unit. Copy the directory
# so a newly added licensed resource cannot be bundled without its attribution.
/usr/bin/ditto \
    "$macos_root/Resources/THIRD_PARTY_NOTICES" \
    "$app_root/Contents/Resources/THIRD_PARTY_NOTICES"

for resource_bundle in "$macos_root"/.build/arm64-apple-macosx/debug/*.bundle; do
    if [[ -d "$resource_bundle" ]]; then
        /usr/bin/ditto "$resource_bundle" "$app_root/Contents/Resources/$(basename "$resource_bundle")"
    fi
done

codesign --force --deep --sign - "$app_root"
codesign --verify --deep --strict --verbose=2 "$app_root"

printf '%s\n' "$app_root"
