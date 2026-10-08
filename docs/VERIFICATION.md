# Verification

This guide answers the question asked before a change is called verified: which check proves this claim, and how to run it without touching the operator's work.
It owns only what no other document does: the choice of check, the traps that have made a check prove nothing, the tools for native QA, and device checks.
How a test is written, its fixture included, lives in [TESTING.md](TESTING.md); the gates are listed in [CONTRIBUTING.md](../CONTRIBUTING.md), build identity lives in [dev-runtime.md](dev-runtime.md), and runtime isolation and measurement live in [PERFORMANCE_TESTING.md](PERFORMANCE_TESTING.md).

## Pick the check by the claim

A claim is verified by the check that observes what a caller of the behavior observes; a green `verify` proves only the lanes it ran.

| The change claims | Check | Owner |
| --- | --- | --- |
| Core state, a runtime event, a Herdr fixture, the hided wire | `bash scripts/verify-cargo.sh test` | [CONTRIBUTING.md: CI gates](../CONTRIBUTING.md#ci-gates) |
| Local mailbox intake, safe doorbells, durable watches and activity privacy | Named library checks plus actual isolated runtime observations | [delivery.md: Verification](delivery.md#verification) |
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

For a file-persistence claim, run the platform's public contract and OS fault regressions through `bash scripts/verify-cargo.sh test-scoped -p hide-platform` on each supported system.
The durable writer's regressions observe returned phases and causes, installed bytes and identity, retained cleanup residue, and Windows's unsupported access modes; unit faults are scoped to the test thread at the OS boundary and exercise the public writer without replacing owned helpers.
A readable file or legacy `sync_dir` success alone proves no durable acknowledgement, and a file-parent barrier does not prove persistence of a newly created ancestor chain or recovery of the caller's state after an uncertain replacement.
The public bootstrap regressions check the 64-component bound, refusal of escape and untrusted paths, retained partial chains after real mkdir or barrier failure, and recovery that checks existing levels again; the supplied ancestor's established durability remains a caller precondition.
The same scoped platform lane exercises guarded launches and captures against real children: stdin payload, both output streams, 64 KiB and 1 MiB limits, inherited-pipe timeout, abrupt owner death with a helper outside its group, and repeated work returning the descendant count to its baseline.
Capture failures must preserve their primary outcome and report unconfirmed cleanup while retaining ownership; local platform timing checks alone do not prove a consumer's complete 1.85-second deadline or native behavior on another operating system.
Existing-anchor regressions establish the canonical chain with real barriers before creating sibling home and state directories, reject links and untrusted leaves without mutation, and preserve identity through a reported barrier failure and explicit recovery that checks the existing chain again.
Windows's pre-mutation `Unsupported` is fail-closed interim behavior, not cross-OS bootstrap completion, and callers must not become ready without an established parent-chain boundary.

`python3 scripts/check-herdr-schema.py --herdr-bin <pinned binary>` compares the binary's own schema and version with the committed contract and manifest on macOS, Linux and Windows.
It clears inherited `HERDR_*` and needs no server; the full zsh contract check delegates this same comparison before its live-server checks.
The schema digest is canonical JSON (sorted keys, UTF-8, two-space indentation and a trailing newline).
A configured nightly matrix proves no executed job, native input or package launch by itself.
Record the actual head, attempt, job URLs, failures and skips; a package smoke with a private HOME or simulated hook does not prove a physical IME, an operator PATH or a real agent session.
The macOS package job (nightly, and a pull request's `package` lane) extracts and checks the actual archive, then runs the existing isolated install-kit and packaged-app session-search tests.
The Windows/Linux package lanes check the real archive's headless daemon/kit behavior; their actual GUI and physical input still need device evidence.

`hide-platform/tests/process.rs` exercises `run_to_end` with a real parent that starts a same-group helper inheriting both outputs and exits 0.
The call must return the parent's code and both markers within the original five-second deadline, with the helper gone; test recovery is armed with that helper's identity before its parent exits.
This regression checks process and pipe ownership, not a native app window or Unix crash containment.

CI retries a failed e2e or Rust test once, so a green lane can hide a test whose first attempt failed.
The `Report flaky and failed tests` step and the open `quarantine` issues say which; read them before claiming a flow verified, and treat a flaky test as unproven ([TESTING.md](TESTING.md#flaky-tests) owns the policy).

A scenario someone would check by hand becomes a spec when it can; [TESTING.md](TESTING.md) says how to write it.
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

## Manual native QA

Read [PERFORMANCE_TESTING.md](PERFORMANCE_TESTING.md#2-identify-the-build-and-protect-the-operator) sections 2 and 3 first; they own isolation and the foreground boundary, and this section adds the tools.

### Why targeting is the hard part

- The operator's `/Applications/hide.app` and a packaged candidate from `desktop/out/` share the name `hide` and the bundle id `me.grab.hide.desktop`.
  Anything that resolves an app by name or bundle id reaches whichever one it finds first: open-computer-use, System Events (even `first process whose unix id is N` dereferences by name), `open -a`, and `quit app id`.
- An unpackaged `pnpm --dir desktop dev` instance and every desktop e2e instance run as `Electron` (`com.github.Electron`), so those collide with each other instead.
- Without its own `HIDE_DESKTOP_USER_DATA_DIR`, a dev instance focuses the operator's app and exits ([dev-runtime.md](dev-runtime.md#one-instance-per-profile)).
- Identify the app by the candidate executable and the PID that owns its window; a name-only process match does not prove which build rendered it.
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
`web/e2e/recent-agents-restart.spec.ts` restarts hided and reloads the page, then reaches an agent pane visited only before the restart from the same cycle.
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

## Local agent bell measurement

Run `python3 scripts/agent-live-check.py --agents codex --model codex=gpt-6-luna` before proposing a new bell target, after changing the pinned Herdr, and after a substantial agent CLI update.
Omitting `--agents` selects every adapter declared by the current source.
Confirm the cheapest suitable model for the logged-in account and pass `--model ID=MODEL`; recipe defaults can become unavailable.
Fetch the pinned binary with `scripts/fetch-herdr-runtime.sh` first, and build this worktree's `hided` and `hide` with `bash scripts/verify-cargo.sh build -p hided`.
The tool never builds, upgrades a CLI, logs in, or changes bell declarations.
An unclassified startup menu remains measurable when the pinned CLI's readiness wait expires: its pane, workspace, cwd, agent kind and actual menu must match the owned launch before observation continues.
Registration may still be absent after that timeout; if an agent name is present, it must match the requested name.
Other startup refusals retain their screen and agent evidence without being counted as arrival.

The command starts the verified pinned Herdr on a private socket and this worktree's hided on private state, creates disposable Git checkouts, and attests their actual shells through the candidate `hide workspace bootstrap`.
It never opens a renderer or controls the installed app.
Native agents keep the operator HOME for existing login.
Supported per-command config roots use bounded private copies of existing authentication, with source/version provenance in each provider row.
Private copies have mode 0600, never update the account's login, and are removed on controlled exit even if another teardown step fails.
An uncatchable controller kill can retain private copies in the ignored run directory; remove a stale run only after confirming its owned processes have ended.
Authenticated probing currently requires macOS `sandbox-exec`; a real write/socket denial self-test must pass before a provider starts.
Unsupported hosts fail closed.
The process guardian supports macOS and the trusted Linux fixture lane; Linux authenticated probing remains unsupported.
Every direct child starts in its own session and process group.
An EOF guardian sends CONT and TERM to that group, allows a bounded grace period, then sends KILL and requires the kernel to report the group absent.
It retains the direct child's unreaped identity until the last group signal, so its group ID cannot be reused during signalling.
No group signal is sent after that child is reaped.
Group metadata and RSS enforce the process and memory caps; a partial host-wide resource summary never proves cleanup.
A readable, birth-rechecked owner marker also identifies helpers that left the group, including markers from an earlier run of this checkout whose guardian has ended.
A different live guardian's marker belongs to concurrent work and is excluded.
Private issued marker families are retained under `agents/runs/process-owner-families/` so a fabricated scope prefix cannot claim a previous run.
The registry has a locked 4096-family cap; exhausting it is a reported failure rather than an unbounded directory.
The private Herdr server and hided also receive their protocol close before process teardown.
Unrelated orphans whose environment is empty or unreadable are neither signalled nor treated as cleanup failures.
The report lists these as "출처 확인 못 함" with only PID, birth and executable name, within a declared record cap.
A known owned identity that becomes uninspectable still makes cleanup unconfirmed.
Every launched guardian must produce a confirmed cleanup receipt; a missing receipt, an unconfirmed receipt or a diagnostic write failure fails cleanup.
Once every owned process and guardian is positively confirmed ended, the probe and short socket directories are removed independently even if a protocol close or log write failed.
Those errors remain failures with exit 2; missing process confirmation retains the directories.
An unseen double-fork descendant that clears its marker and leaves the owned group may escape attribution; record this limitation in the PR's Review section.
This ownership boundary follows the explicitly recorded B8/D-09 scope decision; it does not claim complete workstation process attribution.
Linux fixture supervision uses a child subreaper and validates its procfs namespace and visibility before using process identities.
Darwin group enumeration uses Apple's [process-group API](https://github.com/apple-oss-distributions/xnu/blob/main/libsyscall/wrappers/libproc/libproc.c).
Token lookup preserves the existing target-width alignment and birth checks and records no argument, environment or marker contents.
`--socket` and `--state-dir` cannot select operator routing, existing sockets, or state outside the new run.

Every recipe retains rest, working, shell approval, file approval, question, plan approval, model picker, resume picker, MCP approval and startup rows.
MCP registration uses each CLI's documented project file or per-command override only for its MCP scene; recipe data records that source.
Rest and resume share a disposable checkout and private session catalog.
Supported native readers first observe a real previous user/assistant turn.
Other providers seed a real prompt and require a positive native identity and a working-to-ready transition; that weaker history evidence cannot verify assistant text or mail.
Resume arrival additionally requires that owned session's label, filename or identity on the picker screen.
Missing history, an empty picker or an unobserved native transition remains unknown.
The tool checks scene arrival independently of Herdr status, types the exact current bell and Enter only when the observed status is not `blocked`, then observes the effect.
Approval work stays within `probe-N.txt` in the disposable checkout.
A confirmed selection, approval, session replacement, settings write or unsubmitted draft is unsafe, even when other scenes remain unknown.
An unrecognized, inaccessible or timed-out scene never counts as safe.
These direct input experiments measure the CLI and Herdr detection boundary; they do not replace the core's draft, session, identity or plan-approval guards.

Each new private `agents/runs/live-check-<timestamp>-<id>/` contains `report.md`, `report.json`, screen reads and private configuration backups.
Reports record the actual binary digests, Herdr version, active server manifest versions and sources, CLI versions, selected models and positively observed native integration provenance.
The verified pinned binary generates its own integration assets in a disposable HOME, then the tool routes them through documented provider settings while preserving the authenticated provider's operator HOME.
The loaded-version claim requires an isolated configuration route, unchanged prepared artifact hashes, one common version across its possible native emitters and a native session report, sampled again after the first turn.
Prepared files are inventoried separately; the claim does not say every prepared frontend asset loaded.
A prepared file alone never proves load; Cursor's project hooks and copied OpenCode configuration cannot exclude external-hook or plugin ambiguity, so their loaded versions remain unproven.
Only adapter-listed configuration/trust files are read and backed up with private permissions, with a 16 MiB per-file and 256 MiB total backup limit.
Nonshared declared files retain exact byte guards.
The declared shared Claude JSON and Codex TOML files are compared read-only and are never restored or rewritten, preserving other live sessions' updates.
Shared-file changes are reported as other-session changes without failure, and entries in their `projects` tables keyed by this run or its probe descendants are reported by file and key as B6 leftovers without failure.
Shared parsing uses Python 3.11 or later's standard TOML reader, rejects unreadable tables and caps each project table at 50,000 entries.
The remaining configuration trees use path, kind, size and mtime metadata only; file contents and link targets are never read or hashed.
Installation subtrees named `node_modules`, `extensions`, `marketplace`, `marketplaces`, `bundled`, names containing `cache`, and code directories directly beneath `plugins` are excluded; plugin registry files remain observable.
The inventory stops at 50,000 scanned entries and reports partial coverage, a lower bound on omitted entries and the number of uninspected subtrees, without walking the remainder just to count it.
The excluded-boundary count includes explicitly listed roots inside an excluded tree and is not a deduplicated count of physical installation trees.
An addition or removal is reported only where the opposite inventory observed absence, so partial coverage cannot manufacture a change.
Metadata changes outside the known file list are named and preserved without failing the run or attempting restoration.
The native sandbox restricts writes to the disposable probe/temp/socket trees and declared history roots, with exact denials for nonshared declared configuration/trust files.
Existing and new history files use the same history allowance and their metadata changes remain named in the bounded inventory; the profile never enumerates session files.
The command cap remains 15 seconds regardless of history count.
Unexpected or concurrent changes to nonshared known configuration/trust files are preserved and fail the run; observing a diff never authorizes overwriting the operator.
Only the test lane's exclusively owned disposable HOME permits attributed restoration.

Delivery is verified only by a fresh marker sent through the private hided mailbox and then found in the native assistant reply to the exact bell turn.
The controller generates the marker after the earlier prompt and holds it only in memory; it writes no helper or marker receipt for the agent to read.
The native sandbox denies other-process argument inspection and private mailbox storage access; its exact candidate capability and claim files remain available for ordinary `hide inbox`.
The owned pane claims and verifies its exact persistent reference before the native CLI starts, preventing unclaimed-reference expiry during a long scene.
The Info request used for that claim may return the explicit `renderer_unavailable` refusal in this headless run; the daemon validates the capability first, and the CLI must still complete the claim and leave its claim file.
Other refusals and missing claim files fail the measurement.
Typed text, hook installation, a tool's output and transport arrival alone are insufficient.
Unknown native conversation formats, permission holds and unavailable integration remain unknown delivery.
No unavailable CLI or unauthenticated account is a safety PASS; the table retains its login instruction without attempting login.

Exit 1 identifies an unsafe scene for a source-declared bell target; exit 2 reports isolation, configuration, resource or cleanup failure; exit 3 reports incomplete measurement; exit 0 means the selected rows were fully measured without a target safety failure.
A result applies only to its recorded versions, manifests, model, environment and reached scenes.
It does not prove a different provider/model/version, a different remote manifest, or release readiness.
Report the actual native result and its unknown/skipped cells in PR Evidence; never commit these run artifacts.

CI runs `web/e2e/agent-live-check.spec.ts` with the existing compiled Claude/Codex shim on real pinned Herdr and candidate hided.
The fixture report is marked synthetic and proves tool plumbing, not authenticated model behavior.
That lane includes operator-routing refusal, byte-exact configuration restoration, runtime failure, owned-server loss and Ctrl-C cleanup.
`scripts/tests/test_agent_live_protection.py` exercises real filesystem recovery/conflicts, OS denial, timeout/output caps and owner-death cleanup; `test_agent_live_results.py` challenges unsafe verdicts and false delivery evidence.
The scene preparation and overlay tests cover supported MCP formats, positive native identity, role separation and bounded private authentication copies.

## A device check

`desktop/e2e/remote-workspace.spec.ts` covers remote routes and `desktop/e2e/device-kit.spec.ts` the install kit against an isolated SSH server; a check against a real device is for what those specs cannot reach.
That server logs in as the account running the suite and connecting with consent installs Hide's kit there, so its sessions must get a private `HOME` (`SetEnv` in its config); `desktop/e2e/device-home.ts` describes the setup and refuses to register a device until it has proved both.

- Start a private `herdr server` on the device with the same isolation variables, sent as a script over `ssh <alias> 'bash -s' < script.sh`, and keep the device's real `HOME` so its agent CLIs stay logged in.
- Drive it from a private hided: the web e2e fixtures `startHerdr` and `startHided` with the fixture home's `.ssh` linked to the operator's, because hided resolves the alias from `$HOME/.ssh/config`.
- Register the device with `register_device` and `host_consent: false`, which installs no helper and no kit on the device; a check that needs the kit there needs a private `HOME` on the device too.
- Reuse one SSH connection (`-o ControlMaster=auto -o ControlPath=<short path> -o ControlPersist=120`); dozens of fresh connections fail with "Too many authentication failures".
- A web-attached pane grows its row count, so read it with a large `--lines`.
- Run every remote cleanup after the local daemon and server stop, or in its own `try`; an ssh that threw inside `finally` once leaked a hided.
