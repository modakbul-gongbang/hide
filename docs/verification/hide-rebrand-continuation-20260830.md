# Hide rebrand continuation verification

Date: 2026-08-30.

Worktree: `/Users/hoyeonlee/projects/herdr-ide.worktrees/hide-rebrand`.

Branch: `prd/hide-rebrand`.

Implementation commit under verification: `a25af41` (`Fix remote device selection targeting`).

## User-reported layout failure

The supplied screenshots [user-report-broken-layout-2026-08-30.png](evidence/hide-rebrand/user-report-broken-layout-2026-08-30.png) and [user-report-remote-mini-broken-2026-08-30.png](evidence/hide-rebrand/user-report-remote-mini-broken-2026-08-30.png) were opened directly before verification.

The local report showed the main content collapsed into a short intrinsic-height region, with the toolbar and status bar floating in the middle of a black window.

It also showed the pane fallback list instead of terminal surfaces and hardcoded `Session` tab labels.

The remote report showed a separate remote pane presentation, clipped panic output, and the same collapsed grid geometry.

The layout correction is in `0853ecb` and uses one pane-card/grid presentation for local and remote panes.

The workspace identity correction is in `f626816` and normalizes component-boundary paths before matching the focused checkout and authoritative pane layout.

The resulting pane grid fills the main viewport, renders SwiftTerm contents, retains pane headers with agent and cwd, and keeps the Workbench panel aligned to the same window height.

The tab strip now uses the Herdr tab payload label instead of the placeholder `Session` label.

## Mac mini selection and remote attach

The installed release app was started from `/Applications/hide.app` with exactly one `HerdrMacOS` process, PID `27163`.

A fresh classic Peekaboo snapshot identified the Mac mini row as `Mac mini` with a distinct selected-state value.

The exact fresh AX click receipt is [final-installed-mini-click-20260830.json](evidence/hide-rebrand/final-installed-mini-click-20260830.json).

After the click and an eight-second observation window, the fresh snapshot reported `Mac mini` as `Selected` instead of `Not selected`.

The main header changed to the remote device context and reported `mini ready`.

The post-click observation is [final-installed-remote-after-device-20260830.json](evidence/hide-rebrand/final-installed-remote-after-device-20260830.json).

The Swift-only fix is `a25af41` and contains only `macos/Sources/HerdrMacOS/HideUI.swift` and `macos/Sources/HerdrMacOS/ShellModel.swift`.

The fix gives each device row a stable `hide-device-*` accessibility target and a visible `Selected` or `Not selected` value.

It also keeps remote selection explicit and reports a missing SSH alias instead of silently falling back to the local device.

The noninteractive SSH failure path is covered by a login-shell command and an allocated PTY.

The attach command uses `ssh -tt` so the remote process receives a PTY and a non-empty terminal type.

The remote command runs through `zsh -ilc`, which restores the login-shell PATH containing the installed Herdr executable.

The attach wrapper suppresses raw remote stderr and emits a human-readable terminal initialization failure with the target and recovery action.

The core russh path independently requests a PTY with the validated endpoint TERM and rejects empty TERM or dimensions as an explicit `RemoteStage::Pty` failure.

No raw Rust panic text is used as the product failure message.

## Local and remote full-window evidence

Both screenshots were captured from the same installed `/Applications/hide.app` process and have dimensions 2880x1856.

| Surface | Evidence |
| --- | --- |
| Local | [final-installed-local-20260830.png](evidence/hide-rebrand/final-installed-local-20260830.png) |
| Remote mini, dedicated test workspace | [final-installed-remote-20260830.png](evidence/hide-rebrand/final-installed-remote-20260830.png) |

The local screenshot shows the full-height pane grid, real terminal contents, actual workspace and tab labels, bottom status bar, and Workbench panel.

The remote screenshot shows the same shared pane-card and grid geometry, a remote pane header with the mini cwd, terminal content, and the Workbench remote state.

The corresponding fresh observation JSON files are [final-installed-local-20260830.json](evidence/hide-rebrand/final-installed-local-20260830.json) and [final-installed-remote-20260830.json](evidence/hide-rebrand/final-installed-remote-20260830.json).

The dedicated workspace click receipt is [final-installed-remote-workspace-click-20260830.json](evidence/hide-rebrand/final-installed-remote-workspace-click-20260830.json).

## User-requested pane-less checkout deviation

The original PRD describes a pane-less checkout as an empty state that prompts the user to start a terminal.

The user explicitly changed this behavior to: selecting a checkout with no Herdr pane automatically creates a new tab and terminal pane in that checkout cwd.

The worktree row remains visible whenever the checkout exists, regardless of its current pane count.

This is an intentional product deviation from the original SC1, AC6, R3, and D-23 wording.

The reason is to make a selected checkout immediately ready to work in instead of showing a dead-end empty state.

The core focus event remains a pure selection operation, while the Swift selection policy requests the terminal start as the caller-facing orchestration step.

The regression test `paneLessCheckoutSelectionRequestsAnAutomaticTerminal` passed in the Swift suite.

The existing-pane path remains focus-only and is covered by `checkoutWithAExistingPaneOnlyChangesFocus`.

## Dedicated mini workspace boundary

The remote live check used only the dedicated workspace `w4M` labeled `hide-remote-verification` at `/tmp/hide-rebrand-remote-verify-20260830`.

The final remote screenshot was captured from that workspace before cleanup.

The exact workspace close command was `herdr workspace close w4M` on mini.

The exact empty test directory was removed with `rmdir /tmp/hide-rebrand-remote-verify-20260830` after the workspace closed.

The post-cleanup snapshot contains no `w4M`, `hide-remote-verification`, or test path match.

The cleanup evidence is [mini-cleanup-20260830.json](evidence/hide-rebrand/mini-cleanup-20260830.json).

No other mini workspace was closed, deleted, or recreated.

## Build and package verification

`cargo fmt --all -- --check` passed.

The Rust suite passed with 91 library tests, 25 FFI integration tests, and 0 doc-tests.

The Swift suite passed with 69 tests using `swift test --package-path macos --scratch-path /tmp/hide-finisher-swift`.

The release Swift build passed with `swift build --package-path macos --configuration release --disable-keychain --disable-sandbox --scratch-path /tmp/hide-finisher-swift`.

The core build artifacts were kept outside the worktree with `CARGO_TARGET_DIR=/tmp/hide-finisher-cargo`.

The release archive is `dist/hide-v0.1.0-macos-arm64.zip`.

The archive SHA-256 is `5bf8f4534b66c68b3be40ef2b24194a590412548736184e6016eba7af706f3d2`.

The bundled Herdr version is `0.8.2`.

The bundled Herdr SHA-256 is `bba6c79874689d5c8ec45811518ecf5cef9b521e61b081a9f56ddd406a482328`.

The installed and packaged `HerdrMacOS` executable SHA-256 is `e883ca79b9ab4df26d08466ad4dfea57140724aa823d8864cd2e0b45e715f6d7`.

`codesign --verify --deep --strict` passed for both the dist bundle and the installed bundle.

The T8 and Spotlight details are in [t8-spotlight-install-20260830.md](evidence/hide-rebrand/t8-spotlight-install-20260830.md).

## Spotlight verification

The installed bundle is `/Applications/hide.app`.

The bundle identifier is `me.grab.hide`.

The root volume and `/Applications` both report `Indexing enabled`.

`/usr/bin/mdfind -name hide` returns `/Applications/hide.app`.

`mdls` reports the installed bundle name and identifier as `hide.app` and `me.grab.hide`.

The installed app remains running as the single coordinated instance PID `27163` for the UI agent's follow-up verification.

## Delivery gates

No public push was performed.

No draft release was published.

The final dark-design taste approval remains a human gate.

The public push requires the user's explicit approval after the secret and personal-data scan result is attached.

The draft release publish action remains a user-only step.
