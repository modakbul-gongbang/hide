# Browser panes

A Hide tab contains a layout of panes.
Each pane has content independent of that layout: terminal or browser today.
A browser can sit beside a terminal in the same tab, or occupy its own tab.
Herdr still owns pane existence and split geometry; Hide renders browser content advertised by the browser host.

The browser is a real Chromium page in an existing named chromux profile.
Hide displays that exact target through CDP screencast and forwards pointer, keyboard and committed text input to it.
It does not transplant a Chrome window, create a second browser engine, or copy profile storage.

## Open from an agent terminal

Requirements: Node.js 22 or newer, the Herdr runtime pinned in [herdr-bundle.json](../macos/Sources/HerdrMacOS/Resources/herdr-bundle.json), and chromux on the process PATH.
Select an existing managed chromux profile explicitly.
`live` and `external-*` profiles are not accepted by this host.
The profile must use chromux's unpaused default mode and a loopback TCP daemon.

From this checkout:

```sh
node plugins/browser/browser-pane.mjs profiles
node plugins/browser/browser-pane.mjs open --profile work --url http://localhost:3000
```

From an installed app:

```sh
node /Applications/hide.app/Contents/Resources/browser-pane/browser-pane.mjs profiles
node /Applications/hide.app/Contents/Resources/browser-pane/browser-pane.mjs open \
  --profile work --target-pane w1:p1 --url http://localhost:3000
```

Inside Herdr, omitting `--target-pane` selects the calling pane, never the UI-focused pane.
Outside Herdr, the target must be explicit.
New panes inherit the target's working directory and use `--no-focus`.
Use `--direction down` for a vertical split, or `--placement tab` for a new tab.

To show an existing session or exact Chromium target, replace `--url` with one of:

```sh
--session checkout-qa
--target-id ABCD1234
```

Choose exactly one source.
The JSON result includes `pane_id`, `tab_id`, `profile`, `session`, `target_id`, `cdp_http_url`, `binding_id`, and `reused`.
Continue automation against the returned profile and session:

```sh
chromux --profile work snapshot SESSION_FROM_RESULT --interactive
chromux --profile work click SESSION_FROM_RESULT @1
```

The same request reuses its pane without navigation or a focus change.
Use a new `--key` to express a deliberate second pane for otherwise identical input.
Reusing a key with different input is rejected.

## Ownership and failures

| Source | Closing the pane |
| --- | --- |
| `--url` | Closes the Chromium tab created for this pane |
| `--session` | Detaches the viewer; the existing session and tab stay open |
| `--target-id` | Detaches the viewer and any host-created attachment; the existing tab stays open |

A detached lease process refreshes the selected chromux session while the pane exists.
It checks pane retirement every ten seconds so it can finish cleanup even when Herdr kills the pane process group.
It never closes a borrowed session or kills the shared Chrome profile.
Diagnostics live in Herdr's plugin state directory, in the binding's `.log` file.
The host pane prints that path, and startup/lease failures also appear through pane metadata.

Hide's Reconnect button reconnects its viewer to the same target.
Tabs created by `--url` follow the pane's content size, including responsive CSS layout.
Borrowed tabs retain their existing viewport and are letterboxed without changing device emulation.
It does not substitute a new target if that target has closed.
Reopen the pane with a new request when the selected session or target no longer exists.
Browser panes on remote Herdr hosts display an explicit unavailable state; remote port forwarding is not implemented.
Browser hosting requires the live host process and is not restored by a cold Herdr server restart.
Restarting the Hide viewer alone does not retire the Herdr pane or its browser lease.

The first open links the `hide.browser` plugin from the command's directory.
If it is already linked to another checkout or app bundle, the command refuses to silently replace it.
Close browser panes from that installation before explicitly unlinking and linking the desired installation with Herdr.

## Verification

```sh
node --test plugins/browser/browser-pane.test.mjs
scripts/rust-test.sh
scripts/swift-test.sh
macos/scripts/build_dev_app.sh
```

Native verification must use the running Hide build and screenshots, not just a Chrome screenshot or passing unit tests.
A Browser plugin pane is only a verification surface when the browser-pane product itself is under test.
For the native shell, editor, Git diff, sidebar, build, or installed app, verify one identified installed app instance with a real native capture and do not open or manipulate an operator Browser pane.
Keep screenshots, profiles and run transcripts under local-only `agents/runs/`.
