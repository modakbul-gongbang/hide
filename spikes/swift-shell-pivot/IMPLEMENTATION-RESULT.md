# Swift shell pivot implementation result

Status: **Partially Done**.

Pre-deletion source checkpoint and `rust-native-final` target: `b2582ade493bc6569af0e9719aa672751962df96`.
T5 deletion checkpoint: `99c4aa85e8b7b0c388c1848967690584a5e40754`.
R11/R6 cleanup checkpoint: `48e2a56421344a2490db3051fd50a866cb8003a5`.
Pane command source checkpoint: `18653e56eee3c9ba226739864d395b6e8478d55b`.
Current pane-grid, lifecycle, shortcut Settings, Close Pane, and zoom-buffer correction: working tree based on `b2670107dc152eaa67d448dce4d4681151170448`, pending the checkpoint recorded later in this report.

This report describes the committed T5 deletion, the follow-up contract cleanup, the pane command feature, and retained runtime evidence.
The user supplied the authoritative physical-keyboard terminal verdict `1,2,3,4 모두 잘 맞아 다 된다`.
The terminal V9 axis passes, the editor axis remains deferred by prior agreement, T3 and AC1-AC3 are satisfied, and T1-T4 are fully passed.
The user explicitly authorized T5, and the tag, approved deletion, dependency removal, focused build/test checks, deterministic assembly, codesign, and native screenshot are complete.
The later direction `검증은 내가 어느정도 다 했으니 마무리하는 식으로 얼른 가자.` limits remaining work to short focused checks and honest manual-acceptance or pending labels.

## User-visible changes

- The development app is a signed macOS 14 arm64 SwiftUI application with a resizable three-column Agents, Terminal, and Workbench layout plus native menus.
- The sidebar projects live Herdr session tokens into seven authoritative states, two-line summaries, and `sort_rank` then `activity` ordering.
- The center terminal renders Herdr's recursive current-tab pane layout as a simultaneous grid with one retained SwiftTerm 1.20.0 view and one live attach per pane.
- The blue focus accent uses the system accent color and follows only the authoritative snapshot focus; hover has no visual focus effect, and click requests asynchronous focus before the accent moves.
- The workbench browses existing local files, renders image, Markdown, code, and text surfaces, and supports local text drafts and saves without adding create, rename, move, or delete operations.
- Browser status and opening are delegated to the literal chromux executable and the `default` profile, with visible unavailable, stale, absent-profile, loading, and ready states.
- The mini positive path, environment status, destructive-consequence previews, persistent UI state, and the selective-click-through pet surface are represented in the shell.
- The native Pane menu exposes Split Right (`Command-D`), Split Down (`Command-Shift-D`), Toggle Zoom (`Command-Option-Return`), and Close Pane (`Command-W`); the actions travel through the six-function FFI event contract and the recursive grid derives focus and zoom from the core snapshot.
- Zoom is visual-only: hidden terminals retain their view, attach, feed, and nonzero frame, while unzoom resizes the changed terminal through the existing `terminal_resize` path.
- The standard Settings scene provides only a Keyboard section for rebinding those four pane actions, with persisted defaults, validation, explicit fallback diagnostics, and immediate command-policy routing.
- The p3 checkpoints add machine-tested composition byte suppression, re-anchor the SwiftTerm marked-text overlay after pane echo, and hide the block caret while composition is active; the human owner then passed all four terminal checks with `1,2,3,4 모두 잘 맞아 다 된다`.

The current human-readiness runtime is captured in [v9-b2582ad-human-retest-ready.png](evidence/v9-b2582ad-human-retest-ready.png) and [v9-b2582ad-human-retest-ready.json](evidence/v9-b2582ad-human-retest-ready.json).
The earlier d290928 and ecc46fb readiness artifacts are retained as superseded chronology, not current acceptance evidence.

## Implemented module and ownership boundaries

| Module | Current responsibility | Boundary status |
| --- | --- | --- |
| `herdr-core/src/model.rs` | Versioned options, snapshot, status, terminal, editor, IME, sidebar, and UI-state data | Platform-neutral committed contract |
| `herdr-core/src/ffi.rs` and `include/herdr_core.h` | Six-function C ABI, Rust-owned buffers, callback registration, panic containment, owner-thread enforcement | Implemented; off-owner snapshot and destroy reject, notify, and preserve an observable error for the owner |
| `herdr-core/src/runtime.rs` | Event validation, authoritative pane-layout transitions, per-pane terminal generations, asynchronous pane controls, persistence, file actions, browser and remote status | Implemented; failures are reflected through status diagnostics and `last_error` |
| `herdr-core/src/live.rs` | Local Herdr socket polling, protocol 21 enforcement, asynchronous PTY attaches, shell-free pane controls, terminal output, resize, and input routing | Implemented and runtime-evidenced on prefix-owned multi-pane fixtures |
| `herdr-core/src/sidebar.rs` | Seven-state token projection and authoritative ordering | Implemented and unit/runtime-evidenced |
| `herdr-core/src/files.rs` | Existing-file open, draft, save, conflict, and diff projection | Implemented; full native failure matrix remains incomplete |
| `herdr-core/src/chromux.rs` and macOS operational models | Pure planning plus default-profile executor, CDP read-only status, visible results | Implemented for approved default and safe failure branches |
| `herdr-core/src/environment.rs` | Enumerable `SSH_AUTH_SOCK`, `PATH`, and `HERDR_SOCKET_PATH` contracts without value disclosure | Implemented; Swift consumes only registry state and has no direct raw environment read |
| `herdr-core/src/persistence.rs` | Versioned UI state including shortcut bindings, safe missing/corrupt fallback, atomic replacement | Implemented; full native V17/V28 relaunch restoration evidence remains incomplete |
| `herdr-core/src/fixture.rs` and `src/bin/herdr-ide-fixture.rs` | Prefix validation, local/mini fixture plan/create/status/cleanup | Implemented and used for the retained V9 fixture |
| `macos/Sources/HerdrMacOS` | SwiftUI shell, authoritative pane grid, retained per-pane terminals, native pane commands and Settings, workbench, browser/mini status, notices, and pet | Production application surface after the approved T5 removal |
| `spikes/swift-shell-pivot` | Stage-0 spike, IME traces, runtime receipts, and verdict ledger | Preserved; proxy IME checks are diagnostic only |

The approved T5 deletion and crate split are complete.
The platform-neutral `herdr-core` crate remains the six-function ABI owner, and the obsolete root Rust AppKit/wgpu paths and nine dependency families were removed after the verified `rust-native-final` tag.

## Exact current C ABI

```c
HerdrCore *herdr_core_create(const uint8_t *options_json, size_t len);
void herdr_core_dispatch(HerdrCore *core, const uint8_t *event_json, size_t len);
HerdrBytes herdr_core_snapshot(HerdrCore *core);
void herdr_core_on_change(HerdrCore *core, HerdrChangeCallback callback, void *context);
void herdr_core_free_bytes(HerdrBytes bytes);
void herdr_core_destroy(HerdrCore *core);
```

`HerdrBytes` is `{ uint8_t *ptr; size_t len; size_t cap; }` and `HerdrChangeCallback` is `void (*)(void *context)`.
Rust owns `HerdrCore` and every returned buffer, and the caller returns buffers with `herdr_core_free_bytes`.
The callback carries only the opaque context and may run from a worker thread; Swift must hop to the main thread and pull a snapshot.
Malformed events, unknown kinds, schema mismatch, invalid payloads, and wrong-thread dispatch become explicit errors where a live core exists.

Off-owner snapshot returns empty bytes after recording and notifying `ffi.wrong_thread`.
Off-owner destroy records and notifies the same error without freeing the handle, so destruction remains owned by the creating thread.

## Options schema

| Field | Exact committed type | Rule |
| --- | --- | --- |
| `schema_version` | `u32` | Required and exactly `1` |
| `herdr_socket_path` | `string | null` | Optional; a present string must be non-empty |
| `herdr_bin_path` | `string | null` | Optional with serde default; a present string must be non-empty |
| `remote_targets` | `RemoteTarget[]` | Required array; IDs must be unique |
| `app_state_path` | `string` | Required and non-empty |

`RemoteTarget` is `{ id: string, label: string, ssh_alias: string }`, with every field non-empty.

## Event envelope and payload schema

Every event is `{ schema_version: u32, kind: string, payload: object }` with schema version `1`.

| `kind` | Exact payload fields |
| --- | --- |
| `key` | `{ pane_id: string, bytes_base64: string }` |
| `terminal_output` | `{ pane_id: string, bytes_base64: string }` |
| `session_snapshot` | `{ focused_pane_id: string | null, layouts: SessionLayout[], agents: SessionAgent[] }`; each layout includes workspace/tab/focus/zoom, area, pane rects, and split direction/ratio rects, and each agent includes optional identity/status fields plus `tokens: object` |
| `click` | `{ surface: sidebar | terminal | workbench | pet, x: f64, y: f64, button: left | right, click_count: u8 }` |
| `focus_pane` | `{ pane_id: string }` |
| `open_browser` | `{ profile: string }` |
| `browser_status` | `{ state: string, profile: string, current_url: string | null, current_title: string | null, message: string | null, last_checked_at_unix_ms: u64 }` |
| `create_workspace` | `{ path: string, label: string, create_worktree: bool }` |
| `create_tab` | `{ workspace_id: string, label: string }` |
| `create_pane` | `{ tab_id: string, cwd: string, command: string | null, direction: right | down }` |
| `toggle_zoom` | `{ pane_id: string }` |
| `close_workspace` | `{ workspace_id: string, confirmed: bool }` |
| `close_tab` | `{ tab_id: string, confirmed: bool }` |
| `close_pane` | `{ pane_id: string, confirmed: bool }` |
| `file_open` | `{ path: string }` |
| `file_draft` | `{ contents_utf8: string }` |
| `file_save` | `{ path: string, contents_utf8: string, expected_modified_at_unix_ms: u64 | null }` |
| `file_conflict` | `{ action: reload | keep_editing }` |
| `ui_state_update` | `{ expanded_paths: string[], selected_path: string | null, selected_pane_id: string | null, shortcut_bindings: Map<string, string> }` |
| `retry_connect` | `{ target_id: string }` |
| `terminal_resize` | `{ pane_id: string, cols: u16, rows: u16 }`, both dimensions positive |

Unknown `kind` becomes `status.last_error.kind = "event.unknown_kind"`.
A malformed payload becomes `event.invalid_payload`, malformed JSON becomes `event.invalid_json`, and a version mismatch becomes `schema_version.mismatch`.

## Full snapshot schema

The top-level snapshot has exactly these 14 fields:

```text
Snapshot {
  schema_version: u32,
  navigator: {
    root_path: string | null,
    focused_workspace_id: string | null,
    workspaces: [{ id: string, label: string, path: string,
                   remote_target_id: string | null, expanded: bool }],
    agents: [{ id: string, pane_id: string, workspace_label: string,
               agent_kind: string, state: string, symbol: string,
               summary: string, elapsed: string, sort_rank: string,
               activity: string }]
  },
  overlay: {
    kind: string | null, title: string | null, message: string | null,
    actions: [{ id: string, label: string, destructive: bool }]
  },
  tab: {
    id: string | null, workspace_id: string | null, label: string | null,
    panes: [{ id: string, label: string, cwd: string, state: string,
              summary: string | null, activity_at_unix_ms: u64 | null }]
  },
  connection: { kind: string, state: string, target_id: string | null },
  zoomed: string | null,
  focused: { surface: sidebar | terminal | workbench | pet,
             pane_id: string | null },
  pane_layout: {
    workspace_id: string, tab_id: string,
    focused_pane_id: string, zoomed: bool,
    root: PaneLayoutNode
  } | null,
  terminal: {
    pane_id: string | null, sequence: u64,
    chunks: [{ pane_id: string, sequence: u64, bytes_base64: string }],
    closed: bool, exit_code: i32 | null,
    panes: [{ pane_id: string, closed: bool, exit_code: i32 | null }]
  },
  editor: {
    path: string | null, language: string | null,
    contents_utf8: string | null,
    opened_modified_at_unix_ms: u64 | null,
    dirty: bool, readonly_reason: string | null,
    conflict: { disk_modified_at_unix_ms: u64,
                opened_modified_at_unix_ms: u64 } | null,
    diff: { added_lines: u32[], removed_lines: u32[] } | null
  },
  ui_state: {
    expanded_paths: string[], selected_path: string | null,
    selected_pane_id: string | null,
    shortcut_bindings: Map<string, string>
  },
  ime: {
    marked_text: string,
    selected_range: { location: u64, length: u64 },
    replacement_range: { location: u64, length: u64 } | null
  },
  input_generation: u64,
  status: {
    herdr: { state: string, socket_path: string | null,
             message: string | null, last_checked_at_unix_ms: u64 | null },
    remote: [{ target_id: string, state: string, message: string | null,
               last_checked_at_unix_ms: u64 | null }],
    chromux: { state: string, profile: string,
               current_url: string | null, current_title: string | null,
               message: string | null, last_checked_at_unix_ms: u64 | null },
    environment: [{ key: string, required: bool, format: string,
                    state: string, absent_behavior: string, message: string }],
    diagnostics: [{ kind: string, message: string, occurred_at: u64 }],
    last_error: { kind: string, message: string, retryable: bool,
                  occurred_at: u64 } | null
  }
}
```

`PaneLayoutNode` is either `{ type: pane, pane_id }` or `{ type: split, direction: right | down, ratio: f32, first: PaneLayoutNode, second: PaneLayoutNode }`.
The authoritative `focused_pane_id` is the only focus-accent and input-routing source.
All pane leaves keep retained nonzero frames and live terminal feeds during zoom; only the focused pane receives a full-canvas visual frame.

This resolves OPEN-2 from the implemented source rather than from the earlier spike prose.

## OPEN-1 and the IME gate

OPEN-1 is resolved to **human ownership**.
The earlier real-input Stage-0 result passed English immediate echo, candidate-window tracking, and adjacent-character preservation, but Korean composition Backspace failed twice.
The user then deferred the defect with: "지금 원인은 모르겠는데 백스페이스 계속 안된다. 우선 이거 패스하고 다른거 작업부터 쭉 하게 하자".

Commit `d290928` added machine-tested composition-byte suppression and live pane attach.
Commit `ecc46fb` then re-anchored marked text after pane echo, and `b2582ad` hid the block caret during active composition.
The intermediate ecc46fb human session reported the main behavior as roughly working but identified the visible block caret as a remaining defect.
The final `b2582ad` physical-keyboard terminal verdict is exactly `1,2,3,4 모두 잘 맞아 다 된다`.
These source changes did not replace the human V9 owner; the user supplied the terminal-axis PASS.
The user-owned p3 work is recorded in [pane-attach-ime-verification.json](evidence/pane-attach-ime-verification.json).
The editor axis remains deferred by prior user agreement.
Under that explicit scope resolution, T3 and AC1-AC3 are satisfied, V9 passes for the terminal axis, and T1-T4 form a fully passed spike gate.

## T1 through T19

| Task | Status | Current result |
| --- | --- | --- |
| T1 | PASS | Stage-0 six-function ABI, callbacks, buffers, and concrete schema were proven in the spike. |
| T2 | PASS | Stage-0 SSH/PTY to SwiftTerm byte round trip passed. |
| T3 | PASS | The physical-keyboard terminal verdict is `1,2,3,4 모두 잘 맞아 다 된다`; the editor axis remains deferred by prior agreement. |
| T4 | PASS | SwiftPM, Rust staticlib, app assembly, arm64, macOS 14, and ad-hoc signing passed. |
| T5 | PASS | Annotated pre-deletion tag verified; approved Rust-native shell files and nine obsolete dependency families removed; V21/build/native evidence passes. |
| T6 | PASS | The three-key Rust registry is enumerable, value-safe, tested, and the only raw environment-read boundary. |
| T7 | PASS | The exact six-function ABI, status schema, buffers, callbacks, and owner-thread snapshot/destroy enforcement pass focused tests. |
| T8 | PASS | Signed native three-column shell, authoritative recursive pane grid, native menus, and V8 11-pane runtime evidence pass. |
| T9 | PASS | Live SwiftTerm attach and byte routing are machine/runtime proven, and the user passed all four terminal-axis V9 checks; editor axis remains deferred. |
| T10 | PASS | Seven-state two-line sidebar and authoritative ordering are implemented and evidenced. |
| T11 | PARTIAL | Existing-file browse/render/edit paths exist; the complete save-failure/conflict browser-runtime matrix is not proven. |
| T12 | PASS | Approved default launch/reuse/focus/status and safe absent/stale/PATH branches are evidenced without profile mutation. |
| T13 | PARTIAL | Mini positive display/attach/split/browse evidence exists; full in-app flow and disconnect recovery do not. |
| T14 | PARTIAL | Persistence and shortcut-binding contracts pass in process; full native missing/corrupt/valid relaunch and legacy-sentinel row remains incomplete. |
| T15 | PARTIAL | Focused-pane close now executes through the core with idle immediate and working/attention confirmation policy; complete workspace/tab/worktree result evidence remains open. |
| T16 | PARTIAL | Several visible states exist, but V29, V30, V33, and V34 are incomplete. |
| T17 | PASS | Two unchanged-source T5 app assemblies produced identical bundle and executable hashes and passed strict deep codesign. |
| T18 | PASS | Selective pet hit region, click-through corners, focus behavior, lifecycle, and offscreen recovery are evidenced. |
| T19 | PASS | Prefix-owned plan/create/status/cleanup exists; the current retained fixture is exactly one workspace and one pane. |

## Requirement coverage

| Requirement | Status | Evidence and remaining gap |
| --- | --- | --- |
| R1 | PASS | [t8-swiftui-shell-runtime.json](evidence/t8-swiftui-shell-runtime.json) and signed native screenshots. |
| R2 | PASS | Sidebar unit tests plus [additive-t9-t14-ui.png](evidence/additive-t9-t14-ui.png). |
| R3 | PASS | Live attach and byte policy pass, and all four physical-keyboard terminal checks pass; the editor axis is deferred by prior agreement. |
| R4 | PARTIAL | Workbench surfaces exist; complete V4/AC15 native failure and conflict proof is missing. |
| R5 | PASS | [t12-chromux-lifecycle.json](evidence/t12-chromux-lifecycle.json), safe failure receipts, and decoded-title evidence. |
| R6 | PASS | Six functions and error tests pass; off-owner snapshot/destroy are rejected with observable `ffi.wrong_thread` state. |
| R6a | PASS | Exact options types are fixed in `model.rs` and tested. |
| R6b | PASS | Exact event kinds and payloads are fixed in `runtime.rs` and tested. |
| R6c | PASS | Full 13-field snapshot and nested status schema are fixed in `model.rs`. |
| R6d | PASS | Unknown kind, schema mismatch, malformed payload, invalid options, and off-owner dispatch tests pass in process. |
| R7 | PARTIAL | Mini positive path passes; disconnect/reconnect and full in-app navigation remain open. |
| R8 | PARTIAL | In-process persistence passes; V17/V28 native relaunch proof is incomplete. |
| R9 | PASS | Two current T5-source assemblies converged; arm64, minimum macOS 14.0, and strict deep codesign pass. |
| R10 | PASS | Missing summary fallback and OPENROUTER nonaccess receipts pass. |
| R11 | PASS | One Rust registry declares all three keys; Swift consumes only snapshot state and raw values never enter errors or logs. |
| R12 | PARTIAL | Consequences and several state cards exist; V29/V30/V33/V34 remain incomplete. |

## Acceptance criteria coverage

| AC | Status | Evidence and remaining gap |
| --- | --- | --- |
| AC1 | PASS | On `b2582ad`, the user confirmed the Korean candidate window follows the terminal cursor; the editor axis is deferred by prior agreement. |
| AC2 | PASS | On `b2582ad`, the user confirmed Backspace deletes the Korean composition without leaking DEL. |
| AC3 | PASS | On `b2582ad`, the user confirmed multi-character composition preserves adjacent characters; the editor axis is deferred by prior agreement. |
| AC4 | PASS | The owned fixture focus receipt and screenshot show a click request followed by Herdr's authoritative focused-pane transition before the system-accent highlight moved. |
| AC5 | PASS | [v6-path-hidden.png](evidence/v6-path-hidden.png) and receipt. |
| AC6 | PASS | Default chromux reuse retained PID 9609 without a second instance. |
| AC7 | NOT RUN | Mini disconnect/reconnect injection was not performed. |
| AC8 | PASS | The 11-pane grid fixture measured app RSS 120976 KB, below 400 MB; app plus eleven attach children totaled 220752 KB. |
| AC9 | BLOCKED | Final six-axis V10 human review is missing. |
| AC10 | PASS | In-process unknown-kind snapshot test passes. |
| AC11 | PARTIAL | Core registry behavior passes; complete owned native missing-socket evidence and registry-only reads do not. |
| AC12 | PASS | Current app is signed arm64 with minimum macOS 14.0 and runs. |
| AC13 | PARTIAL | Historical repeat assembly passed, but current `b2582ad` was assembled once in this readiness run. |
| AC14 | PASS | [v7-working-pane-warning.png](evidence/v7-working-pane-warning.png) states the process consequence before confirmation. |
| AC15 | PARTIAL | Draft retention exists in process; full native save-failure before/after evidence is missing. |

## V1 through V35 coverage

| V | Status | Evidence and remaining gap |
| --- | --- | --- |
| V1 | PASS | Current `cargo test -p herdr-core`: 25 unit and 19 FFI tests, including required error paths, owner-thread behavior, asynchronous lifecycle, Close Pane policy, and authoritative multi-pane routing. |
| V2 | PASS | Stage-0 byte round trip and later [live-input-proof.png](evidence/live-input-proof.png). |
| V3 | PARTIAL | Live polling/attach exists; full focus transition and both absence modes are not one complete runtime receipt. |
| V4 | PARTIAL | File contracts and surfaces exist; all render/save/conflict branches are not directly evidenced. |
| V5 | PASS | Default chromux lifecycle and decoded title are evidenced. |
| V6 | PASS | PATH-hidden owned-app unavailable state is evidenced. |
| V7 | PARTIAL | Warning is native-evidenced, but the actual close result executor is incomplete. |
| V8 | PASS | [pane-grid-v8-diagnostic.json](evidence/pane-grid-v8-diagnostic.json) and [grid-v8-11-pane.png](evidence/grid-v8-11-pane.png): app RSS 120976 KB with eleven simultaneous panes. |
| V9 | PASS | Human terminal verdict: `1,2,3,4 모두 잘 맞아 다 된다`; editor axis deferred by prior agreement. |
| V10 | 미실행-pending | Final six-axis visual-quality review was not executed as a complete V10 row. |
| V11 | PASS | Two unchanged-source assemblies have identical bundle and executable hashes; launch and strict signing pass. |
| V12 | PASS | Wrong unused loopback port produced stale plus last-checked without daemon disruption. |
| V13 | PASS | `herdr-ide-verify-absent` guidance passed without profile creation. |
| V14 | PARTIAL | Core absence contract passes; full native no-secret screen evidence is incomplete. |
| V15 | PARTIAL | Protocol mismatch is explicit in process; complete native runtime evidence is missing. |
| V16 | PARTIAL | Mini positive path passes; not every action was driven through the final app. |
| V17 | PARTIAL | In-process missing/corrupt fallback passes; native log/status proof is incomplete. |
| V18 | 미실행-pending | Live Herdr server-down and recovery injection was not performed. |
| V19 | 미실행-pending | Mini disconnect/reconnect injection was not performed. |
| V20 | PASS | Missing summary fallback and nonaccess receipt pass. |
| V21 | PASS | T5 dependency reachability is empty, the six-function ABI is intact, owner-thread tests pass, and Swift has no registry bypass. |
| V22 | 미실행-pending | Finder-launched remote authentication path was not completed. |
| V23 | PASS | Stage-0 actual linked-app 100-callback evidence is retained in [runtime.json](evidence/runtime.json). |
| V24 | PASS | Selective hit-region receipt and real pet screenshot pass. |
| V25 | PASS | Static and owned-process OPENROUTER sentinel nonaccess evidence passes. |
| V26 | PASS | Seven states, two-line layout, and ordering are evidenced. |
| V27 | PARTIAL | Rust diff projection is tested; final signed-app known-diff screenshot is incomplete. |
| V28 | PARTIAL | Core round trip and no production `navigator.json` read pass; full native restoration is missing. |
| V29 | PARTIAL | Some empty states are visible; all four reason-and-next-action states are not directly evidenced. |
| V30 | PARTIAL | Binary/read reasons exist; real PTY exit-code surface is not directly evidenced. |
| V31 | PARTIAL | Aggregate warning is shown; actual workspace close result is incomplete. |
| V32 | PASS | All three keys have tested present/absent or default contracts, value-free status, and a zero-result Swift raw-access scan. |
| V33 | PARTIAL | Some browser/mini loading states exist; all five loading surfaces are not directly evidenced. |
| V34 | 미실행-pending | Focused-pane path versus tree-root divergence was not demonstrated. |
| V35 | PARTIAL | Tab/worktree/attention warnings exist; final execution results are incomplete. |

## Verification performed

### Build and static

- The current pane-grid boundary passed 25 `herdr-core` unit tests, 19 FFI integration tests, denied-warning core clippy, Rust formatting, and 32 Swift tests.
- `/bin/bash macos/scripts/build_dev_app.sh` built the release static library, built the Swift package, assembled the app, and passed strict deep codesign from the exact feature source.
- The current executable SHA-256 is `4d6869e96f8c5825bc9f21f2fc90facd2f16209ec6302b24d055146a9cb9afe2` and the `Info.plist` SHA-256 is `ad91f1cc110ad7f5f7646d518182474bda5aec78a5bc9516b75b3b147e314bc4`.
- The executable is a thin `arm64` Mach-O with minimum macOS `14.0` and strict deep codesign passed.
- The first bundle hash attempt inherited an unavailable `C.UTF-8` locale and failed before producing a claim; repeating with process-local `LC_ALL=C` succeeded without changing the user environment.

### Real native runtime

- Peekaboo reported Screen Recording and Accessibility granted.
- The d290928 readiness capture correctly records 85 fixture-bootstrap output bytes, zero automated input, and an unobservable `input_generation`; p3 later terminated that app and superseded it.
- The ecc46fb captures are also retained, including one capture that visibly contains later human typing and a clean r2 capture; both are superseded by the block-caret finding fixed in `b2582ad`.
- For the current run, T19 created `herdr-ide-verify-v9-b2582ad-w01` as workspace `w3B` with pane `w3B:p1` under `/tmp/herdr-ide-verify-v9-b2582ad`.
- The exact fixture-owned `/usr/bin/tail -f /dev/null` PID 58850 was terminated externally after foreground and cwd validation, without sending Ctrl-C or bytes to the pane, leaving zsh PID 58253 in the foreground.
- The zsh-ready pane starts with 172 visible bytes, SHA-256 `d42f5c54b19b152db3dd457e4e11644121a77811cb5bc0d77883e7abaa54c396`; prelaunch and post-capture values are identical and contain no probe or sentinel.
- The human-accepted V9 app was PID `62007`, with one key, on-screen, frontmost window `2582` titled `Herdr IDE`; after the verdict was durably recorded, that exact app and attach child were terminated.
- Exactly one child, PID `62025`, runs `/Users/hoyeonlee/.local/bin/herdr pane attach w3B:p1`.
- The terminal header and accessibility snapshot identify `w3B:p1` through `terminal-panel`.
- `automated_input_count` is zero and this Implementor sent no probe, sentinel, synthetic key, input-source switch, click, or typed byte.
- The core field `input_generation` is not exposed through the committed Swift model, AX tree, persisted state, or a read-only runtime endpoint, so this run records it as unobservable rather than inventing zero.
- The concrete `ImeTerminalView` first-responder identity is not publicly exposed; only the focused terminal panel and pane label are observable read-only.

The current screenshot and fresh Peekaboo observations are [v9-b2582ad-human-retest-ready.png](evidence/v9-b2582ad-human-retest-ready.png), [apps JSON](evidence/v9-b2582ad-human-retest-ready-apps.json), [windows JSON](evidence/v9-b2582ad-human-retest-ready-windows.json), [AX JSON](evidence/v9-b2582ad-human-retest-ready-see.json), and [readiness receipt](evidence/v9-b2582ad-human-retest-ready.json).

The later prefix-owned shortcut fixture `herdr-ide-verify-shortcuts-20260829` started as workspace `w3C` with one pane.
Native menu dispatch created a right pane and a down pane, then toggled `w3C:p1` zoom through the core event path.
Peekaboo menu inventory exposes the three shortcut equivalents and the signed app rendered `w3C:p1 · zoomed` in [pane-shortcuts-native.png](evidence/pane-shortcuts-native.png).
The one raw `Command-D` attempt returned indeterminate and is not acceptance evidence; a later fourth owned fixture pane is recorded without assigning its cause.
The bounded receipt is [pane-shortcuts-runtime.json](evidence/pane-shortcuts-runtime.json).

The later sidebar freeze diagnosis reproduced the user's real pause and sampled owner-thread destruction blocking in `portable_pty::Child.wait` and `wait4`.
Pane attach and pane-control work now runs asynchronously, while the FFI owner remains the main actor and requested, ready, failed, and authoritative-refresh outcomes stay observable.
The recursive Herdr layout tree is the sole grid authority; Swift does not invent pane topology.
Each current-tab pane owns a stable terminal view and attach, only the authoritative focused pane receives the system-accent border and keyboard routing, and hover has no focus state or visual response.
The 11-pane T19 fixture rendered eleven concurrent panes and eleven attaches at 120976 KB app RSS and 220752 KB including attach children, within the 400 MB app budget.
See [pane-grid-v8-diagnostic.json](evidence/pane-grid-v8-diagnostic.json) and [grid-v8-11-pane.png](evidence/grid-v8-11-pane.png).

The first zoom implementation removed hidden terminal views from the SwiftUI hierarchy.
Because the core attach workers continued draining output, unzoom recreated empty SwiftTerm buffers that showed only future fragments.
The fix retains every terminal view, attach, feed, and nonzero authoritative frame through zoom; only opacity, hit testing, accessibility visibility, z-order, and the focused pane's visual frame change.
SwiftTerm frame changes continue through `sizeChanged` to the `terminal_resize` event and the matching PTY resize, with `terminal.resize_failed` exposed on error.
The owned three-pane continuous-output sequence proves all three buffers advance across before, zoomed, and restored screenshots without attach PID replacement.
See [zoom-buffer-retention-diagnostic.json](evidence/zoom-buffer-retention-diagnostic.json), [before](evidence/zoom-buffer-retention-before.png), [zoomed](evidence/zoom-buffer-retention-on.png), and [restored](evidence/zoom-buffer-retention-after.png).

The standard Settings scene renders its Keyboard-only pane-binding form in [pane-actions-settings-diagnostic-r2-settings-latest.png](evidence/pane-actions-settings-diagnostic-r2-settings-latest.png).
Default, missing, corrupt, invalid, duplicate, reserved, persistence, and dynamic command-routing seams pass focused tests.
The exact-window TextField driver did not change the visible value and returned indeterminate receipts, so it is not recorded as successful native rebind evidence and was not retried.
Actual Settings edit, immediate Pane-menu refresh, rebound action routing, and relaunch persistence remain on the final human checklist, as recorded in [pane-actions-settings-driver-limitation.json](evidence/pane-actions-settings-driver-limitation.json).

### External and remote

- Approved positive mini evidence exists in [t13-t19-mini-positive-runtime.json](evidence/t13-t19-mini-positive-runtime.json).
- No mini connection was cut and V19 was not run.
- Default chromux Chrome PID 9609 was launched/reused in the approved earlier batch and remains running.
- No Chrome, chromux daemon, Herdr daemon, or live socket was stopped or disrupted.
- Before the fixture-retarget regression, stale/global focus redirected a diagnostic action to user-owned w2Y and caused at least one unintended pane split; the exact count is unproven, input stopped immediately, and no out-of-ownership cleanup was attempted.
- After that regression, w2Y received no further focus, split, zoom, cleanup, automated input, or mutation, and its later zoom screenshot was inspected read-only.
- The final `w3B` fixture remains historical evidence without an app attach; historical prefix-owned w38, w39, and w3A fixtures also remain preserved.

## Automated tests and regression risks

| Test group | Current count | Regression guarded |
| --- | ---: | --- |
| `herdr-core` unit | 25 | Token projection, three-key environment registry, persistence including shortcuts, fixtures, authoritative layouts, direct-socket focus, and shell-free pane command planning |
| `herdr-core` FFI | 19 | Six ABI calls, buffers/callbacks, schema/error paths, owner-thread rejection, authoritative multi-pane state, async control latency, nonblocking attach destruction, Close Pane policy, and routing |
| Swift package | 32 | IME byte policy, CDP title decoding, PATH-hidden preflight, pet hit geometry, consequence previews, pane operation status, shortcut defaults/validation/persistence/routing, authoritative focus, and zoom view retention |
| Stage-0 spike Rust | 5 retained | ABI callback and buffer boundary |
| Stage-0 composition proxy | Diagnostic only | Coordinate and lifecycle reasoning; never V9 acceptance |

Native screenshots are used for real rendering, process, window, attach, and visibility claims because code-only assertions cannot prove a macOS surface.
No low-maintenance automated test can replace the physical Korean IME verdict.

## Environment registry

| Key | Required | Format | Absence behavior |
| --- | --- | --- | --- |
| `SSH_AUTH_SOCK` | No | Absolute Unix-domain socket path | Disable remote features with a visible reason; keep local features available |
| `PATH` | No | Colon-separated executable search path containing the chromux install directory | Disable chromux actions with visible guidance |
| `HERDR_SOCKET_PATH` | No | Absolute Unix-domain socket path | Use the configured default local Herdr socket path |

Values remain external and the Rust registry never puts them in status messages.
The macOS source has no direct `ProcessInfo.processInfo.environment` access; browser and remote models consume registry state, while Rust applies the socket override at core creation.

## Deviations and pipeline record

- The user initially deferred the unresolved T3 defect with the exact earlier quote and explicitly directed downstream additive work before the T3/T5 dependency gate completed.
- The later authoritative verdict `1,2,3,4 모두 잘 맞아 다 된다` supersedes that terminal-axis defect state and satisfies AC1-AC3, T3, and the Stage 0 gate; the editor axis remains deferred by prior agreement.
- The legacy v5 record tree was moved, not deleted, from `agents/runs/swift-shell-pivot` to `agents/runs/swift-shell-pivot-v5-archive`.
- Archive integrity was 13 files and 279437 bytes with identical per-file SHA-256 before and after; the move is reversible.
- The canonical PRD AC table was mechanically converted to v6 columns without changing any of the 15 criterion strings.
- Conversion evidence is [v6-prd-ac-conversion.json](evidence/v6-prd-ac-conversion.json) and [v6-prd-ac-conversion.diff](evidence/v6-prd-ac-conversion.diff).
- v6 gap-audit remained `BLOCK` after two closure attempts, v6 spec remained `BLOCK` after one closure attempt, and the one authorized v6 implement start refused because those gates were not PASS.
- No later Sasu run, override, gate rerun, start workaround, or finalize was attempted.
- The earlier V17 screenshot from PID 33580 was captured while a concurrent app PID existed and is explicitly invalidated as acceptance evidence.
- The CodeEditSourceEditor path was replaced by the PRD RISK-3 pre-authorized independent `NSTextView` plus Highlightr fallback after the package plugin could not load `sourcekitdInProc`.

## Deletion, tags, failure injection, and delivery

- T5: approved deletion and dependency removal performed after the tag precondition passed.
- `rust-native-final`: annotated tag object `9f388c3378dcba3f23b07ba24f76b6b092d7bda7`, peeled commit `b2582ade493bc6569af0e9719aa672751962df96`.
- Deleted files: `src/render.rs`, `src/terminal.rs`, `src/accessibility.rs`, `src/browser.rs`, and `src/openrouter.rs`.
- Retired AppKit/wgpu content was removed from `src/app.rs`; only its historical module note remains.
- Removed dependency families: `wgpu`, `glyphon`, `bytemuck`, `raw-window-handle`, `objc2`, `objc2-app-kit`, `objc2-foundation`, `alacritty_terminal`, and `security-framework`.
- No spike copy, evidence, user file, or non-T5 source was deleted.
- External live-service failure injection: not performed.
- Allowed in-process unknown-kind, schema-mismatch, malformed-payload, invalid-options, and off-owner tests: performed and passing.
- Non-invasive wrong unused loopback port check: performed without stopping a live daemon.
- Prefix-owned historical w38, w39, w3A, and w3B fixtures remain preserved; no app attach is retained.
- Delivery mode: local.
- T5 deletion commit: `99c4aa85e8b7b0c388c1848967690584a5e40754`.
- Push/PR: not performed.

## Remaining human review and follow-ups

1. The four-item V9 terminal checklist is complete with `1,2,3,4 모두 잘 맞아 다 된다`; the editor axis remains deferred by prior agreement.
2. V10 needs the final six-axis human review: three-column structure/density, typography/Hangul, state expression, dark mode, resize behavior, and native conventions.
3. Rows not covered by retained evidence or legitimate manual acceptance remain `미실행-pending`, especially V18 and V19 live-service failure injection.
4. Expose a read-only live `input_generation` counter if zero-input readiness must be machine-proven in future sessions.

## T5 verification result

The structured receipt is [t5-native-removal-verification.json](evidence/t5-native-removal-verification.json), and the real native screenshot is [t5-rust-native-deletion-native.png](evidence/t5-rust-native-deletion-native.png).
`cargo check --workspace --all-targets`, formatting, focused denied-warning clippy, ordinary workspace clippy, all workspace tests, 17 Swift tests, two deterministic app assemblies, strict deep codesign, arm64 inspection, minimum macOS 14.0 inspection, and the single-instance native capture passed.
The workspace test total was 106 passed, zero failed, and one pre-existing ignored live-fixture test.
The initial workspace-wide denied-warning clippy attempt found nine retained legacy warnings outside T5; this is recorded as a failure rather than relabeled, while `herdr-core` passes denied-warning clippy.
The repeated bundle manifest SHA-256 is `2935021416052171287be2aed2969633cbbe6d716844291425501eb853adcfc8`, and the executable SHA-256 is `5d0027358d112b0294c758137e2f296b5421a5c8b5f84945ef94074a82c57964`.
The native screenshot SHA-256 is `d9dc6455a0393f18f3754a2b3c01484c26d15284f65c733139f1fcc52fd82dcf`.
The exact owned app PID 38574 and its attach child were terminated after capture.

The user directed `검증은 내가 어느정도 다 했으니 마무리하는 식으로 얼른 가자.`
This report therefore distinguishes retained machine evidence, legitimate `사용자 수동 검증으로 수용됨` behavior, and `미실행-pending` rows without manufacturing execution or a Sasu completion receipt.

## R11 and R6 contract verification

The enumerable environment registry now contains `SSH_AUTH_SOCK`, `PATH`, and `HERDR_SOCKET_PATH` with optionality, format, validation state, and absence behavior.
Swift no longer reads process environment values directly.
Browser PATH and remote SSH availability come from `status.environment`, while the Rust creation boundary applies a valid Herdr socket override without serializing it.
The exact six C ABI signatures remain unchanged.
Off-owner snapshot returns empty bytes after recording and notifying `ffi.wrong_thread`; off-owner destroy records and notifies the same error without freeing the handle, leaving final destruction to the owner thread.

`cargo test -p herdr-core` passed 21 unit and 13 FFI tests, `cargo clippy -p herdr-core --all-targets -- -D warnings` passed, formatting passed, and 18 Swift tests passed.
The focused PATH-hidden test proves the app stops before process launch when the registry reports PATH absent.
The structured receipt is [r6-r11-contract-verification.json](evidence/r6-r11-contract-verification.json).

## Principles applied

- Engineering 4, 9, and 10: build, capture, attach, schema, and ownership failures are explicit in bounded logs and the readiness receipt rather than being collapsed into success.
- Engineering 5: this build/fixture/report batch did not edit p3-owned terminal-input source and keeps core, live attach, UI, fixture, and evidence boundaries separate.
- Engineering 11: exact app count, fixture name, manifest ownership, one attach child, repeat status, and unchanged bootstrap hash make repeated preparation converge.
- Engineering 12 and the outcome-focused test practice: stable logic is covered in process, while native rendering and the V9 readiness claim use a real signed app and fresh screenshot.
- Design 3, 5, and 7: the frequent split and zoom actions use the native Pane menu, iTerm2-compatible shortcuts, and snapshot-derived visible zoom state.
- Design 4 and 7: the shell derives connection, pane, browser, mini, and agent state and represents it structurally instead of asking the user to calculate it.
- Design 6: no destructive action was taken; existing preview surfaces state process, workspace, tab, and checkout consequences before confirmation.
- The environment practice keeps the declared `SSH_AUTH_SOCK` contract in Rust code and values external, while this report calls out the remaining Swift bypass instead of hiding it.

This implementation is not Done and has no receipt-backed Sasu completion claim.
