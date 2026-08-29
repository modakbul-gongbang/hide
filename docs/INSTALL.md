# Install hide

1. Download the `hide-...-macos-arm64.zip` release asset.
2. Open the archive and move `hide.app` to `/Applications`.
3. The first time, Control-click `hide.app`, choose **Open**, and confirm the Gatekeeper dialog.

If macOS still marks the app as quarantined, run this one-line alternative in Terminal:

```sh
xattr -dr com.apple.quarantine /Applications/hide.app
```

hide is an unsigned arm64 macOS app for macOS 14 or later.
It includes a verified herdr v0.8.2 runtime, but an installed compatible herdr or a live local socket takes precedence.
Authentication remains owned by SSH, herdr, and the selected agent CLI.
