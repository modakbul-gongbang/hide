use std::cell::{Cell, OnceCell, RefCell};
use std::collections::VecDeque;
use std::ffi::c_void;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::ptr::NonNull;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject, Sel};
use objc2::{DefinedClass, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSAccessibilityElement, NSAccessibilityGroupRole, NSAccessibilityListRole,
    NSAccessibilityTabGroupRole, NSAccessibilityTextAreaRole, NSApp, NSApplication,
    NSApplicationActivationPolicy, NSApplicationDelegate, NSAutoresizingMaskOptions,
    NSBackingStoreType, NSEvent, NSEventModifierFlags, NSScreen, NSTextInputClient, NSView,
    NSWindow, NSWindowDelegate, NSWindowOrderingMode, NSWindowStyleMask,
};
use objc2_foundation::{
    MainThreadMarker, NSArray, NSAttributedString, NSAttributedStringKey, NSNotification, NSObject,
    NSObjectProtocol, NSPoint, NSRange, NSRangePointer, NSRect, NSSize, NSString, NSTimer,
    NSUInteger, ns_string,
};
use serde::Serialize;
use serde_json::json;

use crate::browser::BrowserRuntime;
use crate::layout::{
    CanvasGeometry, PaneId, ZoomController, ZoomOutcome, ZoomState, browser_frame,
};
use crate::preflight;
use crate::pty::{PtySession, TerminalSnapshot, visible_transcript};
use crate::render::{FrameModel, PresentObservation, Renderer, VisualState};

const WINDOW_WIDTH: f64 = 1180.0;
const WINDOW_HEIGHT: f64 = 720.0;
const TICK_INTERVAL_SECONDS: f64 = 0.016;
const FIXTURE_WORKSPACE_COUNT: usize = 7;
const FIXTURE_PANE_COUNT: usize = 11;
const T1_LATENCY_PROBE_PREFIX: &str = "917-";

fn layout_invariant_text_for_key_code(key_code: u16) -> Option<&'static str> {
    match key_code {
        18 => Some("1"),
        19 => Some("2"),
        20 => Some("3"),
        21 => Some("4"),
        22 => Some("6"),
        23 => Some("5"),
        25 => Some("9"),
        26 => Some("7"),
        27 => Some("-"),
        28 => Some("8"),
        29 => Some("0"),
        36 => Some("\r"),
        _ => None,
    }
}

fn option_meta_text_for_key_code(key_code: u16) -> Option<&'static str> {
    match key_code {
        3 => Some("f"),
        _ => None,
    }
}

fn is_option_meta_key(key_code: u16, flags: NSEventModifierFlags) -> bool {
    option_meta_text_for_key_code(key_code).is_some()
        && flags.contains(NSEventModifierFlags::Option)
        && !flags.intersects(
            NSEventModifierFlags::Command
                | NSEventModifierFlags::Control
                | NSEventModifierFlags::Shift,
        )
}

fn modifier_names(flags: NSEventModifierFlags) -> Vec<&'static str> {
    let mut names = Vec::new();
    if flags.contains(NSEventModifierFlags::Shift) {
        names.push("shift");
    }
    if flags.contains(NSEventModifierFlags::Control) {
        names.push("control");
    }
    if flags.contains(NSEventModifierFlags::Option) {
        names.push("option");
    }
    if flags.contains(NSEventModifierFlags::Command) {
        names.push("command");
    }
    names
}

// Chord ingress diagnosis: a physical chord delivers the modifier's own
// flags-changed transition before the modified key, so this event proves
// whether the modifier reached AppKit even when the key event itself never
// arrives. Payload keys must stay disjoint from preflight::RESERVED_FIELDS.
fn modifier_flags_details(
    key_code: u16,
    flags: NSEventModifierFlags,
    focus: serde_json::Value,
    input_monotonic_ns: u64,
) -> serde_json::Value {
    json!({
        "source": "appkit",
        "key_code": key_code,
        "modifiers": modifier_names(flags),
        "focus": focus,
        "input_monotonic_ns": input_monotonic_ns,
    })
}

fn is_plain_key_control(action: Option<&str>, key_code: u16, flags: NSEventModifierFlags) -> bool {
    action == Some("terminal.plain_key_control")
        && key_code == 28
        && !flags.intersects(
            NSEventModifierFlags::Option
                | NSEventModifierFlags::Command
                | NSEventModifierFlags::Control
                | NSEventModifierFlags::Shift,
        )
}

#[derive(Clone, Debug)]
struct Options {
    report_path: Option<PathBuf>,
    probe_count: usize,
    autoclose_after: Option<Duration>,
    browser_enabled: bool,
    visual_state: VisualState,
    preflight_launch_mode: Option<String>,
    preflight_run_id: Option<String>,
    preflight_phase: Option<String>,
    preflight_events: Option<PathBuf>,
    preflight_scenario: Option<PathBuf>,
    preflight_verification_profile: Option<String>,
    preflight_action_socket: Option<PathBuf>,
}

impl Options {
    fn parse() -> Result<Self> {
        let mut options = Self {
            report_path: None,
            probe_count: 0,
            autoclose_after: None,
            browser_enabled: false,
            visual_state: VisualState::Normal,
            preflight_launch_mode: None,
            preflight_run_id: None,
            preflight_phase: None,
            preflight_events: None,
            preflight_scenario: None,
            preflight_verification_profile: None,
            preflight_action_socket: None,
        };
        let mut arguments = std::env::args().skip(1);
        while let Some(argument) = arguments.next() {
            match argument.as_str() {
                "--report" => {
                    options.report_path =
                        Some(PathBuf::from(arguments.next().ok_or_else(|| {
                            anyhow!("stage=cli.parse option=--report cause=missing-path")
                        })?));
                }
                "--probe-count" => {
                    let raw = arguments.next().ok_or_else(|| {
                        anyhow!("stage=cli.parse option=--probe-count cause=missing-value")
                    })?;
                    options.probe_count = raw
                        .parse()
                        .context("stage=cli.parse option=--probe-count cause=invalid-integer")?;
                }
                "--autoclose-ms" => {
                    let raw = arguments.next().ok_or_else(|| {
                        anyhow!("stage=cli.parse option=--autoclose-ms cause=missing-value")
                    })?;
                    let millis = raw
                        .parse()
                        .context("stage=cli.parse option=--autoclose-ms cause=invalid-integer")?;
                    options.autoclose_after = Some(Duration::from_millis(millis));
                }
                "--browser-closed" => {}
                "--t1-visual-state" => {
                    let state = arguments.next().ok_or_else(|| {
                        anyhow!("stage=cli.parse option=--t1-visual-state cause=missing-value")
                    })?;
                    options.visual_state = match state.as_str() {
                        "normal" => VisualState::Normal,
                        "attention" => VisualState::Attention,
                        "remote" => VisualState::Remote,
                        _ => {
                            return Err(anyhow!(
                                "stage=cli.parse option=--t1-visual-state cause=invalid-value value={state:?}"
                            ));
                        }
                    };
                }
                "--browser-url"
                | "--browser-profile"
                | "--remote-debugging-port"
                | "--t1-browser-url"
                | "--t1-browser-profile"
                | "--t1-remote-debugging-port" => {
                    arguments.next().ok_or_else(|| {
                        anyhow!("stage=cli.parse option={argument} cause=missing-value")
                    })?;
                    if argument == "--browser-url" {
                        options.browser_enabled = true;
                    }
                }
                "--t1-browser-mode" => {
                    let mode = arguments.next().ok_or_else(|| {
                        anyhow!("stage=cli.parse option=--t1-browser-mode cause=missing-value")
                    })?;
                    if !matches!(mode.as_str(), "browser-closed" | "browser-included") {
                        return Err(anyhow!(
                            "stage=cli.parse option=--t1-browser-mode cause=invalid-value value={mode:?}"
                        ));
                    }
                    options.preflight_launch_mode = Some(mode);
                }
                "--t1-preflight-phase" => {
                    let phase = arguments.next().ok_or_else(|| {
                        anyhow!("stage=cli.parse option=--t1-preflight-phase cause=missing-value")
                    })?;
                    if !matches!(
                        phase.as_str(),
                        "clean_closed" | "warm_closed" | "browser_included" | "relaunch_closed"
                    ) {
                        return Err(anyhow!(
                            "stage=cli.parse option=--t1-preflight-phase cause=invalid-value value={phase:?}"
                        ));
                    }
                    options.preflight_phase = Some(phase);
                }
                "--t1-preflight-run-id" => {
                    options.preflight_run_id = Some(arguments.next().ok_or_else(|| {
                        anyhow!("stage=cli.parse option=--t1-preflight-run-id cause=missing-value")
                    })?);
                }
                "--t1-preflight-events" => {
                    options.preflight_events =
                        Some(PathBuf::from(arguments.next().ok_or_else(|| {
                            anyhow!(
                                "stage=cli.parse option=--t1-preflight-events cause=missing-path"
                            )
                        })?));
                }
                "--t1-preflight-scenario" => {
                    options.preflight_scenario =
                        Some(PathBuf::from(arguments.next().ok_or_else(|| {
                            anyhow!(
                                "stage=cli.parse option=--t1-preflight-scenario cause=missing-path"
                            )
                        })?));
                }
                "--t1-preflight-profile" => {
                    options.preflight_verification_profile =
                        Some(arguments.next().ok_or_else(|| {
                            anyhow!(
                                "stage=cli.parse option=--t1-preflight-profile cause=missing-value"
                            )
                        })?);
                }
                "--t1-preflight-action-socket" => {
                    options.preflight_action_socket =
                        Some(PathBuf::from(arguments.next().ok_or_else(|| {
                            anyhow!(
                                "stage=cli.parse option=--t1-preflight-action-socket cause=missing-path"
                            )
                        })?));
                }
                "--help" => {
                    println!(
                        "herdr-integrated-preflight [--browser-closed | --browser-url URL --browser-profile PATH --remote-debugging-port PORT] [--report PATH] [--probe-count N] [--autoclose-ms N] [--t1-visual-state normal|attention|remote] [--t1-preflight-phase clean_closed|warm_closed|browser_included|relaunch_closed --t1-browser-mode browser-closed|browser-included --t1-preflight-events PATH (--t1-preflight-run-id ID | --t1-preflight-scenario PATH) --t1-preflight-profile PROFILE --t1-preflight-action-socket PATH]"
                    );
                    std::process::exit(0);
                }
                other => {
                    return Err(anyhow!(
                        "stage=cli.parse target={other} cause=unknown-argument retryable=false"
                    ));
                }
            }
        }
        Ok(options)
    }
}

#[derive(Debug)]
struct PreflightAction {
    action: String,
    key_code: u16,
    armed_at: Instant,
}

impl PreflightAction {
    // Payload keys must stay disjoint from preflight::RESERVED_FIELDS; the run
    // phase already rides on the telemetry envelope and may not be repeated here.
    fn armed_details(&self) -> serde_json::Value {
        json!({
            "action": self.action,
            "key_code": self.key_code,
            "input_contract": "scenario-action",
        })
    }

    fn consume_details(&self) -> serde_json::Value {
        json!({
            "action": self.action,
            "key_code": self.key_code,
            "input_contract": "scenario-action",
            "armed_age_ms": self.armed_at.elapsed().as_secs_f64() * 1000.0,
        })
    }
}

struct PreflightActionListener {
    listener: UnixListener,
    path: PathBuf,
    expected_phase: Option<String>,
}

impl PreflightActionListener {
    fn bind(path: Option<&Path>, expected_phase: Option<&str>) -> Result<Option<Self>> {
        let Some(path) = path else {
            return Ok(None);
        };
        if path.exists() {
            return Err(anyhow!(
                "stage=preflight.action_socket.bind path={} cause=must-not-preexist retryable=false",
                path.display()
            ));
        }
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent).with_context(|| {
                format!(
                    "stage=preflight.action_socket.mkdir path={} retryable=false",
                    parent.display()
                )
            })?;
        }
        let listener = UnixListener::bind(path).with_context(|| {
            format!(
                "stage=preflight.action_socket.bind path={} retryable=false",
                path.display()
            )
        })?;
        listener.set_nonblocking(true).with_context(|| {
            format!(
                "stage=preflight.action_socket.nonblocking path={} retryable=false",
                path.display()
            )
        })?;
        Ok(Some(Self {
            listener,
            path: path.to_owned(),
            expected_phase: expected_phase.map(str::to_owned),
        }))
    }

    fn poll(&self) -> Result<Option<PreflightAction>> {
        let (mut stream, _) = match self.listener.accept() {
            Ok(connection) => connection,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => return Ok(None),
            Err(error) => {
                return Err(anyhow!(
                    "stage=preflight.action_socket.accept path={} cause={error} retryable=false",
                    self.path.display()
                ));
            }
        };
        stream.set_nonblocking(false).with_context(|| {
            format!(
                "stage=preflight.action_socket.blocking path={} retryable=false",
                self.path.display()
            )
        })?;
        stream
            .set_read_timeout(Some(Duration::from_millis(250)))
            .with_context(|| {
                format!(
                    "stage=preflight.action_socket.timeout path={} retryable=false",
                    self.path.display()
                )
            })?;
        let mut line = String::new();
        BufReader::new(stream.try_clone().with_context(|| {
            format!(
                "stage=preflight.action_socket.clone path={} retryable=false",
                self.path.display()
            )
        })?)
        .read_line(&mut line)
        .with_context(|| {
            format!(
                "stage=preflight.action_socket.read path={} retryable=false",
                self.path.display()
            )
        })?;
        let request: serde_json::Value = serde_json::from_str(line.trim()).with_context(|| {
            format!(
                "stage=preflight.action_socket.parse path={} cause=invalid-json retryable=false",
                self.path.display()
            )
        })?;
        let action = request
            .get("action")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| anyhow!("stage=preflight.action_socket.parse cause=action-missing"))?;
        let phase = request
            .get("phase")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| anyhow!("stage=preflight.action_socket.parse cause=phase-missing"))?;
        let key_code = request
            .get("key_code")
            .and_then(serde_json::Value::as_u64)
            .ok_or_else(|| anyhow!("stage=preflight.action_socket.parse cause=key-code-missing"))?;
        let key_code = u16::try_from(key_code)
            .map_err(|_| anyhow!("stage=preflight.action_socket.parse cause=key-code-invalid"))?;
        if action != "terminal.plain_key_control" {
            return Err(anyhow!(
                "stage=preflight.action_socket.contract cause=unsupported-action action={action:?}"
            ));
        }
        if self.expected_phase.as_deref() != Some(phase) {
            return Err(anyhow!(
                "stage=preflight.action_socket.contract cause=phase-mismatch expected={:?} actual={phase:?}",
                self.expected_phase
            ));
        }
        if key_code != 28 {
            return Err(anyhow!(
                "stage=preflight.action_socket.contract cause=key-code-mismatch expected=28 actual={key_code}"
            ));
        }
        let response = json!({
            "status": "armed",
            "action": action,
            "phase": phase,
            "key_code": key_code,
            "input_contract": "scenario-action",
        });
        stream
            .write_all(serde_json::to_string(&response)?.as_bytes())
            .and_then(|_| stream.write_all(b"\n"))
            .with_context(|| {
                format!(
                    "stage=preflight.action_socket.write path={} retryable=false",
                    self.path.display()
                )
            })?;
        Ok(Some(PreflightAction {
            action: action.to_owned(),
            key_code,
            armed_at: Instant::now(),
        }))
    }
}

impl Drop for PreflightActionListener {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

#[derive(Debug)]
struct UiState {
    zoom: ZoomController,
    editor_text: String,
    ime_marked: String,
    ime_committed: String,
    visible_failure: Option<String>,
    input_generation: u64,
    pending_input: VecDeque<(u64, Instant)>,
    pending_probes: VecDeque<(u64, String, u64)>,
    terminal_input_line: String,
    input_to_present_ms: Vec<f64>,
    last_terminal_generation: u64,
    dirty: bool,
    browser_enabled: bool,
    ime_marked_observed: bool,
    visual_state: VisualState,
}

impl UiState {
    fn new(browser_enabled: bool, visual_state: VisualState) -> Self {
        Self {
            zoom: ZoomController::new(vec![0.55, 0.45], PaneId::TerminalA),
            editor_text: "# native-workbench.rs\n\nAppKit owns lifecycle and native views.\nWGPU owns IDE, terminal, and editor pixels.\nCEF stays a separate native child view.".to_owned(),
            ime_marked: String::new(),
            ime_committed: String::new(),
            visible_failure: None,
            input_generation: 0,
            pending_input: VecDeque::new(),
            pending_probes: VecDeque::new(),
            terminal_input_line: String::new(),
            input_to_present_ms: Vec::new(),
            last_terminal_generation: 0,
            dirty: true,
            browser_enabled,
            ime_marked_observed: false,
            visual_state,
        }
    }

    fn model(&self, terminal: &TerminalSnapshot) -> FrameModel {
        FrameModel {
            zoomed: match self.zoom.state() {
                ZoomState::Normal => None,
                ZoomState::Zoomed { target, .. } => Some(*target),
            },
            focused: self.zoom.current().focused,
            terminal_status: self
                .visible_failure
                .as_deref()
                .unwrap_or(&terminal.status)
                .to_owned(),
            terminal_text: visible_transcript(&terminal.transcript, 16),
            editor_text: self.editor_text.clone(),
            input_generation: self.input_generation,
            browser_enabled: self.browser_enabled,
            visual_state: self.visual_state,
        }
    }

    fn note_presented(
        &mut self,
        generation: u64,
        now: Instant,
        visible_terminal: &str,
    ) -> Vec<(String, u64)> {
        while let Some((pending_generation, started)) = self.pending_input.front().copied() {
            if pending_generation > generation {
                break;
            }
            self.pending_input.pop_front();
            self.input_to_present_ms
                .push(now.duration_since(started).as_secs_f64() * 1000.0);
        }
        let mut presented = Vec::new();
        while let Some((pending_generation, probe, input_ns)) = self.pending_probes.front().cloned()
        {
            if pending_generation > generation || !visible_terminal.contains(&probe) {
                break;
            }
            self.pending_probes.pop_front();
            presented.push((probe, input_ns));
        }
        presented
    }
}

#[derive(Debug, Serialize)]
struct RuntimeReport {
    schema: &'static str,
    build: &'static str,
    architecture: &'static str,
    process_id: u32,
    pty_child_process_id: Option<u32>,
    window_id: isize,
    adapter: String,
    physical_width: u32,
    physical_height: u32,
    scale_factor: f64,
    first_usable_ms: f64,
    input_samples: usize,
    input_to_present_p95_ms: Option<f64>,
    input_to_present_max_ms: Option<f64>,
    zoom_state: &'static str,
    ax_children: [&'static str; 4],
    ime_contract: &'static str,
    pty_contract: &'static str,
}

struct ViewIvars {
    renderer: RefCell<Option<Renderer>>,
    pty: PtySession,
    ui: RefCell<UiState>,
    options: Options,
    action_listener: RefCell<Option<PreflightActionListener>>,
    armed_action: RefCell<Option<PreflightAction>>,
    started_at: Instant,
    first_presented_at: Cell<Option<Instant>>,
    window_id: Cell<isize>,
    scale_factor: Cell<f64>,
    tick_count: Cell<u64>,
    probe_index: Cell<usize>,
    probe_is_marked: Cell<bool>,
    input_method_handled: Cell<bool>,
    browser_ready_emitted: Cell<bool>,
    close_requested: Cell<bool>,
    relaunch_state_hash: RefCell<Option<String>>,
}

define_class!(
    // SAFETY:
    // - NSView supports subclassing.
    // - The class is main-thread-only, so AppKit and renderer state never crosses threads.
    // - The WGPU surface drops before AppKit releases its owning NSView.
    #[unsafe(super = NSView)]
    #[thread_kind = MainThreadOnly]
    #[ivars = ViewIvars]
    struct RenderView;

    impl RenderView {
        #[unsafe(method(acceptsFirstResponder))]
        fn accepts_first_responder(&self) -> bool {
            true
        }

        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty_rect: NSRect) {
            match self.render_frame() {
                Ok(()) => {
                    let mut ui = self.ivars().ui.borrow_mut();
                    if clear_transient_render_failure(&mut ui.visible_failure) {
                        ui.dirty = true;
                        drop(ui);
                        self.setNeedsDisplay(true);
                    }
                }
                Err(error) => {
                    let message = format!("Render failed: {error:#}");
                    self.ivars().ui.borrow_mut().visible_failure = Some(message.clone());
                    eprintln!("event=render.failed stage=draw retryable=true error={message:?}");
                }
            }
        }

        #[unsafe(method(keyDown:))]
        fn key_down(&self, event: &NSEvent) {
            if let Err(error) = self.poll_preflight_action() {
                self.record_preflight_action_failure(error);
                return;
            }
            if is_zoom_shortcut(event.keyCode(), event.modifierFlags()) {
                route_zoom_shortcut();
                return;
            }
            if is_quit_shortcut(event.keyCode(), event.modifierFlags()) {
                request_orderly_quit();
                return;
            }
            if is_option_meta_key(event.keyCode(), event.modifierFlags()) {
                self.commit_option_meta(event.keyCode(), "f");
                return;
            }
            if let Some(action) = self.take_preflight_action(event.keyCode(), event.modifierFlags())
            {
                preflight::emit("input.action.consume", action.consume_details());
                self.commit_plain_key_control(event.keyCode(), "8");
                return;
            }
            self.ivars().input_method_handled.set(false);
            self.interpretKeyEvents(&NSArray::from_slice(&[event]));
            if !self.ivars().input_method_handled.get()
                && !event
                    .modifierFlags()
                    .intersects(NSEventModifierFlags::Command | NSEventModifierFlags::Control)
            {
                let characters = layout_invariant_text_for_key_code(event.keyCode())
                    .map(|value| value.to_owned())
                    .or_else(|| {
                        event
                            .characters()
                            .or_else(|| event.charactersIgnoringModifiers())
                            .map(|value| value.to_string())
                            .filter(|value| !value.is_empty())
                    });
                if let Some(characters) = characters {
                    self.commit_text(&characters);
                }
            }
        }

        #[unsafe(method(flagsChanged:))]
        fn flags_changed(&self, event: &NSEvent) {
            if preflight::active() {
                preflight::emit(
                    "input.modifier.flags_changed",
                    modifier_flags_details(
                        event.keyCode(),
                        event.modifierFlags(),
                        self.focus_snapshot(),
                        preflight::monotonic_ns(),
                    ),
                );
            }
            let _: () = unsafe { msg_send![super(self), flagsChanged: event] };
        }

        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, event: &NSEvent) {
            let point = self.convertPoint_fromView(event.locationInWindow(), None);
            let bounds = self.bounds();
            let geometry = CanvasGeometry::for_size(bounds.size.width, bounds.size.height);
            let split = geometry.split_x;
            let pane = if point.x >= split { PaneId::Editor } else { PaneId::TerminalA };
            let mut ui = self.ivars().ui.borrow_mut();
            let from = ui.zoom.current().focused;
            ui.zoom.set_focus(pane);
            ui.dirty = true;
            drop(ui);
            preflight::emit(
                "focus.changed",
                json!({ "from": from, "to": pane, "focused_pane": pane }),
            );
            crate::browser::blur_browser();
            self.update_accessibility_tree();
            self.setNeedsDisplay(true);
        }

        #[unsafe(method(viewDidChangeBackingProperties))]
        fn view_did_change_backing_properties(&self) {
            let _: () = unsafe { msg_send![super(self), viewDidChangeBackingProperties] };
            self.ivars().ui.borrow_mut().dirty = true;
            self.update_accessibility_tree();
            self.setNeedsDisplay(true);
        }

        #[unsafe(method(tick:))]
        fn tick(&self, _timer: &NSTimer) {
            self.tick_impl();
        }
    }

    // SAFETY: NSObjectProtocol has no additional safety requirements.
    unsafe impl NSObjectProtocol for RenderView {}

    // SAFETY: Every required NSTextInputClient selector has the generated signature.
    unsafe impl NSTextInputClient for RenderView {
        #[unsafe(method(insertText:replacementRange:))]
        unsafe fn insert_text_replacement_range(&self, string: &AnyObject, _range: NSRange) {
            let committed_text = objc_text(string);
            if !committed_text.is_empty() {
                self.ivars().input_method_handled.set(true);
                self.commit_text(&committed_text);
            }
        }

        #[unsafe(method(doCommandBySelector:))]
        unsafe fn do_command_by_selector(&self, selector: Sel) {
            if selector == sel!(insertNewline:) {
                self.ivars().input_method_handled.set(true);
                self.commit_text("\r");
            } else if selector == sel!(deleteBackward:) {
                self.ivars().input_method_handled.set(true);
                self.commit_text("\u{7f}");
            } else {
                eprintln!("event=input.command.unhandled selector={selector:?} retryable=false");
            }
        }

        #[unsafe(method(setMarkedText:selectedRange:replacementRange:))]
        unsafe fn set_marked_text_selected_range_replacement_range(
            &self,
            string: &AnyObject,
            _selected_range: NSRange,
            _replacement_range: NSRange,
        ) {
            let marked_text = objc_text(string);
            let has_marked_text = !marked_text.is_empty();
            if has_marked_text {
                self.ivars().input_method_handled.set(true);
            }
            let mut ui = self.ivars().ui.borrow_mut();
            ui.ime_marked = marked_text;
            ui.ime_marked_observed |= has_marked_text;
            ui.dirty = true;
            drop(ui);
            self.setNeedsDisplay(true);
        }

        #[unsafe(method(unmarkText))]
        fn unmark_text(&self) {
            let mut ui = self.ivars().ui.borrow_mut();
            ui.ime_marked.clear();
            ui.dirty = true;
        }

        #[unsafe(method(selectedRange))]
        fn selected_range(&self) -> NSRange {
            not_found_range()
        }

        #[unsafe(method(markedRange))]
        fn marked_range(&self) -> NSRange {
            let length = self.ivars().ui.borrow().ime_marked.encode_utf16().count();
            if length == 0 { not_found_range() } else { NSRange::new(0, length) }
        }

        #[unsafe(method(hasMarkedText))]
        fn has_marked_text(&self) -> bool {
            !self.ivars().ui.borrow().ime_marked.is_empty()
        }

        #[unsafe(method_id(attributedSubstringForProposedRange:actualRange:))]
        unsafe fn attributed_substring_for_proposed_range_actual_range(
            &self,
            _range: NSRange,
            actual_range: NSRangePointer,
        ) -> Option<Retained<NSAttributedString>> {
            if !actual_range.is_null() {
                unsafe { actual_range.write(not_found_range()) };
            }
            None
        }

        #[unsafe(method_id(validAttributesForMarkedText))]
        fn valid_attributes_for_marked_text(&self) -> Retained<NSArray<NSAttributedStringKey>> {
            NSArray::new()
        }

        #[unsafe(method(firstRectForCharacterRange:actualRange:))]
        unsafe fn first_rect_for_character_range_actual_range(
            &self,
            range: NSRange,
            actual_range: NSRangePointer,
        ) -> NSRect {
            if !actual_range.is_null() {
                unsafe { actual_range.write(range) };
            }
            let local = NSRect::new(NSPoint::new(280.0, 92.0), NSSize::new(2.0, 22.0));
            self.window().map(|window| window.convertRectToScreen(local)).unwrap_or(local)
        }

        #[unsafe(method(characterIndexForPoint:))]
        fn character_index_for_point(&self, _point: NSPoint) -> NSUInteger {
            0
        }
    }
);

impl RenderView {
    fn focus_snapshot(&self) -> serde_json::Value {
        let app = NSApplication::sharedApplication(self.mtm());
        let window = self.window();
        let render_view_pointer = NonNull::from(self).cast::<c_void>();
        let render_view_first_responder = window
            .as_ref()
            .and_then(|window| window.firstResponder())
            .is_some_and(|responder| {
                Retained::as_ptr(&responder).cast::<c_void>() == render_view_pointer.as_ptr()
            });
        json!({
            "app_frontmost": app.isActive(),
            "key_window": window.as_ref().is_some_and(|window| window.isKeyWindow()),
            "render_view_first_responder": render_view_first_responder,
        })
    }

    fn emit_focus_state(&self) {
        if !preflight::active() {
            return;
        }
        preflight::emit(
            "input.focus.state",
            json!({
                "focus": self.focus_snapshot(),
                "focused_pane": self.focused_pane(),
                "window_id": self.ivars().window_id.get(),
            }),
        );
    }

    fn focused_pane(&self) -> PaneId {
        self.ivars().ui.borrow().zoom.current().focused
    }

    fn zoom_state(&self) -> ZoomState {
        self.ivars().ui.borrow().zoom.state().clone()
    }

    fn toggle_zoom(&self, target: PaneId) {
        let (outcome, topology) = {
            let mut ui = self.ivars().ui.borrow_mut();
            ui.zoom.set_focus(target);
            let outcome = ui.zoom.toggle(Some(target));
            let topology = format!("{:?}", ui.zoom.current());
            ui.dirty = true;
            (outcome, topology)
        };
        eprintln!("event=layout.zoom outcome={outcome:?}");
        match outcome {
            ZoomOutcome::Entered(target) => preflight::emit(
                "zoom.entered",
                json!({
                    "target": target,
                    "before_topology_hash": topology,
                    "window_id": self.ivars().window_id.get(),
                }),
            ),
            ZoomOutcome::Restored(target) => preflight::emit(
                "zoom.restored",
                json!({
                    "target": target,
                    "after_topology_hash": topology,
                    "window_id": self.ivars().window_id.get(),
                }),
            ),
            _ => {}
        }
        self.update_accessibility_tree();
        self.setNeedsDisplay(true);
    }

    fn new(
        mtm: MainThreadMarker,
        frame: NSRect,
        pty: PtySession,
        options: Options,
        started_at: Instant,
    ) -> Result<Retained<Self>> {
        let action_listener = PreflightActionListener::bind(
            options.preflight_action_socket.as_deref(),
            options.preflight_phase.as_deref(),
        )?;
        let this = Self::alloc(mtm).set_ivars(ViewIvars {
            renderer: RefCell::new(None),
            pty,
            ui: RefCell::new(UiState::new(options.browser_enabled, options.visual_state)),
            options,
            action_listener: RefCell::new(action_listener),
            armed_action: RefCell::new(None),
            started_at,
            first_presented_at: Cell::new(None),
            window_id: Cell::new(0),
            scale_factor: Cell::new(1.0),
            tick_count: Cell::new(0),
            probe_index: Cell::new(0),
            probe_is_marked: Cell::new(false),
            input_method_handled: Cell::new(false),
            browser_ready_emitted: Cell::new(false),
            close_requested: Cell::new(false),
            relaunch_state_hash: RefCell::new(None),
        });
        Ok(unsafe { msg_send![super(this), initWithFrame: frame] })
    }

    fn initialize_renderer(&self) -> Result<()> {
        self.setWantsLayer(true);
        let (width, height, scale_factor) = self.backing_dimensions();
        let pointer = NonNull::from(self).cast::<c_void>();
        let renderer = unsafe { Renderer::new(pointer, width, height, scale_factor) }?;
        self.ivars().scale_factor.set(scale_factor);
        self.ivars().renderer.replace(Some(renderer));
        let geometry =
            CanvasGeometry::for_size(self.bounds().size.width, self.bounds().size.height);
        self.ivars()
            .pty
            .resize(geometry.canvas_width as u32, geometry.canvas_height as u32)?;
        self.update_accessibility_tree();
        self.setNeedsDisplay(true);
        Ok(())
    }

    fn shutdown_owned_resources(&self) {
        self.ivars().pty.shutdown();
        let _ = self.ivars().action_listener.borrow_mut().take();
    }

    fn poll_preflight_action(&self) -> Result<()> {
        let action = {
            let listener = self.ivars().action_listener.borrow();
            listener
                .as_ref()
                .map(PreflightActionListener::poll)
                .transpose()?
                .flatten()
        };
        let Some(action) = action else {
            return Ok(());
        };
        if self.ivars().armed_action.borrow().is_some() {
            return Err(anyhow!(
                "stage=preflight.action_socket.contract cause=action-already-armed retryable=false"
            ));
        }
        preflight::emit("input.action.armed", action.armed_details());
        self.ivars().armed_action.replace(Some(action));
        Ok(())
    }

    fn take_preflight_action(
        &self,
        key_code: u16,
        flags: NSEventModifierFlags,
    ) -> Option<PreflightAction> {
        let mut armed = self.ivars().armed_action.borrow_mut();
        if armed
            .as_ref()
            .is_some_and(|action| is_plain_key_control(Some(&action.action), key_code, flags))
        {
            armed.take()
        } else {
            None
        }
    }

    fn record_preflight_action_failure(&self, error: anyhow::Error) {
        let message = format!("Input action failed: {error:#}");
        self.ivars().ui.borrow_mut().visible_failure = Some(message.clone());
        preflight::emit(
            "input.action.failed",
            json!({
                "input_contract": "scenario-action",
                "error": message,
            }),
        );
        eprintln!("event=input.action.failed retryable=false error={message:?}");
        self.setNeedsDisplay(true);
    }

    fn backing_dimensions(&self) -> (u32, u32, f64) {
        let backing = self.convertRectToBacking(self.bounds());
        let scale = self
            .window()
            .map(|window| window.backingScaleFactor())
            .unwrap_or(1.0);
        (
            backing.size.width.round().max(1.0) as u32,
            backing.size.height.round().max(1.0) as u32,
            scale,
        )
    }

    fn render_frame(&self) -> Result<()> {
        let (width, height, scale_factor) = self.backing_dimensions();
        let terminal_snapshot = self.ivars().pty.snapshot();
        let terminal = terminal_snapshot
            .lock()
            .map_err(|_| anyhow!("stage=terminal.snapshot cause=poisoned-lock retryable=false"))?;
        let model = self.ivars().ui.borrow().model(&terminal);
        let visible_terminal = visible_transcript(&terminal.transcript, 128);
        drop(terminal);

        let observation = {
            let mut renderer = self.ivars().renderer.borrow_mut();
            let renderer = renderer
                .as_mut()
                .ok_or_else(|| anyhow!("stage=wgpu.render cause=renderer-not-initialized"))?;
            renderer.resize(width, height, scale_factor);
            renderer.render(&model)?
        };
        self.after_present(observation, scale_factor, &visible_terminal)
    }

    fn after_present(
        &self,
        observation: PresentObservation,
        scale_factor: f64,
        visible_terminal: &str,
    ) -> Result<()> {
        let now = Instant::now();
        if self.ivars().first_presented_at.get().is_none() {
            self.ivars().first_presented_at.set(Some(now));
            eprintln!(
                "event=app.first-usable first_usable_ms={:.3} window_id={} adapter={:?}",
                now.duration_since(self.ivars().started_at).as_secs_f64() * 1000.0,
                self.ivars().window_id.get(),
                observation.adapter_name
            );
            preflight::emit(
                "app.usable",
                json!({
                    "first_usable_ms": now.duration_since(self.ivars().started_at).as_secs_f64() * 1000.0,
                    "window_id": self.ivars().window_id.get(),
                    "adapter": observation.adapter_name,
                    "scale_factor": scale_factor,
                    "launch_mode": if self.ivars().options.browser_enabled { "browser-included" } else { "browser-closed" },
                    "cef_initialized": self.ivars().options.browser_enabled,
                }),
            );
            self.emit_browser_ready_if_needed();
            self.emit_fixture_ready();
            if let Some(state_hash) = self.ivars().relaunch_state_hash.borrow_mut().take() {
                preflight::emit("relaunch.restored", json!({ "state_hash": state_hash }));
            }
        }
        {
            let mut ui = self.ivars().ui.borrow_mut();
            let presented_probes =
                ui.note_presented(observation.input_generation, now, visible_terminal);
            ui.dirty = false;
            drop(ui);
            let present_ns = preflight::monotonic_ns();
            for (probe, input_monotonic_ns) in presented_probes {
                preflight::emit(
                    "terminal.input_presented",
                    json!({
                        "probe": probe,
                        "input_monotonic_ns": input_monotonic_ns,
                        "present_monotonic_ns": present_ns,
                    }),
                );
            }
        }
        self.ivars().scale_factor.set(scale_factor);
        self.write_report(&observation)
    }

    fn tick_impl(&self) {
        let tick = self.ivars().tick_count.get() + 1;
        self.ivars().tick_count.set(tick);
        if let Err(error) = self.poll_preflight_action() {
            self.record_preflight_action_failure(error);
        }
        self.emit_focus_state();
        self.sync_terminal_generation();
        self.run_probe_if_due(tick);
        if self.ivars().ui.borrow().dirty {
            self.setNeedsDisplay(true);
        }
        if let Some(limit) = self.ivars().options.autoclose_after
            && self.ivars().started_at.elapsed() >= limit
            && !self.ivars().close_requested.replace(true)
        {
            eprintln!("event=app.autoclose elapsed_ms={}", limit.as_millis());
            request_orderly_quit();
        }
    }

    fn sync_terminal_generation(&self) {
        let snapshot = self.ivars().pty.snapshot();
        let Ok(snapshot) = snapshot.lock() else {
            let mut ui = self.ivars().ui.borrow_mut();
            ui.visible_failure = Some("PTY state unavailable: poisoned lock".to_owned());
            ui.dirty = true;
            return;
        };
        let mut ui = self.ivars().ui.borrow_mut();
        if snapshot.dirty_generation != ui.last_terminal_generation {
            ui.last_terminal_generation = snapshot.dirty_generation;
            ui.dirty = true;
        }
    }

    fn run_probe_if_due(&self, tick: u64) {
        let index = self.ivars().probe_index.get();
        if index >= self.ivars().options.probe_count || tick < 8 || !tick.is_multiple_of(2) {
            return;
        }
        let replacement = not_found_range();
        if !self.ivars().probe_is_marked.get() {
            let text = NSString::from_str(&format!("한글조합-{:02}", index + 1));
            let _selected_range = NSRange::new(text.length(), 0);
            let _replacement_range = replacement;
            let mut ui = self.ivars().ui.borrow_mut();
            ui.ime_marked = text.to_string();
            ui.dirty = true;
            self.ivars().probe_is_marked.set(true);
        } else {
            let text = NSString::from_str(&format!("한글입력-{:02}", index + 1));
            let _replacement_range = replacement;
            self.commit_text(&text.to_string());
            self.commit_text("\r");
            self.ivars().probe_index.set(index + 1);
            self.ivars().probe_is_marked.set(false);
        }
    }

    fn commit_text(&self, text: &str) {
        let focused = self.ivars().ui.borrow().zoom.current().focused;
        let input_monotonic_ns = preflight::monotonic_ns();
        let result = match focused {
            PaneId::TerminalA | PaneId::TerminalB => self.ivars().pty.write_text(text),
            PaneId::Editor => {
                self.ivars().ui.borrow_mut().editor_text.push_str(text);
                Ok(())
            }
            PaneId::Browser => Err(anyhow!(
                "stage=input.route target=browser cause=browser-owned-by-cef retryable=false"
            )),
        };
        let mut ui = self.ivars().ui.borrow_mut();
        ui.ime_marked.clear();
        ui.ime_committed = match text {
            "\r" | "\n" => "Return".to_owned(),
            "\u{7f}" => "Delete".to_owned(),
            _ => text.to_owned(),
        };
        ui.input_generation += 1;
        let generation = ui.input_generation;
        ui.pending_input.push_back((generation, Instant::now()));
        if matches!(focused, PaneId::TerminalA | PaneId::TerminalB) {
            match text {
                "\r" | "\n" => {
                    let line = std::mem::take(&mut ui.terminal_input_line);
                    let is_probe = line.starts_with(T1_LATENCY_PROBE_PREFIX);
                    if is_probe {
                        ui.pending_probes
                            .push_back((generation, line, input_monotonic_ns));
                    }
                }
                "\u{7f}" => {
                    ui.terminal_input_line.pop();
                }
                _ => {
                    ui.terminal_input_line.push_str(text);
                }
            }
        }
        let ime_committed =
            !text.is_ascii() && ui.ime_marked_observed && !text.starts_with("T1PROBE");
        let marked_observed = ui.ime_marked_observed;
        if ime_committed {
            ui.ime_marked_observed = false;
        }
        ui.visible_failure = result.err().map(|error| format!("Input failed: {error:#}"));
        ui.dirty = true;
        drop(ui);
        if ime_committed {
            preflight::emit(
                "ime.committed",
                json!({ "text": text, "marked_observed": marked_observed }),
            );
            if self.ivars().options.preflight_phase.as_deref() == Some("warm_closed") {
                match self.persist_preflight_state() {
                    Ok(state_hash) => {
                        preflight::emit("state.persisted", json!({ "state_hash": state_hash }));
                    }
                    Err(error) => {
                        eprintln!("event=state.persist.failed retryable=false error={error:#?}")
                    }
                }
            }
        }
        self.setNeedsDisplay(true);
    }

    fn commit_option_meta(&self, key_code: u16, key: &str) {
        let focused = self.ivars().ui.borrow().zoom.current().focused;
        let input_monotonic_ns = preflight::monotonic_ns();
        let focus = self.focus_snapshot();
        let bytes = [0x1b_u8, key.as_bytes().first().copied().unwrap_or_default()];
        let bytes_hex = format!("{:02x} {:02x}", bytes[0], bytes[1]);
        preflight::emit(
            "input.option_meta.appkit",
            json!({
                "source": "appkit",
                "key_code": key_code,
                "key": key,
                "modifiers": ["option"],
                "focus": focus,
                "input_monotonic_ns": input_monotonic_ns,
            }),
        );

        let route_result = match focused {
            PaneId::TerminalA | PaneId::TerminalB => {
                preflight::emit(
                    "input.option_meta.app-routing",
                    json!({
                        "source": "app-routing",
                        "target": "terminal",
                        "key_code": key_code,
                        "key": key,
                        "modifiers": ["option"],
                        "bytes_hex": bytes_hex,
                        "focus": self.focus_snapshot(),
                        "input_monotonic_ns": input_monotonic_ns,
                    }),
                );
                preflight::emit(
                    "input.option_meta.herdr",
                    json!({
                        "source": "herdr",
                        "target": format!("{:?}", focused),
                        "key_code": key_code,
                        "key": key,
                        "modifiers": ["option"],
                        "bytes_hex": bytes_hex,
                        "focus": self.focus_snapshot(),
                        "input_monotonic_ns": input_monotonic_ns,
                    }),
                );
                self.ivars().pty.write_bytes(&bytes)
            }
            PaneId::Editor | PaneId::Browser => Err(anyhow!(
                "stage=input.route target={focused:?} cause=option-meta-requires-terminal retryable=false"
            )),
        };
        let pty_monotonic_ns = preflight::monotonic_ns();
        let status = if route_result.is_ok() {
            "accepted"
        } else {
            "rejected"
        };
        let error = route_result.as_ref().err().map(ToString::to_string);
        {
            let mut ui = self.ivars().ui.borrow_mut();
            ui.input_generation += 1;
            let generation = ui.input_generation;
            ui.pending_input.push_back((generation, Instant::now()));
            ui.visible_failure = error.clone();
            ui.dirty = true;
        }
        preflight::emit(
            "input.option_meta.pty",
            json!({
                "source": "pty",
                "status": status,
                "key_code": key_code,
                "key": key,
                "modifiers": ["option"],
                "bytes_hex": bytes_hex,
                "focus": self.focus_snapshot(),
                "input_monotonic_ns": input_monotonic_ns,
                "pty_monotonic_ns": pty_monotonic_ns,
                "error": error,
            }),
        );
        self.setNeedsDisplay(true);
    }

    fn commit_plain_key_control(&self, key_code: u16, key: &str) {
        let focused = self.ivars().ui.borrow().zoom.current().focused;
        let input_monotonic_ns = preflight::monotonic_ns();
        let focus = self.focus_snapshot();
        let bytes = [key.as_bytes().first().copied().unwrap_or_default()];
        let bytes_hex = format!("{:02x}", bytes[0]);
        preflight::emit(
            "input.plain_key_control.appkit",
            json!({
                "source": "appkit",
                "key_code": key_code,
                "key": key,
                "modifiers": [],
                "focus": focus,
                "input_monotonic_ns": input_monotonic_ns,
            }),
        );

        let route_result = match focused {
            PaneId::TerminalA | PaneId::TerminalB => {
                preflight::emit(
                    "input.plain_key_control.app-routing",
                    json!({
                        "source": "app-routing",
                        "target": "terminal",
                        "key_code": key_code,
                        "key": key,
                        "modifiers": [],
                        "bytes_hex": bytes_hex,
                        "focus": self.focus_snapshot(),
                        "input_monotonic_ns": input_monotonic_ns,
                    }),
                );
                preflight::emit(
                    "input.plain_key_control.herdr",
                    json!({
                        "source": "herdr",
                        "target": format!("{:?}", focused),
                        "key_code": key_code,
                        "key": key,
                        "modifiers": [],
                        "bytes_hex": bytes_hex,
                        "focus": self.focus_snapshot(),
                        "input_monotonic_ns": input_monotonic_ns,
                    }),
                );
                self.ivars().pty.write_bytes(&bytes)
            }
            PaneId::Editor | PaneId::Browser => Err(anyhow!(
                "stage=input.route target={focused:?} cause=plain-key-control-requires-terminal retryable=false"
            )),
        };
        let pty_monotonic_ns = preflight::monotonic_ns();
        let status = if route_result.is_ok() {
            "accepted"
        } else {
            "rejected"
        };
        let error = route_result.as_ref().err().map(ToString::to_string);
        {
            let mut ui = self.ivars().ui.borrow_mut();
            ui.input_generation += 1;
            let generation = ui.input_generation;
            ui.pending_input.push_back((generation, Instant::now()));
            ui.visible_failure = error.clone();
            ui.dirty = true;
        }
        preflight::emit(
            "input.plain_key_control.pty",
            json!({
                "source": "pty",
                "status": status,
                "key_code": key_code,
                "key": key,
                "modifiers": [],
                "bytes_hex": bytes_hex,
                "focus": self.focus_snapshot(),
                "input_monotonic_ns": input_monotonic_ns,
                "pty_monotonic_ns": pty_monotonic_ns,
                "error": error,
            }),
        );
        self.setNeedsDisplay(true);
    }

    fn write_report(&self, observation: &PresentObservation) -> Result<()> {
        let Some(path) = &self.ivars().options.report_path else {
            return Ok(());
        };
        let first = self
            .ivars()
            .first_presented_at
            .get()
            .ok_or_else(|| anyhow!("stage=report.write cause=first-present-missing"))?;
        let ui = self.ivars().ui.borrow();
        let report = RuntimeReport {
            schema: "herdr.native-spike.runtime.v1",
            build: concat!(env!("CARGO_PKG_NAME"), "@", env!("CARGO_PKG_VERSION")),
            architecture: std::env::consts::ARCH,
            process_id: std::process::id(),
            pty_child_process_id: self.ivars().pty.process_id(),
            window_id: self.ivars().window_id.get(),
            adapter: observation.adapter_name.clone(),
            physical_width: observation.width,
            physical_height: observation.height,
            scale_factor: self.ivars().scale_factor.get(),
            first_usable_ms: first.duration_since(self.ivars().started_at).as_secs_f64() * 1000.0,
            input_samples: ui.input_to_present_ms.len(),
            input_to_present_p95_ms: percentile_95(&ui.input_to_present_ms),
            input_to_present_max_ms: ui.input_to_present_ms.iter().copied().reduce(f64::max),
            zoom_state: ui.zoom.indicator(),
            ax_children: [
                "Workspaces navigator",
                "Native spike tab",
                "Terminal pane A",
                "Editor pane B",
            ],
            ime_contract: "NSTextInputClient marked-text and commit selectors route UTF-8 to WGPU text or the PTY",
            pty_contract: "portable-pty master/slave runs /bin/zsh -f with explicit UTF-8 terminal environment",
        };
        drop(ui);
        write_json_atomic(path, &report)
    }

    fn emit_fixture_ready(&self) {
        let bounds = self.bounds();
        let geometry = CanvasGeometry::for_size(bounds.size.width, bounds.size.height);
        let canvas_width = geometry.canvas_width;
        let canvas_height = geometry.canvas_height;
        let terminal = self.cg_screen_point(NSPoint::new(
            geometry.canvas_left + canvas_width * 0.25,
            geometry.canvas_top + canvas_height * 0.5,
        ));
        let editor = self.cg_screen_point(NSPoint::new(
            geometry.canvas_left + canvas_width * 0.72,
            geometry.canvas_top + canvas_height * 0.5,
        ));
        let browser = self.cg_screen_point(NSPoint::new(
            geometry.canvas_left + canvas_width * 0.84,
            geometry.canvas_top + canvas_height * 0.35,
        ));
        preflight::emit(
            "fixture.ready",
            json!({
                "workspace_count": FIXTURE_WORKSPACE_COUNT,
                "pane_count": FIXTURE_PANE_COUNT,
                "browser_open": crate::browser::browser_is_open(),
                "cef_initialized": self.ivars().options.browser_enabled,
                "window_id": self.ivars().window_id.get(),
                "focused_pane": self.focused_pane(),
                "focus": self.focus_snapshot(),
                "cg_screen_points": {
                    "terminal": { "x": terminal.0, "y": terminal.1 },
                    "editor": { "x": editor.0, "y": editor.1 },
                    "browser": { "x": browser.0, "y": browser.1 },
                },
            }),
        );
    }

    fn emit_browser_ready_if_needed(&self) {
        if !self.ivars().options.browser_enabled || !crate::browser::browser_is_open() {
            return;
        }
        let Some(config) = crate::browser::current_config() else {
            return;
        };
        if self.ivars().browser_ready_emitted.replace(true) {
            return;
        }
        let window_id = self.ivars().window_id.get();
        preflight::emit(
            "browser.opened",
            json!({ "url": config.url, "browser_open": true }),
        );
        preflight::emit(
            "browser.profile.ready",
            json!({
                "path": config.profile_dir,
                "persistent": true,
                "browser_open": true,
                "cef_initialized": true,
                "window_id": window_id,
                "cdp_http_endpoint": format!("http://127.0.0.1:{}", config.remote_debugging_port),
            }),
        );
    }

    fn preflight_state_path(&self) -> Result<PathBuf> {
        let events = self
            .ivars()
            .options
            .preflight_events
            .as_ref()
            .ok_or_else(|| anyhow!("stage=state.path cause=preflight-events-missing"))?;
        let parent = events
            .parent()
            .ok_or_else(|| anyhow!("stage=state.path cause=events-parent-missing"))?;
        Ok(parent.join("workspace-state.json"))
    }

    fn persist_preflight_state(&self) -> Result<String> {
        let ui = self.ivars().ui.borrow();
        let (logical_width, logical_height) = self
            .window()
            .and_then(|window| window.contentView())
            .map(|view| (view.bounds().size.width, view.bounds().size.height))
            .unwrap_or((WINDOW_WIDTH, WINDOW_HEIGHT));
        let state = json!({
            "schema": "herdr.integrated-preflight.state.v1",
            "focused_pane": format!("{:?}", ui.zoom.current().focused),
            "split_ratios": ui.zoom.current().ratios,
            "editor_text": ui.editor_text,
            "logical_width": logical_width,
            "logical_height": logical_height,
        });
        drop(ui);
        let path = self.preflight_state_path()?;
        write_json_atomic(&path, &state)?;
        let bytes = std::fs::read(&path)
            .with_context(|| format!("stage=state.hash path={}", path.display()))?;
        Ok(stable_state_hash(&bytes))
    }

    fn restore_preflight_state(&self) -> Result<String> {
        let path = self.preflight_state_path()?;
        let bytes = std::fs::read(&path)
            .with_context(|| format!("stage=state.restore.read path={}", path.display()))?;
        let state: serde_json::Value = serde_json::from_slice(&bytes)
            .with_context(|| format!("stage=state.restore.parse path={}", path.display()))?;
        if state.get("schema").and_then(serde_json::Value::as_str)
            != Some("herdr.integrated-preflight.state.v1")
        {
            return Err(anyhow!(
                "stage=state.restore cause=unsupported-schema path={}",
                path.display()
            ));
        }
        let focused = match state
            .get("focused_pane")
            .and_then(serde_json::Value::as_str)
        {
            Some("TerminalA") => PaneId::TerminalA,
            Some("TerminalB") => PaneId::TerminalB,
            Some("Editor") => PaneId::Editor,
            Some("Browser") => PaneId::Browser,
            other => {
                return Err(anyhow!(
                    "stage=state.restore field=focused_pane cause=invalid-value value={other:?}"
                ));
            }
        };
        let ratios = state
            .get("split_ratios")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| anyhow!("stage=state.restore field=split_ratios cause=missing"))?
            .iter()
            .map(|value| {
                value
                    .as_f64()
                    .map(|value| value as f32)
                    .ok_or_else(|| anyhow!("stage=state.restore field=split_ratios cause=invalid"))
            })
            .collect::<Result<Vec<_>>>()?;
        let editor_text = state
            .get("editor_text")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| anyhow!("stage=state.restore field=editor_text cause=missing"))?
            .to_owned();
        {
            let mut ui = self.ivars().ui.borrow_mut();
            ui.zoom = ZoomController::new(ratios, focused);
            ui.editor_text = editor_text;
            ui.dirty = true;
        }
        if let (Some(width), Some(height), Some(window)) = (
            state
                .get("logical_width")
                .and_then(serde_json::Value::as_f64),
            state
                .get("logical_height")
                .and_then(serde_json::Value::as_f64),
            self.window(),
        ) {
            window.setContentSize(NSSize::new(width, height));
        }
        Ok(stable_state_hash(&bytes))
    }

    fn cg_screen_point(&self, local: NSPoint) -> (f64, f64) {
        let Some(window) = self.window() else {
            return (0.0, 0.0);
        };
        let window_point = self.convertPoint_toView(local, None);
        let screen_point = window.convertPointToScreen(window_point);
        let primary_height = NSScreen::mainScreen(self.mtm())
            .map(|screen| screen.frame().size.height)
            .unwrap_or(0.0);
        (screen_point.x, primary_height - screen_point.y)
    }

    fn update_accessibility_tree(&self) {
        let bounds = self.bounds();
        let geometry = CanvasGeometry::for_size(bounds.size.width, bounds.size.height);
        let navigator_width = geometry.navigator_width;
        let canvas_width = geometry.canvas_width;
        let split = geometry.split_x;
        let tab_height = geometry.tab_height;
        let canvas_height = geometry.canvas_height;
        let parent: &AnyObject = self;
        let navigator = unsafe {
            NSAccessibilityElement::accessibilityElementWithRole_frame_label_parent(
                NSAccessibilityListRole,
                NSRect::new(
                    NSPoint::new(0.0, 0.0),
                    NSSize::new(navigator_width, bounds.size.height),
                ),
                Some(ns_string!("Workspaces navigator")),
                Some(parent),
            )
        };
        let tab = unsafe {
            NSAccessibilityElement::accessibilityElementWithRole_frame_label_parent(
                NSAccessibilityTabGroupRole,
                NSRect::new(
                    NSPoint::new(navigator_width, 0.0),
                    NSSize::new(canvas_width, tab_height),
                ),
                Some(ns_string!("Native spike tab")),
                Some(parent),
            )
        };
        let terminal = unsafe {
            NSAccessibilityElement::accessibilityElementWithRole_frame_label_parent(
                NSAccessibilityTextAreaRole,
                NSRect::new(
                    NSPoint::new(navigator_width, tab_height),
                    NSSize::new(split - navigator_width, canvas_height),
                ),
                Some(ns_string!("Terminal pane A")),
                Some(parent),
            )
        };
        let editor = unsafe {
            NSAccessibilityElement::accessibilityElementWithRole_frame_label_parent(
                NSAccessibilityTextAreaRole,
                NSRect::new(
                    NSPoint::new(split, tab_height),
                    NSSize::new(bounds.size.width - split, canvas_height),
                ),
                Some(ns_string!("Editor pane B")),
                Some(parent),
            )
        };
        let ui = self.ivars().ui.borrow();
        let zoom_value = NSString::from_str(ui.zoom.indicator());
        unsafe {
            let _: () = msg_send![&*tab, setAccessibilityValue: &*zoom_value];
            let _: () = msg_send![&*terminal, setAccessibilityFocused: ui.zoom.current().focused == PaneId::TerminalA];
            let _: () = msg_send![&*editor, setAccessibilityFocused: ui.zoom.current().focused == PaneId::Editor];
        }
        let children = match ui.zoom.state() {
            ZoomState::Normal => NSArray::from_slice(&[&*navigator, &*tab, &*terminal, &*editor]),
            ZoomState::Zoomed {
                target: PaneId::Editor,
                ..
            } => NSArray::from_slice(&[&*navigator, &*tab, &*editor]),
            ZoomState::Zoomed { .. } => NSArray::from_slice(&[&*navigator, &*tab, &*terminal]),
        };
        drop(ui);
        unsafe {
            let _: () = msg_send![self, setAccessibilityElement: false];
            let _: () = msg_send![self, setAccessibilityRole: NSAccessibilityGroupRole];
            let _: () = msg_send![self, setAccessibilityChildren: &*children];
        }
    }
}

struct WorkspaceHandles {
    root: Retained<NSView>,
    render: Retained<RenderView>,
    browser_parent: Retained<NSView>,
    browser_enabled: bool,
}

thread_local! {
    static WORKSPACE: RefCell<Option<WorkspaceHandles>> = const { RefCell::new(None) };
}

pub fn is_zoom_shortcut(key_code: u16, flags: NSEventModifierFlags) -> bool {
    flags.contains(NSEventModifierFlags::Command)
        && flags.contains(NSEventModifierFlags::Shift)
        && matches!(key_code, 36 | 76)
}

pub fn is_quit_shortcut(key_code: u16, flags: NSEventModifierFlags) -> bool {
    key_code == 12
        && flags.contains(NSEventModifierFlags::Command)
        && !flags.contains(NSEventModifierFlags::Shift)
}

pub fn request_orderly_quit() {
    let browser_enabled = WORKSPACE.with(|workspace| {
        workspace
            .borrow()
            .as_ref()
            .is_some_and(|workspace| workspace.browser_enabled)
    });
    preflight::complete(browser_enabled);
    let Some(mtm) = MainThreadMarker::new() else {
        eprintln!("event=app.quit.failed cause=not-main-thread retryable=false");
        return;
    };
    if browser_enabled {
        crate::browser::request_app_quit();
    } else {
        NSApplication::sharedApplication(mtm).terminate(None);
    }
}

pub fn route_zoom_shortcut() {
    WORKSPACE.with(|workspace| {
        let workspace = workspace.borrow();
        let Some(workspace) = workspace.as_ref() else {
            eprintln!("event=layout.zoom.unavailable cause=workspace-not-ready");
            return;
        };
        let focused = if workspace.browser_enabled && crate::browser::owns_first_responder() {
            PaneId::Browser
        } else {
            workspace.render.focused_pane()
        };
        workspace.render.toggle_zoom(focused);
    });
    apply_workspace_layout();
}

pub fn browser_ready() {
    apply_workspace_layout();
    WORKSPACE.with(|workspace| {
        if let Some(workspace) = workspace.borrow().as_ref()
            && workspace.render.ivars().first_presented_at.get().is_some()
        {
            workspace.render.emit_browser_ready_if_needed();
        }
    });
}

pub fn stop_native_message_loop() {
    let Some(mtm) = MainThreadMarker::new() else {
        eprintln!("event=appkit.message-loop.stop failed=not-main-thread retryable=false");
        return;
    };
    NSApp(mtm).stop(None);
    eprintln!("event=appkit.message-loop.stop requested=true");
}

fn apply_workspace_layout() {
    WORKSPACE.with(|workspace| {
        let workspace = workspace.borrow();
        let Some(workspace) = workspace.as_ref() else {
            return;
        };
        let bounds = workspace.root.bounds();
        workspace.render.setFrame(bounds);
        workspace.render.setHidden(false);
        let zoom = workspace.render.zoom_state();
        match browser_frame(
            bounds.size.width,
            bounds.size.height,
            workspace.browser_enabled,
            &zoom,
        ) {
            Some(frame) => {
                workspace.browser_parent.setFrame(NSRect::new(
                    NSPoint::new(frame.x, frame.y),
                    NSSize::new(frame.width, frame.height),
                ));
                workspace.browser_parent.setHidden(false);
                crate::browser::layout_native_child();
                if matches!(
                    zoom,
                    ZoomState::Zoomed {
                        target: PaneId::Browser,
                        ..
                    }
                ) {
                    let focused = crate::browser::focus_browser();
                    preflight::emit(
                        "focus.changed",
                        json!({ "from": "wgpu", "to": "browser", "accepted": focused, "focused_pane": "Browser" }),
                    );
                }
            }
            None => {
                workspace.browser_parent.setHidden(true);
                crate::browser::blur_browser();
                if !matches!(zoom, ZoomState::Normal)
                    && let Some(window) = workspace.render.window()
                {
                    let accepted = window.makeFirstResponder(Some(&workspace.render));
                    preflight::emit(
                        "focus.changed",
                        json!({ "from": "browser", "to": workspace.render.focused_pane(), "accepted": accepted, "focused_pane": workspace.render.focused_pane() }),
                    );
                }
            }
        }
        workspace.render.setNeedsDisplay(true);
    });
}

struct AppDelegateIvars {
    window: OnceCell<Retained<NSWindow>>,
    root: OnceCell<Retained<NSView>>,
    view: OnceCell<Retained<RenderView>>,
    browser_parent: OnceCell<Retained<NSView>>,
    timer: OnceCell<Retained<NSTimer>>,
    options: Options,
    started_at: Instant,
}

define_class!(
    // SAFETY:
    // - NSObject has no additional subclassing invariants.
    // - AppKit only invokes this delegate on the main thread.
    // - Retained window, view, and timer values share the delegate lifetime.
    #[unsafe(super = NSObject)]
    #[thread_kind = MainThreadOnly]
    #[ivars = AppDelegateIvars]
    struct AppDelegate;

    // SAFETY: NSObjectProtocol has no additional safety requirements.
    unsafe impl NSObjectProtocol for AppDelegate {}

    // SAFETY: The implemented selector has the generated protocol signature.
    unsafe impl NSApplicationDelegate for AppDelegate {
        #[unsafe(method(applicationDidFinishLaunching:))]
        fn application_did_finish_launching(&self, notification: &NSNotification) {
            let app = notification
                .object()
                .and_then(|object| object.downcast::<NSApplication>().ok())
                .unwrap_or_else(|| NSApplication::sharedApplication(self.mtm()));
            if let Err(error) = self.launch(&app) {
                eprintln!(
                    "event=app.launch.failed stage=native-shell retryable=false error={error:#?}"
                );
                app.terminate(None);
            }
        }

        #[unsafe(method(applicationDidBecomeActive:))]
        fn application_did_become_active(&self, _notification: &NSNotification) {
            preflight::emit(
                "app.lifecycle.activate",
                json!({ "source": "NSApplicationDelegate", "active": true }),
            );
        }

        #[unsafe(method(applicationDidResignActive:))]
        fn application_did_resign_active(&self, _notification: &NSNotification) {
            preflight::emit(
                "app.lifecycle.resign",
                json!({ "source": "NSApplicationDelegate", "active": false }),
            );
        }
    }

    // SAFETY: The implemented selector has the generated protocol signature.
    unsafe impl NSWindowDelegate for AppDelegate {
        #[unsafe(method(windowShouldClose:))]
        fn window_should_close(&self, _sender: &NSWindow) -> bool {
            if !self.ivars().options.browser_enabled || crate::browser::is_closing() {
                true
            } else {
                crate::browser::request_app_quit();
                false
            }
        }

        #[unsafe(method(windowWillClose:))]
        fn window_will_close(&self, _notification: &NSNotification) {
            if self.ivars().options.browser_enabled {
                crate::browser::detach_native_child_for_window_close();
                eprintln!("event=browser.top-level-window.closed");
            } else {
                NSApplication::sharedApplication(self.mtm()).terminate(None);
            }
        }

        #[unsafe(method(windowDidResize:))]
        fn window_did_resize(&self, _notification: &NSNotification) {
            apply_workspace_layout();
            if let Some(window) = self.ivars().window.get() {
                let size = window.contentView().map(|view| view.bounds().size);
                if let Some(size) = size {
                    preflight::emit(
                        "window.resized",
                        json!({
                            "logical_width": size.width,
                            "logical_height": size.height,
                            "physical_width": size.width * window.backingScaleFactor(),
                            "physical_height": size.height * window.backingScaleFactor(),
                            "scale_factor": window.backingScaleFactor(),
                        }),
                    );
                }
            }
        }

        #[unsafe(method(windowDidBecomeKey:))]
        fn window_did_become_key(&self, _notification: &NSNotification) {
            preflight::emit(
                "window.lifecycle.key",
                json!({
                    "source": "NSWindowDelegate",
                    "window_id": self.ivars().window.get().map(|window| window.windowNumber()),
                    "key": true,
                }),
            );
        }

        #[unsafe(method(windowDidResignKey:))]
        fn window_did_resign_key(&self, _notification: &NSNotification) {
            preflight::emit(
                "window.lifecycle.resign",
                json!({
                    "source": "NSWindowDelegate",
                    "window_id": self.ivars().window.get().map(|window| window.windowNumber()),
                    "key": false,
                }),
            );
        }
    }
);

impl AppDelegate {
    fn new(mtm: MainThreadMarker, options: Options, started_at: Instant) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(AppDelegateIvars {
            window: OnceCell::new(),
            root: OnceCell::new(),
            view: OnceCell::new(),
            browser_parent: OnceCell::new(),
            timer: OnceCell::new(),
            options,
            started_at,
        });
        unsafe { msg_send![super(this), init] }
    }

    fn launch(&self, app: &NSApplication) -> Result<()> {
        let mtm = self.mtm();
        let frame = NSRect::new(
            NSPoint::new(0.0, 0.0),
            NSSize::new(WINDOW_WIDTH, WINDOW_HEIGHT),
        );
        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(mtm),
                frame,
                NSWindowStyleMask::Titled
                    | NSWindowStyleMask::Closable
                    | NSWindowStyleMask::Miniaturizable
                    | NSWindowStyleMask::Resizable,
                NSBackingStoreType::Buffered,
                false,
            )
        };
        unsafe { window.setReleasedWhenClosed(false) };
        window.setTitle(ns_string!("Herdr Integrated Preflight"));
        window.center();
        window.setContentMinSize(NSSize::new(760.0, 480.0));
        window.setDelegate(Some(ProtocolObject::from_ref(self)));

        let pty = PtySession::spawn()?;
        preflight::emit(
            "terminal.ready",
            json!({
                "pty": "portable-pty",
                "child_pid": pty.process_id(),
                "shell": "/bin/zsh -f",
            }),
        );
        let view = RenderView::new(
            mtm,
            NSRect::new(
                NSPoint::new(0.0, 0.0),
                NSSize::new(WINDOW_WIDTH, WINDOW_HEIGHT),
            ),
            pty,
            self.ivars().options.clone(),
            self.ivars().started_at,
        )?;
        view.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewWidthSizable
                | NSAutoresizingMaskOptions::ViewHeightSizable,
        );
        let root = NSView::initWithFrame(NSView::alloc(mtm), frame);
        let browser_parent = NSView::initWithFrame(NSView::alloc(mtm), frame);
        root.addSubview(&view);
        root.addSubview_positioned_relativeTo(
            &browser_parent,
            NSWindowOrderingMode::Above,
            Some(&view),
        );
        browser_parent.setHidden(!self.ivars().options.browser_enabled);
        window.setContentView(Some(&root));
        window.makeKeyAndOrderFront(None);
        if !window.makeFirstResponder(Some(&view)) {
            return Err(anyhow!(
                "stage=input.first-responder cause=appkit-rejected-render-view retryable=false"
            ));
        }
        view.ivars().window_id.set(window.windowNumber());
        if self.ivars().options.preflight_phase.as_deref() == Some("relaunch_closed") {
            let state_hash = view.restore_preflight_state()?;
            view.ivars().relaunch_state_hash.replace(Some(state_hash));
        }
        view.initialize_renderer()?;
        preflight::emit(
            "AX",
            json!({
                "children": [
                    "Workspaces navigator",
                    "Native spike tab",
                    "Terminal pane A",
                    "Editor pane B"
                ],
                "ime": "NSTextInputClient",
            }),
        );
        WORKSPACE.with(|workspace| {
            *workspace.borrow_mut() = Some(WorkspaceHandles {
                root: root.clone(),
                render: view.clone(),
                browser_parent: browser_parent.clone(),
                browser_enabled: self.ivars().options.browser_enabled,
            });
        });
        if self.ivars().options.browser_enabled {
            crate::browser::attach_parent(&browser_parent);
        }
        apply_workspace_layout();

        let timer = unsafe {
            NSTimer::scheduledTimerWithTimeInterval_target_selector_userInfo_repeats(
                TICK_INTERVAL_SECONDS,
                &view,
                sel!(tick:),
                None,
                true,
            )
        };
        self.ivars()
            .window
            .set(window)
            .map_err(|_| anyhow!("stage=app.retain target=window cause=already-set"))?;
        self.ivars()
            .root
            .set(root)
            .map_err(|_| anyhow!("stage=app.retain target=root cause=already-set"))?;
        self.ivars()
            .view
            .set(view)
            .map_err(|_| anyhow!("stage=app.retain target=view cause=already-set"))?;
        self.ivars()
            .browser_parent
            .set(browser_parent)
            .map_err(|_| anyhow!("stage=app.retain target=browser-parent cause=already-set"))?;
        self.ivars()
            .timer
            .set(timer)
            .map_err(|_| anyhow!("stage=app.retain target=timer cause=already-set"))?;

        app.setActivationPolicy(NSApplicationActivationPolicy::Regular);
        #[allow(deprecated)]
        app.activateIgnoringOtherApps(true);
        eprintln!(
            "event=app.launched architecture={} lifecycle=appkit ide_pixels=wgpu browser_pixels={} terminal=portable-pty",
            std::env::consts::ARCH,
            if self.ivars().options.browser_enabled {
                "cef-native-child"
            } else {
                "closed"
            }
        );
        Ok(())
    }

    fn prepare_shutdown(&self) {
        if let Some(timer) = self.ivars().timer.get() {
            timer.invalidate();
        }
        if let Some(window) = self.ivars().window.get() {
            window.setDelegate(None);
            window.setContentView(None);
        }
        eprintln!("event=appkit.resources.release requested=true");
    }
}

fn objc_text(value: &AnyObject) -> String {
    if let Some(text) = value.downcast_ref::<NSString>() {
        return text.to_string();
    }
    if let Some(attributed) = value.downcast_ref::<NSAttributedString>() {
        return attributed.string().to_string();
    }
    let description: Retained<NSString> = unsafe { msg_send![value, description] };
    description.to_string()
}

fn not_found_range() -> NSRange {
    NSRange::new(NSUInteger::MAX, 0)
}

fn percentile_95(values: &[f64]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let index = ((sorted.len() as f64 * 0.95).ceil() as usize)
        .saturating_sub(1)
        .min(sorted.len() - 1);
    Some(sorted[index])
}

fn write_json_atomic(path: &Path, value: &impl Serialize) -> Result<()> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("stage=report.mkdir path={}", parent.display()))?;
    }
    let temporary = path.with_extension(format!("tmp-{}", std::process::id()));
    let bytes = serde_json::to_vec_pretty(value).context("stage=report.serialize")?;
    std::fs::write(&temporary, bytes)
        .with_context(|| format!("stage=report.write path={}", temporary.display()))?;
    std::fs::rename(&temporary, path).with_context(|| {
        format!(
            "stage=report.publish from={} to={}",
            temporary.display(),
            path.display()
        )
    })
}

fn stable_state_hash(bytes: &[u8]) -> String {
    let hash = bytes.iter().fold(0xcbf29ce484222325_u64, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
    });
    format!("fnv1a64-{hash:016x}")
}

fn clear_transient_render_failure(failure: &mut Option<String>) -> bool {
    if failure
        .as_deref()
        .is_some_and(|message| message.starts_with("Render failed:"))
    {
        *failure = None;
        true
    } else {
        false
    }
}

pub fn run(runtime: Option<BrowserRuntime>) -> Result<()> {
    let mut options = Options::parse()?;
    options.browser_enabled = runtime.is_some();
    let actual_launch_mode = if options.browser_enabled {
        "browser-included"
    } else {
        "browser-closed"
    };
    if let Some(expected_launch_mode) = options.preflight_launch_mode.as_deref()
        && expected_launch_mode != actual_launch_mode
    {
        return Err(anyhow!(
            "stage=preflight.launch-mode cause=runtime-mismatch expected={expected_launch_mode} actual={actual_launch_mode}"
        ));
    }
    let preflight_contract = preflight::Contract::from_parts(
        options.preflight_run_id.clone(),
        options.preflight_phase.clone(),
        options.preflight_events.clone(),
        options.preflight_scenario.clone(),
    )?;
    if let Some(contract) = preflight_contract {
        contract.start()?;
    }
    let browser_enabled = options.browser_enabled;
    let started_at = Instant::now();
    let mtm = MainThreadMarker::new()
        .ok_or_else(|| anyhow!("stage=app.bootstrap cause=not-main-thread retryable=false"))?;
    let app = if options.browser_enabled {
        NSApp(mtm)
    } else {
        NSApplication::sharedApplication(mtm)
    };
    let delegate = AppDelegate::new(mtm, options, started_at);
    app.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    let mut runtime = runtime;
    if runtime.is_some() {
        app.finishLaunching();
        eprintln!("event=browser.message-loop.run owner=cef-macos-appkit-pump");
        cef::run_message_loop();
    } else {
        app.run();
    }
    WORKSPACE.with(|workspace| {
        let mut workspace = workspace.borrow_mut();
        if let Some(handles) = workspace.as_ref() {
            handles.render.shutdown_owned_resources();
        }
        *workspace = None;
    });
    eprintln!("event=workspace.shutdown complete=true");
    delegate.prepare_shutdown();
    app.setDelegate(None);
    drop(delegate);
    eprintln!("event=appkit.resources.release complete=true");
    if let Some(runtime) = runtime.as_mut() {
        runtime.shutdown()?;
    }
    preflight::complete(browser_enabled);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        PreflightAction, clear_transient_render_failure, is_option_meta_key, is_plain_key_control,
        layout_invariant_text_for_key_code, modifier_flags_details, option_meta_text_for_key_code,
    };
    use crate::preflight;
    use objc2_app_kit::NSEventModifierFlags;
    use serde_json::json;

    #[test]
    fn modifier_transition_telemetry_is_envelope_safe_and_names_the_modifier() {
        let details = modifier_flags_details(
            58,
            NSEventModifierFlags::Option,
            json!({ "app_frontmost": true }),
            42,
        );
        preflight::validate_details("input.modifier.flags_changed", &details).unwrap();
        assert_eq!(details["key_code"], 58);
        assert_eq!(details["modifiers"], json!(["option"]));
        let released = modifier_flags_details(58, NSEventModifierFlags::empty(), json!({}), 43);
        assert_eq!(released["modifiers"], json!([]));
    }

    #[test]
    fn preflight_action_payloads_stay_disjoint_from_the_telemetry_envelope() {
        let action = PreflightAction {
            action: "terminal.plain_key_control".to_owned(),
            key_code: 28,
            armed_at: std::time::Instant::now(),
        };
        preflight::validate_details("input.action.armed", &action.armed_details()).unwrap();
        preflight::validate_details("input.action.consume", &action.consume_details()).unwrap();
    }

    #[test]
    fn layout_invariant_probe_key_codes_have_deterministic_text() {
        let keys = [25, 18, 26, 27, 29, 29, 29, 36];
        let text: String = keys
            .into_iter()
            .filter_map(layout_invariant_text_for_key_code)
            .collect();
        assert_eq!(text, "917-000\r");
        assert_eq!(layout_invariant_text_for_key_code(5), None);
    }

    #[test]
    fn option_meta_route_is_limited_to_unmodified_physical_f_key() {
        assert_eq!(option_meta_text_for_key_code(3), Some("f"));
        assert!(is_option_meta_key(3, NSEventModifierFlags::Option));
        assert!(!is_option_meta_key(
            3,
            NSEventModifierFlags::Option | NSEventModifierFlags::Command
        ));
        assert!(!is_option_meta_key(5, NSEventModifierFlags::Option));
    }

    #[test]
    fn plain_key_control_is_limited_to_unmodified_physical_8_key() {
        assert!(!is_plain_key_control(
            None,
            28,
            NSEventModifierFlags::empty()
        ));
        assert!(is_plain_key_control(
            Some("terminal.plain_key_control"),
            28,
            NSEventModifierFlags::empty()
        ));
        assert!(!is_plain_key_control(
            Some("terminal.other_action"),
            28,
            NSEventModifierFlags::empty()
        ));
        assert!(!is_plain_key_control(
            Some("terminal.plain_key_control"),
            29,
            NSEventModifierFlags::empty()
        ));
        assert!(!is_plain_key_control(
            Some("terminal.plain_key_control"),
            28,
            NSEventModifierFlags::Command
        ));
        assert!(!is_plain_key_control(
            Some("terminal.plain_key_control"),
            28,
            NSEventModifierFlags::Option
        ));
    }

    #[test]
    fn successful_present_clears_only_transient_render_failure() {
        let mut failure = Some("Render failed: stage=wgpu.present retryable=true".to_owned());
        assert!(clear_transient_render_failure(&mut failure));
        assert_eq!(failure, None);

        let mut input_failure = Some("Input action failed: focus lost".to_owned());
        assert!(!clear_transient_render_failure(&mut input_failure));
        assert_eq!(
            input_failure.as_deref(),
            Some("Input action failed: focus lost")
        );
    }
}
