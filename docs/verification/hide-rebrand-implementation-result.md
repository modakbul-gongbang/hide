# hide rebrand implementation result

Status: The P0 Finder/open zero-window regression is fixed and caller-observable native evidence is PASS on the fresh bundle.

The final dark-design taste review is still a human gate and is not approved here.

## Implementation tree and current HEAD

- Worktree: `/Users/hoyeonlee/projects/herdr-ide.worktrees/hide-rebrand`.
- Branch: `prd/hide-rebrand`.
- Main base: `0b6c80679ac1d122c025deddbd67a55c0cd29fbb`.
- Current HEAD before this coherent fix commit: `70006b5747c01ee2d17a02c97c82983e4435d38e`.
- Implementation tree on top of that HEAD: `HerdrApp.swift`, `WorkbenchPanel.swift`, `OperationalPolicyTests.swift`, `docs/verification/hide-rebrand-p0.md`, this report, and the retained native evidence files under `docs/verification/evidence/hide-rebrand/`.
- The final current HEAD is the coherent commit that contains this implementation tree and is reported separately by the handoff after commit.
- The approved PRD and interview source remain unchanged.

## What changed

- The main Hide window is activated, ordered front regardless of Finder activation state, made key, and re-observed on the next main-run-loop turn.
- Runtime initialization is deferred until after the first window is visible.
- The Workbench no longer falls back to the process current directory when no workspace is focused.
- A missing active workspace renders a visible `No workspace` state instead of recursively scanning Finder's broad launch directory.
- Workspace-tree loading is detached from the main actor and publishes only non-cancelled results.
- Existing bridge diagnostics remain the visible failure path for runtime, socket, server, and attach errors.
- Pet behavior, the Pet modules, the Pet theme packaging, the herdr-ide URL scheme, and candidate-02 icon integration remain preserved.

## P0 failure evidence

Three exact Finder/open attempts remained alive with zero externally observed windows before the Workbench fix.

- PID `82773`, `/usr/bin/open dist/hide.app`, `2026-08-29T23:04:50Z`, exact bundle path `/Users/hoyeonlee/projects/herdr-ide.worktrees/hide-rebrand/dist/hide.app/Contents/MacOS/HerdrMacOS`, Peekaboo zero, System Events zero, screenshot `docs/verification/evidence/hide-rebrand/finder-open-no-window-2026-08-29T230450Z.png` with SHA-256 `6e49dd979c31adba7b3ca2398f32c970461e0534577fd946262771473887dd86`.
- PID `39718`, `/usr/bin/open dist/hide.app`, `2026-08-29T23:17:02Z`, the same exact bundle path, Peekaboo zero, System Events zero, screenshot `docs/verification/evidence/hide-rebrand/finder-open-no-window-orderfront-2026-08-29T231702Z.png` with SHA-256 `7cec16d790e1fb116569961a7573fabe61fa39ad45e4f3ae440a09eeda943a3d`.
- PID `69815`, `/usr/bin/open dist/hide.app`, `2026-08-29T23:23:51Z`, the same exact bundle path, Peekaboo zero, System Events zero, screenshot `docs/verification/evidence/hide-rebrand/finder-open-current-2026-08-29T232351Z.png` with SHA-256 `e5df21749b1639fbd7dfb8e65c7ad976fe0fe915f2e315accae478dd1360e7eb`.
- The retained sample for PID `69815` is `docs/verification/evidence/hide-rebrand/finder-open-no-window-sample-2026-08-30.txt` with SHA-256 `fb7ba555292c12cf64222ad5625c7a16fb931d14aca3d46596430533c5a53788`.
- The sample shows the main thread in SwiftUI layout, `WorkbenchPanel`, recursive `WorkspaceTree`, and `FileManager.contentsOfDirectoryAtURL`, with a 6.4G physical footprint.
- Each failed test PID was cleaned up with an exact `/bin/kill -TERM <pid>`, followed by a zero-instance check.

The failure was a main-thread recursive scan caused by the process-directory fallback for a nil workspace root.

The Desktop privacy prompt was a visible consequence of that broad scan, not a credential flow.

## P0 success evidence

- Build: `./scripts/build-app.sh`.
- Launch: `/usr/bin/open dist/hide.app`.
- UTC timestamp: `2026-08-29T23:31:34Z`.
- `open` exit status: `0`.
- Agent-created verification PID: `24041`.
- Exact bundle executable: `/Users/hoyeonlee/projects/herdr-ide.worktrees/hide-rebrand/dist/hide.app/Contents/MacOS/HerdrMacOS`.
- Peekaboo: Hide main window `1440x928`, on screen, frontmost, and key, plus a separate Pet window `500x500`.
- System Events window count: `2`.
- Full screenshot: `docs/verification/evidence/hide-rebrand/finder-open-current-2026-08-29T233134Z.png` with SHA-256 `fea56895319a2d7db7af078d53fbd56867d0b48f6a2ef9b3a12699ec755d1b2e`.
- Focused main-window screenshot: `docs/verification/evidence/hide-rebrand/finder-open-main-window-2026-08-29T233134Z.png` with SHA-256 `483a24aaa9937a35fcf17368d605f3b9b927f8aee2ad21ed6fdb48a712fb289b`.
- OSLog includes `main_window.pre_runtime`, `runtime_initialization.resolved selected_live-socket_v0.8.2`, and final `herdr.status connected`.
- PID `24041` was terminated with an exact `/bin/kill -TERM 24041` after capture, and a follow-up process check reported `pid_24041_remaining=0` with no other HerdrMacOS instance.

The Finder/open P0 result is PASS for AC1 and SC7.

## P2 and package evidence

- Declared and actual bundled Herdr version: `0.8.2`.
- Declared, recorded, and actual bundled Herdr SHA-256: `bba6c79874689d5c8ec45811518ecf5cef9b521e61b081a9f56ddd406a482328`.
- Login-shell PATH uses `zsh -ilc` and the fresh startup trace records `login_shell_path.ready available`.
- Hide performs no CLI version lower-bound gate.
- Hide does not provide credential UI, credential storage, or credential handling, and authentication remains delegated to the Herdr CLI/server and SSH agent.
- Candidate 02 source is `docs/assets/hide-icon-candidates/hide-icon-02.png`, selected by the human decision `2로 최종적으로 가자`.
- Final icon is `macos/Resources/hide.icns`, a 1024x1024 `ic12` ICNS with SHA-256 `665b5082de2df100c72717e317b55238e8aa45d06958439b0c1ce8bcb2be7d8d`.

## Automated verification

- Rust workspace: 82 tests passed.
- FFI suite: 23 tests passed.
- Swift suite: 62 tests passed.
- Focused regression coverage includes `mainWindowPresentationIsVisibleBeforeRuntimeWorkStarts` and `finderLaunchWithoutAnActiveWorkspaceDoesNotScanTheProcessDirectory`.
- Fresh bundle build, signature, Info.plist, Herdr binary pin, icon, and archive checks passed.

These already-passed suites were not rerun during this report-only continuation.

## Remaining boundary

- Native P0 and the caller-observable V5 launch proof are PASS.
- Sasu verify/finalize was not run.
- No public push was performed.
- No draft release was published.
- No mini session or workspace was created or modified.
- The final dark-design taste approval remains pending.

The next action is to present the fresh main-window screenshot and related dark-design evidence at the product taste gate, then stop at `OBSERVER_BLOCK` kind `product` without claiming approval.
