use std::ffi::c_void;
use std::io::{Read, Write};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::ptr;
use std::slice;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};
use serde::{Deserialize, Serialize};
use serde_json::Value;

const SCHEMA_VERSION: u32 = 1;
const CALLBACK_BURST_TARGET_ID: &str = "spike-callback-burst";
static OUTSTANDING_BUFFERS: AtomicUsize = AtomicUsize::new(0);

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CoreOptions {
    pub schema_version: u32,
    pub herdr_socket_path: Option<String>,
    pub remote_targets: Vec<RemoteTarget>,
    pub app_state_path: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RemoteTarget {
    pub id: String,
    pub label: String,
    pub ssh_alias: String,
}

#[derive(Clone, Debug, Deserialize)]
struct EventEnvelope {
    schema_version: u32,
    kind: String,
    payload: Value,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct KeyPayload {
    pane_id: String,
    bytes_base64: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct ClickPayload {
    surface: Surface,
    x: f64,
    y: f64,
    button: MouseButton,
    click_count: u8,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct FocusPanePayload {
    pane_id: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct OpenBrowserPayload {
    profile: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct CreateWorkspacePayload {
    path: String,
    label: String,
    create_worktree: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct CreateTabPayload {
    workspace_id: String,
    label: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct CreatePanePayload {
    tab_id: String,
    cwd: String,
    command: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct CloseWorkspacePayload {
    workspace_id: String,
    confirmed: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct CloseTabPayload {
    tab_id: String,
    confirmed: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct ClosePanePayload {
    pane_id: String,
    confirmed: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct FileOpenPayload {
    path: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct FileSavePayload {
    path: String,
    contents_utf8: String,
    expected_modified_at_unix_ms: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct RetryConnectPayload {
    target_id: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum Surface {
    Sidebar,
    Terminal,
    Workbench,
    Pet,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum MouseButton {
    Left,
    Right,
}

#[derive(Clone, Debug, Serialize)]
struct Snapshot {
    schema_version: u32,
    navigator: NavigatorSnapshot,
    overlay: OverlaySnapshot,
    tab: TabSnapshot,
    connection: ConnectionSnapshot,
    zoomed: Option<String>,
    focused: FocusedSnapshot,
    terminal: TerminalSnapshot,
    editor: EditorSnapshot,
    ime: ImeSnapshot,
    input_generation: u64,
    status: StatusSnapshot,
    spike: SpikeSnapshot,
}

#[derive(Clone, Debug, Serialize)]
struct NavigatorSnapshot {
    root_path: Option<String>,
    focused_workspace_id: Option<String>,
    workspaces: Vec<WorkspaceSnapshot>,
}

#[derive(Clone, Debug, Serialize)]
struct WorkspaceSnapshot {
    id: String,
    label: String,
    path: String,
    remote_target_id: Option<String>,
    expanded: bool,
}

#[derive(Clone, Debug, Serialize)]
struct OverlaySnapshot {
    kind: Option<String>,
    title: Option<String>,
    message: Option<String>,
    actions: Vec<OverlayActionSnapshot>,
}

#[derive(Clone, Debug, Serialize)]
struct OverlayActionSnapshot {
    id: String,
    label: String,
    destructive: bool,
}

#[derive(Clone, Debug, Serialize)]
struct TabSnapshot {
    id: Option<String>,
    workspace_id: Option<String>,
    label: Option<String>,
    panes: Vec<PaneSnapshot>,
}

#[derive(Clone, Debug, Serialize)]
struct PaneSnapshot {
    id: String,
    label: String,
    cwd: String,
    state: String,
    summary: Option<String>,
    activity_at_unix_ms: Option<u64>,
}

#[derive(Clone, Debug, Serialize)]
struct ConnectionSnapshot {
    kind: String,
    state: String,
    target_id: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
struct FocusedSnapshot {
    surface: Surface,
    pane_id: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
struct TerminalSnapshot {
    pane_id: Option<String>,
    sequence: u64,
    chunks: Vec<TerminalChunk>,
    closed: bool,
    exit_code: Option<i32>,
}

#[derive(Clone, Debug, Serialize)]
struct TerminalChunk {
    sequence: u64,
    bytes_base64: String,
}

#[derive(Clone, Debug, Serialize)]
struct EditorSnapshot {
    path: Option<String>,
    language: Option<String>,
    contents_utf8: Option<String>,
    dirty: bool,
    readonly_reason: Option<String>,
    conflict: Option<EditorConflictSnapshot>,
    diff: Option<DiffSnapshot>,
}

#[derive(Clone, Debug, Serialize)]
struct EditorConflictSnapshot {
    disk_modified_at_unix_ms: u64,
    opened_modified_at_unix_ms: u64,
}

#[derive(Clone, Debug, Serialize)]
struct DiffSnapshot {
    added_lines: Vec<u32>,
    removed_lines: Vec<u32>,
}

#[derive(Clone, Debug, Serialize)]
struct ImeSnapshot {
    marked_text: String,
    selected_range: TextRangeSnapshot,
    replacement_range: Option<TextRangeSnapshot>,
}

#[derive(Clone, Debug, Serialize)]
struct TextRangeSnapshot {
    location: u64,
    length: u64,
}

#[derive(Clone, Debug, Serialize)]
struct StatusSnapshot {
    herdr: ProviderStatusSnapshot,
    remote: Vec<RemoteStatusSnapshot>,
    chromux: ChromuxStatusSnapshot,
    last_error: Option<LastErrorSnapshot>,
}

#[derive(Clone, Debug, Serialize)]
struct ProviderStatusSnapshot {
    state: String,
    socket_path: Option<String>,
    message: Option<String>,
    last_checked_at_unix_ms: Option<u64>,
}

#[derive(Clone, Debug, Serialize)]
struct RemoteStatusSnapshot {
    target_id: String,
    state: String,
    message: Option<String>,
    last_checked_at_unix_ms: Option<u64>,
}

#[derive(Clone, Debug, Serialize)]
struct ChromuxStatusSnapshot {
    state: String,
    profile: String,
    current_url: Option<String>,
    current_title: Option<String>,
    message: Option<String>,
    last_checked_at_unix_ms: Option<u64>,
}

#[derive(Clone, Debug, Serialize)]
struct LastErrorSnapshot {
    kind: String,
    message: String,
    retryable: bool,
    occurred_at: u64,
}

#[derive(Clone, Debug, Serialize)]
struct SpikeSnapshot {
    callback_emitted: u32,
    remote_tui_ready: bool,
    delegate_bytes_sent: u64,
}

#[derive(Clone, Copy)]
struct CallbackRegistration {
    callback: extern "C" fn(*mut c_void),
    context: usize,
}

struct Inner {
    options: CoreOptions,
    snapshot: Mutex<Snapshot>,
    callback: Mutex<Option<CallbackRegistration>>,
    pty: Mutex<Option<PtyProcess>>,
    destroyed: AtomicBool,
}

pub struct HerdrCore {
    inner: Arc<Inner>,
}

struct PtyProcess {
    _master: Box<dyn MasterPty + Send>,
    child: Box<dyn Child + Send + Sync>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
}

impl Drop for PtyProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

#[repr(C)]
pub struct HerdrBytes {
    pub ptr: *mut u8,
    pub len: usize,
    pub cap: usize,
}

#[unsafe(no_mangle)]
pub extern "C" fn herdr_core_create(options_json: *const u8, len: usize) -> *mut HerdrCore {
    catch_unwind(AssertUnwindSafe(|| {
        let bytes = unsafe { input_bytes(options_json, len)? };
        let options: CoreOptions = serde_json::from_slice(bytes).ok()?;
        if options.schema_version != SCHEMA_VERSION {
            return None;
        }
        let snapshot = initial_snapshot(&options);
        Some(Box::into_raw(Box::new(HerdrCore {
            inner: Arc::new(Inner {
                options,
                snapshot: Mutex::new(snapshot),
                callback: Mutex::new(None),
                pty: Mutex::new(None),
                destroyed: AtomicBool::new(false),
            }),
        })))
    }))
    .ok()
    .flatten()
    .unwrap_or(ptr::null_mut())
}

#[unsafe(no_mangle)]
pub extern "C" fn herdr_core_dispatch(core: *mut HerdrCore, event_json: *const u8, len: usize) {
    let result = catch_unwind(AssertUnwindSafe(|| {
        let Some(core) = (unsafe { core.as_ref() }) else {
            return;
        };
        let Some(bytes) = (unsafe { input_bytes(event_json, len) }) else {
            set_error(
                &core.inner,
                "event.invalid_bytes",
                "event JSON bytes are missing",
                false,
            );
            return;
        };
        let event = match serde_json::from_slice::<EventEnvelope>(bytes) {
            Ok(event) => event,
            Err(error) => {
                set_error(
                    &core.inner,
                    "event.invalid_json",
                    &format!("event JSON is invalid: {error}"),
                    false,
                );
                return;
            }
        };
        if event.schema_version != SCHEMA_VERSION {
            set_error(
                &core.inner,
                "schema_version.mismatch",
                &format!(
                    "event schema_version {} does not match {}",
                    event.schema_version, SCHEMA_VERSION
                ),
                false,
            );
            return;
        }
        if let Err(message) = dispatch_event(&core.inner, event) {
            set_error(&core.inner, "event.invalid_payload", &message, false);
        }
    }));
    if result.is_err()
        && let Some(core) = unsafe { core.as_ref() }
    {
        set_error(
            &core.inner,
            "ffi.panic",
            "Rust panic was contained at herdr_core_dispatch",
            false,
        );
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn herdr_core_snapshot(core: *mut HerdrCore) -> HerdrBytes {
    catch_unwind(AssertUnwindSafe(|| {
        let Some(core) = (unsafe { core.as_ref() }) else {
            return HerdrBytes::empty();
        };
        let snapshot = core
            .inner
            .snapshot
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        serde_json::to_vec(&*snapshot)
            .map(HerdrBytes::from_vec)
            .unwrap_or_else(|_| HerdrBytes::empty())
    }))
    .unwrap_or_else(|_| HerdrBytes::empty())
}

#[unsafe(no_mangle)]
pub extern "C" fn herdr_core_on_change(
    core: *mut HerdrCore,
    callback: Option<extern "C" fn(*mut c_void)>,
    context: *mut c_void,
) {
    let _ = catch_unwind(AssertUnwindSafe(|| {
        let Some(core) = (unsafe { core.as_ref() }) else {
            return;
        };
        let mut slot = core
            .inner
            .callback
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        *slot = callback.map(|callback| CallbackRegistration {
            callback,
            context: context as usize,
        });
    }));
}

#[unsafe(no_mangle)]
pub extern "C" fn herdr_core_free_bytes(bytes: HerdrBytes) {
    if bytes.ptr.is_null() {
        return;
    }
    unsafe {
        drop(Vec::from_raw_parts(bytes.ptr, bytes.len, bytes.cap));
    }
    OUTSTANDING_BUFFERS.fetch_sub(1, Ordering::SeqCst);
}

#[unsafe(no_mangle)]
pub extern "C" fn herdr_core_destroy(core: *mut HerdrCore) {
    if core.is_null() {
        return;
    }
    let _ = catch_unwind(AssertUnwindSafe(|| unsafe {
        let boxed = Box::from_raw(core);
        boxed.inner.destroyed.store(true, Ordering::SeqCst);
        let mut callback = boxed
            .inner
            .callback
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        *callback = None;
        drop(callback);
        let mut pty = boxed
            .inner
            .pty
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        *pty = None;
    }));
}

unsafe fn input_bytes<'a>(pointer: *const u8, len: usize) -> Option<&'a [u8]> {
    if pointer.is_null() || len == 0 {
        return None;
    }
    Some(unsafe { slice::from_raw_parts(pointer, len) })
}

impl HerdrBytes {
    fn empty() -> Self {
        Self {
            ptr: ptr::null_mut(),
            len: 0,
            cap: 0,
        }
    }

    fn from_vec(mut value: Vec<u8>) -> Self {
        let bytes = Self {
            ptr: value.as_mut_ptr(),
            len: value.len(),
            cap: value.capacity(),
        };
        std::mem::forget(value);
        OUTSTANDING_BUFFERS.fetch_add(1, Ordering::SeqCst);
        bytes
    }
}

fn initial_snapshot(options: &CoreOptions) -> Snapshot {
    let now = unix_ms();
    Snapshot {
        schema_version: SCHEMA_VERSION,
        navigator: NavigatorSnapshot {
            root_path: None,
            focused_workspace_id: None,
            workspaces: options
                .remote_targets
                .iter()
                .map(|target| WorkspaceSnapshot {
                    id: format!("remote:{}", target.id),
                    label: target.label.clone(),
                    path: String::new(),
                    remote_target_id: Some(target.id.clone()),
                    expanded: false,
                })
                .collect(),
        },
        overlay: OverlaySnapshot {
            kind: None,
            title: None,
            message: None,
            actions: Vec::new(),
        },
        tab: TabSnapshot {
            id: None,
            workspace_id: None,
            label: None,
            panes: Vec::new(),
        },
        connection: ConnectionSnapshot {
            kind: "remote".to_owned(),
            state: "disconnected".to_owned(),
            target_id: None,
        },
        zoomed: None,
        focused: FocusedSnapshot {
            surface: Surface::Terminal,
            pane_id: None,
        },
        terminal: TerminalSnapshot {
            pane_id: None,
            sequence: 0,
            chunks: Vec::new(),
            closed: false,
            exit_code: None,
        },
        editor: EditorSnapshot {
            path: None,
            language: None,
            contents_utf8: None,
            dirty: false,
            readonly_reason: None,
            conflict: None,
            diff: None,
        },
        ime: ImeSnapshot {
            marked_text: String::new(),
            selected_range: TextRangeSnapshot {
                location: 0,
                length: 0,
            },
            replacement_range: None,
        },
        input_generation: 0,
        status: StatusSnapshot {
            herdr: ProviderStatusSnapshot {
                state: "unknown".to_owned(),
                socket_path: options.herdr_socket_path.clone(),
                message: None,
                last_checked_at_unix_ms: None,
            },
            remote: options
                .remote_targets
                .iter()
                .map(|target| RemoteStatusSnapshot {
                    target_id: target.id.clone(),
                    state: "disconnected".to_owned(),
                    message: None,
                    last_checked_at_unix_ms: Some(now),
                })
                .collect(),
            chromux: ChromuxStatusSnapshot {
                state: "unknown".to_owned(),
                profile: "default".to_owned(),
                current_url: None,
                current_title: None,
                message: None,
                last_checked_at_unix_ms: None,
            },
            last_error: None,
        },
        spike: SpikeSnapshot {
            callback_emitted: 0,
            remote_tui_ready: false,
            delegate_bytes_sent: 0,
        },
    }
}

fn dispatch_event(inner: &Arc<Inner>, event: EventEnvelope) -> Result<(), String> {
    match event.kind.as_str() {
        "key" => {
            let payload: KeyPayload = payload(event.payload)?;
            let bytes = BASE64
                .decode(payload.bytes_base64)
                .map_err(|error| format!("key.bytes_base64 is invalid: {error}"))?;
            let writer = {
                let pty = inner.pty.lock().unwrap_or_else(|error| error.into_inner());
                pty.as_ref().map(|pty| Arc::clone(&pty.writer))
            };
            let Some(writer) = writer else {
                return Err("key event requires an active terminal PTY".to_owned());
            };
            let mut writer = writer
                .lock()
                .map_err(|_| "PTY writer lock is poisoned".to_owned())?;
            writer
                .write_all(&bytes)
                .and_then(|()| writer.flush())
                .map_err(|error| format!("PTY write failed: {error}"))?;
            let mut snapshot = inner
                .snapshot
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            snapshot.focused.pane_id = Some(payload.pane_id);
            snapshot.spike.delegate_bytes_sent += bytes.len() as u64;
            snapshot.input_generation += 1;
            snapshot.status.last_error = None;
            drop(snapshot);
            notify(inner);
        }
        "click" => accept_payload::<ClickPayload>(inner, event.payload)?,
        "focus_pane" => {
            let payload: FocusPanePayload = payload(event.payload)?;
            accept(inner);
            let mut snapshot = inner
                .snapshot
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            snapshot.focused.pane_id = Some(payload.pane_id);
        }
        "open_browser" => accept_payload::<OpenBrowserPayload>(inner, event.payload)?,
        "create_workspace" => accept_payload::<CreateWorkspacePayload>(inner, event.payload)?,
        "create_tab" => accept_payload::<CreateTabPayload>(inner, event.payload)?,
        "create_pane" => accept_payload::<CreatePanePayload>(inner, event.payload)?,
        "close_workspace" => accept_payload::<CloseWorkspacePayload>(inner, event.payload)?,
        "close_tab" => accept_payload::<CloseTabPayload>(inner, event.payload)?,
        "close_pane" => accept_payload::<ClosePanePayload>(inner, event.payload)?,
        "file_open" => accept_payload::<FileOpenPayload>(inner, event.payload)?,
        "file_save" => accept_payload::<FileSavePayload>(inner, event.payload)?,
        "retry_connect" => {
            let payload: RetryConnectPayload = payload(event.payload)?;
            if payload.target_id == CALLBACK_BURST_TARGET_ID {
                start_callback_burst(Arc::clone(inner));
            } else {
                start_remote_tui(Arc::clone(inner), &payload.target_id)?;
            }
        }
        unknown => {
            set_error(
                inner,
                "event.unknown_kind",
                &format!("unknown event kind: {unknown}"),
                false,
            );
        }
    }
    Ok(())
}

fn payload<T: for<'de> Deserialize<'de>>(value: Value) -> Result<T, String> {
    serde_json::from_value(value).map_err(|error| error.to_string())
}

fn accept_payload<T: for<'de> Deserialize<'de>>(
    inner: &Arc<Inner>,
    value: Value,
) -> Result<(), String> {
    let _: T = payload(value)?;
    accept(inner);
    Ok(())
}

fn accept(inner: &Arc<Inner>) {
    let mut snapshot = inner
        .snapshot
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    snapshot.input_generation += 1;
    snapshot.status.last_error = None;
    drop(snapshot);
    notify(inner);
}

fn start_callback_burst(inner: Arc<Inner>) {
    thread::Builder::new()
        .name("swift-shell-spike-callback".to_owned())
        .spawn(move || {
            for count in 1..=100 {
                if inner.destroyed.load(Ordering::SeqCst) {
                    return;
                }
                {
                    let mut snapshot = inner
                        .snapshot
                        .lock()
                        .unwrap_or_else(|error| error.into_inner());
                    snapshot.input_generation = count;
                    snapshot.connection.state = "connected".to_owned();
                    snapshot.connection.target_id = Some(CALLBACK_BURST_TARGET_ID.to_owned());
                    snapshot.spike.callback_emitted = count as u32;
                    snapshot.status.last_error = None;
                }
                notify(&inner);
                thread::sleep(Duration::from_millis(4));
            }
        })
        .expect("callback burst thread must start");
}

fn start_remote_tui(inner: Arc<Inner>, target_id: &str) -> Result<(), String> {
    let target = inner
        .options
        .remote_targets
        .iter()
        .find(|target| target.id == target_id)
        .cloned()
        .ok_or_else(|| format!("unknown remote target: {target_id}"))?;
    let mut pty_slot = inner.pty.lock().unwrap_or_else(|error| error.into_inner());
    if pty_slot.is_some() {
        return Err("remote terminal is already running".to_owned());
    }
    let pair = native_pty_system()
        .openpty(PtySize {
            rows: 30,
            cols: 100,
            pixel_width: 900,
            pixel_height: 600,
        })
        .map_err(|error| format!("PTY open failed: {error}"))?;
    let fixture_source = r#"
import os, sys, termios, tty
old = termios.tcgetattr(0)
tty.setraw(0)
try:
    sys.stdout.write("\x1b[?1049h\x1b[2J\x1b[H")
    sys.stdout.write("Herdr Agent TUI Fixture - isolated remote SSH\r\n")
    sys.stdout.write("REMOTE_TUI_READY\r\n")
    sys.stdout.write("Type through SwiftTerm. Closing the spike app ends the fixture.\r\n")
    sys.stdout.flush()
    buffered = b""
    while True:
        data = os.read(0, 1024)
        if not data:
            break
        sys.stdout.buffer.write(data)
        sys.stdout.buffer.flush()
        buffered += data
        while b"\r" in buffered or b"\n" in buffered:
            separators = [index for index in (buffered.find(b"\r"), buffered.find(b"\n")) if index >= 0]
            split_at = min(separators)
            line, buffered = buffered[:split_at], buffered[split_at + 1:]
            while buffered.startswith((b"\r", b"\n")):
                buffered = buffered[1:]
            text = line.decode("utf-8", "replace")
            sys.stdout.write("\r\nINPUT_UTF8:" + text + "\r\n")
            sys.stdout.flush()
finally:
    termios.tcsetattr(0, termios.TCSADRAIN, old)
    sys.stdout.write("\x1b[?1049l")
    sys.stdout.flush()
"#;
    let encoded = BASE64.encode(fixture_source.as_bytes());
    let remote_command =
        format!("python3 -u -c 'import base64;exec(base64.b64decode(\"{encoded}\"))'");
    let mut command = CommandBuilder::new("/usr/bin/ssh");
    command.arg("-tt");
    command.arg("-o");
    command.arg("BatchMode=yes");
    command.arg("-o");
    command.arg("ConnectTimeout=10");
    command.arg(target.ssh_alias);
    command.arg(remote_command);
    command.env("TERM", "xterm-256color");
    command.env("LANG", "en_US.UTF-8");
    command.env("LC_ALL", "en_US.UTF-8");
    let child = pair
        .slave
        .spawn_command(command)
        .map_err(|error| format!("SSH PTY spawn failed: {error}"))?;
    drop(pair.slave);
    let writer = Arc::new(Mutex::new(
        pair.master
            .take_writer()
            .map_err(|error| format!("PTY writer failed: {error}"))?,
    ));
    let mut reader = pair
        .master
        .try_clone_reader()
        .map_err(|error| format!("PTY reader failed: {error}"))?;
    let reader_inner = Arc::clone(&inner);
    thread::Builder::new()
        .name("swift-shell-spike-ssh-reader".to_owned())
        .spawn(move || {
            let mut bytes = [0_u8; 8192];
            loop {
                match reader.read(&mut bytes) {
                    Ok(0) => {
                        let mut snapshot = reader_inner
                            .snapshot
                            .lock()
                            .unwrap_or_else(|error| error.into_inner());
                        snapshot.terminal.closed = true;
                        snapshot.connection.state = "disconnected".to_owned();
                        drop(snapshot);
                        notify(&reader_inner);
                        return;
                    }
                    Ok(count) => {
                        let chunk = &bytes[..count];
                        let ready = chunk
                            .windows(b"REMOTE_TUI_READY".len())
                            .any(|window| window == b"REMOTE_TUI_READY");
                        let mut snapshot = reader_inner
                            .snapshot
                            .lock()
                            .unwrap_or_else(|error| error.into_inner());
                        snapshot.terminal.sequence += 1;
                        let sequence = snapshot.terminal.sequence;
                        snapshot.terminal.chunks.push(TerminalChunk {
                            sequence,
                            bytes_base64: BASE64.encode(chunk),
                        });
                        if snapshot.terminal.chunks.len() > 256 {
                            snapshot.terminal.chunks.remove(0);
                        }
                        if ready {
                            snapshot.spike.remote_tui_ready = true;
                            snapshot.connection.state = "connected".to_owned();
                            snapshot.status.last_error = None;
                        }
                        drop(snapshot);
                        notify(&reader_inner);
                    }
                    Err(error) => {
                        set_error(
                            &reader_inner,
                            "remote_pty.read_failed",
                            &format!("remote PTY read failed: {error}"),
                            true,
                        );
                        return;
                    }
                }
            }
        })
        .map_err(|error| format!("SSH reader thread failed: {error}"))?;
    {
        let mut snapshot = inner
            .snapshot
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        snapshot.connection.target_id = Some(target_id.to_owned());
        snapshot.connection.state = "connecting".to_owned();
        snapshot.terminal.pane_id = Some("remote-tui-fixture".to_owned());
        snapshot.terminal.closed = false;
        snapshot.status.last_error = None;
    }
    *pty_slot = Some(PtyProcess {
        _master: pair.master,
        child,
        writer,
    });
    Ok(())
}

fn set_error(inner: &Arc<Inner>, kind: &str, message: &str, retryable: bool) {
    let mut snapshot = inner
        .snapshot
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    snapshot.status.last_error = Some(LastErrorSnapshot {
        kind: kind.to_owned(),
        message: message.to_owned(),
        retryable,
        occurred_at: unix_ms(),
    });
    drop(snapshot);
    notify(inner);
}

fn notify(inner: &Arc<Inner>) {
    if inner.destroyed.load(Ordering::SeqCst) {
        return;
    }
    let registration = *inner
        .callback
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if let Some(registration) = registration {
        (registration.callback)(registration.context as *mut c_void);
    }
}

fn unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::atomic::AtomicUsize;
    use std::time::Instant;

    static CALLBACKS: AtomicUsize = AtomicUsize::new(0);

    extern "C" fn count_callback(_: *mut c_void) {
        CALLBACKS.fetch_add(1, Ordering::SeqCst);
    }

    fn options() -> Vec<u8> {
        serde_json::to_vec(&CoreOptions {
            schema_version: SCHEMA_VERSION,
            herdr_socket_path: None,
            remote_targets: vec![RemoteTarget {
                id: "mini".to_owned(),
                label: "Mac mini".to_owned(),
                ssh_alias: "mini".to_owned(),
            }],
            app_state_path: "/tmp/herdr-ide-verify-swift-shell-state.json".to_owned(),
        })
        .unwrap()
    }

    fn create() -> *mut HerdrCore {
        let options = options();
        let core = herdr_core_create(options.as_ptr(), options.len());
        assert!(!core.is_null());
        core
    }

    fn snapshot(core: *mut HerdrCore) -> Value {
        let bytes = herdr_core_snapshot(core);
        assert!(!bytes.ptr.is_null());
        let value =
            serde_json::from_slice(unsafe { slice::from_raw_parts(bytes.ptr, bytes.len) }).unwrap();
        herdr_core_free_bytes(bytes);
        value
    }

    fn dispatch(core: *mut HerdrCore, event: Value) {
        let bytes = serde_json::to_vec(&event).unwrap();
        herdr_core_dispatch(core, bytes.as_ptr(), bytes.len());
    }

    #[test]
    fn create_rejects_schema_mismatch() {
        let mut options: Value = serde_json::from_slice(&options()).unwrap();
        options["schema_version"] = json!(99);
        let bytes = serde_json::to_vec(&options).unwrap();
        assert!(herdr_core_create(bytes.as_ptr(), bytes.len()).is_null());
    }

    #[test]
    fn unknown_kind_and_event_version_are_observable() {
        let core = create();
        dispatch(
            core,
            json!({"schema_version": 1, "kind": "invented", "payload": {}}),
        );
        assert_eq!(
            snapshot(core)["status"]["last_error"]["kind"],
            "event.unknown_kind"
        );
        dispatch(
            core,
            json!({"schema_version": 99, "kind": "click", "payload": {}}),
        );
        assert_eq!(
            snapshot(core)["status"]["last_error"]["kind"],
            "schema_version.mismatch"
        );
        herdr_core_destroy(core);
    }

    #[test]
    fn all_declared_event_payloads_have_concrete_types() {
        let core = create();
        let events = [
            json!({"schema_version":1,"kind":"click","payload":{"surface":"terminal","x":1.0,"y":2.0,"button":"left","click_count":1}}),
            json!({"schema_version":1,"kind":"focus_pane","payload":{"pane_id":"p1"}}),
            json!({"schema_version":1,"kind":"open_browser","payload":{"profile":"default"}}),
            json!({"schema_version":1,"kind":"create_workspace","payload":{"path":"/tmp/herdr-ide-verify-workspace","label":"verify","create_worktree":false}}),
            json!({"schema_version":1,"kind":"create_tab","payload":{"workspace_id":"w1","label":"tab"}}),
            json!({"schema_version":1,"kind":"create_pane","payload":{"tab_id":"t1","cwd":"/tmp","command":null}}),
            json!({"schema_version":1,"kind":"close_workspace","payload":{"workspace_id":"w1","confirmed":true}}),
            json!({"schema_version":1,"kind":"close_tab","payload":{"tab_id":"t1","confirmed":true}}),
            json!({"schema_version":1,"kind":"close_pane","payload":{"pane_id":"p1","confirmed":true}}),
            json!({"schema_version":1,"kind":"file_open","payload":{"path":"/tmp/file"}}),
            json!({"schema_version":1,"kind":"file_save","payload":{"path":"/tmp/file","contents_utf8":"value","expected_modified_at_unix_ms":null}}),
        ];
        for event in events {
            dispatch(core, event);
            assert!(snapshot(core)["status"]["last_error"].is_null());
        }
        herdr_core_destroy(core);
    }

    #[test]
    fn rust_thread_emits_one_hundred_callbacks() {
        CALLBACKS.store(0, Ordering::SeqCst);
        let core = create();
        herdr_core_on_change(core, Some(count_callback), ptr::null_mut());
        dispatch(
            core,
            json!({"schema_version":1,"kind":"retry_connect","payload":{"target_id":CALLBACK_BURST_TARGET_ID}}),
        );
        let deadline = Instant::now() + Duration::from_secs(3);
        while CALLBACKS.load(Ordering::SeqCst) < 100 && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(CALLBACKS.load(Ordering::SeqCst), 100);
        assert_eq!(snapshot(core)["spike"]["callback_emitted"], 100);
        herdr_core_destroy(core);
    }

    #[test]
    fn every_snapshot_buffer_is_returned_to_rust() {
        let core = create();
        for _ in 0..1_000 {
            let bytes = herdr_core_snapshot(core);
            herdr_core_free_bytes(bytes);
        }
        assert_eq!(OUTSTANDING_BUFFERS.load(Ordering::SeqCst), 0);
        herdr_core_destroy(core);
    }
}
