# Install hide

hide supports Apple Silicon Macs running macOS 14 or later.
You can install a published release when one is available or build the same app bundle from source today.

## Requirements

- An Apple Silicon Mac with macOS 14 or later.
- Xcode Command Line Tools or full Xcode with Swift 6.
- A current stable Rust toolchain with Rust 2024 edition support for source builds.
- Network access during the first source build for pinned Rust crates, Swift packages, and the official Herdr runtime asset.
- Claude Code or Codex installed and signed in only if you want hide to launch that agent.

Check the build tools before a source install:

```sh
swift --version
rustc --version
cargo --version
```

If `swift` is unavailable, install Apple's command line tools:

```sh
xcode-select --install
```

## Install a release

Open the [hide Releases page](https://github.com/modakbul-gongbang/hide/releases).
If it says there are no releases, continue with [Build and install from source](#build-and-install-from-source).

Download both files for the same version:

- `hide-v<version>-macos-arm64.zip`
- `hide-v<version>-macos-arm64.zip.sha256`

Verify the archive from the directory that contains both downloads:

```sh
shasum -a 256 -c hide-v<version>-macos-arm64.zip.sha256
```

Continue only when the command prints `OK`.
Expand the archive, move `hide.app` to `/Applications`, then launch it:

```sh
open /Applications/hide.app
```

The release is ad-hoc signed and is not notarized.
On the first launch, Control-click `hide.app`, choose **Open**, and confirm the Gatekeeper dialog.

If macOS still keeps the verified app in quarantine, remove the quarantine attribute only after confirming the downloaded checksum:

```sh
xattr -dr com.apple.quarantine /Applications/hide.app
open /Applications/hide.app
```

## Build and install from source

Clone the repository and build the release bundle:

```sh
git clone https://github.com/modakbul-gongbang/hide.git
cd hide
./scripts/build-app.sh
```

The build must finish with these outputs:

- `dist/hide.app`
- `dist/hide-v0.1.0-macos-arm64.zip`, or the version supplied through `HIDE_VERSION`
- the matching `.zip.sha256` sidecar

Install the app into the system Applications directory and launch the installed bundle:

```sh
sudo /usr/bin/ditto --rsrc --extattr --qtn dist/hide.app /Applications/hide.app
open /Applications/hide.app
```

The build script compiles `herdr-core`, builds the Swift shell, copies the app icon and pet theme, downloads the pinned official Herdr v0.8.2 arm64 binary when needed, verifies its version and SHA-256 digest, ad-hoc signs the bundle, and creates the release archive and checksum.

## Verify the installed app

Verify the bundle signature:

```sh
/usr/bin/codesign --verify --deep --strict --verbose=2 /Applications/hide.app
```

After opening hide, confirm the installed executable is the one running:

```sh
pgrep -fl '/Applications/hide.app/Contents/MacOS/HerdrMacOS'
```

Exactly one matching process should be active before checking the UI.

## First launch

hide resolves its Herdr runtime in this order:

1. A compatible installed Herdr connected to an existing live socket.
2. A compatible Herdr binary available in the login shell path.
3. The verified Herdr v0.8.2 runtime bundled inside `hide.app`.

The bundled runtime makes a separate Herdr installation optional.
Authentication is not bundled: SSH, Herdr, Claude Code, and Codex continue to own their own sign-in state and credentials.

If you want to start agents from hide, install and sign in to the relevant CLI before launching the app.
hide resolves those executables from the macOS login shell path and reports an explicit error when the selected CLI is unavailable.

## Update

Quit hide, verify the new release archive or rebuild from the new source revision, then replace `/Applications/hide.app` with the new bundle and reopen it.
Application state under `~/Library/Application Support/hide/` remains separate from the app bundle and is not removed by an update.

## Uninstall

Quit hide and move `/Applications/hide.app` to the Trash.
This removes the application but keeps its saved UI state under `~/Library/Application Support/hide/`.
Delete that directory only when you deliberately want to reset hide's saved state.

## Troubleshooting

### The app opens from the checkout but the installed app looks different

A source fix is not visible to an app bundle that was built earlier.
Quit every `HerdrMacOS` process, rebuild, reinstall, and launch the exact `/Applications/hide.app` bundle.

### hide says Herdr is unavailable

The installed Herdr and the bundled runtime were both unavailable or invalid.
Rebuild or reinstall hide and check the visible startup diagnostic instead of starting an unrelated fallback session.

### An agent cannot start

Run `command -v claude` or `command -v codex` in your login shell and complete that CLI's own sign-in flow.
hide never substitutes the other agent when the selected executable is missing.

### Remote workspaces do not connect

Confirm the SSH host works outside hide first:

```sh
ssh <host-alias> true
```

hide delegates authentication to the existing SSH configuration and agent.
It does not display, copy, or store a password.
