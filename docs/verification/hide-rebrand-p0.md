# hide rebrand P0 launch verification

Status: The Finder/open P0 regression is fixed and has fresh caller-observable evidence on the rebuilt bundle.

The final dark-design taste review was subsequently approved by the user with “미리 승인. 여튼 끝까지 마무리해바”.

The approval evidence is [ac15-dark-taste-approval-20260830.md](evidence/hide-rebrand/ac15-dark-taste-approval-20260830.md).

This note records the implementation tree, the failed native attempts, the root cause, the fix, and the successful native proof.

## Scope and tree

- The implementation worktree is `.`.
- The branch is `prd/hide-rebrand`.
- The branch is a descendant of main commit `0b6c80679ac1d122c025deddbd67a55c0cd29fbb`.
- The prior committed implementation and icon unit is `70006b5747c01ee2d17a02c97c82983e4435d38e` (`Fix Finder launch startup ordering`).
- The current P0 implementation tree is the changes on top of that commit in `HerdrApp.swift`, `WorkbenchPanel.swift`, `OperationalPolicyTests.swift`, and this evidence record.
- The approved PRD and interview source were not modified.
- No unrelated pB worktree, existing session, or mini workspace was touched.

## P0 acceptance contract

The required caller is the Finder-equivalent command `/usr/bin/open dist/hide.app`.

The acceptance result is based on a positive main Hide window count and a fresh screenshot from that command.

Direct execution of `dist/hide.app/Contents/MacOS/HerdrMacOS` is diagnostic comparison only.

The app must create the main window before runtime discovery can block, fail, invoke a privacy prompt, or attach to Herdr.

Any runtime or attach failure must remain visible in that already-created window through the existing startup diagnostic state.

## Failed native attempts and retained evidence

The following attempts were made against the fresh `dist/hide.app` bundle before the workspace-tree fix.

Each attempt used exactly one app instance, and the agent-created process was terminated with an exact PID-targeted `TERM` after the evidence was captured.

No unrelated process or user instance was terminated.

### Attempt A: PID 82773, initial post-ordering run

- Command: `/usr/bin/open dist/hide.app`.
- UTC timestamp: `2026-08-29T23:04:50Z`.
- Process: PID `82773`.
- Bundle executable: `./dist/hide.app/Contents/MacOS/HerdrMacOS`.
- Bundle identifier: `me.grab.hide`.
- The process remained alive after `open` returned.
- Peekaboo window inventory returned zero windows.
- System Events reported zero windows.
- `peekaboo see --app hide --json` could not attribute a visible target.
- The allowlisted environment inspection recorded `USER`, `LOGNAME`, `HOME`, `SHELL`, and inherited `PATH` values without printing secrets.
- OSLog reached delegate initialization, initial core creation, `application.did_finish.begin`, `main_window.visible`, and `application.did_finish.ready`, but did not record a later resolved runtime or visible failure.
- Screenshot: `docs/verification/evidence/hide-rebrand/finder-open-no-window-2026-08-29T230450Z.png`.
- Screenshot SHA-256: `6e49dd979c31adba7b3ca2398f32c970461e0534577fd946262771473887dd86`.
- The screenshot shows the desktop and a `hide` Desktop-folder privacy prompt, with no visible main Hide window.
- PID `82773` was terminated with `/bin/kill -TERM 82773`, and the remaining Herdr instance count was verified as zero.

### Attempt B: PID 39718, after unconditional window ordering

- Command: `/usr/bin/open dist/hide.app`.
- UTC timestamp: `2026-08-29T23:17:02Z`.
- Process: PID `39718`.
- Bundle executable: `./dist/hide.app/Contents/MacOS/HerdrMacOS`.
- The process remained alive after `open` returned.
- Peekaboo window inventory returned zero externally observable windows.
- System Events reported zero windows.
- Internal launch logging recorded `main_window.visible` with `windows_1`, but the native observers could not see that window.
- `peekaboo see --app hide --json` again failed target attribution.
- Screenshot: `docs/verification/evidence/hide-rebrand/finder-open-no-window-orderfront-2026-08-29T231702Z.png`.
- Screenshot SHA-256: `7cec16d790e1fb116569961a7573fabe61fa39ad45e4f3ae440a09eeda943a3d`.
- The screenshot again shows the Desktop-folder privacy prompt and no visible main Hide window.
- PID `39718` was terminated with `/bin/kill -TERM 39718`, and the remaining Herdr instance count was verified as zero.

### Attempt C: PID 69815, after deferring runtime initialization

- Command: `/usr/bin/open dist/hide.app`.
- UTC timestamp: `2026-08-29T23:23:51Z`.
- Process: PID `69815`.
- Bundle executable: `./dist/hide.app/Contents/MacOS/HerdrMacOS`.
- The process remained alive after `open` returned.
- Peekaboo window inventory returned zero externally observable windows.
- System Events reported zero windows.
- Internal launch logging recorded `main_window.visible` with `windows_1` and `application.did_finish.ready`, but no `main_window.pre_runtime` event occurred.
- Screenshot: `docs/verification/evidence/hide-rebrand/finder-open-current-2026-08-29T232351Z.png`.
- Screenshot SHA-256: `e5df21749b1639fbd7dfb8e65c7ad976fe0fe915f2e315accae478dd1360e7eb`.
- The screenshot again shows the Desktop-folder privacy prompt and no visible main Hide window.
- A native `sample` trace retained the main-thread failure evidence at `docs/verification/evidence/hide-rebrand/finder-open-no-window-sample-2026-08-30.txt`.
- Sample SHA-256: `fb7ba555292c12cf64222ad5625c7a16fb931d14aca3d46596430533c5a53788`.
- The sample captured `NSApplication.run` in SwiftUI layout, then `WorkbenchPanel.body`, `WorkspaceTree.load`, recursive `WorkspaceTree.node`, and `FileManager.contentsOfDirectoryAtURL`.
- The sample reported a 6.4G physical footprint while the main thread was blocked in that recursive scan.
- PID `69815` was terminated with `/bin/kill -TERM 69815`, and the remaining Herdr instance count was verified as zero.

These observations distinguish the Finder/open failure from a simple process-liveness issue.

The direct executable launched from the repository shell working directory created two windows and attached to Herdr because that working directory was a small project tree rather than the broad Finder launch directory.

## Root cause

The earlier delegate and runtime ordering fixes were necessary but insufficient.

`CoreBridge` now creates an initial snapshot without synchronous runtime discovery, and `HerdrApplicationDelegate` orders the main window before scheduling runtime startup.

However, `WorkbenchPanel.activeRoot` previously fell back from `model.focusedPath` to `model.core.workspaceRoot`.

When no workspace registration existed, `Snapshot.navigator.root_path` was nil and `workspaceRoot` resolved to the process current directory.

Finder/open supplied a broad current directory, so the first SwiftUI layout recursively scanned that directory on the main thread.

The scan triggered the Desktop privacy prompt and starved the main run loop before the deferred runtime block and the native observers could see a usable window.

This explains why internal logging could say `windows_1` while Peekaboo and System Events reported zero, and why direct executable launch behaved differently.

The persisted hide state used during diagnosis had no workspace registrations, so the empty-workspace path is a valid first-run state rather than an invalid fixture.

## P0 implementation

- `MainWindowPresentation` activates the regular application policy, orders the window front regardless of activation state, and makes it key.
- `HerdrApplicationDelegate` presents the main window before runtime discovery and reasserts or observes visibility on the next main-run-loop turn.
- Runtime initialization is dispatched after `application.did_finish.ready`, so login-shell and CLI work cannot delay initial window creation.
- `WorkbenchPanel.activeRoot` now uses only the focused workspace path and never substitutes the process current directory when no workspace is selected.
- A nil active root renders a visible `No workspace` empty state instead of scanning an implicit directory.
- Workspace-tree loading is performed in a detached task keyed by the active root, with cancellation checked before publishing results to the view.
- `WorkspaceFileNode` is `Sendable` for the detached tree-load boundary.
- Existing `bridgeError` rendering remains the visible path for runtime resolution, core replacement, server launch, server exit, socket, and attach failures.
- Existing bounded login-shell and runtime probes retain explicit logged failure outcomes.

The fix removes the invalid process-directory fallback instead of adding a path-specific exception.

It preserves the CoreBridge, C ABI, snapshot, and Pet boundaries.

Pet behavior and all Pet* modules remain preserved.

## Fresh Finder/open proof

The following proof was run after the workspace-tree fix and after a fresh release build.

- Build command: `./scripts/build-app.sh`.
- Finder-equivalent launch command: `/usr/bin/open dist/hide.app`.
- UTC launch timestamp: `2026-08-29T23:31:34Z`.
- `open` exit status: `0`.
- Process PID: `24041`.
- Exact process path: `./dist/hide.app/Contents/MacOS/HerdrMacOS`.
- Process and bundle identity: `me.grab.hide` from the fresh `dist/hide.app` bundle.
- The preflight instance count was zero, and the launch produced exactly one HerdrMacOS instance.
- The allowlisted launch-environment inspection recorded `USER=local-user`, `LOGNAME=local-user`, `HOME=~`, `SHELL=zsh`, and inherited PATH values without printing credentials.
- Peekaboo reported the main Hide window as window `6710`, bounds `x=144 y=72 width=1440 height=928`, on screen, frontmost, and key.
- Peekaboo separately reported the Pet window as window `6717`, bounds `x=0 y=617 width=500 height=500`.
- System Events reported two windows for the app, consisting of the main Hide window and the separate Pet window.
- Full desktop screenshot: `docs/verification/evidence/hide-rebrand/finder-open-current-2026-08-29T233134Z.png`.
- Full desktop screenshot SHA-256: `fea56895319a2d7db7af078d53fbd56867d0b48f6a2ef9b3a12699ec755d1b2e`.
- Main-window screenshot: `docs/verification/evidence/hide-rebrand/finder-open-main-window-2026-08-29T233134Z.png`.
- Main-window screenshot SHA-256: `483a24aaa9937a35fcf17368d605f3b9b927f8aee2ad21ed6fdb48a712fb289b`.
- The main-window screenshot is a fresh native capture of the dark Hide UI, sidebar, panes, and Workbench.
- The Pet window is not used as the main-window proof.
- The full desktop capture includes the earlier Desktop-folder privacy prompt, but the main Hide window is visibly present behind it.
- The app-only capture isolates the main Hide window and contains no prompt over the product surface.

The Finder/open caller-observable result is PASS for P0, AC1, and SC7 on this fresh bundle.

This is the first positive proof after the three retained zero-window attempts.

## Fresh startup trace

The OSLog trace for PID `24041` includes the following sequence.

```text
2026-08-30 08:31:34.927 delegate.init.begin
2026-08-30 08:31:34.932 herdr.status not_connected
2026-08-30 08:31:34.932 core_bridge.init.ready initial_core_without_runtime duration 4
2026-08-30 08:31:34.933 delegate.init.ready duration 6
2026-08-30 08:31:34.989 application.did_finish.begin
2026-08-30 08:31:35.059 main_window.visible source_launch_visible_true_windows_1
2026-08-30 08:31:35.066 application.did_finish.ready duration 76
2026-08-30 08:31:35.420 herdr.status connected
2026-08-30 08:31:35.420 core.error pane.attach_failed existing session issue
2026-08-30 08:31:35.425 main_window.observed source_launch_visible_true_windows_3
2026-08-30 08:31:35.426 main_window.pre_runtime visible_true_windows_3
2026-08-30 08:31:35.426 runtime_initialization.begin
2026-08-30 08:31:36.608 login_shell_path.ready available
2026-08-30 08:31:37.651 runtime_initialization.resolved selected_live-socket_v0.8.2 duration 2224
2026-08-30 08:31:37.652 herdr.status not_connected
2026-08-30 08:31:37.652 core_bridge.replace.ready runtime_live-socket
2026-08-30 08:31:37.652 runtime_initialization.server_not_needed
2026-08-30 08:31:38.082 herdr.status connected
```

The existing session attach error is externally observable and did not prevent the main window from appearing.

The trace proves that the main window was visible before login-shell PATH resolution, runtime selection, and socket replacement.

## Required P2 advisory checks

### Bundled Herdr version pin and SHA-256

- Declared release version: `0.8.2`.
- Recorded Herdr SHA-256: `bba6c79874689d5c8ec45811518ecf5cef9b521e61b081a9f56ddd406a482328`.
- Fresh release staging actual version: `herdr 0.8.2`.
- Fresh release staging actual SHA-256: `bba6c79874689d5c8ec45811518ecf5cef9b521e61b081a9f56ddd406a482328`.
- The fresh release staging bundle is `macos/build/assembled-release/hide.app`.
- Build output recorded archive SHA-256: `6bdaf91a2a56a2981680f11651be37397a42bf7bf952c775ebd34af47b842a33`.
- The fresh dist bundle and release staging bundle contain the verified Herdr version and binary hash.

The declared version, actual bundled binary version, and recorded SHA-256 match.

### Login-shell PATH inheritance

- `HideRuntimeEnvironment.childEnvironment()` obtains PATH through `zsh -ilc`.
- The Finder-like app launch environment was observed through an allowlist containing `HOME`, `USER`, `PATH`, and optional routing variables only.
- Runtime OSLog recorded `login_shell_path.ready available` before runtime resolution.
- The app does not depend on a personal hardcoded executable path.

The login-shell PATH inheritance contract is PASS for this run.

### No CLI version lower-bound enforcement

- `AgentCLIAvailability.isUsable` is presence-based and does not compare a CLI version.
- The unavailable or old installation guidance behavior remains separate from agent CLI availability.
- The Herdr runtime bundle version and SHA chain remains because it is the PRD-mandated runtime integrity check, not a CLI lower-bound gate.
- The focused policy tests cover the presence-only CLI availability contract.

The hide app does not impose a CLI version lower-bound gate.

### Credential non-handling

- hide has no credential UI.
- hide has no credential storage path.
- hide does not read, display, or persist credentials.
- Authentication remains delegated to the Herdr CLI, Herdr server, SSH agent, and the selected external tooling.
- The child environment uses the documented non-secret allowlist and did not emit credential values in the captured evidence.

The credential non-handling contract is PASS.

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
- The fresh release staging bundle has bundle identifier `me.grab.hide`, display name `hide`, URL scheme `herdr-ide`, Pet theme resources, a valid Info.plist, and a valid deep code signature.

The icon choice is authorized by the recorded human decision.

Final dark-design taste approval is satisfied by the later human gate.

## Automated and package verification

- `cargo test --workspace --locked`: 82 Rust tests passed.
- FFI contract suite: 23 tests passed.
- `swift test --package-path macos --disable-keychain --disable-sandbox --no-parallel`: 62 tests passed.
- Focused startup coverage includes `mainWindowPresentationIsVisibleBeforeRuntimeWorkStarts`.
- Focused Finder-like workspace coverage includes `finderLaunchWithoutAnActiveWorkspaceDoesNotScanTheProcessDirectory`.
- Existing focused policy coverage includes visible missing-runtime failure, visible unlaunchable-runtime failure, immediate snapshot creation before runtime resolution, PATH behavior, CLI presence-only behavior, and credential environment allowlisting.
- `./scripts/build-app.sh` completed successfully for the fresh bundle and archive.
- Code signing, Info.plist validation, bundled Herdr extraction, icon verification, and archive creation completed successfully.
- Swift linking emitted the existing macOS 15.2 object versus macOS 14.0 target warning with exit status 0.
- `git diff --check` is required after the report update and must pass before commit.

## Verification matrix

- V1: implementation-level build and automated suite evidence is green, while the Sasu V1 row remains NOT_RUN until the run is verified through the harness.
- V2: focused and full automated behavior evidence is green, while the Sasu V2 row remains NOT_RUN until the run is verified through the harness.
- V3: PASS for the fresh native Finder/open launch, with the main Hide window and Pet window separately observed.
- V4: not run by this P0 continuation, and no mini session or dedicated mini workspace was created.
- V5: PASS for the caller-observable Finder/open P0 proof on the fresh dist bundle, including positive window count, startup trace, and fresh screenshot.
- V6: blocked by the separate public-push approval gate, and no public push or draft release publish was performed.

AC1, SC7, and the native P0 portion of V5 are PASS for the fresh bundle.

AC15 and the final dark-design taste decision are satisfied; public-push approval remains pending.

## Sasu state and completion boundary

The Sasu run remains active with open implementation tasks and verification rows.

No `sasu implement verify` or `sasu implement finalize` was run after this P0 change.

This record is not a Done receipt and does not claim public-push approval or release publication.

The next authorized boundary is the separate public-push approval after review of the sanitized-tree and reachable-history findings.
