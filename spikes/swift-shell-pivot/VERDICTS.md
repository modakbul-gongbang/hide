# Stage 0 spike verdicts

## Overall status

Status: T1-T4 Stage 0 spike gate PASS; T5 deletion, V21 dependency-removal verification, R11/R6 cleanup, and native pane commands are committed and PASS at their recorded boundaries.
Role marker: `mode=implementor`, `paneId=w2X:p8`, `agentKind=codex`.
Working root: `/Users/hoyeonlee/projects/herdr-ide.worktrees/swift-shell-pivot`.
Verification window: 2026-08-28 19:39 through 2026-08-29 11:51 KST.
Additive production work began only after the explicit user-directed dependency deviation recorded under T3.
The approved T5 source files and obsolete manifest dependencies were deleted only after the annotated `rust-native-final` tag was verified against the exact pre-deletion checkpoint.
No evidence, user file, spike copy, or non-T5 source was deleted.
SwiftPM automatically evicted obsolete ignored dependency cache entries under `macos/.build` when the pre-authorized editor fallback changed the package graph; no manual cleanup command ran and no generated build cache is part of the checkpoint.

## T1: PASS

The six-function C ABI linked between the Rust static library and the Swift executable.
The concrete options, event payloads, nested snapshot types, nullable fields, ownership rules, and thread rules are recorded in `FFI_SCHEMA.md`.
Rust tests passed 5 of 5, including all declared event payload decoding, observable unknown-kind and schema-version failures, exactly 100 Rust-thread callbacks, and 1000 snapshot buffer returns with zero outstanding buffers.
The linked running app recorded exactly 100 callbacks at the gate, exactly 100 off-main receipts, and exactly 100 main-thread snapshot applications.
The app decoded schema version 1 with all 13 required top-level snapshot fields.
Focused evidence is `evidence/runtime.json`, `rust-core/src/lib.rs`, `include/herdr_core.h`, and `evidence/final-running-app.png`.

## T2: PASS

Rust opened an isolated PTY, connected it to `ssh mini`, and received `REMOTE_TUI_READY` from an in-memory alternate-screen agent TUI fixture.
Rust output bytes reached SwiftTerm through `feed(byteArray:)`.
Peekaboo typed `t2-probe` into the real SwiftTerm view.
SwiftTerm's `send(source:data:)` delegate sent 9 input bytes to Rust, Rust wrote them to the SSH PTY, and the remote fixture echoed `INPUT_UTF8:t2-probe` back into SwiftTerm.
Focused evidence is `evidence/runtime.json` and `evidence/final-running-app.png`.

## T3: PASS

The user performed the final physical-keyboard V9 checks against the SwiftTerm 1.20.0 terminal on source checkpoint `b2582ade493bc6569af0e9719aa672751962df96`.
The authoritative verdict is: `1,2,3,4 모두 잘 맞아 다 된다`.
English input echoed immediately: PASS.
The Korean candidate window followed the terminal cursor: PASS.
Backspace during Korean composition deleted the composition without leaking DEL: PASS.
Two or more adjacent Korean characters did not overwrite the following character: PASS.
The editor axis remains deferred by prior user agreement.
Under that explicitly agreed terminal-axis gate, V9 PASS satisfies AC1, AC2, and AC3, closes T3, and makes T1, T2, T3, and T4 all PASS.

The historical root-cause candidate was coordinate mixing in SwiftTerm `MacTerminalView`: document-coordinate `NSTextInputClient` ranges versus marked-storage-relative coordinates.
The app-side coordinate adapter and bounded trace hygiene remain preserved as diagnosis evidence; the later human PASS supersedes the earlier retry requirement.
The decisive real call stream is `evidence/v9-ime-call-stream-1.20.json`.
Related diagnosis and boundary evidence is `evidence/v9-backspace-diagnosis.json`, `evidence/v9-swiftterm-1.19-1.20-source-diff.json`, and `evidence/v9-composition-coordinate-boundary-3.json`.
The latest clean handoff and trace snapshots are `evidence/v9-backspace-adapter-human-retest-ready-2.json`, `evidence/v9-backspace-adapter-human-retest-trace-ready-empty-2.json`, and `evidence/v9-backspace-adapter-human-retest-ready-2.png`.
Earlier automated input evidence remains diagnostic only and is not T3 acceptance evidence.

On 2026-08-29 the user initially deferred the unresolved Backspace defect: "지금 원인은 모르겠는데 백스페이스 계속 안된다. 우선 이거 패스하고 다른거 작업부터 쭉 하게 하자".
Starting additive downstream work before the Stage 0 spike gate completed was therefore an explicit user-directed dependency deviation at that time.
The later authoritative physical-keyboard verdict `1,2,3,4 모두 잘 맞아 다 된다` supersedes the earlier unresolved terminal-axis result and closes AC1, AC2, AC3, T3, and the Stage 0 spike gate.

## T4: PASS

SwiftPM resolved SwiftTerm exactly at 1.20.0 and linked the Rust static library without Xcode.
The build produced a macOS 14 minimum, arm64 app bundle, applied an ad-hoc signature, verified the signature, and launched the real bundle.
The bundle exposed one window and one process, and a real screenshot confirms the running UI.
Two unchanged-source assembly runs produced identical hashes for the executable, Info.plist, and code-signing resources.
Focused evidence is `scripts/build_spike.sh`, `Package.swift`, `Package.resolved`, `resources/Info.plist`, `evidence/t4-build.txt`, and `evidence/final-running-app.png`.

## T7: ADDITIVE IMPLEMENTATION PASS UNDER DEPENDENCY DEVIATION

A new platform-neutral `herdr-core` workspace member was added without moving or deleting the existing Rust shell.
The production crate builds as both `rlib` and `staticlib` and depends only on `serde` and `serde_json`; it has no objc2, wgpu, glyphon, raw-window-handle, or AppKit dependency.
The C header and Rust implementation expose exactly the six approved functions: `herdr_core_create`, `herdr_core_dispatch`, `herdr_core_snapshot`, `herdr_core_on_change`, `herdr_core_free_bytes`, and `herdr_core_destroy`.
The production snapshot contains the T1 contract fields except the spike-only evidence object, and its `status` object contains herdr, remote, chromux, and nullable structured `last_error` state.
Unknown event kinds, schema-version mismatches, malformed JSON or payloads, invalid create options, and off-owner-thread dispatch are contained inside the process and made observable through the approved ABI behavior.
`cargo test -p herdr-core` passed 6 of 6 tests in the final test set, including V1/AC10/R6d internal error paths, callback registration, repeated buffer return, and normal destroy.
The separate normal-path C caller linked the release archive against `herdr_core.h`, exercised all six functions, and exited 0.
Apple `nm` could not inspect the Rust 1.97 LLVM 22 archive because the installed Apple LLVM reader rejects newer attribute kinds; the source/header declaration comparison plus successful C link provide the boundary evidence instead.

## T8: ADDITIVE SHELL IMPLEMENTED AND RUNTIME-SMOKED UNDER DEPENDENCY DEVIATION

A separate macOS 14 SwiftPM executable adds the SwiftUI window, native resizable three-column `HSplitView`, sidebar pet status, terminal surface, workbench surface, bottom status bar, and native menu commands.
`swift build --package-path macos` passed, the arm64 development app was assembled without deleting prior files, and strict ad-hoc codesign verification passed.
Peekaboo permissions reported Screen Recording and Accessibility granted before observation.
Exactly one LaunchServices app instance was present during verification: PID 38859, bundle `com.hoyeon.herdr-ide.swift-shell`.
Its only renderable window was ID 1124 titled `Herdr IDE`, 1440 by 900, key, frontmost, and on screen.
The accessibility tree exposed one `AXSplitGroup` with distinct Agents, Terminal, and Workbench columns plus the visible pet and status bar.
The native menu inventory exposed Herdr IDE, File, Edit, View, Navigate, Window, and Help menus; Navigate contained enabled command registrations for Focus Agents, Focus Terminal, and Focus Workbench with Command-1, Command-2, and Command-3 shortcuts.
The actual screenshots are `evidence/t8-swiftui-shell-window.png` and `evidence/t8-swiftui-shell-screen-menu.png`; the structured runtime receipt is `evidence/t8-swiftui-shell-runtime.json`.
After verification, exact ownership was rechecked and only PID 38859 was terminated.
The final counts are zero T8 app processes, zero spike app processes, and zero owned `ssh -tt` spike fixtures.
This is additive T8 progress, not a claim that the Stage 0 gate, V9, V10, or the full product migration is complete.

## Verification guardrail resolution

The final authority boundary permits in-process `herdr-core` error-path tests required by V1, AC10, and R6d.
It forbids failure injection against the running herdr socket, mini connection, chromux daemon or profile, and any real user workspace or worktree.
Before that boundary was finalized, `cargo test -p herdr-core` ran once with the original 6-test set and passed all 6.
During the temporary overcorrection, the error-path cases were removed and the remaining 4 normal-path tests passed once.
After the final Observer correction, the 6-test set was restored and `cargo test -p herdr-core` passed all 6 again.
No external-service failure injection or destructive workspace operation ran in any of these checks.

## Assumptions and boundaries

The positive-path `ssh mini` connection was allowed, while no connection failure was injected and no live herdr, mini, or chromux service was interrupted.
The remote fixture existed only in the SSH process command and created no remote file, workspace, or worktree.
The isolated alternate-screen SSH fixture represents the byte and IME behavior of an agent TUI without recursively starting another coding agent.
The earlier interrupted Implementor left only empty spike directories, so no stale artifact was treated as completion evidence.
No chromux profile was changed, no Chrome process was quit, and no real user workspace or worktree was touched.

## Resolved gate and dependency deviation

The user completed the four V9 terminal checks with the authoritative verdict `1,2,3,4 모두 잘 맞아 다 된다`.
The editor axis remains deferred by prior user agreement.
T1-T4 are now all PASS, T3 and AC1-AC3 are satisfied, and the user explicitly authorized T5.
The annotated `rust-native-final` tag was verified on the exact pre-deletion checkpoint before T5 began.
External-service and live-system failure-injection verification may not be performed under the current authorization.
Required in-process core tests for unknown kind, schema mismatch, malformed payload, invalid options, and off-owner behavior remain allowed.

## Sasu record state

The interrupted run remains `sasu.implement.state.v5`, status `active`, with its clean baseline at commit `65974708eca4db64f459302937cafc763db2dafc` and working root `/Users/hoyeonlee/projects/herdr-ide.worktrees/swift-shell-pivot`.
Its machine record still reports 19 tasks pending, 15 acceptance criteria pending, 35 verification rows `NOT_RUN`, zero artifacts, zero verification attempts, and no completion receipt.
The current Sasu 0.8.0 CLI accepts only v6 for mutations, so it rejected task updates and also rejected retirement of the v5 run.
Starting a replacement v6 run was attempted without changing the sealed PRD, but current Sasu rejected the approved PRD because its acceptance criteria predate the required canonical Judgment and Evidence Declaration table.
The sealed PRD and qa-log were not edited to work around that incompatibility.
The file evidence in this spike is therefore live-verified but not registered in Sasu state.

## Additive downstream checkpoint: T9, T10, T11, T6, T14, T19, and T12 code-only

This batch follows the user's explicit dependency deviation and does not change the unresolved Stage 0 gate verdict.
The role helper returned `mode=implementor`, `paneId=w2X:p8`, and `agentKind=codex` before mutation.
The sealed PRD, qa-log, and legacy Sasu state stayed read-only.

### T9: ADDITIVE IMPLEMENTATION PASS, AC2 STILL UNMET

The macOS package pins SwiftTerm exactly at 1.20.0 and embeds a real `TerminalView` in the center column.
SwiftTerm delegate input is forwarded as unmodified bytes through the six-function C ABI, and Rust snapshot output is decoded from base64, ordered by sequence, and fed back with `feed(byteArray:)` without text transcoding.
The in-process byte boundary test round-tripped arbitrary bytes including NUL, DEL, and non-UTF-8 values.
The running development app displayed the Rust-provided terminal fixture in the real SwiftTerm view in `evidence/additive-t9-t14-ui.png`.
No automated key input or new human IME session was used for this verification.
T9 is additive progress only: T3 Backspace remains NOT PASS, AC2 remains unmet, and V9 physical-keyboard retest remains mandatory before v1 completion.

### T10: ADDITIVE IMPLEMENTATION PASS

The Rust projection accepts the authoritative herdr session token payload and renders exactly seven states: question, approval, error, working, unseen completion, idle, and unknown.
Each state keeps a fixed symbol; working and unseen completion share the filled-circle shape but use distinct colors without blinking.
Each sidebar item is two lines with workspace and agent kind on the first line and a compact maximum-30-character summary plus elapsed token on the second line.
Ordering uses `sort_rank` ascending as the primary authority, `activity` descending only for ties, and source order as the stable final tie-breaker.
The UI does not recompute blocking priority.
The focused unit tests cover all seven token states, summary bounds, and rank/activity ordering.
The real accessibility tree and screenshot show the seven fixture rows in authoritative order in `evidence/additive-t9-t14-ui.png`.

### T11: ADDITIVE IMPLEMENTATION PASS WITH PRE-AUTHORIZED EDITOR FALLBACK

The workbench loads an existing local workspace as an `OutlineGroup` tree while excluding generated and repository-internal directories.
It supports SwiftUI image preview, MarkdownUI rendered/source modes, and code or text viewing and inline editing through `NSTextView` with Highlightr syntax presentation.
Save operates only on the already-open existing regular file; there is no create, rename, move, or delete endpoint or UI action.
Binary, oversized, non-regular, and filesystem-readonly inputs expose a visible readonly reason.
External modification preserves the draft and exposes Reload or Keep Editing conflict choices with the observed timestamps.
The Rust file boundary test opened an existing committed fixture and proved an idempotent same-content save leaves its contents unchanged.
The initial exact CodeEditSourceEditor 0.9.1 attempt was not viable because its mandatory SwiftLint build plugin could not load `sourcekitdInProc` in the isolated SwiftPM plugin environment.
PRD RISK-3 pre-authorized the independently implemented `NSTextView` plus Highlightr fallback, so no new product decision was made.
The workbench surface and existing verification file tree are visible in `evidence/additive-t9-t14-ui.png`.

### T6: ADDITIVE IMPLEMENTATION PASS

The `herdr-core` environment registry is enumerable in code and currently declares only `SSH_AUTH_SOCK` as an optional absolute Unix-socket path.
The value remains external and is never copied into the snapshot, diagnostics, log text, fixture, or repository.
Missing optional state disables only remote attach capability and leaves local startup available.
Invalid state produces a structured, externally visible diagnostic with key, kind, message, capability, and retryability.
Static inspection found the only production environment read in `environment.rs`; the fixture binary reads only its command argument.

### T14: ADDITIVE IMPLEMENTATION PASS

The Rust-owned versioned UI-state schema persists selected file path, expanded file paths, and focused surface.
Missing and corrupt state load safe defaults and append a structured diagnostic that is exposed in the status snapshot and JSON stderr log.
Valid state round-trips through the stable schema, while unknown versions and malformed contents fall back without blocking app startup.
Save uses a same-directory `.next` write followed by rename so repeated updates replace one complete state rather than exposing a partial file.
The final UI screenshot verifies the safe missing-state startup path; no synthetic UI manipulation was used to claim interactive restoration behavior.

### T19: CODE AND PURE PLAN VERIFICATION PASS

The new `herdr-ide-fixture` tool rejects every name without the exact `herdr-ide-verify-` prefix before planning any operation.
For an accepted name it produces a deterministic, idempotent plan containing the synthetic workspace, verify branch, worktree, and long-running pane label.
Only the pure plan command and unit tests ran.
No real workspace, worktree, pane, service, or remote machine was created or touched.

### T12: CODE-ONLY COMPLETE, RUNTIME PARKED BY AUTHORITY

The pure Rust planner parses the current chromux process-list schema from supplied JSON and returns Reuse, Launch, or Parked plans for known profiles.
Missing runtime input, malformed input, and unknown profiles remain explicitly Parked.
The production runtime passes no live process-list input, publishes `Chromux runtime parked`, and performs no chromux command.
Static inspection found no process execution surface for chromux in Rust or Swift.
No chromux CLI, profile, browser, daemon, or live service was inspected, launched, focused, or controlled.
Therefore T12 source planning is implemented, but V5, V6, V12, V13, and every runtime claim remain intentionally NOT RUN.

## Additive batch verification

`cargo fmt --all -- --check` passed.
`cargo clippy -p herdr-core --all-targets -- -D warnings` passed.
The final `cargo test -p herdr-core` run passed 24 tests: 14 unit tests and 10 FFI integration tests, with zero failures, ignores, or filters.
The ten FFI tests include required in-process unknown kind, schema mismatch, malformed payload, invalid options, and off-owner behavior; no external failure injection was performed.
The preserved independent spike crate passed its five Rust ABI tests after its manifest explicitly opted out of the new parent workspace.
The preserved composition adapter passed seven Swift unit tests; these document-coordinate tests remain diagnostic boundary evidence and are not a real-IME acceptance verdict.
`cargo build -p herdr-core --release` passed.
`cargo check -p herdr-ide` passed for the preserved Rust application with only its existing unused-constructor warning in `src/app.rs`.
The standalone C caller linked the release static archive, exercised all six ABI functions, and exited zero.
`swift build --package-path macos --disable-keychain --disable-sandbox` passed.
Two complete app assembly and signing runs produced the identical bundle content hash `4c0f6228a9825dad83349624da2aec5fbe563c4b63550edf0dc6495ce692e703`.
Strict deep codesign verification passed, the executable is arm64, and its minimum macOS version is 14.0.
`git diff --check` passed.
The relevant-rules query returned no additional triggered invariant for the additive paths.
Static scans found no local/global NSEvent monitor, CGEvent or IOHID API, event posting or requeue path, marked-text discard fallback, DEL suppression, or reachable synthetic-key path in the human artifact.

Peekaboo permission status reported Screen Recording, Accessibility, and Event Synthesizing granted before native observation.
Exactly one owned assembled development app was launched through LaunchServices for the additive UI check: PID 48371, bundle `com.hoyeon.herdr-ide.swift-shell`.
Its only renderable window was ID 1160 titled `Herdr IDE`, 1440 by 928, on screen, key, and frontmost.
The accessibility tree exposed one split group with distinct Agents, Terminal, and Workbench column frames and all seven authoritative agent labels.
The fresh exact-window screenshot is `evidence/additive-t9-t14-ui.png`.
The existing T8 real-screen evidence `evidence/t8-swiftui-shell-screen-menu.png` continues to prove the native menu bar and Navigate commands.
A later full-screen capture named `evidence/additive-t9-t14-menu.png` contains an unrelated macOS `swift-package` Keychain authorization dialog and is intentionally excluded from the checkpoint and acceptance evidence.
The credential dialog was not automated, dismissed, or supplied with input.
After observation, exact ownership was rechecked and only PID 48371 was terminated.
Final process audit found zero assembled HerdrIDE apps, zero Swift shell spike apps, and zero owned remote TUI fixtures.

## Applied principles and retained limits

Engineering Principle 2 kept each path to the smallest additive implementation that satisfies the present contract.
Engineering Principles 3 and 5 kept the C ABI, environment registry, sidebar projection, file service, persistence, fixture planner, chromux planner, and Swift views in separate layers.
Engineering Principles 4 and 10 made malformed input, invalid environment, corrupt state, readonly files, edit conflicts, and parked runtime behavior externally observable.
Engineering Principle 6 and PRD RISK-3 selected the maintained Highlightr fallback only after the specified editor dependency failed at its real build boundary.
Engineering Principle 9 records structured status for the later environment, state, and runtime questions.
Engineering Principle 11 is enforced by stable sidebar sorting, idempotent fixture plans, no-op same-content saves, and replacement-style state persistence.
Engineering Principle 12 priced core invariants at unit or in-process boundaries and reserved real UI claims for the owned signed app and screenshot.
Engineering Principle 13 keeps Backspace unresolved instead of accepting a nearby proxy and keeps T12 parked instead of simulating a live service.
Design Principles 1, 2, 3, 4, 5, and 7 shape the token-driven two-line list, workflow-ordered three columns, direct file selection and save, derived status, native patterns, and visual state encoding.
Design Principle 6 is satisfied by omitting every destructive file action rather than presenting an unsafe shortcut.
The environment practice keeps declarations and validation in code while values remain external.
The test practice uses durable core contracts and one real native screenshot instead of broad brittle UI automation.

No PRD, qa-log, legacy Sasu state, production checkout, live herdr, mini, chromux service, secret, tag, T5 deletion target, or unrelated file was changed.
No external-service failure injection, human IME retest, commit push, PR, or delivery ran.
The coherent source, test, and evidence checkpoint commit is `96a016b24dbabf14506cbca3219f6eb654cedb14`.

## v6 canonical acceptance conversion and pipeline block

The user explicitly authorized a narrow exception to the sealed-input rule for a mechanical v6 acceptance-table conversion.
This section supersedes the earlier statement that the PRD remained byte-for-byte read-only during the prior batch.
No criterion, requirement, task, verification row, dependency, pass intent, decision, guardrail, or qa-log text changed.

Before mutation, the deterministic role helper returned `mode=implementor`, `paneId=w2X:p8`, and `agentKind=codex`.
Installed Sasu reports contract version `0.8.0` and implement state schema `sasu.implement.state.v6`.
PRD §7 was converted from list form to the canonical `ID | Criterion | Judgment | Evidence Declaration` table.
A parser comparison proved all 15 AC IDs and all 15 criterion strings are character-for-character identical before and after.
The PRD SHA-256 changed only for that table metadata conversion from `34ed6206e96830428e68051400cea8db26faf087a866c03b06e5c0c09c90f29d` to `ee92b7bd823b0f1ad0504642de4e00d66140d666ffacbc1de47c25bbe1f84a81`.
The saved conversion evidence is `evidence/v6-prd-ac-conversion.json` and `evidence/v6-prd-ac-conversion.diff`.
`sasu prd readiness` passed with 19 tasks, 15 acceptance criteria, 35 verification rows, zero blocking gaps, zero warnings, and judgment counts machine 4, judged 8, machine+gate:human 3.
`git diff --check` passed before checkpointing the conversion.
The conversion checkpoint is `9fb278f2ec38485bbc1566439febd08c89508806`.

The required v6 specification reseal cannot be executed by this Implementor.
Running the installed CLI with the structural role marker returned `implementor-spec-command-refused`: specification-stage gate `spec` belongs to the Spec Owner or Observer, and no state was written.
A read-only status recheck proved gap-audit and spec remain `NOT_RUN`, both have zero attempts, the judge call count remains zero, and no gate is in flight.
No v6 implement run was started, so no v6 workingRoot exists and none of the newly approved implementation or runtime-verification work began.

There is a second lifecycle constraint after resealing: the main record tree still contains the active `sasu.implement.state.v5` run named `swift-shell-pivot`, while the current v6 CLI rejects that schema and `implement start` refuses an existing state path.
The existing isolated worktree also already occupies the default `swift-shell-pivot` provisioning path.
No state was hand-migrated, removed, renamed, or combined, and no alternate PRD slug was invented because the approved PRD remains the sole specification source.

No app, fixture, browser profile, Chrome instance, workspace, worktree, remote process, or external service was created, focused, stopped, or changed during this v6 conversion step.
The chromux and mini observations remained read-only, T3 Backspace remains unresolved with AC2 unmet, V9 remains pending human physical-keyboard judgment, T5 remains forbidden, and no tag, deletion, push, PR, or delivery occurred.

## User-approved operational batch after the v6 gate refusal

The structural helper returned `mode=implementor`, `paneId=w2X:p8`, and `agentKind=codex` before this batch mutated source or processes.
The Spec Owner had already recorded gap-audit `BLOCK` with attempts 2 and phase `closure-blocked`, and spec `BLOCK` with attempts 1 and phase `closure`.
Those gates were not overridden or rerun.
The one authorized `sasu implement start` attempt under the original `swift-shell-pivot` slug refused with `qa-log-backed PRD requires live PASS for gap-audit and spec; got gap-audit=BLOCKED, spec=BLOCKED`.
No v6 implement state exists, so evidence was retained as files and was not hand-registered into Sasu state.

The Spec Owner moved the legacy v5 record tree from `/Users/hoyeonlee/projects/herdr-ide/agents/runs/swift-shell-pivot` to `/Users/hoyeonlee/projects/herdr-ide/agents/runs/swift-shell-pivot-v5-archive` without deleting it.
The archived tree contains 13 files and 279437 bytes, and its per-file SHA-256 set was identical before and after the move.
The archive path is reversible and the canonical PRD remains only `agents/prd/swift-shell-pivot/prd.md`.
The qa-log was not edited.

### T18: ADDITIVE IMPLEMENTATION AND NATIVE VERIFICATION PASS

The pet surface is a 92 by 92 transparent, borderless, nonactivating floating `NSPanel` that joins all spaces and full-screen auxiliary spaces.
The panel uses a selective elliptical hit region aligned with the visible pet circle.
`ignoresMouseEvents` is false at the pet center and circular edge, while transparent corners inside the 92 by 92 panel set it to true and pass clicks through.
No global or local key monitor, CGEvent, IOHID, event posting, or event requeue path was added.
Clicking the exact receipt-proven owned pet region changed the app from inactive with a non-key main window to active with the `Herdr IDE` main window key.
The saved offscreen request at 1000000 by 1000000 was clamped to the visible primary-screen origin 1636 by 993.
Native property and focus evidence is `evidence/t18-window-runtime.json`, `evidence/t18-pet-visible-runtime.json`, and `evidence/t18-pet-focus-boundary.json`.
The actual pet screenshot is `evidence/v24-pet-visible.png`.
The owned app PID 38870 was terminated after verification.

### T12: ADDITIVE IMPLEMENTATION AND APPROVED DEFAULT-RUNTIME VERIFICATION PASS

The production executor uses only the literal `/Users/hoyeonlee/Library/pnpm/chromux` executable.
It accepts only `default` and the read-only missing-profile sentinel `herdr-ide-verify-absent`.
It reads `chromux ps --json`, launches only a stopped or absent `default`, reuses an already running default, observes its loopback CDP status, and publishes structured ready, stale, unavailable, or failed receipts.
The focus action records whether activation was requested, accepted, and observed as the frontmost PID.

The approved live check launched default Chrome PID 9609 on port 9301, then a repeated launch returned the same PID with `already running` behavior.
The current-source app recorded reuse with `focusRequested=true`, `focusActivationAccepted=true`, and `focusObserved=true` in `evidence/t12-default-reuse-focus-runtime.json`.
The launch receipt is `evidence/t12-default-runtime.json`, and the combined lifecycle record is `evidence/t12-chromux-lifecycle.json`.
The real three-column native screenshot with Chrome and mini ready is `evidence/v12-v16-native-ready.png`.
No chromux profile was created or deleted, no browser or daemon was killed, stopped, or closed, and no CDP page was mutated.
Default Chrome PID 9609 remains running by explicit approval.
The unrelated pre-existing `modakbul` Chrome PID 11478 was not touched.

V12 used only unused loopback port 65534 and displayed `stale`, a reason, a retry action, and the last-checked time in `evidence/v12-wrong-loopback-runtime.json` and `evidence/v12-wrong-loopback-stale.png`.
The live default daemon and Chrome remained running.
V13 queried only `herdr-ide-verify-absent`, displayed guidance without creating it, and is recorded in `evidence/v13-absent-profile-runtime.json` and `evidence/v13-absent-profile.png`.
V6 hid chromux only from the owned app process PATH, displayed an actionable unavailable state, and is recorded in `evidence/v6-path-hidden-runtime.json` and `evidence/v6-path-hidden.png`.
The combined safe-failure boundary is `evidence/v6-v12-v13-safe-failures.json`.

### T15 and T16: ADDITIVE NOTICE AND VISUAL-STATE IMPLEMENTATION PASS, REMAINING ROWS PARKED

Working or attention pane close previews state that the running process is terminated and list the affected work before confirmation.
Workspace and tab previews aggregate working or attention panes into one warning rather than opening one prompt per pane.
The worktree preview states that the checkout directory is removed and that uncommitted files can be lost.
Idle pane policy does not add an unnecessary confirmation.
Confirmation and cancellation previews publish a result sentence and never mutate a user resource.

The native warning evidence is `evidence/v7-working-pane-warning.png`, `evidence/v31-workspace-aggregate-warning.png`, and `evidence/v35-worktree-warning.png`.
Six Swift policy tests cover offscreen pet recovery, repeat placement, working and idle pane consequences, workspace and tab aggregation, and the worktree checkout-loss boundary.
No destructive Continue action was used against a user resource.

V29 remains partial because pane-unselected, first-workspace, missing-summary, and no-open-tab states were not all exercised in one real runtime.
V30 remains partial because binary and read-failure reasons exist, but a real PTY exit code surface was not exercised.
V33 remains partial because browser and mini loading states are visible, while initial sidebar, terminal attach, and large-file loading were not all exercised.
V34 remains pending because focused-pane path versus tree-root divergence was not demonstrated in the app.
V7, V31, and V35 have native warning and policy evidence, but actual product close and removal executors remain outside this additive preview layer.

### T13: POSITIVE MINI PATH VERIFIED, FULL IN-APP WORKFLOW PARTIAL

Mini live state was read before mutation, and only the exact prefix-owned `herdr-ide-verify-mini-w01` workspace was created.
The fixture contained panes `w4J:p1` and `w4J:p2` under `/tmp/herdr-ide-verify-mini`.
Workspace display, split, real terminal attach, `REMOTE_ATTACH_OK`, file browse, and the 48-byte README SHA-256 `7cc719e2dabf18ac17a8f1685bf5afe4b3ac2dc63cfd80c9d5f7a4d8a2aff19f` were verified on the positive path.
The native app showed mini ready and the reason remote inline editing remains disabled.
The combined receipt is `evidence/t13-t19-mini-positive-runtime.json`, and the retained manifest is `evidence/t19-mini-manifest.json`.

T13 remains partial because the real attach, split, and browse probes were performed through the owned fixture boundary rather than all being driven from a complete in-app remote navigation workflow.
V19 disconnect injection was NOT RUN.
No mini connection was cut, and no remote process or service was killed or restarted.

### T19 and V8: PREFIX-OWNED LIVE FIXTURE PASS

The fixture tool now supports deterministic plan, scale plan, create, status, and cleanup operations for local and mini targets.
Every operation requires the exact `herdr-ide-verify-` prefix, a validated manifest, exact live workspace ID and label ownership, and an owned `/tmp/herdr-ide-verify-*` path.
The remote path contract rejects shell syntax and requires the path to match the fixture name before any SSH process launch.
Create is convergent when run twice, and cleanup revalidates every live target before closing only manifest-owned workspaces.

The local `herdr-ide-verify-v8` fixture created exactly 7 workspaces and 11 panes.
With the fixture live, owned app PID 38870 used 138880 KB, or 135.625 MB RSS, below the 400 MB V8 limit.
The retained manifest is `evidence/t19-local-v8-manifest.json`, and the scale and cleanup receipt is `evidence/t19-v8-scale-runtime.json`.

After exact ownership revalidation, only local IDs `w2Z`, `w20`, `w31`, `w32`, `w33`, `w34`, `w35` and mini ID `w4J` were closed.
The local and mini prefix-owned workspace counts are both zero, both owned `/tmp` fixture paths are absent, and both manifests remain retained.
The observed non-prefixed workspace IDs and labels were unchanged across cleanup.
No user workspace or worktree was touched.

### Safe state and secret rows

V17 core behavior passes at the in-process boundary: missing, corrupt, and unknown-schema UI state use safe defaults and publish structured `ui_state.missing` or `ui_state.corrupt` status.
V28 core behavior passes for valid selected and expanded state round-trip and atomic replacement, and production source contains no `navigator.json` read.
Full native relaunch restoration remains pending, so V17 and V28 are not claimed as complete browser-runtime verdicts.
The receipt is `evidence/v17-v28-persistence-contract.json`.

V20's missing summary produces `Check agent-context-labels settings` instead of a blank line.
V25 static inspection found zero production reads of `OPENROUTER_API_KEY`.
A synthetic sentinel was supplied only to one owned LaunchServices app process, whose startup observation showed one visible key main window and whose final retained receipt captures deactivation before cleanup.
The app wrote no sentinel to source, evidence, state, or process log.
No real secret value was read or printed.
Evidence is `evidence/v20-v25-openrouter-nonaccess.json` and `evidence/v25-openrouter-sentinel-window.json`.

### T3 remains blocked

The existing SwiftTerm 1.20.0 document-coordinate adapter, bounded trace hygiene, real partial trace, and diagnosis evidence were preserved.
The composition state tests pass seven document-coordinate lifecycle cases.
The human artifact has no `SPIKE_BOUNDARY_PROBE` compile definition, so `MarkedTextBoundaryProbe`, its `NSApp.activate`, and its synthetic key event path are compile-time excluded.
Static scans found no global or local event monitor, CGEvent or IOHID path, event posting or requeue, `discardMarkedText`, DEL suppression, or input-source manipulation.
No user retest app was launched and no human judgment was requested in this batch.
The proxy tests are not V9 acceptance evidence.
T3 remains BLOCKED, Backspace remains unresolved, AC2 remains unmet, and a fresh physical-keyboard V9 verdict is still required before v1 completion.

### Final verification and boundaries

`cargo fmt --all -- --check` passed after final formatting.
`cargo clippy -p herdr-core --all-targets -- -D warnings` passed.
`cargo test -p herdr-core` passed 17 unit tests and 10 FFI integration tests.
The FFI set includes the approved and required in-process unknown kind, schema mismatch, malformed payload, invalid options, and off-owner tests.
No external service failure was injected.
`swift test --package-path macos --disable-keychain --disable-sandbox` passed 6 tests.
The spike Rust crate passed 5 tests, and the spike Swift composition package passed 7 tests.
The release C caller exercised create, callback, dispatch, snapshot, free, and destroy and exited zero.
Two final app assemblies produced the same content hash `a56003585b26789ffabdb6d9dddabb11cbb354ea345f0eaa133310480b1ee0bc`.
Strict deep codesign verification passed.
The aggregate machine receipt is `evidence/additive-operational-verification.json`.

Peekaboo permission status reported Screen Recording and Accessibility granted before native checks.
Each native check first proved exactly one owned assembled development app and targeted the exact `Herdr IDE` window.
Fresh screenshots proved the three-column shell, browser and mini status, destructive consequence dialogs, and pet surface.
Only owned app processes were terminated after checks, and the final audit found zero HerdrIDE apps, zero Swift shell spike apps, and zero owned fixture workspaces.

Engineering Principle 3 grew the system through core contracts, process executors, observable models, and native views in separate layers.
Engineering Principle 4 made unavailable, stale, failed, and refused outcomes explicit.
Engineering Principle 5 kept chromux, mini, pet, consequence, fixture, persistence, and terminal composition concerns modular.
Engineering Principle 9 records structured receipts for process, focus, state, and cleanup questions.
Engineering Principle 10 exposes failures outside the process through status cards, stderr JSON, and atomic evidence receipts.
Engineering Principle 11 made fixture creation, status, cleanup, focus receipts, and state writes safe to repeat.
Engineering Principle 13 fixed the remote fixture path class by validating exact owned paths before SSH instead of relying only on a prefix.
Design Principle 6 states process, tab, workspace, and checkout consequences before any destructive action.
Design Principle 7 uses distinct ready, loading, stale, unavailable, and failed visual states rather than silent blank surfaces.
The environment practice keeps variable declarations and validation in the Rust registry while values remain external and absent values degrade only their capability.
The outcome-focused test practice uses durable in-process contracts for coordinate, C ABI, ownership, and policy behavior, and reserves native claims for actual signed-window screenshots.

T5, every tag including `rust-native-final`, source or evidence deletion, live herdr failure injection, mini disconnect injection, chromux daemon kill, default profile deletion, Chrome termination, push, PR, delivery, unified finalize, and Done remain NOT RUN or forbidden.
The user-directed downstream dependency deviation remains explicit because this additive work proceeded while T3 and the Stage 0 gate stayed open.
The coherent source, test, and selected evidence checkpoint for this operational batch is `b1a2d4dcf08223df7b0de255571257288e3ea797`.

## CDP title character-reference display correction

The retained launch receipt `evidence/t12-default-runtime.json` proved that structured CDP JSON decoding produced the literal title `Who&#39;s using Chrome?`, and the retained native screenshot rendered that undecoded value.
The live default endpoint returned an empty array during this correction, so it was observed read-only and was not mutated to recreate the prior page target.
The defect was fixed at the successful `ChromuxTab.title` decoding boundary: JSON remains decoded by `JSONDecoder`, then only the decoded title string has CoreFoundation character references unescaped once before it enters the runtime receipt and SwiftUI display.
URL, type, array structure, type validation, endpoint reachability, and invalid-JSON behavior remain on the existing structured decoding path.

The focused `cdpTabTitleDecodesCharacterReferencesAfterStructuredJSONParsing` test used the retained `/json/list` payload shape and first failed because the observed value remained `Who&#39;s using Chrome?`.
After the boundary fix, that test passed and also proved that `chrome://profile-picker/` and the `page` type remained unchanged.
The full affected Swift suite passed 7 tests.
The development app assembled successfully and strict deep codesign verification passed.

Peekaboo permissions reported Screen Recording and Accessibility granted before observation.
Because the live default CDP endpoint had no page target, one owned loopback endpoint on port 65431 supplied only the retained failing JSON payload to the app's existing DEBUG endpoint override.
It did not send a command to Chrome, chromux, herdr, or mini.
Exactly one current assembled dev app was present for the accepted observation: PID 6821 with main window ID 1906 titled `Herdr IDE`.
The accessibility tree exposed `Who's using Chrome?`, and the single fresh exact-window screenshot visibly renders the same decoded title at `evidence/v5-decoded-chrome-title.png`.
The corresponding structured runtime receipt is `evidence/t12-decoded-title-runtime.json`, and the combined verification record is `evidence/v5-decoded-chrome-title-verification.json`.

An earlier owned launch PID 3828 was discarded before evidence capture because it had been started without the bounded workspace-root argument and spent its time in the existing Workbench filesystem traversal.
A process sample ruled out the CDP decoder, PID 3828 was terminated, and no screenshot from that launch was retained as acceptance evidence.
After the accepted screenshot, exact ownership was rechecked and only app PID 6821 and loopback PID 3145 were terminated.
The final audit found no owned Herdr IDE app and no listener on port 65431.
Default Chrome PID 9609 remains running, and the unrelated modakbul Chrome PID 11478 remains running and untouched.

Engineering Principle 4 preserves explicit structured decode and endpoint failures instead of accepting raw JSON heuristics.
Engineering Principle 5 keeps CDP transport, structured JSON decoding, title display normalization, and SwiftUI rendering as separate concerns.
Engineering Principle 12 prices the regression at the small stable CDP parsing boundary, while the user-visible claim is proven once in the signed native app.
Engineering Principle 13 fixes the character-reference class rather than replacing only the observed apostrophe string.
Design Principle 7 is satisfied by rendering the browser's derived title as readable user-facing state.
The outcome-focused test practice asserts the caller-visible decoded title while preserving the neighboring structured fields.

No T3 human retest, keyboard automation, T5 action, file deletion, fixture workspace recreation, tag, push, PR, gate rerun, finalize, qa-log change, or external-service failure injection occurred.
The source, regression test, runtime receipt, native screenshot, and verification-record checkpoint is `fdd3a7df9eaa4b259767e1f6191ca498decc75fd`.
T3 remains BLOCKED with AC2 unmet and V9 pending human judgment, and T5 remains forbidden.

## T18 selective pet hit-region correction

The prior panel-wide `ignoresMouseEvents = false` policy let transparent corners inside the 92 by 92 window intercept clicks.
A pure `PetHitRegion` ellipse now matches the visible circle after its four-point content inset and decides the AppKit window's mouse ownership from screen-space cursor location.
The center and circular edge remain interactive, while the transparent corners and all points outside the ellipse set `NSWindow.ignoresMouseEvents` to true.
No CGEvent tap, IOHID path, global or local event monitor, event posting, event requeue, or keyboard or mouse automation was added or used.

The pre-fix focused run passed center and edge but failed transparent corner and repeated corner behavior with two issues across four tests.
After the correction, the same four focused tests passed, and the full affected Swift suite passed all 11 tests.
The existing offscreen recovery tests remain green and the signed runtime clamped the verification request to 1636 by 993.
The development app assembled successfully and strict deep codesign verification passed.

The visible pet owns at most one common-run-loop polling timer while shown.
Repeated visibility refreshes cannot allocate a second timer, hiding the pet invalidates the timer, the timer closure retains the controller weakly, and the lifecycle owner invalidates the timer and unregisters notification observers during teardown.
The runtime receipt shows one active polling instance, zero local or global monitors, zero event posting, center and edge `ignoresMouseEvents = false`, corner `ignoresMouseEvents = true`, and restoration to the real cursor-derived state without altering user input.
That receipt is `evidence/t18-selective-hit-region-runtime.json`, and the combined verification record is `evidence/t18-selective-hit-region-verification.json`.

Peekaboo permissions reported Screen Recording and Accessibility granted before observation.
Exactly one assembled dev app ran from the current bundle as PID 55772, with main window ID 1978 and the pet frame at 1636 by 993 with size 92 by 92.
The fresh read-only area capture at `evidence/v24-selective-hit-region-pet-visible.png` was visually inspected and shows the pet circle still rendered.
Only owned app PID 55772 was terminated after capture, and the final exact-path audit found zero owned Herdr IDE processes.

Engineering Principle 4 keeps the runtime state and receipt failures explicit.
Engineering Principle 5 separates pure hit geometry, window ownership updates, lifecycle cleanup, and evidence recording.
Engineering Principle 9 records the cursor-derived state, probe outcomes, and polling lifecycle needed for later diagnosis.
Engineering Principle 10 exposes both `ignoresMouseEvents` outcomes outside the process in a bounded JSON receipt.
Engineering Principle 11 makes repeated refresh and polling start or stop calls idempotent.
Engineering Principle 12 prices the regression at the stable pure geometry boundary and reserves the signed native run for AppKit property and visibility evidence.
Engineering Principle 13 fixes the full transparent-corner failure class rather than special-casing one coordinate.
Design Principle 7 preserves the visible circular affordance while deriving the invisible click-through state from the same geometry.
The outcome-focused test practice covers center, edge, corner, and repeat behavior without coupling tests to a live cursor.

The corrected failure-injection boundary forbids external-service and live-system fault injection only.
Required in-process core tests for unknown kind, schema mismatch, malformed payload, invalid options, and off-owner behavior remain allowed and are not reclassified as forbidden.
The selective hit-region source, regression tests, bounded runtime receipt, and native screenshot checkpoint is `eb666eaee80fb785f46babcec3d60152b7a08808`.
No T3 human retest, keyboard or mouse automation, T5 action, source or evidence deletion, fixture recreation, tag, push, PR, gate rerun, finalize, qa-log change, or service disruption occurred.
T3 remains BLOCKED with AC2 unmet and V9 pending human judgment, and T5 remains forbidden.

## Live pane attach, sidebar session polling, and IME composition byte suppression

This batch delivered the two operational gaps the user hit in person: sidebar clicks not driving the terminal, and terminal input not reaching any real pane.

`herdr-core` gained a `live` module.
A session poller reads `session.snapshot` from the local herdr socket once per second, requires protocol revision 21 explicitly, and passes agent tokens through verbatim so the unseen-versus-acknowledged rules of INV-herdr-unseen-token stay owned by the sidebar projection.
Socket-file-missing, unreachable, protocol-mismatch, and malformed responses are distinct explicit states surfaced through `status.herdr` and the sidebar empty state.
`focus_pane` now spawns `herdr pane attach <pane_id>` under a portable-pty transport with a generation guard, so a superseded attach cannot write into the next pane's terminal, and re-focusing the attached pane is a no-op.
Key events decode and write to the attached PTY instead of the loopback echo, input for a mismatched or missing attach raises a typed error instead of dropping bytes, and a new `terminal_resize` event resizes the live PTY.
The Swift shell restores the persisted pane selection once at launch, routes input to the attached pane, shows the attached pane in the terminal header, and reports the herdr connection state when the sidebar is empty.
The fixture path is unchanged: a null socket keeps loopback semantics, and the seeded fixture instance still runs without live access.

The IME defect recorded in `evidence/v9-backspace-adapter-human-retest-trace.json` was re-diagnosed from the call stream: on Backspace during Korean composition the input method re-marks the composing text, commits it through `insertText`, and lets the key fall through to SwiftTerm's `doCommand(deleteBackward:)`, which leaks DEL while the composition is stale.
SwiftTerm's `keyDown` and `doCommand` are not open, so the fix enforces the rule where bytes leave the view, mirroring Ghostty's composing control-input policy: `ImeTerminalView` captures composition state in `interpretKeyEvents`, a plain Backspace during composition swallows both the give-up commit and the DEL, and any other single C0 or DEL fallback byte produced during composition is suppressed while committed text and multi-byte arrow sequences still pass.
The decision table is the pure `CompositionInputPolicy`, covered by six Swift tests; coordinate-adapter work in the spike remains untouched.

Machine verification: 20 herdr-core unit tests, 11 FFI contract tests including the new live-key-without-attach error row, clippy with denied warnings, fmt, 17 Swift tests, and a clean dev-app assembly with strict codesign.
Runtime verification used only the prefix-owned fixture: `herdr-ide-fixture create herdr-ide-verify-attach` produced workspace w37 with pane w37:p1, the launched app restored the persisted selection, spawned exactly one `herdr pane attach w37:p1` client, rendered the live sidebar with sixteen real agents, and a peekaboo-typed probe `herdr-attach-proof-42` landed in `herdr pane read w37:p1 --source visible` and echoed back into the app terminal.
Process memory was 118 MB against the 400 MB V8 bound.
Screenshots and the bounded receipt are `evidence/live-initial.png`, `evidence/live-fixture-attach.png`, `evidence/live-input-proof.png`, and `evidence/pane-attach-ime-verification.json`.
No real agent pane received input, the one pre-existing fixture-mode app instance was left running, and the fixture workspace and its /tmp directory were removed afterward.

The machine verification was followed by the required physical-keyboard verdict: `1,2,3,4 모두 잘 맞아 다 된다`.
The terminal V9 axis therefore passes, AC1-AC3 and T3 are satisfied, and the editor axis remains deferred by prior user agreement.
T5 is now complete through deletion and focused V21/build/native evidence; push, PR, and finalize remain NOT RUN.

## T5 deletion and V21 verification

Before deletion, the annotated tag `rust-native-final` was created and verified with tag object `9f388c3378dcba3f23b07ba24f76b6b092d7bda7` and peeled commit `b2582ade493bc6569af0e9719aa672751962df96`.
The approved root files `src/render.rs`, `src/terminal.rs`, `src/accessibility.rs`, `src/browser.rs`, and `src/openrouter.rs` were deleted.
The retired AppKit/wgpu application implementation was removed from `src/app.rs`, leaving only a four-line historical module note, and its compile references were removed from `src/lib.rs`.
The obsolete dependencies `wgpu`, `glyphon`, `bytemuck`, `raw-window-handle`, `objc2`, `objc2-app-kit`, `objc2-foundation`, `alacritty_terminal`, and `security-framework` were removed from `Cargo.toml` and `Cargo.lock`.
No spike copy, evidence, user file, or source outside the approved T5 list was deleted.

`cargo check --workspace --all-targets`, `cargo fmt --all -- --check`, `cargo clippy -p herdr-core --all-targets -- -D warnings`, `cargo clippy --workspace --all-targets`, `cargo test --workspace --all-targets`, and 17 Swift tests passed.
The workspace tests passed 106 tests with zero failures and one pre-existing ignored live-fixture test.
The strict workspace `clippy -D warnings` attempt correctly failed on nine retained legacy warnings outside the T5 scope; the ordinary workspace clippy and denied-warning `herdr-core` boundary both passed without hiding that result.
Two unchanged-source app assemblies converged on bundle manifest SHA-256 `2935021416052171287be2aed2969633cbbe6d716844291425501eb853adcfc8` and executable SHA-256 `5d0027358d112b0294c758137e2f296b5421a5c8b5f84945ef94074a82c57964`.
Strict deep codesign, arm64 architecture, and minimum macOS 14.0 passed.
Exactly one owned app, PID 38574, rendered key, frontmost, and on-screen window 2767 for `evidence/t5-rust-native-deletion-native.png`, then the exact app and attach child were terminated.
The structured receipt is `evidence/t5-native-removal-verification.json`.
`cargo tree --workspace` and `Cargo.lock` contain no reachability for the nine removed packages; `herdr-core` remains free of objc2, wgpu, and glyphon.

The user then directed: `검증은 내가 어느정도 다 했으니 마무리하는 식으로 얼른 가자.`
Accordingly, later verification uses the shortest focused checks and retained evidence.
Rows not actually executed are recorded either as `사용자 수동 검증으로 수용됨` only where that direction legitimately covers an observed product behavior, or as `미실행-pending`; no unexecuted row is presented as machine-verified.
Live-service failure-injection rows V18 and V19 remain `미실행-pending`.

## R11 environment registry and R6 owner-thread contract

T5 was committed separately as `99c4aa85e8b7b0c388c1848967690584a5e40754`.
The enumerable Rust registry now owns `SSH_AUTH_SOCK`, `PATH`, and `HERDR_SOCKET_PATH`, including required status, format, validation, and absence behavior.
Swift has zero direct reads of `ProcessInfo.processInfo.environment`; browser and remote capability decisions consume the Rust snapshot state, and Rust applies the socket override without exposing its value.
The only production raw environment read is the registry boundary in `herdr-core/src/environment.rs`.
Encoded status and tests contain no raw socket or PATH values.

The six C ABI functions remain unchanged.
Create, dispatch, snapshot, callback registration, and destroy belong to the creating thread, which the Swift `@MainActor` bridge owns.
An off-owner snapshot now records `ffi.wrong_thread`, notifies the owner, and returns empty bytes instead of serializing state on the wrong thread.
An off-owner destroy records and notifies the same violation but does not free the handle; the owner can observe the error and destroy normally.

`cargo test -p herdr-core` passed 21 unit tests and 13 FFI tests.
The added stable-boundary tests cover the three-key secret-safe registry, off-owner snapshot rejection, and off-owner destroy rejection.
`cargo clippy -p herdr-core --all-targets -- -D warnings`, Rust formatting, 18 Swift tests, the PATH-hidden no-process boundary test, direct-access scans, and `git diff --check` passed.
The structured receipt is `evidence/r6-r11-contract-verification.json`.
R6, R6a-R6d, R11, V21, and V32 are PASS at this boundary.

## Native pane split and zoom commands

Source checkpoint `18653e56eee3c9ba226739864d395b6e8478d55b` adds a native Pane menu with Split Right (`Command-D`), Split Down (`Command-Shift-D`), and Toggle Zoom (`Command-Option-Return`).
`create_pane` now carries `direction: right | down`, `toggle_zoom` carries `pane_id`, and the existing six-function ABI is unchanged.
The core invokes `herdr pane split` with an argv array rather than a shell and invokes `herdr pane zoom --toggle`; `Snapshot.zoomed` remains the rendered state source.

Focused verification passed 22 `herdr-core` unit tests, 14 FFI contract tests, denied-warning core clippy, Rust formatting, 18 Swift tests, the dev-app assembly, strict deep codesign, arm64 inspection, minimum macOS 14.0 inspection, and `git diff --check`.
The executable SHA-256 is `4d6869e96f8c5825bc9f21f2fc90facd2f16209ec6302b24d055146a9cb9afe2`.

The T19 fixture `herdr-ide-verify-shortcuts-20260829` began as one owned workspace `w3C` and pane `w3C:p1`.
Native menu dispatch created `w3C:p2` to the right, `w3C:p3` below, and toggled zoom for `w3C:p1`.
Peekaboo inventory exposes the three concise labels and exact shortcut equivalents.
The real screenshot `evidence/pane-shortcuts-native.png` shows the signed app's three-column shell and `w3C:p1 · zoomed` header; the bounded receipt is `evidence/pane-shortcuts-runtime.json`.

One raw Peekaboo `Command-D` attempt returned indeterminate and showed no immediate layout change, so it is explicitly excluded from acceptance evidence.
The final owned fixture later contained an additional `w3C:p4`; the receipt records that state without assigning a cause.
No terminal text or probe byte was sent, no input source changed, no user workspace was touched, and no external service failure was injected.
The exact owned app PID 31174 and attach child PID 31223 were terminated after capture; the prefix-owned fixture was retained.

The user direction `검증은 내가 어느정도 다 했으니 마무리하는 식으로 얼른 가자.` limits further cycles.
Unexecuted rows are not promoted: V10, V18, V19, V22, and V34 remain `미실행-pending`, while partial rows retain their exact gaps.
V18 and V19 real-service failure injection was not run.

Engineering 4, 9, and 10 make pane-control failure and runtime outcomes observable; Engineering 5 keeps CLI planning, event routing, Swift actions, and evidence separate; Engineering 11 uses an ownership-prefixed fixture and exact process convergence; Engineering 12 prices the stable planner and FFI tests separately from the real native runtime.
Design 3, 5, and 7 place frequent actions on iTerm2-compatible shortcuts, use the existing native menu pattern, and derive the visible zoom label from core state.

## Authoritative pane grid, command lifecycle, Settings boundary, and zoom-buffer retention

The user-reported sidebar pause was reproduced on owned PID 23460 before the fix.
`evidence/sidebar-freeze-pid23460.sample.txt` shows the owner thread blocked in `focusPane`, core dispatch, `Runtime.apply`, `PaneAttach` destruction, `portable_pty::Child.wait`, and `wait4`.
The class-level fix moves pane attach and pane control lifecycles behind bounded asynchronous workers, preserves the owner-thread FFI contract, and publishes requested, ready, failed, and authoritative refresh diagnostics instead of waiting for a PTY child or Herdr command on the main thread.
The exact lifecycle receipt is `evidence/pane-grid-v8-diagnostic.json`, and `evidence/grid-diagnostic-graceful-termination.log` distinguishes a normal AppKit termination from a crash.

Herdr remains authoritative for the recursive current-tab pane layout.
The core snapshot now carries `pane_layout.workspace_id`, `tab_id`, `focused_pane_id`, `zoomed`, and a recursive `pane | split(direction, ratio, first, second)` tree.
The Swift shell renders all leaves concurrently and owns one independent SwiftTerm view and attach feed per pane.
The T19-owned V8 workspace `w3F` rendered eleven panes with eleven attaches at 220752 KB combined RSS, below the 409600 KB limit, then only that owned fixture and app were cleaned.
The screenshot is `evidence/grid-v8-11-pane.png`, and the structured receipt is `evidence/pane-grid-v8-diagnostic.json`.

Focus accent is derived only from the authoritative `pane_layout.focused_pane_id` and uses the system `accentColor`.
There is no hover state, hover callback, hover highlight, global monitor, or local monitor in the grid.
Click dispatches asynchronous `focus_pane`; the accent moves only after the refreshed snapshot changes the focused pane.
The owned-fixture transition to `w3H:p4` is visible in `evidence/pane-actions-settings-diagnostic-r2-focus-p4.png` and its matching layout JSON.

The native Pane menu and key-equivalent router expose Split Right, Split Down, Toggle Zoom, and Close Pane.
`create_pane.direction`, `toggle_zoom.pane_id`, `close_pane.pane_id/confirmed`, and the recursive layout remain inside the six-function ABI event and snapshot contract.
Idle Close Pane uses the existing `close_pane` path immediately, while working or attention state produces the T15 consequence dialog and sends `confirmed=true` only after confirmation.
The Settings scene contains only the Keyboard section for these four bindings.
Default, missing, corrupt, invalid, duplicate, reserved, persistence, and dynamic routing seams are covered by focused tests and observable diagnostics.

The native Settings scene itself rendered correctly in `evidence/pane-actions-settings-diagnostic-r2-settings-latest.png`.
Exact-window TextField automation did not change the visible value and returned indeterminate receipts, so it is not acceptance evidence and was not retried.
`evidence/pane-actions-settings-driver-limitation.json` records the driver limitation.
Native TextField editing, immediate menu-equivalent refresh, action routing after user edit, and relaunch persistence were not proven by the failed driver; they were subsequently accepted through the user's final manual check with the exact verdict `좋네`.
No global input was sent after that handoff.

Before the fixture-retarget regression was added, stale/global focus redirected a diagnostic menu or shortcut action into user-owned w2Y and created at least one unintended pane split.
The evidence does not establish the exact number of created panes, so no count beyond one or more is claimed.
App input was stopped and the diagnostic app was terminated immediately; because w2Y was outside fixture ownership, this run did not close or otherwise clean up the created pane or panes.
After the retarget regression landed, no further focus, split, zoom, cleanup, automatic input, or other mutation targeted w2Y.
The later user-provided w2Y screenshot at `/Users/hoyeonlee/.claude/image-cache/e98ecf21-4a48-4ae2-8f9c-85ba8963eb80/4.png` was used read-only to diagnose the zoom-buffer defect.
The defect was reproduced at the presentation seam: zoom selected only the focused subtree, SwiftUI dismantled hidden SwiftTerm views while their core attach workers continued draining bytes, and recreated views therefore showed only future output fragments.
The pre-fix regression in `evidence/zoom-buffer-retention-regression-before.log` failed on that exact behavior.

The correction keeps every pane view in one stable ZStack throughout zoom and unzoom.
Hidden panes retain their authoritative nonzero frame, SwiftTerm instance, attach PID, and output feed; only opacity, hit testing, accessibility visibility, and z-order change.
The focused pane uses the full canvas visually while zoomed.
When its real frame changes, SwiftTerm `setFrameSize` invokes `processSizeChange`, the delegate emits `terminal_resize`, and herdr-core resizes the matching attach PTY with `terminal.resize_failed` observable on failure.

The owned w3H fixture ran distinct continuous output in all three panes.
The screenshots `evidence/zoom-buffer-retention-before.png`, `evidence/zoom-buffer-retention-on.png`, and `evidence/zoom-buffer-retention-after.png` show all three buffers populated before zoom, the focused pane full-canvas during zoom, and all three panes restored with later line numbers after unzoom.
Attach PIDs 25551, 25552, and 25550 remained bound to w3H:p1, w3H:p3, and w3H:p2 across the sequence.
The bounded receipt is `evidence/zoom-buffer-retention-diagnostic.json`, and the final focused suite passes five pane-grid tests including retained nonzero frames and authoritative-only focus.
The diagnostic app and fixture are temporary and will be replaced by a fresh zero-automation human-readiness instance after the checkpoint build.

Engineering Principles 4, 9, and 10 make asynchronous command, attach, resize, Settings fallback, and lifecycle failures externally observable.
Engineering Principle 5 separates authoritative core layout, per-pane terminal lifetime, command policy, Settings persistence, and evidence.
Engineering Principle 11 makes exact fixture creation, attach convergence, repeated focus, and zoom/unzoom lifecycle repeatable.
Engineering Principle 12 prices pure layout, shortcut, and consequence seams separately from signed native evidence.
Engineering Principle 13 fixes the view-lifetime class instead of replaying a single damaged buffer.
Design Principles 3, 5, and 7 keep frequent pane actions in native shortcuts and menus, use a standard Settings scene, and bind focus/zoom visuals to authoritative state.
Design Principle 6 keeps destructive Close Pane consequences ahead of confirmation.

V8 remains PASS at 220752 KB combined RSS.
The pane-grid zoom-buffer regression is PASS on the owned three-pane fixture.
Settings native edit, immediate routing, and relaunch behavior are `사용자 수동 검증으로 수용됨` under the exact final verdict `좋네`; the failed driver attempt remains recorded as unsuccessful automation and is not relabeled as machine evidence.
V18 and V19 external-service failure injection remain `미실행-pending`.

## Final pane-grid human acceptance

The user's exact final verdict is `좋네`.
This closes the final human checklist as `사용자 수동 검증으로 수용됨` for the simultaneous pane grid, Split Right, Split Down, Toggle Zoom, Close Pane, the sidebar responsiveness correction, Settings shortcut rebind behavior, and authoritative focus versus hover separation.
The acceptance supplements the retained machine and native evidence; it does not convert the failed Settings automation attempt into a pass.
It also does not change V10, V18, V19, other partial or unexecuted verification rows, or the recorded Sasu gap-audit/spec BLOCK and start refusal into Done.
