# Stage 0 FFI schema

This file resolves OPEN-2 for the spike and records the implemented production extension.
The production source of truth is `herdr-core/include/herdr_core.h` plus the serializable types in `herdr-core/src/model.rs` and `herdr-core/src/runtime.rs`.
All JSON uses UTF-8 and `schema_version` is currently `1`.

## C ABI

```c
HerdrCore *herdr_core_create(const uint8_t *options_json, size_t len);
void herdr_core_dispatch(HerdrCore *core, const uint8_t *event_json, size_t len);
HerdrBytes herdr_core_snapshot(HerdrCore *core);
void herdr_core_on_change(HerdrCore *core, HerdrChangeCallback callback, void *context);
void herdr_core_free_bytes(HerdrBytes bytes);
void herdr_core_destroy(HerdrCore *core);
```

`HerdrBytes` is `{ ptr: uint8_t*, len: size_t, cap: size_t }`.
Rust owns `HerdrCore` and every returned buffer.
Swift unregisters the callback before destroy and returns every snapshot buffer with `herdr_core_free_bytes`.
The callback carries only the opaque context and may run on any thread.
Swift hops to the main thread and then pulls a fresh snapshot.
Rust contains panics at the FFI boundary and exposes dispatch failures through `status.last_error`.
Create, dispatch, snapshot, callback registration, and destroy belong to the thread that created the core; the Swift bridge creates and calls them on its main actor.
An off-owner snapshot returns an empty buffer after recording `ffi.wrong_thread`, and an off-owner destroy records the same error without freeing the core so the owner can observe and destroy it safely.

## Options

```text
CoreOptions {
  schema_version: u32,
  herdr_socket_path: String?,
  herdr_bin_path: String?,
  remote_targets: RemoteTarget[],
  app_state_path: String
}

RemoteTarget {
  id: String,
  label: String,
  ssh_alias: String
}
```

## Event envelope and payloads

Every event is `{ schema_version: u32, kind: String, payload: Object }`.

```text
key              { pane_id: String, bytes_base64: String }
terminal_output  { pane_id: String, bytes_base64: String }
terminal_resize  { pane_id: String, cols: u16, rows: u16 }
session_snapshot { focused_pane_id: String?, layouts: SessionLayout[], agents: SessionAgent[] }
click            { surface: Surface, x: f64, y: f64, button: MouseButton, click_count: u8 }
focus_pane       { pane_id: String }
open_browser     { profile: String }
browser_status   { state: String, profile: String, current_url: String?, current_title: String?, message: String?, last_checked_at_unix_ms: u64 }
create_workspace { path: String, label: String, create_worktree: bool }
create_tab       { workspace_id: String, label: String }
create_pane      { tab_id: String, cwd: String, command: String?, direction: PaneSplitDirection }
toggle_zoom      { pane_id: String }
close_workspace  { workspace_id: String, confirmed: bool }
close_tab        { tab_id: String, confirmed: bool }
close_pane       { pane_id: String, confirmed: bool }
file_open        { path: String }
file_draft       { contents_utf8: String }
file_save        { path: String, contents_utf8: String, expected_modified_at_unix_ms: u64? }
file_conflict    { action: "reload" | "keep_editing" }
ui_state_update  { expanded_paths: String[], selected_path: String?, selected_pane_id: String?, shortcut_bindings: Map<String, String> }
retry_connect    { target_id: String }

Surface = sidebar | terminal | workbench | pet
MouseButton = left | right
PaneSplitDirection = right | down

SessionLayout {
  workspace_id: String,
  tab_id: String,
  zoomed: bool,
  area: Rect,
  focused_pane_id: String,
  panes: { pane_id: String, rect: Rect }[],
  splits: { direction: PaneLayoutDirection, ratio: f32, rect: Rect }[]
}

SessionAgent carries the Herdr pane identity, workspace/cwd, agent status, and authoritative token map used by the sidebar projection.
Rect = { x: u16, y: u16, width: u16, height: u16 }.
```

An unknown `kind` produces `status.last_error.kind = "event.unknown_kind"`.
A mismatched event version produces `status.last_error.kind = "schema_version.mismatch"`.
Malformed event JSON and invalid payloads are also observable through `status.last_error`.

## Snapshot

```text
Snapshot {
  schema_version: u32,
  navigator: Navigator,
  overlay: Overlay,
  tab: Tab,
  connection: Connection,
  zoomed: String?,
  focused: Focused,
  pane_layout: PaneLayout?,
  terminal: Terminal,
  editor: Editor,
  ui_state: UiState,
  ime: Ime,
  input_generation: u64,
  status: Status
}

Navigator {
  root_path: String?,
  focused_workspace_id: String?,
  workspaces: Workspace[]
}

Workspace {
  id: String,
  label: String,
  path: String,
  remote_target_id: String?,
  expanded: bool
}

Overlay {
  kind: String?,
  title: String?,
  message: String?,
  actions: OverlayAction[]
}

OverlayAction { id: String, label: String, destructive: bool }

Tab {
  id: String?,
  workspace_id: String?,
  label: String?,
  panes: Pane[]
}

Pane {
  id: String,
  label: String,
  cwd: String,
  state: String,
  summary: String?,
  activity_at_unix_ms: u64?
}

Connection { kind: String, state: String, target_id: String? }
Focused { surface: Surface, pane_id: String? }

PaneLayout {
  workspace_id: String,
  tab_id: String,
  focused_pane_id: String,
  zoomed: bool,
  root: PaneLayoutNode
}

PaneLayoutNode =
  { type: "pane", pane_id: String }
  | {
      type: "split",
      direction: PaneLayoutDirection,
      ratio: f32,
      first: PaneLayoutNode,
      second: PaneLayoutNode
    }

PaneLayoutDirection = right | down

Terminal {
  pane_id: String?,
  sequence: u64,
  chunks: TerminalChunk[],
  closed: bool,
  exit_code: i32?,
  panes: TerminalPane[]
}

TerminalChunk { pane_id: String, sequence: u64, bytes_base64: String }

TerminalPane { pane_id: String, closed: bool, exit_code: i32? }

Editor {
  path: String?,
  language: String?,
  contents_utf8: String?,
  dirty: bool,
  readonly_reason: String?,
  conflict: EditorConflict?,
  diff: Diff?
}

EditorConflict {
  disk_modified_at_unix_ms: u64,
  opened_modified_at_unix_ms: u64
}

Diff { added_lines: u32[], removed_lines: u32[] }

UiState {
  expanded_paths: String[],
  selected_path: String?,
  selected_pane_id: String?,
  shortcut_bindings: Map<String, String>
}

Ime {
  marked_text: String,
  selected_range: TextRange,
  replacement_range: TextRange?
}

TextRange { location: u64, length: u64 }

Status {
  herdr: ProviderStatus,
  remote: RemoteStatus[],
  chromux: ChromuxStatus,
  environment: EnvironmentStatus[],
  diagnostics: Diagnostic[],
  last_error: LastError?
}

EnvironmentStatus {
  key: String,
  required: bool,
  format: String,
  state: String,
  absent_behavior: String,
  message: String
}

Diagnostic { kind: String, message: String, occurred_at: u64 }

ProviderStatus {
  state: String,
  socket_path: String?,
  message: String?,
  last_checked_at_unix_ms: u64?
}

RemoteStatus {
  target_id: String,
  state: String,
  message: String?,
  last_checked_at_unix_ms: u64?
}

ChromuxStatus {
  state: String,
  profile: String,
  current_url: String?,
  current_title: String?,
  message: String?,
  last_checked_at_unix_ms: u64?
}

LastError {
  kind: String,
  message: String,
  retryable: bool,
  occurred_at: u64
}

```

Timestamp fields are Unix milliseconds.
Terminal and key bytes are base64 so arbitrary PTY bytes survive JSON unchanged.
`pane_layout` is the authoritative recursive split tree from Herdr.
Swift renders all leaves simultaneously and uses only `focused_pane_id` as the focus accent and keyboard-routing source.
During zoom, every pane view retains its nonzero authoritative frame, attach process, and output feed; the focused pane alone receives a full-canvas visual frame.
SwiftTerm frame changes emit `terminal_resize`, which Herdr forwards to the matching attach PTY so unzoom triggers reflow and repaint without rebuilding terminal buffers.
`ui_state.shortcut_bindings` stores the four configurable Pane actions as canonical strings keyed by `split_right`, `split_down`, `toggle_zoom`, and `close_pane`.
The enumerable environment registry contains `SSH_AUTH_SOCK`, `PATH`, and `HERDR_SOCKET_PATH`.
Only validation state and absence behavior cross the ABI; raw values remain outside JSON, status messages, and logs.
