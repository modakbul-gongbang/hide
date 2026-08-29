# Stage 0 FFI schema

This file resolves OPEN-2 for the spike.
The executable source of truth is `include/herdr_core.h` plus the serializable types in `rust-core/src/lib.rs`.
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

## Options

```text
CoreOptions {
  schema_version: u32,
  herdr_socket_path: String?,
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
click            { surface: Surface, x: f64, y: f64, button: MouseButton, click_count: u8 }
focus_pane       { pane_id: String }
open_browser     { profile: String }
create_workspace { path: String, label: String, create_worktree: bool }
create_tab       { workspace_id: String, label: String }
create_pane      { tab_id: String, cwd: String, command: String? }
close_workspace  { workspace_id: String, confirmed: bool }
close_tab        { tab_id: String, confirmed: bool }
close_pane       { pane_id: String, confirmed: bool }
file_open        { path: String }
file_save        { path: String, contents_utf8: String, expected_modified_at_unix_ms: u64? }
retry_connect    { target_id: String }

Surface = sidebar | terminal | workbench | pet
MouseButton = left | right
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
  terminal: Terminal,
  editor: Editor,
  ime: Ime,
  input_generation: u64,
  status: Status,
  spike: SpikeEvidence
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

Terminal {
  pane_id: String?,
  sequence: u64,
  chunks: TerminalChunk[],
  closed: bool,
  exit_code: i32?
}

TerminalChunk { sequence: u64, bytes_base64: String }

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
  last_error: LastError?
}

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

SpikeEvidence {
  callback_emitted: u32,
  remote_tui_ready: bool,
  delegate_bytes_sent: u64
}
```

The `spike` object is test-only evidence and is not a production contract requirement.
Timestamp fields are Unix milliseconds.
Terminal and key bytes are base64 so arbitrary PTY bytes survive JSON unchanged.
