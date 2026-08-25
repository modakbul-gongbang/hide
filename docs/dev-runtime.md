# Dev Runtime: Which Pet Is Actually Running

The single biggest time sink so far is verifying a change against the wrong process.
Read this before running or screenshotting the app.

## One instance, always

`Herdr Pet` (bundle display name) and `herdr-pet-app` (executable inside the bundle) are the same app, not two.
Two instances can still be alive at once:

- the installed bundle at `/Applications/Herdr Pet.app`
- a `cargo tauri dev` instance from this checkout

When both run, tray actions, `show`/`hide`, window position state, and the click-to-expand window all cross-talk between them.
Observed symptoms during development: "the pet is not visible" (twice), and "a big dashboard window opens instead of the pet" (once).
Neither was a code bug.

Before any visual check:

```sh
pgrep -fl herdr-pet-app   # must list exactly one process
```

If more than one is listed, kill all of them and start exactly the instance you intend to verify.

## Verify against the installed bundle, not `dev`

A source fix is invisible to the running installed bundle.
Repeatedly "fixing" something the user still sees broken usually means they are looking at an older `/Applications` copy.

For any change the user will confirm visually:

1. Build the release bundle: `cargo tauri build` (output at `target/release/bundle/macos/Herdr Pet.app`).
2. Replace `/Applications/Herdr Pet.app` with it.
3. Relaunch, then confirm with a real screenshot.

State explicitly which build the user is looking at when reporting a fix.

## Window position state survives your edit

The pet persists its position to `~/.config/herdr-pet/window.json` on move and on shutdown.
Editing that file while the app is running is pointless: the app rewrites its in-memory coordinates on exit and overwrites you.
Kill the process first, then reset the file, then relaunch.
See [pet-window-macos.md](pet-window-macos.md) for the off-screen guards.

## Deep links reach the bundle, not `dev`

The pet accepts `herdr-pet://hide`, `herdr-pet://show`, and `herdr-pet://toggle`.
macOS resolves a URL scheme through the bundle's `Info.plist`, which the Tauri bundler writes from the `plugins.deep-link` block in **both** `tauri.conf.json` files.
A `cargo tauri dev` instance has no such plist entry, so `open herdr-pet://toggle` will launch or hit the installed `/Applications` copy instead - the same wrong-process trap as above, wearing a different hat.

Verify deep links against a freshly built bundle:

```sh
plutil -p "/Applications/Herdr Pet.app/Contents/Info.plist" | grep herdr-pet   # scheme registered
open "herdr-pet://toggle"
```

The pet also registers its own global shortcut for the same toggle (Settings -> 단축키).
That path does not go through the URL scheme at all, so a broken deep link and a broken shortcut are separate failures with separate checks.
