#!/bin/sh
set -eu

cd "$(dirname "$0")/.."

cargo build --release --locked \
  --bin herdr-integrated-preflight \
  --bin herdr-integrated-preflight-helper \
  --bin bundle-integrated-preflight
./target/release/bundle-integrated-preflight

app="target/bundle/herdr-integrated-preflight.app"
main="$app/Contents/MacOS/herdr-integrated-preflight"
framework="$app/Contents/Frameworks/Chromium Embedded Framework.framework"

test -x "$main"
test -d "$framework"

for helper in \
  "Helper" \
  "Helper (Alerts)" \
  "Helper (GPU)" \
  "Helper (Plugin)" \
  "Helper (Renderer)"
do
  helper_main="$app/Contents/Frameworks/herdr-integrated-preflight $helper.app/Contents/MacOS/herdr-integrated-preflight $helper"
  test -x "$helper_main"
done

/usr/bin/codesign --force --deep --sign - --timestamp=none "$app"
/usr/bin/codesign --verify --deep --strict --verbose=2 "$app"
/usr/bin/plutil -lint "$app/Contents/Info.plist"

mkdir -p target/evidence
main_sha256=$(LC_ALL=C LANG=C /usr/bin/shasum -a 256 "$main" | /usr/bin/awk '{print $1}')
bundle_kib=$(/usr/bin/du -sk "$app" | /usr/bin/awk '{print $1}')
architectures=$(/usr/bin/lipo -archs "$main")
/usr/bin/jq -n \
  --arg schema "herdr.integrated-preflight.bundle.v1" \
  --arg app "$app" \
  --arg main_sha256 "$main_sha256" \
  --arg architectures "$architectures" \
  --arg cef_crate "151.8.0+151.3.24" \
  --arg cef_rs_commit "a2e15ae659c4b3957883e34de879bd8b38360ce5" \
  --arg objc2 "0.6.4" \
  --arg wgpu "30.0.1" \
  --argjson bundle_kib "$bundle_kib" \
  '{schema: $schema, app: $app, main_sha256: $main_sha256, architectures: ($architectures | split(" ")), bundle_kib: $bundle_kib, helper_bundles: 5, cef_framework: true, ad_hoc_signature_verified: true, pins: {cef_crate: $cef_crate, cef_rs_commit: $cef_rs_commit, objc2: $objc2, wgpu: $wgpu}}' \
  > target/evidence/bundle-manifest.json

printf '%s\n' \
  "bundle-check: PASS" \
  "app=$app" \
  "manifest=target/evidence/bundle-manifest.json"
