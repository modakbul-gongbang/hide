# Verification

This guide answers the question asked before a change is called verified: which check proves this claim, and how to run it without touching the operator's work.
It owns only what no other document does: the choice of check, the tools for native QA, and the traps that have made a check prove nothing.
The gates are listed in [CONTRIBUTING.md](../CONTRIBUTING.md), build identity lives in [dev-runtime.md](dev-runtime.md), and runtime isolation and measurement live in [PERFORMANCE_TESTING.md](PERFORMANCE_TESTING.md).

## Pick the check by the claim

A claim is verified by the check that observes what a caller of the behavior observes; a green `verify` proves only the lanes it ran.

| The change claims | Check | Owner |
| --- | --- | --- |
| Core state, a runtime event, a Herdr fixture, the hided wire | `bash scripts/verify-cargo.sh test` | [CONTRIBUTING.md: CI gates](../CONTRIBUTING.md#ci-gates) |
| Web or desktop logic that needs no window | `bash scripts/verify-web.sh` | [CONTRIBUTING.md: CI gates](../CONTRIBUTING.md#ci-gates) |
| A flow a user performs in the web shell | `pnpm --dir web e2e` | [Before an e2e run](#before-an-e2e-run) |
| A desktop app behavior: window, menu, native view, relaunch, a daemon that goes away | `pnpm --dir desktop e2e` | [BUILD.md: The desktop app](BUILD.md#the-desktop-app) |
| A browser display | The commands in its guide | [BROWSER_DISPLAYS.md: Verification](BROWSER_DISPLAYS.md#verification) |
| A screen matches its design | `node scripts/design-review.mjs review <slug> --baseline <bundle>` | [DESIGN_WORKFLOW.md](DESIGN_WORKFLOW.md#reference-bundle-and-review-run) |
| Responsiveness, rendering, CPU, or memory | The layered procedure | [PERFORMANCE_TESTING.md](PERFORMANCE_TESTING.md) |
| The installed app carries the fix | A rebuilt package, then the identity checks | [dev-runtime.md](dev-runtime.md), [INSTALL.md](INSTALL.md#verify-the-installed-app) |
| A remote device's behavior | A private Herdr on the device | [A device check](#a-device-check) |
| Anything the rows above cannot reach yet | Manual QA on an isolated candidate | [Manual native QA](#manual-native-qa) |

A scenario someone would check by hand becomes a spec when it can.
Playwright drives the renderer over its own connection rather than through OS input, so a spec needs no keyboard focus and cannot type into another app.
Manual QA covers what a spec cannot reach yet, and the pull request's Evidence says the check was manual.

## Before an e2e run

- Both suites run built output: `pnpm --dir web e2e` rebuilds `web/dist` and `pnpm --dir desktop e2e` rebuilds `desktop/dist`, but neither builds `target/debug/hided` or `hide`.
  After a merge or checkout that touches `herdr-core/` or `hided/`, run `bash scripts/verify-cargo.sh cli` first; a spec that fails on behavior main already has usually means the binary predates the merge.
- A single spec run through `pnpm --dir web exec playwright test` skips the build, so run `pnpm --dir web build` first.
- Spec filters resolve against the package directory: `pnpm --dir web exec playwright test e2e/overview.spec.ts`, never `web/e2e/overview.spec.ts`.
  A wrong filter prints "No tests found" and exits 1, which a pipe into `tail` hides; set `-o pipefail` and read the "N passed" line before trusting the result.
- Desktop e2e windows open on the operator's screen, behind their windows.
  Run `pnpm --dir desktop e2e --grep-invert @needs-focus` locally, and leave the focus tests and repeated runs (`--repeat-each`) to CI.
- A relaunch that attaches to a running daemon swaps its page within tens of milliseconds, which Playwright can miss; use `relaunch()` from `desktop/e2e/fixture.ts`, which reads the window through the main process.

## Writing a fixture

- Copy the whole isolation environment from `web/e2e/herdr-fixture.ts` and `desktop/e2e/fixture.ts`, never a subset; [PERFORMANCE_TESTING.md](PERFORMANCE_TESTING.md#3-isolate-runtime-state-before-making-fixtures) lists every variable and why.
- Herdr starts a pane's shell from the server's `SHELL`, so a fixture sets `SHELL=/bin/zsh` beside its private `HOME`.
  The CI runner's login shell is bash, where a prompt planted in the fixture's `.zshrc` never appears; reproduce that with `SHELL=/bin/bash pnpm --dir web e2e`.
- A private `HOME` has no Claude or Codex login, because each CLI keys its credential to `HOME`.
  The fixtures run a compiled `claude` shim instead, which proves the pipeline and not an agent.
  A claim about a real agent session needs the real CLI inside an otherwise isolated fixture: a shim on the pane's `PATH` that runs the CLI under the operator's `HOME`, while the server keeps its private one.

## Manual native QA

Read [PERFORMANCE_TESTING.md](PERFORMANCE_TESTING.md#2-identify-the-build-and-protect-the-operator) sections 2 and 3 first; they own isolation and the foreground boundary, and this section adds the tools.

### Why targeting is the hard part

- The operator's `/Applications/hide.app` and a packaged candidate from `desktop/out/` share the name `hide` and the bundle id `me.grab.hide.desktop`.
  Anything that resolves an app by name or bundle id reaches whichever one it finds first: open-computer-use, System Events (even `first process whose unix id is N` dereferences by name), `open -a`, and `quit app id`.
- An unpackaged `pnpm --dir desktop dev` instance and every desktop e2e instance run as `Electron` (`com.github.Electron`), so those collide with each other instead.
- Without its own `HIDE_DESKTOP_USER_DATA_DIR`, a dev instance focuses the operator's app and exits ([dev-runtime.md](dev-runtime.md#one-instance-per-profile)).
- `pgrep -fl 'hide.app/Contents/MacOS/hide'` also matches the hcoord daemon, which the bundle's executable runs as Node; the app is the PID that owns windows.
- A global input event (a CGEvent tap, typing with `--foreground`, a pointer move) lands in whatever is frontmost when it is delivered, and a frontmost check a moment earlier is not a guard.
  In September 2026 such events selected a row in the operator's Explorer and activated the operator's app in the middle of a run.

### Tools

| Tool | Addresses | Use it for | Not for |
| --- | --- | --- | --- |
| Playwright `_electron` through `desktop/e2e/fixture.ts` | The app it launched | Typing, chords and clicks through the renderer, without OS focus | Native input: a pinch, a native drag, IME composition (tag those `@needs-focus`) |
| `screencapture -x -o -l <window-id>` | One window | A current frame of the candidate without activating it | Minimized, hidden, or off-Space windows |
| The window list below | PID to window id | Resolving the candidate's window id before each capture | |
| open-computer-use (`ocu`, or its MCP server) | An app name or bundle id, never a PID | Reading the accessibility tree; `click` on an element index and `set_value` on a candidate whose name answers exactly one process | A packaged candidate while the operator's app runs; keyboard input into the renderer, which 0.3.5 does not deliver reliably to Chromium; `click_method: global` |
| `herdr pane read <pane> --source recent --lines <N>` under the private socket | One pane | Terminal content without a screenshot | History of a Claude or Codex pane: they draw in the alternate screen, so `recent` is what is visible, and history comes from the transcript |

A capture shows an old frame unless the candidate was launched with `--disable-backgrounding-occluded-windows`, because Chromium stops painting an occluded window.
Before trusting open-computer-use's target, check that `ocu list-apps` shows the candidate's name once and that no other process answers it.
The Peekaboo procedure this repository used to name is retired; Peekaboo is no longer part of the QA toolchain.

The window list prints owner PID, window id, owner name, and title for every on-screen window:

```sh
osascript -l JavaScript -e 'ObjC.import("CoreGraphics"); ObjC.deepUnwrap(ObjC.castRefToObject($.CGWindowListCopyWindowInfo($.kCGWindowListOptionOnScreenOnly | $.kCGWindowListExcludeDesktopElements, 0))).filter(w => w.kCGWindowLayer === 0).map(w => [w.kCGWindowOwnerPID, w.kCGWindowNumber, w.kCGWindowOwnerName, w.kCGWindowName || ""].join("\t")).join("\n")'
```

### Procedure

1. Start a private Herdr server with the pinned binary and the isolation environment, and prove `herdr workspace list` is empty under that environment before creating fixtures.
2. Clear every inherited `HERDR_*` and `HIDE_*` variable, then launch the candidate with its own `HOME`, `HIDE_STATE_DIR`, `HIDE_DESKTOP_USER_DATA_DIR`, and `HERDR_SOCKET_PATH`, and `HERDR_BIN_PATH` naming the pinned binary.
   Pass `--hide-show-inactive --disable-backgrounding-occluded-windows` so it opens behind the operator's windows and keeps painting: `pnpm --dir desktop dev --hide-show-inactive --disable-backgrounding-occluded-windows` appends them to `electron .`, and a `--` before them would reach Electron literally.
   The `host.start` line in `$HIDE_DESKTOP_USER_DATA_DIR/logs/desktop.log` records `packaged`, `version`, and `show_inactive`.
3. Resolve the candidate's PID from the process your launch started, never by name, then its window id from the window list.
   Re-resolve both after any restart.
4. Before each action, confirm the target again; after each action, observe again with a fresh capture, accessibility read, or pane read, rather than trusting the tool's success reply.
5. Drive a scenario that needs real key semantics (an Enter or Tab override, IME composition, a chord through the menu, a pinch, a native drag) with a `@needs-focus` spec, or in a foreground window agreed with the operator.
   A scenario that cannot be driven either way is covered by tests and reported as not observed natively.
6. Quit only what you launched, by exact PID, stop the private server with `herdr server stop` under the same environment, and check that no process of your run remains.

### Evidence

- A visible claim needs a real capture of the candidate's window; a live process or a passing unit test is not UI evidence.
- Record which build answered: `packaged` and `version` from `host.start`, or the installed bundle's `CFBundleShortVersionString`.
- Missing Screen Recording or Accessibility permission (`ocu doctor`) blocks the check.
  The grant has been seen to stop matching after the installed app was replaced; report the check as blocked rather than focusing the window to work around it.
- Artifacts go under `agents/runs/<slug>/` ([AGENTS.md](../AGENTS.md#evidence-belongs-outside-the-repository)); the pull request states what was observed, how, and what was not.

## A device check

`desktop/e2e/remote-workspace.spec.ts` covers remote routes against an isolated SSH server; a check against a real device is for what that spec cannot reach.

- Start a private `herdr server` on the device with the same isolation variables, sent as a script over `ssh <alias> 'bash -s' < script.sh`, and keep the device's real `HOME` so its agent CLIs stay logged in.
- Drive it from a private hided: the web e2e fixtures `startHerdr` and `startHided` with the fixture home's `.ssh` linked to the operator's, because hided resolves the alias from `$HOME/.ssh/config`.
- Register the device with `register_device` and `host_consent: false`, which installs no helper on the device.
- Reuse one SSH connection (`-o ControlMaster=auto -o ControlPath=<short path> -o ControlPersist=120`); dozens of fresh connections fail with "Too many authentication failures".
- A web-attached pane grows its row count, so read it with a large `--lines`.
- Run every remote cleanup after the local daemon and server stop, or in its own `try`; an ssh that threw inside `finally` once leaked a hided.
