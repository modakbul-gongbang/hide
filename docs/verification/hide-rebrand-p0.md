# hide rebrand P0 launch verification

Status: Partially Done and blocked on one native verification action.

This note records the Finder/open zero-window regression work without claiming that the P0 is fixed or that V5 is green.

## Scope and tree

- The implementation worktree is `/Users/hoyeonlee/projects/herdr-ide.worktrees/hide-rebrand`.
- The branch is `prd/hide-rebrand`.
- The implementation base before this commit was `ff31ed6`.
- The branch remains a descendant of main commit `0b6c80679ac1d122c025deddbd67a55c0cd29fbb`.
- The approved PRD and interview source were not modified.
- The current implementation tree contains the P0 startup fix, candidate-02 icon unit, packaging updates, focused tests, and this verification note.
- No unrelated pB worktree or existing mini session was touched.

## P0 reproduction and native boundary

The reported regression is:

```text
open dist/hide.app
process remains alive, window count is 0
dist/hide.app/Contents/MacOS/HerdrMacOS
two windows are created and the app attaches to herdr
```

The protected preview process is PID 1755.

Its command is the direct executable at `dist/hide.app/Contents/MacOS/HerdrMacOS`.

The current observable window count for PID 1755 is 2.

The two windows are the main `hide` window and the Pet window.

The exact `open dist/hide.app` reproduction was not rerun after the fix because PID 1755 is the existing user preview and a second Finder launch would violate the single-instance boundary.

No post-fix Finder/open window count or screenshot exists yet.

The exact native evidence is therefore blocked and must not be inferred from process liveness, Swift tests, or the direct-executable preview.

Required next action: close PID 1755, launch only the freshly built `dist/hide.app` with `open`, then capture the sole-instance window count and a fresh screenshot.

## Observable launch trace and root-cause assessment

Before this change, `HerdrApplicationDelegate` eagerly initialized `ShellModel` before `applicationDidFinishLaunching` could construct its first `NSWindow`.

That path synchronously constructed `CoreBridge`, resolved the login-shell PATH, probed installed runtime versions and SHA-256 values, and could start a Herdr server before any window was visible.

The old server-start failure returned no process and no user-visible diagnostic.

This code path matches the reported Finder timing and silent-failure class, but the exact old Finder subprocess ordering was not captured in a fresh trace because the live user preview was preserved.

The new executable records `delegate.init.begin`, `delegate.init.ready`, `application.did_finish.begin`, `main_window.visible`, `runtime_initialization.begin`, `runtime_initialization.resolved`, `herdr.status`, server start/exit, and failure events through the `me.grab.hide` launch log category.

The next sole-instance Finder run is required to turn this instrumentation into native evidence.

## P0 implementation

- `CoreBridge` now creates a lightweight initial core and snapshot without runtime discovery or an external subprocess.
- `HerdrApplicationDelegate` creates, orders, and activates the main window before starting runtime discovery.
- Runtime resolution and login-shell PATH discovery run in detached startup work after the first window is visible.
- Login-shell and version/SHA-256 probes have bounded two-second waits and explicit logged failure outcomes.
- Server startup returns `notNeeded`, `started`, or `failed` instead of silently returning nil.
- Runtime resolution failure, core replacement failure, server launch failure, and server exit are rendered through `bridgeError` in the existing status bar.
- Reopen handling restores and activates the main window when macOS reopens the app without visible windows.
- The core status state and error kind are logged when they change so socket and attach failures remain externally observable.

The fix keeps the existing CoreBridge, C ABI, snapshot, and Pet boundaries.

Pet behavior and all Pet* modules remain preserved.

## Required P2 advisory checks

### Bundled Herdr version and SHA-256

- Declared release: `0.8.2`.
- Recorded SHA-256: `bba6c79874689d5c8ec45811518ecf5cef9b521e61b081a9f56ddd406a482328`.
- Fresh release staging actual version: `herdr 0.8.2`.
- Fresh release staging actual SHA-256: `bba6c79874689d5c8ec45811518ecf5cef9b521e61b081a9f56ddd406a482328`.
- The release staging bundle is `macos/build/assembled-release/hide.app`.
- The existing `dist/hide.app` contains the same verified Herdr version and SHA-256, but its app executable predates this P0 fix because PID 1755 is running from that bundle.

### Login-shell PATH inheritance

- `HideRuntimeEnvironment.childEnvironment()` obtains PATH from `zsh -ilc`.
- The no-PATH Finder-like test uses `/usr/bin:/bin` only as the explicit safe fallback when login-shell discovery is unavailable.
- The child environment is an allowlist containing HOME, USER, PATH, and optional SSH_AUTH_SOCK and HERDR_CONFIG_PATH.
- Runtime discovery uses the same login-shell PATH after the first window is visible.

### No CLI version lower-bound enforcement

- `AgentCLIAvailability.isUsable` is presence-only and does not compare a CLI version.
- The existing guidance behavior for unavailable or old Herdr installations remains separate from agent CLI availability.
- The Herdr runtime bundle version comparison remains because it is the PRD-mandated Herdr compatibility and integrity chain, not an agent CLI lower-bound gate.

### Credential non-handling

- hide does not provide credential UI or credential storage.
- hide passes only non-secret routing values to child tools.
- Authentication remains delegated to the Herdr server, SSH agent, and selected CLI.
- The focused policy tests protect the environment allowlist and the source contains no credential read, write, or display path.

## Icon and bundle evidence

- Selected source: `docs/assets/hide-icon-candidates/hide-icon-02.png`.
- Human decision: `2로 최종적으로 가자`.
- Source format and dimensions: 1254x1254 RGBA PNG.
- Prepared source: `macos/Resources/hide-icon-1024.png`, 1024x1024 RGBA PNG.
- Final icon: `macos/Resources/hide.icns`, 1024x1024 `ic12` macOS ICNS.
- Final ICNS SHA-256: `665b5082de2df100c72717e317b55238e8aa45d06958439b0c1ce8bcb2be7d8d`.
- The selected source asset is preserved.
- `Info.plist` declares `CFBundleIconFile` as `hide.icns`.
- The fresh dev bundle and fresh release staging bundle contain the same ICNS hash.
- The fresh release staging bundle has bundle identifier `me.grab.hide`, display name `hide`, URL scheme `herdr-ide`, Pet theme resources, valid Info.plist, and a valid deep code signature.

## Automated and package verification

- `cargo test --workspace --locked`: 82 Rust tests passed.
- `cargo test --workspace --locked` FFI contract suite: 23 tests passed.
- `swift test --package-path macos --disable-keychain --disable-sandbox`: 60 tests passed.
- Focused startup policy coverage includes Finder-like missing PATH, visible missing-runtime failure, visible unlaunchable-runtime failure, and immediate snapshot creation before runtime resolution.
- `git diff --check` passed.
- Swift linking emitted the existing macOS 15.2 object versus macOS 14.0 target warning, with exit status 0.

## Verification matrix

- V1: implementation-level build and suite evidence is green, but the Sasu V1 row remains NOT_RUN until the run is verified through the harness.
- V2: focused and full automated behavior evidence is green, but the Sasu V2 row remains NOT_RUN until the run is verified through the harness.
- V3: pending fresh native app screenshots and user-observable launch evidence.
- V4: not run by this P0 continuation, and no mini session was created or modified.
- V5: blocked pending the exact sole-instance `open dist/hide.app` window count and screenshot with the fresh dist bundle.
- V6: blocked by the separate public-push approval gate, and no public push or draft release publish was performed.

AC1, SC7, and V5 remain open at the native Finder boundary.

AC15 remains subject to the separate final dark-design taste review.

## Sasu state and completion boundary

The Sasu run remains active with open implementation tasks and verification rows.

No `sasu implement verify` or `sasu implement finalize` was run after this P0 change.

No complete receipt or implementation-result artifact was generated because the required native proof is still missing.

This note is an implementation and verification record, not a Done receipt.
