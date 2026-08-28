#!/bin/sh
set -eu

cd "$(dirname "$0")/.."

cargo build --release --bin herdr-cef-probe --bin herdr-cef-probe-helper --bin bundle-probe
./target/release/bundle-probe

app="target/bundle/herdr-cef-probe.app"
test -x "$app/Contents/MacOS/herdr-cef-probe"
test -d "$app/Contents/Frameworks/Chromium Embedded Framework.framework"

for helper in \
  "Helper" \
  "Helper (Alerts)" \
  "Helper (GPU)" \
  "Helper (Plugin)" \
  "Helper (Renderer)"
do
  test -x "$app/Contents/Frameworks/herdr-cef-probe $helper.app/Contents/MacOS/herdr-cef-probe $helper"
done

codesign --force --deep --sign - --timestamp=none "$app"
codesign --verify --deep --strict "$app"

printf '%s\n' "bundle-check: PASS"
