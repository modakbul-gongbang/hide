# hide rebrand implementation result

Status: The baseline P0 Finder/open zero-window regression is PASS on the fresh bundle, while the later catalog-projection correction still requires a fresh native proof.

The final dark-design taste review is approved by the user and recorded in [ac15-dark-taste-approval-20260830.md](evidence/hide-rebrand/ac15-dark-taste-approval-20260830.md).

## Implementation tree and current HEAD

- Worktree: `.`.
- Branch: `prd/hide-rebrand`.
- Main base: `0b6c80679ac1d122c025deddbd67a55c0cd29fbb`.
- Current HEAD at this report update: `f626816` (`Expose workspace::normalized_for_comparison for runtime path matching`).
- The current implementation tree contains the Rust catalog/focus projection correction, component-boundary path matching, Swift CoreBridge/ShellModel projection changes, WorkbenchPanel changes, regression tests, build changes, this report, the restored P0 report, and retained native evidence under `docs/verification/evidence/hide-rebrand/`.
- The current implementation tree is not yet represented by a fresh post-projection native proof or a Sasu verification attempt.
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

- PID `82773`, `/usr/bin/open dist/hide.app`, `2026-08-29T23:04:50Z`, exact bundle path `./dist/hide.app/Contents/MacOS/HerdrMacOS`, Peekaboo zero, System Events zero, screenshot `docs/verification/evidence/hide-rebrand/finder-open-no-window-2026-08-29T230450Z.png` with SHA-256 `6e49dd979c31adba7b3ca2398f32c970461e0534577fd946262771473887dd86`.
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
- Exact bundle executable: `./dist/hide.app/Contents/MacOS/HerdrMacOS`.
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

- Historical pre-projection baseline: Rust 82 tests, FFI 23 tests, and Swift 62 tests passed.
- Current focused projection suite: 10 Rust runtime tests passed.
- Current focused FFI regression: 1 test passed.
- Current full Rust workspace: 90 library tests and 25 FFI integration tests passed.
- Current Swift suite: 69 tests passed.
- `cargo fmt --all` and `git diff --check` passed for the current correction.
- Current fresh bundle build: `docs/verification/evidence/hide-rebrand/build-app-after-catalog-projection-fixed-20260830T040742Z.log` completed successfully with bundled Herdr `0.8.2` and the recorded binary SHA-256 match.
- Focused regression coverage includes `mainWindowPresentationIsVisibleBeforeRuntimeWorkStarts` and `finderLaunchWithoutAnActiveWorkspaceDoesNotScanTheProcessDirectory`.
- The prior PID `24041` Finder proof remains valid for the earlier P0 tree only; a fresh native proof after the catalog-projection correction is still required.
- Fresh bundle signature, Info.plist, Herdr binary pin, icon, and archive checks passed.

These already-passed suites were not rerun during this report-only continuation.

## Sasu v6 compatibility recovery

- The installed CLI is contract version `0.8.0` with implement schema support only for `sasu.implement.state.v7`.
- Its read-only status refusal is recorded in `docs/verification/evidence/hide-rebrand/sasu-v6-recovery-20260830T041416Z.txt` and explicitly says that the current `sasu.implement.state.v6` run is unsupported by v7.
- The v7 transition is commit `7d1e2e4`; its exact pre-v7 parent used for recovery is `33b332afde242789acb2f6dd5322228ca127303a`.
- A temporary CLI was built from that commit at `/tmp/sasu-v6-cli.G2rYX2/cli`. It reports contract version `0.8.0` and successfully reads the existing run as `sasu.implement.state.v6`, `hide-rebrand: active`, with 11 open tasks, 20 open acceptance criteria, and verification `NOT_RUN`.
- The v6 read-only status reports the stored baseline HEAD as `ff19b4daf99a12f1ce9903cb1edc987d995a8f24`; the current implementation worktree HEAD is separately `f626816`. This is recorded as state/source freshness context, not as a verification pass.
- `agents/runs/hide-rebrand/state.json` SHA-256 was `38a0b9c6d06694a13dc3feb920a159cd39af39e847161271915bcc65375c82e4` before and after the v6 status command.

The exact recovery path is:

1. Preserve the existing v6 `state.json`, PRD, qa-log, source, and evidence bytes.
2. Rebuild or retain a v6 CLI from Sasu commit `33b332afde242789acb2f6dd5322228ca127303a` in an isolated temporary directory.
3. Use that binary for the existing run's read-only status and then, only after implementation tasks, acceptance checks, and final artifacts are current, run `implement verify --state agents/runs/hide-rebrand/state.json --json`.
4. If a mutating v6 command reports ownership by another session, pass the user's verbatim approval through its required `--adopt "<verbatim user approval>"` flag; do not synthesize approval and do not create a new slug.
5. Run `implement finalize --state agents/runs/hide-rebrand/state.json` only after a fresh v6 verification PASS and after the remaining human gates are resolved.

No migration, replacement run, state overwrite, or v7 verify/finalize command was performed.

## Remaining boundary

- Baseline native P0 and the caller-observable V5 launch proof are PASS for PID `24041`; post-projection native proof remains open.
- Sasu verify/finalize was not run.
- No public push was performed.
- No draft release was published.
- No mini session or workspace was created or modified.
- The final dark-design taste approval is satisfied by the user's exact approval “미리 승인. 여튼 끝까지 마무리해바”.

The next action is the separate public-push approval after the sanitized-tree and reachable-history findings are reviewed.
