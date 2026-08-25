# Pet Window on macOS

Hard-won constraints for the transparent, always-on-top pet window.
Each item below caused a real failure during development.

## Transparency: kill the shadow, in both config files

Tauri v2 defaults `shadow` to on.
On a transparent window macOS then draws a rounded grey backdrop, which reads as "a grey box behind the pet".
The pet window needs `"shadow": false`; the web side already sets `background: transparent`.

There are two `tauri.conf.json` files:

- `apps/pet-app/tauri.conf.json`
- `apps/pet-app/src-tauri/tauri.conf.json`

Window settings must be changed in both.
Editing only one leaves the old behavior in whichever build path reads the other file.

## Off-screen position

`~/.config/herdr-pet/window.json` once held `[542720, 163840]`, which put the pet far outside every display.
`show()` then succeeds and the user sees nothing.
Two guards, both required:

1. `restore_window_position` ignores a saved position that lands on no connected monitor and resets to the primary display.
   Guards against unplugged displays and corrupted state files.
2. `clamp_window_to_monitor` runs on every `WindowEvent::Moved` and snaps an out-of-bounds window back inside its monitor.
   The user drag path now captures the cursor, window bounds, and size on pointer-down and moves the window from `AppHandle::cursor_position()`.
   The pure anchored-delta calculation clamps on every cursor sample, so the pet cannot leave the active monitor during the gesture.

The geometry for both lives in `crates/herdr-core/src/window.rs` as pure functions with tests; `main.rs` only adapts Tauri monitor types onto them.
The real incident coordinates are pinned in those tests.

Remember the state file is rewritten on shutdown; see [dev-runtime.md](dev-runtime.md).

## Manual drag and the interaction surface

Incident context (2026-08-18): dragging the pet stuttered, in two distinct ways.
First, the drag was driven from the hit webview (pointermove → requestAnimationFrame → invoke per frame), and WKWebView throttles timers in unfocused windows, so the tick rate collapsed exactly while dragging.
Second, after moving the loop to Rust, fast drags still hitched once in a while: periodic sync Tauri commands (`get_pet_state` reading agent files off disk every 1-2.5s, plus the 150-250ms polls) run on the main thread and landed between drag ticks.
Both fixes below exist because of that incident; smoothness was verified with a synthetic CGEvent drag whose cursor-to-window offset stayed constant across every sample.

The visible render window is click-through.
An equally sized `pet-hit` window owns pointer capture and reports only pointer-down and pointer-up to Rust.
This avoids the macOS first-click activation trap that consumed the old gesture when Ghostty was frontmost.
The hit window keeps a three-pixel threshold and ignores a click for about 0.5 seconds after a real move, so releasing a drag cannot open the popover.

The drag itself runs entirely on the native side.
`begin_drag` spawns a thread that ticks every 8ms on the main thread, reads the global cursor, and moves both windows through the anchored-delta clamp; `end_drag` stops it.
The webview sends zero IPC during the gesture.
This replaced a pointermove → requestAnimationFrame → `drag_move` loop: WKWebView throttles timers in an unfocused window, and the hit window is never focused, so the per-frame invoke path stuttered exactly while dragging.
`backgroundThrottling: "disabled"` is also set on the render window (in both `tauri.conf.json` files) so the pet animation keeps its frame rate while unfocused.
The main thread's budget belongs to the drag while a session is active: `get_pet_state` is an async command because it reads agent files off disk (sync commands run on the main thread - see INV-pet-state-off-main-thread), the poll commands early-return mid-drag, the hit view suspends its polling intervals, and `applyNativeState` skips the DOM rebuild when the state is unchanged.
AppKit's `NSEvent.pressedMouseButtons` is the release authority inside the tick: a pointerup the webview never delivers cannot leave the pet glued to the cursor.
A `begin_drag` without a matching `end_drag` now means a pet that follows the cursor forever - never call it from a surface that cannot guarantee the pointerup.

Popover and context-menu content live in separate anchored windows.
They never resize the pet or enlarge its hit area.
Their placement flips left/right and top/bottom against the monitor work area.
Focus loss hides them, which replaces the old document-only outside-pointer handler.
The global-cursor poll dismisses them only on an outside *click* (a fresh AppKit button press while the cursor is outside both the pet and the surface), never on hover-out: closing on cursor exit made the popover vanish while the user was crossing the gap between the pet and the surface.

## Raising the hosting terminal

Do not find the terminal window by title.
The original code scanned every window of every app for a title containing the project name; Ghostty window titles contain no such text, so it failed silently, and the scan itself froze the cursor into a loading state for seconds.

`crates/herdr-core/src/window.rs` instead walks the parent-process chain of a herdr session process until it hits an app bundle executable, then activates that pid.
This works for any terminal and assumes nothing about window titles.

### Route the raise by target, and select the tab

Activating the app is not sufficient, and picking the process is not target-agnostic.
Both facts were established by probing a live machine, and both caused the same user-visible bug: clicking a remote agent in the dashboard focused the pane on the remote host but never brought its terminal forward.

1. **Route by target.**
   The old code ran a fixed `pgrep -f "herdr client"` and raised the first match.
   On the recorded machine that pattern matched exactly one process, a child of the *remote* view, so every click raised the remote session regardless of which agent it was.
   A remote target is displayed by a local `herdr --remote <host>` process; a local target by the plain `herdr` session process.
   `select_session_process` picks between them and its tests pin the recorded process list.

2. **Select the tab, not just the app.**
   A user watching a local and a remote session keeps them as two tabs of one terminal window, so `set frontmost` restores whichever tab was last active and can never switch between them.
   Terminal emulators title each tab with the command running in it, which is what makes the match possible: Ghostty exposes tabs as `AXRadioButton` under `tab group 1 of window 1`, named `herdr` and `herdr --remote grab@grabs-mac-mini`.
   The AppleScript clicks the radio button whose name equals the resolved process command.

The ssh alias in `~/.config/herdr-pet/config.toml` and the `--remote` argument routinely disagree - `mini` versus `grab@grabs-mac-mini`.
`remote_argument_matches_host` compares the host parts with `user@` stripped and accepts either containing the other.
That heuristic only ever chooses which tab to raise; it never routes a command.

A tab-name miss is not an error, because the app still came forward.

Also: pane focus succeeding is the success condition.
Raising the terminal window is best-effort and must never turn a successful focus into a user-visible error.
macOS may prompt for Automation permission the first time a new build sends System Events; without it, window raising silently does nothing.

## Moving windows in the private SkyLight Space

The pet and hit windows live in a private SkyLight Space (`SLSSpaceCreate` + `SLSSpaceAddWindowsAndRemoveFromSpaces`) so they float above everything.
That membership breaks programmatic movement: after the **first** programmatic move of the process lifetime, tao/AppKit-originated moves (`window.set_position`, `setFrameOrigin` alone) update AppKit's frame ledger but never reach the window server - the on-screen window freezes while `outer_position()` keeps reporting movement.
This was measured directly: per-tick position logs advanced by exactly one step per app launch while `CGWindowListCopyWindowInfo` bounds never changed.

Two approaches were tried:

1. **Rejected: leave the Space during motion.** `SLSRemoveWindowsFromSpaces` plus `setLevel(0)` makes moves land again, but the level demotion drops the pet behind ordinary windows for the whole motion - it visibly vanishes mid-walk and pops back at the end.
   Do not reintroduce this.
2. **Adopted: move both ledgers.** `macos_window::shift_window` reads the server's own bounds with `SLSGetWindowBounds`, moves the server window with `SLSMoveWindow`, then updates AppKit with `setFrameOrigin`.
   The server read comes first so both ledgers move together or not at all.
   Deltas must be whole points: fractional steps get floor-rounded by the window server, which made leftward walks measurably faster than rightward ones.

The walk loop lives on a native thread (`start_walk`), not in the renderer: an unfocused webview throttles JS timers to ~100ms+, which desynced movement from the CSS leg cycle.
Frame animation is a CSS `steps()` sprite sheet for the same reason.
