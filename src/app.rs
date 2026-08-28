use std::cell::{Cell, OnceCell, RefCell};
use std::collections::VecDeque;
use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::ptr::NonNull;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject, Sel};
use objc2::{DefinedClass, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSAccessibilityElement, NSAccessibilityGroupRole, NSAccessibilityListRole,
    NSAccessibilityTabGroupRole, NSAccessibilityTextAreaRole, NSApplication,
    NSApplicationActivationPolicy, NSApplicationDelegate, NSBackingStoreType, NSEvent,
    NSEventModifierFlags, NSMenu, NSMenuItem, NSPasteboard, NSPasteboardTypeString,
    NSTextInputClient, NSView, NSWindow, NSWindowDelegate, NSWindowStyleMask,
};
use objc2_foundation::{
    MainThreadMarker, NSArray, NSAttributedString, NSAttributedStringKey, NSNotification, NSObject,
    NSObjectProtocol, NSPoint, NSRange, NSRangePointer, NSRect, NSSize, NSString, NSTimer,
    NSUInteger, ns_string,
};
use serde::Serialize;

use crate::accessibility::{semantic_pane_name, workbench_tree_with_state};
use crate::commands::{CommandId, CommandRegistry, Shortcut, ShortcutModifiers};
use crate::diagnostics::{
    DiagnosticContext, DiagnosticStage, DiagnosticsStore, OperationManifest, RecoverySurface,
    StructuredDiagnostic,
};
use crate::domain::DomainProjection;
use crate::herdr::{
    HerdrConnectionConfig, HerdrController, HerdrUpdate, SplitDirection, TopologyOperation,
};
use crate::layout::{CanvasGeometry, PaneId, WindowLayoutState};
use crate::navigator::{NavigatorController, NavigatorView, render_navigator};
use crate::pet::{OptionTabOverlay, OverlayAction, OverlayKey};
use crate::presentation::{AgentPresentationStore, connection_label};
use crate::pty::PtySession;
use crate::render::{FrameModel, PresentObservation, Renderer};
use crate::terminal::{TerminalEvent, TerminalView};

const WINDOW_WIDTH: f64 = 1180.0;
const WINDOW_HEIGHT: f64 = 720.0;
const TICK_INTERVAL_SECONDS: f64 = 0.016;
const ROUTING_TRACE_LIMIT: usize = 128;

fn terminal_meta_text(key_code: u16, shift: bool, fallback: Option<&str>) -> Option<String> {
    let physical_letter = match key_code {
        0 => 'a',
        1 => 's',
        2 => 'd',
        3 => 'f',
        4 => 'h',
        5 => 'g',
        6 => 'z',
        7 => 'x',
        8 => 'c',
        9 => 'v',
        11 => 'b',
        12 => 'q',
        13 => 'w',
        14 => 'e',
        15 => 'r',
        16 => 'y',
        17 => 't',
        31 => 'o',
        32 => 'u',
        34 => 'i',
        35 => 'p',
        37 => 'l',
        38 => 'j',
        40 => 'k',
        45 => 'n',
        46 => 'm',
        51 => return Some("\u{7f}".to_owned()),
        _ => return fallback.filter(|text| !text.is_empty()).map(str::to_owned),
    };
    Some(
        if shift {
            physical_letter.to_ascii_uppercase()
        } else {
            physical_letter
        }
        .to_string(),
    )
}

fn shortcut_from_event(event: &NSEvent) -> Shortcut {
    let flags = event.modifierFlags();
    let mut modifiers = ShortcutModifiers::empty();
    if flags.contains(NSEventModifierFlags::Command) {
        modifiers = modifiers | ShortcutModifiers::COMMAND;
    }
    if flags.contains(NSEventModifierFlags::Shift) {
        modifiers = modifiers | ShortcutModifiers::SHIFT;
    }
    if flags.contains(NSEventModifierFlags::Option) {
        modifiers = modifiers | ShortcutModifiers::OPTION;
    }
    if flags.contains(NSEventModifierFlags::Control) {
        modifiers = modifiers | ShortcutModifiers::CONTROL;
    }
    Shortcut::new(event.keyCode(), modifiers)
}

fn overlay_key_for_event(key_code: u16, shift: bool) -> Option<OverlayKey> {
    Some(match key_code {
        53 => OverlayKey::Escape,
        126 => OverlayKey::ArrowUp,
        125 => OverlayKey::ArrowDown,
        48 if shift => OverlayKey::ShiftTab,
        48 => OverlayKey::Tab,
        36 | 76 => OverlayKey::Enter,
        _ => return None,
    })
}

#[derive(Clone, Debug)]
struct Options {
    report_path: Option<PathBuf>,
    probe_count: usize,
    autoclose_after: Option<Duration>,
    herdr_socket_path: Option<PathBuf>,
    herdr_bin: Option<PathBuf>,
    herdr_session: Option<String>,
    pane_id: Option<String>,
    navigator_state_path: Option<PathBuf>,
    shortcut_settings_path: Option<PathBuf>,
}

impl Options {
    fn parse() -> Result<Self> {
        let mut options = Self {
            report_path: None,
            probe_count: 0,
            autoclose_after: None,
            herdr_socket_path: None,
            herdr_bin: None,
            herdr_session: None,
            pane_id: None,
            navigator_state_path: None,
            shortcut_settings_path: None,
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
                "--herdr-socket" => {
                    options.herdr_socket_path =
                        Some(PathBuf::from(arguments.next().ok_or_else(|| {
                            anyhow!("stage=cli.parse option=--herdr-socket cause=missing-path")
                        })?));
                }
                "--herdr-bin" => {
                    options.herdr_bin = Some(PathBuf::from(arguments.next().ok_or_else(|| {
                        anyhow!("stage=cli.parse option=--herdr-bin cause=missing-path")
                    })?));
                }
                "--herdr-session" => {
                    options.herdr_session = Some(arguments.next().ok_or_else(|| {
                        anyhow!("stage=cli.parse option=--herdr-session cause=missing-value")
                    })?);
                }
                "--pane-id" => {
                    options.pane_id = Some(arguments.next().ok_or_else(|| {
                        anyhow!("stage=cli.parse option=--pane-id cause=missing-value")
                    })?);
                }
                "--navigator-state" => {
                    options.navigator_state_path =
                        Some(PathBuf::from(arguments.next().ok_or_else(|| {
                            anyhow!("stage=cli.parse option=--navigator-state cause=missing-path")
                        })?));
                }
                "--shortcut-settings" => {
                    options.shortcut_settings_path =
                        Some(PathBuf::from(arguments.next().ok_or_else(|| {
                            anyhow!("stage=cli.parse option=--shortcut-settings cause=missing-path")
                        })?));
                }
                "--help" => {
                    println!(
                        "herdr-ide [--report PATH] [--probe-count N] [--autoclose-ms N] \
                         [--herdr-socket PATH] [--herdr-bin PATH] [--herdr-session NAME] \
                         [--pane-id ID] [--navigator-state PATH] [--shortcut-settings PATH]"
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

fn load_navigator(options: &Options) -> NavigatorController {
    let path = options.navigator_state_path.clone().or_else(|| {
        std::env::var_os("HOME").map(|home| {
            PathBuf::from(home).join("Library/Application Support/Herdr IDE/navigator.json")
        })
    });
    let Some(path) = path else {
        eprintln!("event=navigator.persistence.disabled cause=missing-home retryable=true");
        return NavigatorController::in_memory();
    };
    match NavigatorController::load_or_default(&path) {
        Ok(controller) => controller,
        Err(error) => {
            eprintln!(
                "event=navigator.load.failed path={} retryable=true error={error:#}",
                path.display()
            );
            NavigatorController::in_memory()
        }
    }
}

fn load_commands(options: &Options) -> CommandRegistry {
    let path = options.shortcut_settings_path.clone().or_else(|| {
        std::env::var_os("HOME").map(|home| {
            PathBuf::from(home).join("Library/Application Support/Herdr IDE/shortcuts.json")
        })
    });
    let Some(path) = path else {
        eprintln!("event=shortcuts.persistence.disabled cause=missing-home retryable=true");
        return CommandRegistry::default();
    };
    match CommandRegistry::load_or_default(&path) {
        Ok(registry) => registry,
        Err(error) => {
            eprintln!(
                "event=shortcuts.load.failed path={} retryable=true error={error}",
                path.display()
            );
            CommandRegistry::default()
        }
    }
}

#[derive(Debug)]
struct UiState {
    projection: DomainProjection,
    agents: AgentPresentationStore,
    navigator: NavigatorController,
    commands: CommandRegistry,
    overlay: OptionTabOverlay,
    zoom: WindowLayoutState,
    editor_text: String,
    ime_marked: String,
    visible_failure: Option<String>,
    input_generation: u64,
    pending_input: VecDeque<(u64, Instant)>,
    input_to_present_ms: Vec<f64>,
    last_terminal_generation: u64,
    routing_timeline: VecDeque<RoutingTraceEvent>,
    diagnostics: DiagnosticsStore,
    operation_manifest: OperationManifest,
    dirty: bool,
}

impl UiState {
    fn new() -> Self {
        Self::with_navigator(NavigatorController::in_memory())
    }

    fn with_navigator(navigator: NavigatorController) -> Self {
        Self::with_navigator_and_commands(navigator, CommandRegistry::default())
    }

    fn with_navigator_and_commands(
        navigator: NavigatorController,
        commands: CommandRegistry,
    ) -> Self {
        let projection = DomainProjection::default();
        let mut agents = AgentPresentationStore::default();
        agents.rebuild(&projection);
        Self {
            projection,
            agents,
            navigator,
            commands,
            overlay: OptionTabOverlay::default(),
            zoom: WindowLayoutState::default_for_app(),
            editor_text: "No file open\n\nSelect a file from the workspace to start editing."
                .to_owned(),
            ime_marked: String::new(),
            visible_failure: None,
            input_generation: 0,
            pending_input: VecDeque::new(),
            input_to_present_ms: Vec::new(),
            last_terminal_generation: 0,
            routing_timeline: VecDeque::with_capacity(ROUTING_TRACE_LIMIT),
            diagnostics: DiagnosticsStore::default(),
            operation_manifest: OperationManifest::new(
                format!("app-operation-{}", std::process::id()),
                format!("app-process-{}", std::process::id()),
            ),
            dirty: true,
        }
    }

    fn record_diagnostic(
        &mut self,
        stage: DiagnosticStage,
        operation: &str,
        target: &str,
        reason: &str,
        retryable: bool,
        action_required: bool,
        context: DiagnosticContext,
    ) -> StructuredDiagnostic {
        let diagnostic = StructuredDiagnostic::new(
            format!("app-{}-{operation}", std::process::id()),
            stage,
            target,
            reason,
            retryable,
            action_required,
        )
        .with_context(context);
        self.diagnostics.record(diagnostic.clone());
        diagnostic
    }

    fn record_routing_event(&mut self, event: RoutingTraceEvent) {
        if self.routing_timeline.len() == ROUTING_TRACE_LIMIT {
            self.routing_timeline.pop_front();
        }
        self.routing_timeline.push_back(event);
    }

    fn select_navigator_view(&mut self, view: NavigatorView) -> Result<()> {
        self.navigator.select_view(view);
        self.navigator
            .save()
            .context("stage=navigator.persist target=active-view")?;
        self.dirty = true;
        Ok(())
    }

    fn model(&self, terminal: TerminalView) -> FrameModel {
        let recovery = self
            .diagnostics
            .recovery_surface(self.projection.connection());
        let connection_text =
            if matches!(recovery.state, crate::diagnostics::RecoveryState::Connected) {
                connection_label(self.projection.connection())
            } else {
                format!(
                    "{} | {}",
                    connection_label(self.projection.connection()),
                    recovery.title
                )
            };
        let navigator_text = format!(
            "HERDR IDE\n\n{}",
            render_navigator(
                self.navigator.preferences().active_view,
                &self.projection,
                &self.agents,
                self.navigator.preferences(),
            )
        );
        let tab_text = self
            .projection
            .active_workspace()
            .and_then(|workspace| {
                workspace
                    .tabs
                    .iter()
                    .find(|tab| tab.tab_id == workspace.active_tab_id)
                    .map(|tab| format!("{} / {}", workspace.name, tab.name))
            })
            .unwrap_or_else(|| "No workspace selected".to_owned());
        FrameModel {
            navigator_text,
            overlay_text: self.overlay.render_text(),
            tab_text,
            connection_text,
            zoomed: self.zoom.zoomed_pane(),
            focused: self.zoom.focused_pane(),
            terminal: TerminalView {
                failure: self.visible_failure.clone().or(terminal.failure),
                ..terminal
            },
            editor_text: self.editor_text.clone(),
            ime_marked: self.ime_marked.clone(),
            input_generation: self.input_generation,
        }
    }

    fn note_presented(&mut self, generation: u64, now: Instant) {
        while let Some((pending_generation, started)) = self.pending_input.front().copied() {
            if pending_generation > generation {
                break;
            }
            self.pending_input.pop_front();
            self.input_to_present_ms
                .push(now.duration_since(started).as_secs_f64() * 1000.0);
        }
    }
}

#[derive(Clone, Debug, Serialize)]
struct FocusSnapshot {
    app_frontmost: bool,
    key_window: bool,
    render_view_first_responder: bool,
}

#[derive(Clone, Debug, Serialize)]
struct RoutingTraceEvent {
    monotonic_ns: u64,
    source: &'static str,
    event: &'static str,
    outcome: &'static str,
    key_code: Option<u16>,
    modifier_flags: Option<u64>,
    focused_pane: Option<String>,
    pane_id: Option<String>,
    operation: Option<String>,
    bytes: Option<usize>,
}

#[derive(Debug, Serialize)]
struct RuntimeReport {
    schema: &'static str,
    build: &'static str,
    architecture: &'static str,
    process_id: u32,
    pty_child_process_id: Option<u32>,
    herdr_pane_id: Option<String>,
    window_id: isize,
    adapter: String,
    physical_width: u32,
    physical_height: u32,
    scale_factor: f64,
    first_usable_ms: f64,
    input_samples: usize,
    input_to_present_p95_ms: Option<f64>,
    input_to_present_max_ms: Option<f64>,
    focus: FocusSnapshot,
    routing_timeline: Vec<RoutingTraceEvent>,
    zoom_state: &'static str,
    ax_children: [&'static str; 4],
    ime_contract: &'static str,
    pty_contract: &'static str,
    native_menu_contract: &'static str,
    diagnostics: Vec<StructuredDiagnostic>,
    recovery: RecoverySurface,
    operation_manifest: OperationManifest,
}

struct ViewIvars {
    renderer: RefCell<Option<Renderer>>,
    pty: RefCell<PtySession>,
    herdr: HerdrController,
    ui: RefCell<UiState>,
    options: Options,
    started_at: Instant,
    first_presented_at: Cell<Option<Instant>>,
    window_id: Cell<isize>,
    scale_factor: Cell<f64>,
    tick_count: Cell<u64>,
    probe_index: Cell<usize>,
    probe_is_marked: Cell<bool>,
    selecting_terminal: Cell<bool>,
    last_pty_size: Cell<(u32, u32)>,
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
            if let Err(error) = self.render_frame() {
                let message = format!("Render failed: {error:#}");
                let mut ui = self.ivars().ui.borrow_mut();
                ui.visible_failure = Some(message.clone());
                ui.record_diagnostic(
                    DiagnosticStage::App,
                    "render",
                    "native-surface",
                    &message,
                    true,
                    false,
                    DiagnosticContext::default(),
                );
                eprintln!("event=render.failed stage=draw retryable=true error={message:?}");
            }
        }

        #[unsafe(method(keyDown:))]
        fn key_down(&self, event: &NSEvent) {
            let flags = event.modifierFlags();
            let command = flags.contains(NSEventModifierFlags::Command);
            let shift = flags.contains(NSEventModifierFlags::Shift);
            self.record_routing_event(
                "appkit",
                "key_down",
                "received",
                Some(event.keyCode()),
                Some(flags.0 as u64),
                None,
                None,
                None,
            );
            let shortcut = shortcut_from_event(event);
            if self.ivars().ui.borrow().overlay.is_open()
                && shortcut.matches(48, ShortcutModifiers::OPTION)
            {
                self.handle_overlay_key(OverlayKey::Toggle);
                return;
            }
            if self.ivars().ui.borrow().overlay.is_open()
                && let Some(key) = overlay_key_for_event(event.keyCode(), shift)
            {
                self.handle_overlay_key(key);
                return;
            }
            let routed_command = self
                .ivars()
                .ui
                .borrow()
                .commands
                .resolve(shortcut.key_code, shortcut.modifiers);
            match routed_command {
                Some(CommandId::PaneZoom) => {
                    self.record_routing_event(
                        "app-routing",
                        "layout.zoom",
                        "dispatched",
                        Some(event.keyCode()),
                        Some(flags.0 as u64),
                        None,
                        Some("pane.zoom".to_owned()),
                        None,
                    );
                    self.toggle_zoom();
                    return;
                }
                Some(CommandId::NavigatorWorkspaces)
                | Some(CommandId::NavigatorAgents)
                | Some(CommandId::NavigatorWorktrees) => {
                    let view = match routed_command {
                        Some(CommandId::NavigatorWorkspaces) => NavigatorView::Workspaces,
                        Some(CommandId::NavigatorAgents) => NavigatorView::Agents,
                        Some(CommandId::NavigatorWorktrees) => NavigatorView::Worktrees,
                        _ => unreachable!("navigator command arm is exhaustive"),
                    };
                    self.record_routing_event(
                        "app-routing",
                        "navigator.view",
                        "dispatched",
                        Some(event.keyCode()),
                        Some(flags.0 as u64),
                        None,
                        Some(format!("navigator.{view:?}").to_ascii_lowercase()),
                        None,
                    );
                    self.select_navigator_view(view);
                    return;
                }
                Some(CommandId::NewTab) => {
                    self.record_routing_event(
                        "app-routing",
                        "topology.request",
                        "dispatched",
                        Some(event.keyCode()),
                        Some(flags.0 as u64),
                        None,
                        Some("tab.create".to_owned()),
                        None,
                    );
                    self.create_tab();
                    return;
                }
                Some(CommandId::SplitRight) | Some(CommandId::SplitDown) => {
                    let direction = if routed_command == Some(CommandId::SplitDown) {
                        SplitDirection::Down
                    } else {
                        SplitDirection::Right
                    };
                    self.record_routing_event(
                        "app-routing",
                        "topology.request",
                        "dispatched",
                        Some(event.keyCode()),
                        Some(flags.0 as u64),
                        None,
                        Some("pane.split".to_owned()),
                        None,
                    );
                    self.split_pane(direction);
                    return;
                }
                Some(CommandId::ClipboardCopy) if self.terminal_focused() => {
                    self.record_routing_event(
                        "app-routing",
                        "input.route",
                        "clipboard-copy",
                        Some(event.keyCode()),
                        Some(flags.0 as u64),
                        None,
                        Some("clipboard.copy".to_owned()),
                        None,
                    );
                    self.copy_terminal_selection();
                    return;
                }
                Some(CommandId::ClipboardPaste) if self.terminal_focused() => {
                    self.record_routing_event(
                        "app-routing",
                        "input.route",
                        "clipboard-paste",
                        Some(event.keyCode()),
                        Some(flags.0 as u64),
                        None,
                        Some("clipboard.paste".to_owned()),
                        None,
                    );
                    self.paste_terminal_clipboard();
                    return;
                }
                Some(CommandId::WorkspaceStatus) => {
                    self.toggle_overlay();
                    return;
                }
                None => {}
                Some(CommandId::ClipboardCopy) | Some(CommandId::ClipboardPaste) => {}
            }
            if flags.contains(NSEventModifierFlags::Option)
                && !command
                && self.terminal_focused()
            {
                let fallback = event.charactersIgnoringModifiers();
                if let Some(text) = terminal_meta_text(
                    event.keyCode(),
                    shift,
                    fallback.as_ref().map(|characters| characters.to_string()).as_deref(),
                ) {
                    let encoded = crate::terminal::TerminalModel::encode_meta(&text);
                    self.record_routing_event(
                        "app-routing",
                        "input.route",
                        "terminal-meta",
                        Some(event.keyCode()),
                        Some(flags.0 as u64),
                        None,
                        Some("terminal.meta".to_owned()),
                        Some(encoded.len()),
                    );
                    self.commit_text_from("appkit.option-meta", &encoded);
                    return;
                }
            }
            self.record_routing_event(
                "app-routing",
                "input.route",
                "interpret-key-events",
                Some(event.keyCode()),
                Some(flags.0 as u64),
                None,
                Some("ime.interpret".to_owned()),
                None,
            );
            self.interpretKeyEvents(&NSArray::from_slice(&[event]));
        }

        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, event: &NSEvent) {
            let point = self.convertPoint_fromView(event.locationInWindow(), None);
            let bounds = self.bounds();
            let geometry = CanvasGeometry::for_size(bounds.size.width, bounds.size.height);
            let zoomed = self.ivars().ui.borrow().zoom.zoomed_pane();
            let Some(pane) = geometry.pane_at(point.x, point.y, zoomed) else {
                return;
            };
            let mut ui = self.ivars().ui.borrow_mut();
            ui.zoom.set_focus(pane);
            ui.dirty = true;
            drop(ui);
            if pane == PaneId::TerminalA {
                self.begin_terminal_pointer(event, point);
            }
            self.update_accessibility_tree();
            self.setNeedsDisplay(true);
        }

        #[unsafe(method(mouseDragged:))]
        fn mouse_dragged(&self, event: &NSEvent) {
            if !self.ivars().selecting_terminal.get() { return; }
            let point = self.convertPoint_fromView(event.locationInWindow(), None);
            if let Some((row, column)) = self.terminal_cell_at(point)
                && let Err(error) = self.ivars().pty.borrow().terminal().update_selection(row, column)
            {
                self.show_terminal_failure("selection.update", &error);
            }
            self.setNeedsDisplay(true);
        }

        #[unsafe(method(mouseUp:))]
        fn mouse_up(&self, event: &NSEvent) {
            let point = self.convertPoint_fromView(event.locationInWindow(), None);
            if !self.ivars().selecting_terminal.replace(false)
                && let Some((row, column)) = self.terminal_cell_at(point)
            {
                self.send_terminal_mouse(0, false, row, column);
            }
        }

        #[unsafe(method(scrollWheel:))]
        fn scroll_wheel(&self, event: &NSEvent) {
            let point = self.convertPoint_fromView(event.locationInWindow(), None);
            let Some((row, column)) = self.terminal_cell_at(point) else { return };
            let delta = event.scrollingDeltaY();
            if delta == 0.0 { return; }
            let terminal = self.ivars().pty.borrow().terminal();
            let snapshot = terminal.snapshot();
            match snapshot {
                Ok(view) if view.modes.mouse_mode => self.send_terminal_mouse(if delta > 0.0 { 64 } else { 65 }, true, row, column),
                Ok(_) => {
                    let lines = if delta > 0.0 { 3 } else { -3 };
                    if let Err(error) = terminal.scroll(lines) { self.show_terminal_failure("scroll", &error); }
                }
                Err(error) => self.show_terminal_failure("scroll.snapshot", &error),
            }
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
            let text = objc_text(string);
            self.commit_text_from("ime.insert_text", &text);
        }

        #[unsafe(method(doCommandBySelector:))]
        unsafe fn do_command_by_selector(&self, selector: Sel) {
            if selector == sel!(insertNewline:) {
                self.commit_text_from("ime.insert_newline", "\r");
            } else if selector == sel!(deleteBackward:) {
                self.commit_text_from("ime.delete_backward", "\u{7f}");
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
            let mut ui = self.ivars().ui.borrow_mut();
            ui.ime_marked = objc_text(string);
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
            NSRange::new(0, 0)
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
    fn focus_snapshot(&self) -> FocusSnapshot {
        let app = NSApplication::sharedApplication(self.mtm());
        let window = self.window();
        let render_view_pointer = NonNull::from(self).cast::<c_void>();
        let render_view_first_responder = window
            .as_ref()
            .and_then(|window| window.firstResponder())
            .is_some_and(|responder| {
                Retained::as_ptr(&responder).cast::<c_void>() == render_view_pointer.as_ptr()
            });
        FocusSnapshot {
            app_frontmost: app.isActive(),
            key_window: window.as_ref().is_some_and(|window| window.isKeyWindow()),
            render_view_first_responder,
        }
    }

    fn record_routing_event(
        &self,
        source: &'static str,
        event: &'static str,
        outcome: &'static str,
        key_code: Option<u16>,
        modifier_flags: Option<u64>,
        pane_id: Option<String>,
        operation: Option<String>,
        bytes: Option<usize>,
    ) {
        let focused_pane = Some(format!(
            "{:?}",
            self.ivars().ui.borrow().zoom.focused_pane()
        ));
        self.ivars()
            .ui
            .borrow_mut()
            .record_routing_event(RoutingTraceEvent {
                monotonic_ns: self
                    .ivars()
                    .started_at
                    .elapsed()
                    .as_nanos()
                    .min(u64::MAX as u128) as u64,
                source,
                event,
                outcome,
                key_code,
                modifier_flags,
                focused_pane,
                pane_id,
                operation,
                bytes,
            });
    }

    fn terminal_focused(&self) -> bool {
        matches!(
            self.ivars().ui.borrow().zoom.focused_pane(),
            PaneId::TerminalA | PaneId::TerminalB
        )
    }

    fn terminal_cell_at(&self, point: NSPoint) -> Option<(usize, usize)> {
        let bounds = self.bounds();
        let geometry = CanvasGeometry::for_size(bounds.size.width, bounds.size.height);
        let zoomed = self.ivars().ui.borrow().zoom.zoomed_pane();
        let rect = geometry.terminal_rect(zoomed)?;
        let (x, y, width, height) = (rect.x, rect.y, rect.width, rect.height);
        if width <= 0.0 || height <= 0.0 {
            return None;
        }
        if point.x < x || point.y < y || point.x >= x + width || point.y >= y + height {
            return None;
        }
        let view = self.ivars().pty.borrow().terminal().snapshot().ok()?;
        let column = (((point.x - x) / width) * view.columns as f64).floor() as usize;
        let row = (((point.y - y) / height) * view.rows as f64).floor() as usize;
        Some((
            row.min(view.rows.saturating_sub(1)),
            column.min(view.columns.saturating_sub(1)),
        ))
    }

    fn begin_terminal_pointer(&self, event: &NSEvent, point: NSPoint) {
        let Some((row, column)) = self.terminal_cell_at(point) else {
            return;
        };
        let terminal = self.ivars().pty.borrow().terminal();
        let mouse_mode = terminal.snapshot().is_ok_and(|view| view.modes.mouse_mode);
        let force_selection = event.modifierFlags().contains(NSEventModifierFlags::Shift);
        if mouse_mode && !force_selection {
            self.ivars().selecting_terminal.set(false);
            self.send_terminal_mouse(0, true, row, column);
        } else {
            self.ivars().selecting_terminal.set(true);
            if let Err(error) = terminal.begin_selection(row, column) {
                self.show_terminal_failure("selection.begin", &error);
            }
        }
    }

    fn send_terminal_mouse(&self, button: u8, pressed: bool, row: usize, column: usize) {
        let terminal = self.ivars().pty.borrow().terminal();
        match terminal.encode_mouse(button, pressed, row, column) {
            Ok(Some(sequence)) => {
                if let Err(error) = self.ivars().pty.borrow().write_text(&sequence) {
                    self.show_terminal_failure("mouse.write", &error);
                }
            }
            Ok(None) => {}
            Err(error) => self.show_terminal_failure("mouse.encode", &error),
        }
    }

    fn copy_terminal_selection(&self) {
        match self.ivars().pty.borrow().terminal().selected_text() {
            Ok(Some(text)) if !text.is_empty() => {
                let pasteboard = NSPasteboard::generalPasteboard();
                pasteboard.clearContents();
                let string_type = unsafe { NSPasteboardTypeString };
                if !pasteboard.setString_forType(&NSString::from_str(&text), string_type) {
                    self.show_terminal_failure(
                        "clipboard.copy",
                        &anyhow!("macOS pasteboard rejected terminal selection"),
                    );
                }
            }
            Ok(_) => {}
            Err(error) => self.show_terminal_failure("clipboard.selection", &error),
        }
    }

    fn paste_terminal_clipboard(&self) {
        let string_type = unsafe { NSPasteboardTypeString };
        let Some(text) = NSPasteboard::generalPasteboard().stringForType(string_type) else {
            return;
        };
        let terminal = self.ivars().pty.borrow().terminal();
        match terminal.encode_paste(&text.to_string()) {
            Ok(encoded) => {
                if let Err(error) = self.ivars().pty.borrow().write_text(&encoded) {
                    self.show_terminal_failure("clipboard.paste", &error);
                }
            }
            Err(error) => self.show_terminal_failure("clipboard.encode", &error),
        }
    }

    fn show_terminal_failure(&self, operation: &str, error: &anyhow::Error) {
        let message = format!("Terminal failed: {operation}: {error:#}");
        self.ivars().pty.borrow().terminal().set_failure(&message);
        let mut ui = self.ivars().ui.borrow_mut();
        ui.record_diagnostic(
            DiagnosticStage::Pty,
            operation,
            "terminal",
            &message,
            true,
            false,
            DiagnosticContext::default(),
        );
        ui.dirty = true;
        eprintln!(
            "event=terminal.operation.failed operation={operation} retryable=true error={error:?}"
        );
    }

    fn toggle_zoom(&self) {
        let outcome = {
            let mut ui = self.ivars().ui.borrow_mut();
            let focused = ui.zoom.focused_pane();
            let outcome = ui.zoom.toggle_zoom(focused);
            ui.dirty = true;
            outcome
        };
        eprintln!("event=layout.zoom outcome={outcome:?}");
        self.update_accessibility_tree();
        self.setNeedsDisplay(true);
    }

    fn toggle_overlay(&self) {
        let action = {
            let mut ui = self.ivars().ui.borrow_mut();
            let previous_focus = Some(format!("{:?}", ui.zoom.focused_pane()));
            let agents = ui.agents.clone();
            let action = ui.overlay.toggle(&agents, previous_focus);
            ui.dirty = true;
            action
        };
        self.record_routing_event(
            "app-routing",
            "overlay.toggle",
            "dispatched",
            Some(48),
            Some((NSEventModifierFlags::Option).0 as u64),
            None,
            Some("agent-switcher".to_owned()),
            None,
        );
        self.apply_overlay_action(action);
    }

    fn handle_overlay_key(&self, key: OverlayKey) {
        let action = {
            let mut ui = self.ivars().ui.borrow_mut();
            let action = ui.overlay.handle_key(key);
            ui.dirty = true;
            action
        };
        self.record_routing_event(
            "app-routing",
            "overlay.key",
            "dispatched",
            None,
            None,
            None,
            Some(format!("{key:?}")),
            None,
        );
        self.apply_overlay_action(action);
    }

    fn apply_overlay_action(&self, action: OverlayAction) {
        match action {
            OverlayAction::None => {
                self.update_accessibility_tree();
                self.setNeedsDisplay(true);
            }
            OverlayAction::CloseRestoreFocus { previous_focus } => {
                self.record_routing_event(
                    "app-routing",
                    "overlay.close",
                    "accepted",
                    None,
                    None,
                    previous_focus,
                    Some("restore-previous-focus".to_owned()),
                    None,
                );
                let mut ui = self.ivars().ui.borrow_mut();
                ui.dirty = true;
                drop(ui);
                self.update_accessibility_tree();
                self.setNeedsDisplay(true);
            }
            OverlayAction::Focus {
                host,
                workspace_id,
                tab_id,
                pane_id,
                agent_id,
            } => {
                let result = self.ivars().herdr.submit(TopologyOperation::PaneFocus {
                    pane_id: pane_id.clone(),
                });
                self.record_routing_event(
                    "herdr",
                    "overlay.focus",
                    if result.is_ok() {
                        "submitted"
                    } else {
                        "rejected"
                    },
                    None,
                    None,
                    Some(pane_id.clone()),
                    Some("pane.focus".to_owned()),
                    None,
                );
                let mut ui = self.ivars().ui.borrow_mut();
                ui.overlay.close();
                if let Err(error) = &result {
                    ui.record_diagnostic(
                        DiagnosticStage::Herdr,
                        "pane.focus",
                        &pane_id,
                        &format!("{error:#}"),
                        true,
                        false,
                        DiagnosticContext {
                            host_id: Some(host.clone()),
                            workspace_id: Some(workspace_id.clone()),
                            tab_id: Some(tab_id.clone()),
                            pane_id: Some(pane_id.clone()),
                            agent_instance_id: Some(agent_id.clone()),
                            view_id: None,
                        },
                    );
                }
                ui.visible_failure = result.err().map(|error| {
                    format!(
                        "Agent focus failed ({host}/{workspace_id}/{tab_id}/{agent_id}): {error:#}"
                    )
                });
                ui.dirty = true;
                drop(ui);
                self.update_accessibility_tree();
                self.setNeedsDisplay(true);
            }
        }
    }

    fn select_navigator_view(&self, view: NavigatorView) {
        let result = self.ivars().ui.borrow_mut().select_navigator_view(view);
        if let Err(error) = result {
            let mut ui = self.ivars().ui.borrow_mut();
            let message = format!("{error:#}");
            ui.record_diagnostic(
                DiagnosticStage::App,
                "navigator.view",
                "navigator",
                &message,
                true,
                false,
                DiagnosticContext::default(),
            );
            ui.visible_failure = Some(format!("Navigator failed: {error:#}"));
            ui.dirty = true;
            eprintln!("event=navigator.view.failed view={view:?} retryable=true error={error:#}");
        }
        self.update_accessibility_tree();
        self.setNeedsDisplay(true);
    }

    fn create_tab(&self) {
        let operation =
            self.active_workspace_id()
                .map(|workspace_id| TopologyOperation::TabCreate {
                    workspace_id,
                    cwd: None,
                });
        self.submit_topology("tab.create", operation);
    }

    fn split_pane(&self, direction: SplitDirection) {
        let operation = self
            .active_pane_id()
            .map(|pane_id| TopologyOperation::PaneSplit {
                pane_id,
                direction,
                cwd: None,
            });
        self.submit_topology("pane.split", operation);
    }

    fn submit_topology(&self, label: &'static str, operation: Option<TopologyOperation>) {
        let pane_id = self.active_pane_id();
        self.record_routing_event(
            "herdr",
            "topology.request",
            "submitted",
            None,
            None,
            pane_id.clone(),
            Some(label.to_owned()),
            None,
        );
        let result = match operation {
            Some(operation) => {
                let cleared_zoom = self.ivars().ui.borrow_mut().zoom.before_topology_mutation();
                if let Some(outcome) = cleared_zoom {
                    eprintln!("event=layout.zoom outcome={outcome:?} command={label}");
                }
                self.ivars().herdr.submit(operation)
            }
            None => Err(anyhow!(
                "stage=herdr.command target={label} cause=no-active-target retryable=true"
            )),
        };
        self.record_routing_event(
            "herdr",
            "topology.response",
            if result.is_ok() {
                "accepted"
            } else {
                "rejected"
            },
            None,
            None,
            pane_id.clone(),
            Some(label.to_owned()),
            None,
        );
        let mut ui = self.ivars().ui.borrow_mut();
        if let Err(error) = &result {
            ui.record_diagnostic(
                DiagnosticStage::Herdr,
                label,
                pane_id.as_deref().unwrap_or("active-target"),
                &format!("{error:#}"),
                true,
                false,
                DiagnosticContext::default(),
            );
        }
        ui.visible_failure = result.err().map(|error| format!("Herdr failed: {error:#}"));
        ui.dirty = true;
        drop(ui);
        self.update_accessibility_tree();
        self.setNeedsDisplay(true);
    }

    fn active_workspace_id(&self) -> Option<String> {
        self.ivars()
            .ui
            .borrow()
            .projection
            .active_workspace()
            .map(|workspace| workspace.workspace_id.clone())
    }

    fn active_pane_id(&self) -> Option<String> {
        active_terminal_pane_id(&self.ivars().ui.borrow().projection)
    }

    fn sync_herdr_updates(&self) {
        loop {
            let update = match self.ivars().herdr.try_update() {
                Ok(Some(update)) => update,
                Ok(None) => break,
                Err(error) => {
                    self.show_herdr_failure("update", &format!("{error:#}"));
                    break;
                }
            };
            match update {
                HerdrUpdate::Snapshot(snapshot) => {
                    self.apply_herdr_snapshot("session.snapshot", snapshot)
                }
                HerdrUpdate::Applied {
                    operation,
                    snapshot,
                } => self.apply_herdr_snapshot(operation, snapshot),
                HerdrUpdate::Failed { operation, error } => {
                    self.show_herdr_failure(operation, &error)
                }
            }
        }
    }

    fn apply_herdr_snapshot(
        &self,
        operation: &'static str,
        snapshot: crate::domain::DomainSnapshot,
    ) {
        self.record_routing_event(
            "herdr",
            "topology.snapshot",
            "received",
            None,
            None,
            None,
            Some(operation.to_owned()),
            None,
        );
        let target = {
            let mut ui = self.ivars().ui.borrow_mut();
            match ui.projection.apply_snapshot(snapshot) {
                Ok(()) => {
                    let projection = ui.projection.clone();
                    ui.agents.rebuild(&projection);
                    if ui
                        .visible_failure
                        .as_deref()
                        .is_some_and(|failure| failure.starts_with("Herdr failed:"))
                    {
                        ui.visible_failure = None;
                    }
                    ui.dirty = true;
                    active_terminal_pane_id(&ui.projection)
                }
                Err(error) => {
                    let message = error.to_string();
                    ui.record_diagnostic(
                        DiagnosticStage::Herdr,
                        operation,
                        "domain-projection",
                        &message,
                        true,
                        matches!(
                            error,
                            crate::domain::ProjectionFailure::ProtocolMismatch { .. }
                        ),
                        DiagnosticContext::default(),
                    );
                    ui.visible_failure = Some(format!("Herdr failed: {error}"));
                    ui.dirty = true;
                    None
                }
            }
        };
        if let Some(pane_id) = target
            && self.ivars().pty.borrow().pane_id() != Some(pane_id.as_str())
        {
            let config = self.ivars().herdr.config();
            match PtySession::attach(
                &config.herdr_bin,
                config.session_name.as_deref(),
                &config.socket_path,
                &pane_id,
            ) {
                Ok(pty) => {
                    self.ivars().pty.replace(pty);
                    eprintln!(
                        "event=pty.attached operation={operation} pane_id={pane_id} retryable=false"
                    );
                }
                Err(error) => self.show_herdr_failure("pane.attach", &format!("{error:#}")),
            }
        }
        self.update_accessibility_tree();
        self.setNeedsDisplay(true);
    }

    fn show_herdr_failure(&self, operation: &str, error: &str) {
        eprintln!(
            "event=herdr.command.failed operation={operation} retryable=true error={error:?}"
        );
        let mut ui = self.ivars().ui.borrow_mut();
        ui.record_diagnostic(
            DiagnosticStage::Herdr,
            operation,
            "herdr",
            error,
            true,
            error.contains("protocol-mismatch") || error.contains("action required"),
            DiagnosticContext::default(),
        );
        ui.visible_failure = Some(format!("Herdr failed: {operation}: {error}"));
        ui.dirty = true;
        drop(ui);
        self.setNeedsDisplay(true);
    }

    fn new(
        mtm: MainThreadMarker,
        frame: NSRect,
        pty: PtySession,
        herdr: HerdrController,
        options: Options,
        started_at: Instant,
    ) -> Retained<Self> {
        let navigator = load_navigator(&options);
        let commands = load_commands(&options);
        let this = Self::alloc(mtm).set_ivars(ViewIvars {
            renderer: RefCell::new(None),
            pty: RefCell::new(pty),
            herdr,
            ui: RefCell::new(UiState::with_navigator_and_commands(navigator, commands)),
            options,
            started_at,
            first_presented_at: Cell::new(None),
            window_id: Cell::new(0),
            scale_factor: Cell::new(1.0),
            tick_count: Cell::new(0),
            probe_index: Cell::new(0),
            probe_is_marked: Cell::new(false),
            selecting_terminal: Cell::new(false),
            last_pty_size: Cell::new((0, 0)),
        });
        unsafe { msg_send![super(this), initWithFrame: frame] }
    }

    fn initialize_renderer(&self) -> Result<()> {
        self.setWantsLayer(true);
        let (width, height, scale_factor) = self.backing_dimensions();
        let pointer = NonNull::from(self).cast::<c_void>();
        let renderer = unsafe { Renderer::new(pointer, width, height) }?;
        self.ivars().scale_factor.set(scale_factor);
        self.ivars().renderer.replace(Some(renderer));
        self.sync_pty_size(width, height)?;
        self.update_accessibility_tree();
        self.setNeedsDisplay(true);
        Ok(())
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
        self.sync_pty_size(width, height)?;
        let terminal = self
            .ivars()
            .pty
            .borrow()
            .terminal()
            .snapshot()
            .context("stage=terminal.snapshot retryable=false")?;
        let model = self.ivars().ui.borrow().model(terminal);

        let observation = {
            let mut renderer = self.ivars().renderer.borrow_mut();
            let renderer = renderer
                .as_mut()
                .ok_or_else(|| anyhow!("stage=wgpu.render cause=renderer-not-initialized"))?;
            renderer.resize(width, height);
            renderer.render(&model)?
        };
        let recovered_render_failure = {
            let mut ui = self.ivars().ui.borrow_mut();
            if clear_recovered_render_failure(&mut ui.visible_failure) {
                eprintln!("event=render.recovered stage=present retryable=false");
                true
            } else {
                false
            }
        };
        let result = self.after_present(observation, scale_factor);
        if recovered_render_failure {
            // AppKit can coalesce a redraw requested from inside drawRect:.
            // Keep the model dirty so the timer reinforces the clean frame.
            self.ivars().ui.borrow_mut().dirty = true;
            self.setNeedsDisplay(true);
        }
        result
    }

    fn sync_pty_size(&self, width: u32, height: u32) -> Result<()> {
        let geometry = CanvasGeometry::for_size(width as f64, height as f64);
        let zoomed = self.ivars().ui.borrow().zoom.zoomed_pane();
        let Some(terminal_rect) = geometry.terminal_rect(zoomed) else {
            return Ok(());
        };
        let size = (
            terminal_rect.width.max(180.0) as u32,
            terminal_rect.height.max(90.0) as u32,
        );
        if self.ivars().last_pty_size.replace(size) != size {
            self.ivars().pty.borrow().resize(size.0, size.1)?;
            eprintln!(
                "event=terminal.resize width={} height={} retryable=false",
                size.0, size.1
            );
        }
        Ok(())
    }

    fn after_present(&self, observation: PresentObservation, scale_factor: f64) -> Result<()> {
        let now = Instant::now();
        if self.ivars().first_presented_at.get().is_none() {
            self.ivars().first_presented_at.set(Some(now));
            eprintln!(
                "event=app.first-usable first_usable_ms={:.3} window_id={} adapter={:?}",
                now.duration_since(self.ivars().started_at).as_secs_f64() * 1000.0,
                self.ivars().window_id.get(),
                observation.adapter_name
            );
        }
        {
            let mut ui = self.ivars().ui.borrow_mut();
            ui.note_presented(observation.input_generation, now);
            ui.dirty = false;
        }
        self.ivars().scale_factor.set(scale_factor);
        self.write_report(&observation)
    }

    fn tick_impl(&self) {
        let tick = self.ivars().tick_count.get() + 1;
        self.ivars().tick_count.set(tick);
        self.sync_terminal_generation();
        self.sync_terminal_events();
        self.sync_herdr_updates();
        self.run_probe_if_due(tick);
        if self.ivars().ui.borrow().dirty {
            self.setNeedsDisplay(true);
        }
        if let Some(limit) = self.ivars().options.autoclose_after
            && self.ivars().started_at.elapsed() >= limit
        {
            eprintln!("event=app.autoclose elapsed_ms={}", limit.as_millis());
            NSApplication::sharedApplication(self.mtm()).terminate(None);
        }
    }

    fn sync_terminal_events(&self) {
        let events = self.ivars().pty.borrow().drain_terminal_events();
        if events.is_empty() {
            return;
        }
        for event in events {
            match event {
                TerminalEvent::ClipboardStore(_kind, text) => {
                    let pasteboard = NSPasteboard::generalPasteboard();
                    pasteboard.clearContents();
                    let string_type = unsafe { NSPasteboardTypeString };
                    if !pasteboard.setString_forType(&NSString::from_str(&text), string_type) {
                        self.ivars()
                            .pty
                            .borrow()
                            .terminal()
                            .set_failure("Terminal clipboard store was rejected");
                    }
                }
                TerminalEvent::ClipboardLoad(_kind, formatter) => {
                    let string_type = unsafe { NSPasteboardTypeString };
                    let text = NSPasteboard::generalPasteboard()
                        .stringForType(string_type)
                        .map(|text| text.to_string())
                        .unwrap_or_default();
                    if let Err(error) = self.ivars().pty.borrow().write_text(&formatter(&text)) {
                        self.show_terminal_failure("osc52.load", &error);
                    }
                }
                TerminalEvent::Bell => eprintln!(
                    "event=terminal.bell pane_id={:?}",
                    self.ivars().pty.borrow().pane_id()
                ),
                TerminalEvent::Title(_) | TerminalEvent::Exit | TerminalEvent::ChildExit(_) => {}
            }
        }
        self.ivars().ui.borrow_mut().dirty = true;
    }

    fn sync_terminal_generation(&self) {
        let generation = self.ivars().pty.borrow().terminal().generation();
        let mut ui = self.ivars().ui.borrow_mut();
        if generation != ui.last_terminal_generation {
            ui.last_terminal_generation = generation;
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
            self.commit_text_from("probe.ime", &text.to_string());
            self.commit_text_from("probe.return", "\r");
            self.ivars().probe_index.set(index + 1);
            self.ivars().probe_is_marked.set(false);
        }
    }

    fn commit_text_from(&self, source: &'static str, text: &str) {
        let focused = self.ivars().ui.borrow().zoom.focused_pane();
        let bytes = text.len();
        let result = match focused {
            PaneId::TerminalA | PaneId::TerminalB => self.ivars().pty.borrow().write_text(text),
            PaneId::Editor => {
                self.ivars().ui.borrow_mut().editor_text.push_str(text);
                Ok(())
            }
            PaneId::Browser => Err(anyhow!(
                "stage=input.route target=browser cause=browser-owned-by-cef retryable=false"
            )),
        };
        self.record_routing_event(
            "app-routing",
            "input.route",
            if result.is_ok() {
                "accepted"
            } else {
                "rejected"
            },
            None,
            None,
            Some(format!("{focused:?}")),
            Some(source.to_owned()),
            Some(bytes),
        );
        if matches!(focused, PaneId::TerminalA | PaneId::TerminalB) {
            self.record_routing_event(
                "pty",
                "write",
                if result.is_ok() {
                    "accepted"
                } else {
                    "rejected"
                },
                None,
                None,
                Some(format!("{focused:?}")),
                Some(source.to_owned()),
                Some(bytes),
            );
        }
        let mut ui = self.ivars().ui.borrow_mut();
        ui.ime_marked.clear();
        ui.input_generation += 1;
        let generation = ui.input_generation;
        ui.pending_input.push_back((generation, Instant::now()));
        if let Err(error) = &result {
            let message = format!("{error:#}");
            ui.record_diagnostic(
                DiagnosticStage::Pty,
                source,
                "input",
                &message,
                true,
                false,
                DiagnosticContext::default(),
            );
            ui.visible_failure = Some(format!("Input failed: {message}"));
        } else {
            ui.visible_failure = None;
        }
        ui.dirty = true;
        drop(ui);
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
        let navigator_ax_label = match ui.navigator.preferences().active_view {
            NavigatorView::Workspaces => "Workspaces navigator",
            NavigatorView::Agents => "Agents navigator",
            NavigatorView::Worktrees => "Worktrees navigator",
        };
        let diagnostics = ui.diagnostics.entries().cloned().collect();
        let recovery = ui.diagnostics.recovery_surface(ui.projection.connection());
        let operation_manifest = ui.operation_manifest.clone();
        let report = RuntimeReport {
            schema: "herdr.ide.runtime.v1",
            build: concat!(env!("CARGO_PKG_NAME"), "@", env!("CARGO_PKG_VERSION")),
            architecture: std::env::consts::ARCH,
            process_id: std::process::id(),
            pty_child_process_id: self.ivars().pty.borrow().process_id(),
            herdr_pane_id: self.ivars().pty.borrow().pane_id().map(str::to_owned),
            window_id: self.ivars().window_id.get(),
            adapter: observation.adapter_name.clone(),
            physical_width: observation.width,
            physical_height: observation.height,
            scale_factor: self.ivars().scale_factor.get(),
            first_usable_ms: first.duration_since(self.ivars().started_at).as_secs_f64() * 1000.0,
            input_samples: ui.input_to_present_ms.len(),
            input_to_present_p95_ms: percentile_95(&ui.input_to_present_ms),
            input_to_present_max_ms: ui.input_to_present_ms.iter().copied().reduce(f64::max),
            focus: self.focus_snapshot(),
            routing_timeline: ui.routing_timeline.iter().cloned().collect(),
            zoom_state: ui.zoom.indicator(),
            ax_children: [
                navigator_ax_label,
                "Workspace tabs",
                "Terminal pane A",
                "Editor pane B",
            ],
            ime_contract: "NSTextInputClient marked-text and commit selectors route UTF-8 to WGPU text or the PTY",
            pty_contract: "portable-pty attaches the focused Herdr pane; the exact-pinned alacritty_terminal Term owns ANSI, OSC, grid, cursor, selection, scrollback, alternate-screen, and terminal modes",
            native_menu_contract: "AppKit menus and Cmd+T, Cmd+D, Cmd+Shift+D dispatch through the same Herdr topology controller; Cmd+Shift+Enter remains window-local visual zoom",
            diagnostics,
            recovery,
            operation_manifest,
        };
        drop(ui);
        write_json_atomic(path, &report)
    }

    fn update_accessibility_tree(&self) {
        let (zoomed, focused, overlay_open, overlay_text) = {
            let ui = self.ivars().ui.borrow();
            (
                ui.zoom.zoomed_pane(),
                ui.zoom.focused_pane(),
                ui.overlay.is_open(),
                ui.overlay.render_text(),
            )
        };
        let semantic = workbench_tree_with_state(
            zoomed.map(semantic_pane_name),
            Some(semantic_pane_name(focused)),
            overlay_open,
        );
        debug_assert!(semantic.validate().is_ok());
        let navigator_node = &semantic.children[0];
        let tab_node = &semantic.children[1];
        let pane_nodes = &semantic.children[2].children;
        let terminal_node = pane_nodes.iter().find(|node| {
            node.role == crate::accessibility::SemanticRole::TextArea
                && node.label.starts_with("Terminal")
        });
        let editor_node = pane_nodes.iter().find(|node| {
            node.role == crate::accessibility::SemanticRole::TextArea
                && node.label.starts_with("Editor")
        });
        let navigator_label = NSString::from_str(&navigator_node.label);
        let tab_label = NSString::from_str(&tab_node.label);
        let terminal_label = terminal_node.map(|node| NSString::from_str(&node.label));
        let editor_label = editor_node.map(|node| NSString::from_str(&node.label));
        let tab_value = tab_node.value.as_deref().map(NSString::from_str);
        let bounds = self.bounds();
        let geometry = CanvasGeometry::for_size(bounds.size.width, bounds.size.height);
        let navigator_width = geometry.navigator_width;
        let canvas_width = geometry.canvas_width;
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
                Some(&navigator_label),
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
                Some(&tab_label),
                Some(parent),
            )
        };
        let terminal_surface = geometry
            .pane_rect(PaneId::TerminalA, zoomed)
            .unwrap_or(geometry.canvas_rect());
        let editor_surface = geometry
            .pane_rect(PaneId::Editor, zoomed)
            .unwrap_or(geometry.canvas_rect());
        let terminal = unsafe {
            NSAccessibilityElement::accessibilityElementWithRole_frame_label_parent(
                NSAccessibilityTextAreaRole,
                NSRect::new(
                    NSPoint::new(terminal_surface.x, terminal_surface.y),
                    NSSize::new(terminal_surface.width, terminal_surface.height),
                ),
                terminal_label.as_deref(),
                Some(parent),
            )
        };
        let editor = unsafe {
            NSAccessibilityElement::accessibilityElementWithRole_frame_label_parent(
                NSAccessibilityTextAreaRole,
                NSRect::new(
                    NSPoint::new(editor_surface.x, editor_surface.y),
                    NSSize::new(editor_surface.width, editor_surface.height),
                ),
                editor_label.as_deref(),
                Some(parent),
            )
        };
        let overlay_label = semantic
            .children
            .iter()
            .find(|node| {
                node.role == crate::accessibility::SemanticRole::Group
                    && node.label == "Agent switcher"
            })
            .map(|node| NSString::from_str(&node.label));
        let overlay = unsafe {
            NSAccessibilityElement::accessibilityElementWithRole_frame_label_parent(
                NSAccessibilityGroupRole,
                NSRect::new(
                    NSPoint::new(navigator_width + 24.0, tab_height + 24.0),
                    NSSize::new(canvas_width - 48.0, canvas_height - 48.0),
                ),
                overlay_label.as_deref(),
                Some(parent),
            )
        };
        unsafe {
            if let Some(value) = tab_value.as_deref() {
                let _: () = msg_send![&*tab, setAccessibilityValue: &*value];
            }
            let _: () = msg_send![&*terminal, setAccessibilityFocused:
                terminal_node.is_some_and(|node| node.selected)];
            let _: () = msg_send![&*editor, setAccessibilityFocused:
                editor_node.is_some_and(|node| node.selected)];
            if overlay_open {
                let value = NSString::from_str(&overlay_text);
                let _: () = msg_send![&*overlay, setAccessibilityValue: &*value];
                let _: () = msg_send![&*overlay, setAccessibilityFocused: true];
            }
        }
        let include_terminal = terminal_node.is_some();
        let include_editor = editor_node.is_some();
        let children = match (include_terminal, include_editor, overlay_open) {
            (true, true, false) => NSArray::from_slice(&[&*navigator, &*tab, &*terminal, &*editor]),
            (true, false, false) => NSArray::from_slice(&[&*navigator, &*tab, &*terminal]),
            (false, true, false) => NSArray::from_slice(&[&*navigator, &*tab, &*editor]),
            (true, true, true) => {
                NSArray::from_slice(&[&*navigator, &*tab, &*terminal, &*editor, &*overlay])
            }
            (true, false, true) => {
                NSArray::from_slice(&[&*navigator, &*tab, &*terminal, &*overlay])
            }
            (false, true, true) => NSArray::from_slice(&[&*navigator, &*tab, &*editor, &*overlay]),
            (false, false, _) => NSArray::from_slice(&[&*navigator, &*tab]),
        };
        unsafe {
            let _: () = msg_send![self, setAccessibilityElement: false];
            let _: () = msg_send![self, setAccessibilityRole: NSAccessibilityGroupRole];
            let _: () = msg_send![self, setAccessibilityChildren: &*children];
        }
    }
}

struct AppDelegateIvars {
    window: OnceCell<Retained<NSWindow>>,
    view: OnceCell<Retained<RenderView>>,
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
    }

    impl AppDelegate {
        #[unsafe(method(createTab:))]
        fn create_tab(&self, _sender: Option<&AnyObject>) {
            if let Some(view) = self.ivars().view.get() {
                view.create_tab();
            } else {
                eprintln!("event=herdr.command.failed operation=tab.create cause=view-not-ready retryable=true");
            }
        }

        #[unsafe(method(splitPaneRight:))]
        fn split_pane_right(&self, _sender: Option<&AnyObject>) {
            if let Some(view) = self.ivars().view.get() {
                view.split_pane(SplitDirection::Right);
            } else {
                eprintln!("event=herdr.command.failed operation=pane.split-right cause=view-not-ready retryable=true");
            }
        }

        #[unsafe(method(splitPaneDown:))]
        fn split_pane_down(&self, _sender: Option<&AnyObject>) {
            if let Some(view) = self.ivars().view.get() {
                view.split_pane(SplitDirection::Down);
            } else {
                eprintln!("event=herdr.command.failed operation=pane.split-down cause=view-not-ready retryable=true");
            }
        }

        #[unsafe(method(togglePaneZoom:))]
        fn toggle_pane_zoom(&self, _sender: Option<&AnyObject>) {
            if let Some(view) = self.ivars().view.get() {
                view.toggle_zoom();
            } else {
                eprintln!(
                    "event=layout.zoom.failed stage=menu-dispatch cause=view-not-ready retryable=true"
                );
            }
        }
    }

    // SAFETY: The implemented selector has the generated protocol signature.
    unsafe impl NSWindowDelegate for AppDelegate {
        #[unsafe(method(windowDidBecomeKey:))]
        fn window_did_become_key(&self, _notification: &NSNotification) {
            if let (Some(window), Some(view)) =
                (self.ivars().window.get(), self.ivars().view.get())
                && !window.makeFirstResponder(Some(view))
            {
                eprintln!(
                    "event=input.focus.failed stage=window-became-key retryable=true"
                );
            }
        }

        #[unsafe(method(windowWillClose:))]
        fn window_will_close(&self, _notification: &NSNotification) {
            NSApplication::sharedApplication(self.mtm()).terminate(None);
        }
    }
);

impl AppDelegate {
    fn new(mtm: MainThreadMarker, options: Options, started_at: Instant) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(AppDelegateIvars {
            window: OnceCell::new(),
            view: OnceCell::new(),
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
        window.setTitle(ns_string!("Herdr IDE"));
        window.center();
        window.setContentMinSize(NSSize::new(760.0, 480.0));
        window.setDelegate(Some(ProtocolObject::from_ref(self)));

        let config = HerdrConnectionConfig::resolve(
            self.ivars().options.herdr_socket_path.clone(),
            self.ivars().options.herdr_bin.clone(),
            self.ivars().options.herdr_session.clone(),
        )?;
        let pty = match self.ivars().options.pane_id.as_deref() {
            Some(pane_id) => {
                HerdrController::execute_sync(
                    &config,
                    &TopologyOperation::PaneFocus {
                        pane_id: pane_id.to_owned(),
                    },
                )?;
                PtySession::attach(
                    &config.herdr_bin,
                    config.session_name.as_deref(),
                    &config.socket_path,
                    pane_id,
                )?
            }
            None => PtySession::detached(),
        };
        let herdr = HerdrController::start(config)?;
        let view = RenderView::new(
            mtm,
            NSRect::new(
                NSPoint::new(0.0, 0.0),
                NSSize::new(WINDOW_WIDTH, WINDOW_HEIGHT),
            ),
            pty,
            herdr,
            self.ivars().options.clone(),
            self.ivars().started_at,
        );
        window.setContentView(Some(&view));
        window.makeKeyAndOrderFront(None);
        if !window.makeFirstResponder(Some(&view)) {
            return Err(anyhow!(
                "stage=input.first-responder cause=appkit-rejected-render-view retryable=false"
            ));
        }
        view.ivars().window_id.set(window.windowNumber());
        view.initialize_renderer()?;

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
            .view
            .set(view)
            .map_err(|_| anyhow!("stage=app.retain target=view cause=already-set"))?;
        self.ivars()
            .timer
            .set(timer)
            .map_err(|_| anyhow!("stage=app.retain target=timer cause=already-set"))?;

        app.setActivationPolicy(NSApplicationActivationPolicy::Regular);
        #[allow(deprecated)]
        app.activateIgnoringOtherApps(true);
        eprintln!(
            "event=app.launched architecture={} lifecycle=appkit pixels=wgpu menu=appkit accessibility=semantic-bridge",
            std::env::consts::ARCH
        );
        Ok(())
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

fn active_terminal_pane_id(projection: &DomainProjection) -> Option<String> {
    let workspace = projection.active_workspace()?;
    let tab = workspace
        .tabs
        .iter()
        .find(|tab| tab.tab_id == workspace.active_tab_id)?;
    tab.panes
        .iter()
        .find(|pane| pane.pane_id == tab.focused_pane_id)
        .filter(|pane| matches!(pane.surface, crate::domain::SurfaceKind::Terminal))
        .map(|pane| pane.pane_id.clone())
}

fn clear_recovered_render_failure(failure: &mut Option<String>) -> bool {
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

pub fn run() -> Result<()> {
    let options = Options::parse()?;
    let started_at = Instant::now();
    let mtm = MainThreadMarker::new()
        .ok_or_else(|| anyhow!("stage=app.bootstrap cause=not-main-thread retryable=false"))?;
    let app = NSApplication::sharedApplication(mtm);
    let delegate = AppDelegate::new(mtm, options, started_at);
    app.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    install_main_menu(&app, &delegate, mtm);
    app.run();
    Ok(())
}

fn install_main_menu(app: &NSApplication, delegate: &AppDelegate, mtm: MainThreadMarker) {
    let main_menu = NSMenu::initWithTitle(NSMenu::alloc(mtm), ns_string!("Main Menu"));

    let application_root = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            ns_string!("Herdr IDE"),
            None,
            ns_string!(""),
        )
    };
    let application_menu = NSMenu::initWithTitle(NSMenu::alloc(mtm), ns_string!("Herdr IDE"));
    let quit = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            ns_string!("Quit Herdr IDE"),
            Some(sel!(terminate:)),
            ns_string!("q"),
        )
    };
    application_menu.addItem(&quit);
    application_root.setSubmenu(Some(&application_menu));
    main_menu.addItem(&application_root);

    let file_root = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            ns_string!("File"),
            None,
            ns_string!(""),
        )
    };
    let file_menu = NSMenu::initWithTitle(NSMenu::alloc(mtm), ns_string!("File"));
    let new_tab = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            ns_string!("New Tab"),
            Some(sel!(createTab:)),
            ns_string!("t"),
        )
    };
    unsafe { new_tab.setTarget(Some(delegate)) };
    file_menu.addItem(&new_tab);
    file_root.setSubmenu(Some(&file_menu));
    main_menu.addItem(&file_root);

    let pane_root = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            ns_string!("Pane"),
            None,
            ns_string!(""),
        )
    };
    let pane_menu = NSMenu::initWithTitle(NSMenu::alloc(mtm), ns_string!("Pane"));
    let split_right = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            ns_string!("Split Right"),
            Some(sel!(splitPaneRight:)),
            ns_string!("d"),
        )
    };
    unsafe { split_right.setTarget(Some(delegate)) };
    pane_menu.addItem(&split_right);
    let split_down = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            ns_string!("Split Down"),
            Some(sel!(splitPaneDown:)),
            ns_string!("d"),
        )
    };
    split_down
        .setKeyEquivalentModifierMask(NSEventModifierFlags::Command | NSEventModifierFlags::Shift);
    unsafe { split_down.setTarget(Some(delegate)) };
    pane_menu.addItem(&split_down);
    pane_root.setSubmenu(Some(&pane_menu));
    main_menu.addItem(&pane_root);

    let view_root = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            ns_string!("View"),
            None,
            ns_string!(""),
        )
    };
    let view_menu = NSMenu::initWithTitle(NSMenu::alloc(mtm), ns_string!("View"));
    let toggle_zoom = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            ns_string!("Toggle Pane Zoom"),
            Some(sel!(togglePaneZoom:)),
            ns_string!("\r"),
        )
    };
    toggle_zoom
        .setKeyEquivalentModifierMask(NSEventModifierFlags::Command | NSEventModifierFlags::Shift);
    unsafe { toggle_zoom.setTarget(Some(delegate)) };
    view_menu.addItem(&toggle_zoom);
    view_root.setSubmenu(Some(&view_menu));
    main_menu.addItem(&view_root);

    app.setMainMenu(Some(&main_menu));
}

#[cfg(test)]
mod tests {
    use super::{
        ROUTING_TRACE_LIMIT, RoutingTraceEvent, UiState, clear_recovered_render_failure,
        overlay_key_for_event, terminal_meta_text,
    };

    #[test]
    fn successful_present_clears_a_transient_render_failure() {
        let mut failure = Some("Render failed: surface temporarily unavailable".to_owned());
        assert!(clear_recovered_render_failure(&mut failure));
        assert_eq!(failure, None);
    }

    #[test]
    fn successful_present_does_not_hide_an_unrelated_failure() {
        let mut failure = Some("PTY state unavailable: poisoned lock".to_owned());
        assert!(!clear_recovered_render_failure(&mut failure));
        assert_eq!(
            failure.as_deref(),
            Some("PTY state unavailable: poisoned lock")
        );
    }

    #[test]
    fn physical_meta_letters_ignore_active_ime_composition() {
        assert_eq!(
            terminal_meta_text(3, false, Some("ㄹ")).as_deref(),
            Some("f")
        );
        assert_eq!(
            terminal_meta_text(3, true, Some("ㄹ")).as_deref(),
            Some("F")
        );
        assert_eq!(
            terminal_meta_text(51, false, None).as_deref(),
            Some("\u{7f}")
        );
    }

    #[test]
    fn overlay_navigation_key_map_is_bounded_to_overlay_controls() {
        assert_eq!(
            overlay_key_for_event(53, false),
            Some(super::OverlayKey::Escape)
        );
        assert_eq!(
            overlay_key_for_event(126, false),
            Some(super::OverlayKey::ArrowUp)
        );
        assert_eq!(
            overlay_key_for_event(125, false),
            Some(super::OverlayKey::ArrowDown)
        );
        assert_eq!(
            overlay_key_for_event(48, false),
            Some(super::OverlayKey::Tab)
        );
        assert_eq!(
            overlay_key_for_event(48, true),
            Some(super::OverlayKey::ShiftTab)
        );
        assert_eq!(
            overlay_key_for_event(36, false),
            Some(super::OverlayKey::Enter)
        );
        assert_eq!(overlay_key_for_event(0, false), None);
    }

    #[test]
    fn routing_timeline_is_bounded_and_keeps_input_and_topology_sources_distinct() {
        let mut ui = UiState::new();
        for index in 0..(ROUTING_TRACE_LIMIT + 2) {
            ui.record_routing_event(RoutingTraceEvent {
                monotonic_ns: index as u64 + 1,
                source: if index % 2 == 0 { "appkit" } else { "herdr" },
                event: if index % 2 == 0 {
                    "key_down"
                } else {
                    "topology.snapshot"
                },
                outcome: "observed",
                key_code: None,
                modifier_flags: None,
                focused_pane: None,
                pane_id: None,
                operation: None,
                bytes: None,
            });
        }
        assert_eq!(ui.routing_timeline.len(), ROUTING_TRACE_LIMIT);
        assert_eq!(ui.routing_timeline.front().unwrap().monotonic_ns, 3);
        assert_eq!(ui.routing_timeline.front().unwrap().source, "appkit");
        assert_eq!(ui.routing_timeline.get(1).unwrap().source, "herdr");
    }
}
