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

Run the Rust lanes through `scripts/verify-cargo.sh`, never a bare `cargo test` from an agent's pane: the pane carries `HERDR_SOCKET_PATH` for the operator's Herdr, and on 2026-10-02 two daemon tests followed the operator's live Herdr with the real HOME, where the label worker reads the operator's conversations, because the core then took that variable over the socket a test handed it; the core no longer does, it takes its home from hided rather than from the process `HOME`, and the script clears every `HERDR_*` variable first.

`python3 scripts/check-herdr-schema.py --herdr-bin <pinned binary>` compares the binary's own schema and version with the committed contract and manifest on macOS, Linux and Windows.
It clears inherited `HERDR_*` and needs no server; the full zsh contract check delegates this same comparison before its live-server checks.
The schema digest is canonical JSON (sorted keys, UTF-8, two-space indentation and a trailing newline).
A configured nightly matrix proves no executed job, native input or package launch by itself.
Record the actual head, attempt, job URLs, failures and skips; a package smoke with a private HOME or simulated hook does not prove a physical IME, an operator PATH or a real agent session.
The macOS nightly package lane extracts and checks the actual archive, then runs the existing isolated install-kit and packaged-app session-search tests.
The Windows/Linux package lanes check the real archive's headless daemon/kit behavior; their actual GUI and physical input still need device evidence.

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

- Set each fixture's `HCOORD_HOME` to a private folder such as `HOME/.hcoord`, so its daemon gets a launchd label of its own; hcoord gives the plain `com.hcoord.daemon` label only to the account's default `~/.hide/hcoord`, read from the user database, but a fixture that names its own home leaves no doubt.
- A test of a one-time move (`hide connect` moving the state folder, `hcoord home adopt`, the kit's hcoord part) stops only a daemon the test started in its own private folder, and injects launchctl and the label (`plugins/hcoord/test/unit/hcoord-home.test.mjs`); launchd domains are per account, so a real `launchctl` call with the default label reaches the operator's coordinator whatever `HOME` says.
  The desktop fixture refuses a mismatched coordinator before launching a candidate.
- Copy the whole isolation environment from `web/e2e/herdr-fixture.ts` and `desktop/e2e/fixture.ts`, never a subset; [PERFORMANCE_TESTING.md](PERFORMANCE_TESTING.md#3-isolate-runtime-state-before-making-fixtures) lists every variable and why.
- Register every process a fixture starts with `ownUntilWorkerExit` from `web/e2e/worker-owned.ts`, so synchronous cleanup runs on Node-managed worker exit even when a test's `finally` was skipped.
  A `spawn` with no `error` listener is such a death: when `target/debug/hided` was missing, each test killed its worker and left its private Herdr server running under launchd.
  The exit callback cannot run after SIGKILL, an OOM kill or host loss; these require separate recovery and are not proven by a `process.exit()` regression.
- `desktop/e2e/fixture.ts` owns each `isolate` home through both automatic test teardown and worker exit, with at most sixteen unclosed homes per worker.
  Each home records at most sixteen live or pending candidate launches; a launch over that cap fails before starting another process.
  Automatic teardown closes candidate apps, then each home's cleanup checks its recorded process handles for confirmed exit before stopping the private hided, unloading only the hashed hcoord labels for that home's legacy and adopted directories, and confirming each label absent before deleting the home.
  An unconfirmed candidate exit, stop, launchctl query or unload failure fails teardown and retains the home for recovery; the error names the retained path and recovery action.
  When candidate exit is unconfirmed, close only the recorded owned candidate, confirm its exit, then call that fixture's `cleanup()` again.
  `desktop/e2e/fixture-cleanup.unit.ts` injects Electron close failures at the external boundary and checks real home retention, confirmed-exit deletion and recovery without starting native processes.
  `desktop/e2e/lifecycle.spec.ts` checks running and manually stopped services, failed tests, Node-managed worker exit via `process.exit(23)`, and retained state after an unload failure against private homes in real launchd.
  Standalone hcoord fixtures must use `hcoord daemon uninstall --json` with their original `HOME` and `HCOORD_HOME` before removing those paths; `daemon stop` alone keeps the job registered.
- Set `terminal.default_shell` in the private Herdr config, because Windows Herdr does not select its shell from `SHELL`.
  The fixture uses `/bin/zsh` with its private `.zshrc` on Unix and the native `ComSpec` cmd shell with a controlled `PROMPT` on Windows.
  A missing native shell fails fixture setup before starting the server.
  Before each initial agent start, the fixture waits for the prompt and the shell to hold the foreground within the existing ten-second setup bound.
  On Windows, the pinned Herdr can report the shell as foreground while a non-agent child remains, so one bounded, noninteractive PowerShell read also waits for that shell to have no children; a missing shell or failed read fails setup.
  The fixture sends each `agent.start` once and retains the original input and PTY-log assertions.
- Use `fixtureHomeEnv` from `web/e2e/platform-fixture.ts` to move `HOME`, provider config homes and, on Windows, `USERPROFILE`, `APPDATA` and `LOCALAPPDATA` together into the private fixture.
  The fixtures run a compiled `claude` shim instead, which proves the pipeline and not an agent or physical IME.
  On Windows the interactive shim writes its readiness line after console setup; provider/auth replies return before that line, and input logs record only received input.
  The native compiler is `cc` on Unix and `clang.exe` on Windows; its owned child has a 20-second bound and a compiler failure fails setup.
  The private Herdr server, panes and `hided` use `fixturePath`: the shim directory followed by system-tool directories, including `/usr/sbin` for `lsof` on Unix and native Windows and Git directories on Windows.
  `fixtureExecutable` supplies native `.exe` names and `fixtureToolPath` uses the native path delimiter while refusing missing or non-absolute Windows system-root or program-files values.
  Catalog discovery probes every provider, so appending the host's `PATH` also reaches its installed CLIs even when the test selects Claude.
  A spec adding a GitHub or Git shim prepends it to `fixturePath`; explicit `extraEnv.PATH` remains authoritative.
  A claim about a real agent session needs the real CLI inside an otherwise isolated fixture: a shim on the pane's `PATH` that runs the CLI under the operator's `HOME`, while the server keeps its private one.
- A fixture makes agent labels the way the product does, through the core and not through tokens: `labelAgent` in `web/e2e/herdr-fixture.ts` gives a pane a new Claude session and writes a synthetic transcript (`writeFixtureTranscript`) the core's label worker reads, and the fixture `claude` shim acts as the provider that answers the analysis with the label the spec asked for.
  The same session is read again only when the agent's state changes: a spec that needs a different label either gives the pane a new session, or, for a pane declared as another's child, appends a turn to its own session (`continueFixtureTranscript`) and changes its state, because a relationship holds only for the sessions it was written for.
  A spec drives status through Herdr's own `agent_status` with `setFixtureLifecycle` and `finishFixtureTurn` (which needs `elsewhereTab`, so the turn ends unseen), because nothing else produces working, blocked or done.
  No spec writes a label or status pane token; the lineage tokens `declareParent` writes (`parent_pane`, `child_session`, `parent_session`) are the only ones a fixture reports, so a spawned child is labelled before its parent is declared (`spawnAgent` with a label).
  `desktop/e2e/session-labels.spec.ts` is the reference for the session boundary.

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
Its element actions have not yet been exercised against hide's renderer, so observe the effect after every action and record whether it landed.
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

### Focused-area navigation

`web/src/areaCycle.test.ts` checks full device/checkout/area identity for View areas, the Agent pane order across devices and checkouts, commit events, Agent cycling outside a View without inventing visits, and single-tab or retired View no-op boundaries.
`web/e2e/recent.spec.ts` commits an Agent pane in another project on a real isolated Herdr, opens the list from the sidebar, Explorer and Overview, and keeps focused files in their own View scope.
`desktop/e2e/keys.spec.ts` walks ⌃Tab over Agent panes in the desktop host, including from sidebar focus.
The `area-focus` design-review target measures the production area renderer with Korean document fixtures in both themes and two widths, including unchanged geometry when keyboard ownership moves.
These checks do not prove native browser input or terminal readability.
The `area cycle native` case in `desktop/e2e/browser.spec.ts` is tagged `@needs-focus`: it uses real macOS modifier input into the isolated candidate's page, counts core selection events, checks page key consumption and captures that exact native window.
It covers the default chord and a rebound two-modifier chord, whose Control is released while Option is still down, and types into the page the release selected.
Its keys are CGEvents posted at the HID tap, each refused unless the candidate is frontmost.
System Events' `key down control` posts no flags-changed event, so no page ever sees Control go down or come up and a release case fails for a reason the product does not have.
Typed keys are digits, because a key code passes through the operator's input source: under Korean 2-set a letter becomes a jamo and leaves a composition open.
Input the run did not send, such as an operator typing while the candidate is frontmost, reaches the candidate and spoils the run, so the slot must be one where nobody uses the keyboard.
Run it only in an agreed foreground QA slot, using `pnpm --dir desktop exec playwright test browser.spec.ts --project needs-focus --no-deps --grep 'area cycle native'` after building this worktree's web and desktop output.
Record the candidate PID/window, private daemon/server/profile and build head beside the captures.
Compare any temporary weak-blur proposal against the readable treatment in the actual terminal, document and native page, with idle and driven measurements, before choosing it.

## A device check

`desktop/e2e/remote-workspace.spec.ts` covers remote routes and `desktop/e2e/device-kit.spec.ts` the install kit against an isolated SSH server; a check against a real device is for what those specs cannot reach.
That server logs in as the account running the suite and connecting with consent installs Hide's kit there, so its sessions must get a private `HOME` and `HCOORD_HOME` (`SetEnv` in its config); `desktop/e2e/device-home.ts` describes the setup and refuses to register a device until it has proved both.

- Start a private `herdr server` on the device with the same isolation variables, sent as a script over `ssh <alias> 'bash -s' < script.sh`, and keep the device's real `HOME` so its agent CLIs stay logged in.
- Drive it from a private hided: the web e2e fixtures `startHerdr` and `startHided` with the fixture home's `.ssh` linked to the operator's, because hided resolves the alias from `$HOME/.ssh/config`.
- Register the device with `register_device` and `host_consent: false`, which installs no helper and no kit on the device; a check that needs the kit there needs a private `HOME` on the device too.
- Reuse one SSH connection (`-o ControlMaster=auto -o ControlPath=<short path> -o ControlPersist=120`); dozens of fresh connections fail with "Too many authentication failures".
- A web-attached pane grows its row count, so read it with a large `--lines`.
- Run every remote cleanup after the local daemon and server stop, or in its own `try`; an ssh that threw inside `finally` once leaked a hided.
