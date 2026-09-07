# Dev Runtime: Which App Is Actually Running

The single biggest time sink so far is verifying a change against the wrong
process. Read this before running or screenshotting the app.
For responsiveness, rendering, CPU, or memory checks, also read [PERFORMANCE_TESTING.md](PERFORMANCE_TESTING.md) in full.

## One instance, always

`hide` (bundle display name) and `HerdrMacOS` (executable inside the
bundle) are the same app, not two. Several instances can be alive at once:

- an installed copy, if one has been placed in `/Applications`
- the assembled dev bundle at
  `macos/build/assembled/hide.app`, from `macos/scripts/build_dev_app.sh`
- a bare `swift run` from the checkout

When more than one runs, the pet's show/hide state, its saved position, the
menu bar item, and the `herdr-ide://` URL scheme all cross-talk between them.
Observed symptoms while this was still herdr-pet: "the pet is not visible"
(twice) and "a big window opens instead of the pet" (once). Neither was a
code bug.

Before any visual check:

```sh
pgrep -fl HerdrMacOS   # must list exactly one process
```

If more than one is listed, identify each exact PID and bundle before proceeding.
Quit only test instances you own; coordinate with the operator before normally quitting their app, and record its bundle path for restoration.
Never kill all matching processes or stop the operator's Herdr server to obtain a clean screenshot.

## Verify against the assembled bundle, not `swift run`

A source fix is invisible to an already-running app. Repeatedly "fixing"
something the user still sees broken usually means they are looking at an
older copy.

For any change the user will confirm visually:

```sh
macos/scripts/build_dev_app.sh   # prints the assembled .app path
```

That script builds herdr-core in release, builds the Swift shell, copies
`assets/pet-theme` into `Contents/Resources/pet-theme`, writes
`Resources/Info.plist` into the bundle, and ad-hoc signs the result. Launch
that bundle, then confirm with a real screenshot. State explicitly which
build the user is looking at when reporting a fix.

A bare `swift run` has no bundle resources: the pet theme then loads from the
repository checkout instead, and the URL scheme is not registered at all.

## Pet state survives your edit

The pet persists its position, visibility, and global shortcut through herdr-core's UI state file, which `--state-path` can override.
The release bundle defaults to `hide/state.json` under the user's Application Support directory; other bundle identifiers use `hide/instances/<bundle-id>/state.json` there.
`/tmp/herdr-ide-verify-ui-state.json` is only the default for `--verification-ui-fixture`, not a normal launch.
The file is rewritten whenever the pet moves or is toggled, so editing it while the app runs is pointless.
For an owned fixture, quit its exact process before resetting its private state and relaunching.
Do not reset the operator's state file for QA; supply a separate `--state-path` and follow the performance guide's server-isolation procedure.

See [pet-window-macos.md](pet-window-macos.md) for the off-screen guards; a
saved position outside every connected screen is clamped back into view
rather than succeeding invisibly.

## Deep links reach the bundle, not `swift run`

The app accepts `herdr-ide://hide`, `herdr-ide://show`, and
`herdr-ide://toggle`. macOS resolves a URL scheme through the bundle's
`Info.plist` (`CFBundleURLTypes`), which only the assembled bundle has.

```sh
plutil -p "macos/build/assembled/hide.app/Contents/Info.plist" | grep herdr-ide
open "herdr-ide://toggle"
```

The retired pet app's `herdr-pet://` scheme is deliberately **not**
registered. `open herdr-pet://toggle` must not affect this app; if it does
something, an old Herdr Pet bundle is still installed.

The pet's own global shortcut does not go through the URL scheme at all, so a
broken deep link and a broken shortcut are separate failures with separate
checks.

## Driving pet states without real agents

The pet's pose and badge row come from whatever the herdr server reports, and
an unseen error cannot be produced on demand from a real agent.
`macos/scripts/pet_scenario_server.py` serves the `session.snapshot`,
`events.subscribe`, and `agent.list` boundaries over a Unix socket using the
same protocol revision the real server speaks, so the app exercises its
ordinary event-sync path:

```sh
macos/scripts/pet_scenario_server.py --socket /tmp/pet.sock --scenario scenario.json &
HERDR_SOCKET_PATH=/tmp/pet.sock macos/build/assembled/hide.app/Contents/MacOS/HerdrMacOS
```

The scenario file is re-read on every snapshot or agent-list request, so
editing it changes what the next one-second agent refresh sees.
Stopping the server (or deleting the socket) is how the "herdr went away"
case is produced; restarting it proves subscription and telemetry recovery.
