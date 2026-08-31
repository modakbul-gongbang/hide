# T8 release and Spotlight evidence

Date: 2026-08-30.

Source commit: `a61826a` (`Open terminal links through Hide`).

The release Swift executable was built with `swift build --package-path macos --configuration release --disable-keychain --disable-sandbox --scratch-path /tmp/hide-finisher-swift`.

The Herdr core release library and test artifacts were built with `CARGO_TARGET_DIR=/tmp/hide-finisher-cargo`.

The release bundle was assembled at `dist/hide.app` from `/tmp/hide-finisher-swift/arm64-apple-macosx/release/HerdrMacOS` and signed with an ad hoc signature.

`codesign --verify --deep --strict` passed for both `dist/hide.app` and `/Applications/hide.app`.

The installed bundle identifier is `me.grab.hide`.

The installed bundle version is `0.1.0` and the bundled Herdr version is `0.8.2`.

The bundled Herdr SHA-256 is `bba6c79874689d5c8ec45811518ecf5cef9b521e61b081a9f56ddd406a482328`.

The release archive is `dist/hide-v0.1.0-macos-arm64.zip`.

The release archive SHA-256 is `2b2a976bb086736f8a8460591d5e5a65b3ef1edaadfd987e48efb9c878d12a35`.

The exact installed executable and the packaged executable have identical SHA-256 `81e43ca57a8f08ba97edb06e670d1e48cecf40806e24997e4260b651511cb27e`.

The exact install command was `/usr/bin/ditto --rsrc --extattr --qtn dist/hide.app /Applications/hide.app`.

LaunchServices was refreshed with `lsregister -f /Applications/hide.app`.

The root volume and `/Applications` both reported `Indexing enabled` from `mdutil -s`.

The Spotlight command `/usr/bin/mdfind -name hide` returned `/Applications/hide.app`.

`mdls` reported `kMDItemCFBundleIdentifier = "me.grab.hide"`, `kMDItemFSName = "hide.app"`, and the installed metadata path `/System/Volumes/Data/Applications/hide.app`.

The installed app was launched from `/Applications/hide.app` with exactly one `me.grab.hide` process, PID `52310`, and exact executable path `/Applications/hide.app/Contents/MacOS/HerdrMacOS`.

The full-window native local screenshot is [final-installed-local-a61826a-20260830.png](final-installed-local-a61826a-20260830.png).

The full-window native remote screenshot is [final-installed-remote-a61826a-20260830.png](final-installed-remote-a61826a-20260830.png).

The fresh local and remote observation JSON files are [final-installed-local-a61826a-20260830-see.json](final-installed-local-a61826a-20260830-see.json) and [final-installed-remote-a61826a-20260830-see.json](final-installed-remote-a61826a-20260830-see.json).

The remote screenshot was captured after selecting the dedicated `w4Q` workspace and confirming both `w4Q:p1` and `w4Q:p2` terminal surfaces in the fresh accessibility snapshot.

The dedicated mini workspace was closed and its exact temporary directory was removed with `rmdir`; the post-cleanup snapshot is [mini-dedicated-workspace-a61826a-20260830-after-close.json](mini-dedicated-workspace-a61826a-20260830-after-close.json).
