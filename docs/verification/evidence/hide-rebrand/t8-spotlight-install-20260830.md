# T8 release and Spotlight evidence

Date: 2026-08-30.

Source commit: `a25af41` (`Fix remote device selection targeting`).

The release Swift executable was built with `swift build --package-path macos --configuration release --disable-keychain --disable-sandbox --scratch-path /tmp/hide-finisher-swift`.

The Herdr core release library and test artifacts were built with `CARGO_TARGET_DIR=/tmp/hide-finisher-cargo`.

The release bundle was assembled at `dist/hide.app` from `/tmp/hide-finisher-swift/arm64-apple-macosx/release/HerdrMacOS` and signed with an ad hoc signature.

`codesign --verify --deep --strict` passed for both `dist/hide.app` and `/Applications/hide.app`.

The installed bundle identifier is `me.grab.hide`.

The installed bundle version is `0.1.0` and the bundled Herdr version is `0.8.2`.

The bundled Herdr SHA-256 is `bba6c79874689d5c8ec45811518ecf5cef9b521e61b081a9f56ddd406a482328`.

The release archive is `dist/hide-v0.1.0-macos-arm64.zip`.

The release archive SHA-256 is `5bf8f4534b66c68b3be40ef2b24194a590412548736184e6016eba7af706f3d2`.

The exact installed executable and the packaged executable have identical SHA-256 `e883ca79b9ab4df26d08466ad4dfea57140724aa823d8864cd2e0b45e715f6d7`.

The exact install command was `/usr/bin/ditto --rsrc --extattr --qtn dist/hide.app /Applications/hide.app`.

LaunchServices was refreshed with `lsregister -f /Applications/hide.app`.

The root volume and `/Applications` both reported `Indexing enabled` from `mdutil -s`.

The Spotlight command `/usr/bin/mdfind -name hide` returned `/Applications/hide.app`.

`mdls` reported `kMDItemCFBundleIdentifier = "me.grab.hide"`, `kMDItemFSName = "hide.app"`, and the installed metadata path `/System/Volumes/Data/Applications/hide.app`.

The installed app was launched from `/Applications/hide.app` with exactly one `me.grab.hide` process, PID `27163`, and exact executable path `/Applications/hide.app/Contents/MacOS/HerdrMacOS`.

The full-window native local screenshot is [final-installed-local-20260830.png](final-installed-local-20260830.png).

The full-window native remote screenshot is [final-installed-remote-20260830.png](final-installed-remote-20260830.png).

The fresh local and remote observation JSON files are [final-installed-local-20260830.json](final-installed-local-20260830.json) and [final-installed-remote-20260830.json](final-installed-remote-20260830.json).
