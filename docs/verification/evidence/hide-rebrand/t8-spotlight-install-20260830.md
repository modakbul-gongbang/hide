# T8 release and Spotlight evidence

Date: 2026-08-30.

The release Swift executable was built with `swift build --package-path macos --configuration release --disable-keychain --disable-sandbox --scratch-path /tmp/hide-finisher-swift`.

The Herdr core release library was built with `CARGO_TARGET_DIR=/tmp/hide-finisher-cargo cargo build --release --locked -p herdr-core`.

The bundle was assembled at `dist/hide.app` and signed with an ad hoc signature.

`codesign --verify --deep --strict` passed for both `dist/hide.app` and `/Applications/hide.app`.

The installed bundle identifier is `me.grab.hide`.

The installed bundle version is `0.1.0` and the bundled Herdr version is `0.8.2`.

The bundled Herdr SHA-256 is `bba6c79874689d5c8ec45811518ecf5cef9b521e61b081a9f56ddd406a482328`.

The release archive is `dist/hide-v0.1.0-macos-arm64.zip`.

The release archive SHA-256 is `bd2c2b58da9bb2863997f09e3ad258a1c1f1282c8441231d0fe0dac54daa2735`.

The exact installed executable and the packaged executable have identical SHA-256 `a69548fc825e772d021862998eee3d51f3dbdd7363c312bba0e3acd2978df508`.

The exact install command was `/usr/bin/ditto --rsrc --extattr --qtn dist/hide.app /Applications/hide.app`.

LaunchServices was refreshed with `lsregister -f /Applications/hide.app`.

The Spotlight command `/usr/bin/mdfind -name hide` returned `/Applications/hide.app`.

The installed app was launched from `/Applications/hide.app` with exactly one `me.grab.hide` process, PID `95067`, and exact executable path `/Applications/hide.app/Contents/MacOS/HerdrMacOS`.

The full-window native screenshot is [final-installed-local-20260830.png](final-installed-local-20260830.png).
