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
    NSEventModifierFlags, NSTextInputClient, NSView, NSWindow, NSWindowDelegate, NSWindowStyleMask,
};
use objc2_foundation::{
    MainThreadMarker, NSArray, NSAttributedString, NSAttributedStringKey, NSNotification, NSObject,
    NSObjectProtocol, NSPoint, NSRange, NSRangePointer, NSRect, NSSize, NSString, NSTimer,
    NSUInteger, ns_string,
};
use serde::Serialize;

use crate::layout::{PaneId, ZoomController, ZoomState};
use crate::pty::{PtySession, TerminalSnapshot, visible_transcript};
use crate::render::{FrameModel, PresentObservation, Renderer};

const WINDOW_WIDTH: f64 = 1180.0;
const WINDOW_HEIGHT: f64 = 720.0;
const TICK_INTERVAL_SECONDS: f64 = 0.016;

#[derive(Clone, Debug)]
struct Options {
    report_path: Option<PathBuf>,
    probe_count: usize,
    autoclose_after: Option<Duration>,
}

impl Options {
    fn parse() -> Result<Self> {
        let mut options = Self {
            report_path: None,
            probe_count: 0,
            autoclose_after: None,
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
                "--help" => {
                    println!(
                        "herdr-ide-native-spike [--report PATH] [--probe-count N] [--autoclose-ms N]"
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
struct UiState {
    zoom: ZoomController,
    editor_text: String,
    ime_marked: String,
    ime_committed: String,
    visible_failure: Option<String>,
    input_generation: u64,
    pending_input: VecDeque<(u64, Instant)>,
    input_to_present_ms: Vec<f64>,
    last_terminal_generation: u64,
    dirty: bool,
}

impl UiState {
    fn new() -> Self {
        Self {
            zoom: ZoomController::new(vec![0.55, 0.45], PaneId::TerminalA),
            editor_text: "# native-workbench.rs\n\nAppKit owns lifecycle and native views.\nWGPU owns IDE, terminal, and editor pixels.\nCEF stays a separate native child view.".to_owned(),
            ime_marked: String::new(),
            ime_committed: String::new(),
            visible_failure: None,
            input_generation: 0,
            pending_input: VecDeque::new(),
            input_to_present_ms: Vec::new(),
            last_terminal_generation: 0,
            dirty: true,
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
            ime_marked: self.ime_marked.clone(),
            ime_committed: self.ime_committed.clone(),
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
    started_at: Instant,
    first_presented_at: Cell<Option<Instant>>,
    window_id: Cell<isize>,
    scale_factor: Cell<f64>,
    tick_count: Cell<u64>,
    probe_index: Cell<usize>,
    probe_is_marked: Cell<bool>,
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
                self.ivars().ui.borrow_mut().visible_failure = Some(message.clone());
                eprintln!("event=render.failed stage=draw retryable=true error={message:?}");
            }
        }

        #[unsafe(method(keyDown:))]
        fn key_down(&self, event: &NSEvent) {
            let flags = event.modifierFlags();
            let is_zoom = flags.contains(NSEventModifierFlags::Command)
                && flags.contains(NSEventModifierFlags::Shift)
                && matches!(event.keyCode(), 36 | 76);
            if is_zoom {
                let outcome = {
                    let mut ui = self.ivars().ui.borrow_mut();
                    let focused = ui.zoom.current().focused;
                    let outcome = ui.zoom.toggle(Some(focused));
                    ui.dirty = true;
                    outcome
                };
                eprintln!("event=layout.zoom outcome={outcome:?}");
                self.update_accessibility_tree();
                self.setNeedsDisplay(true);
                return;
            }
            self.interpretKeyEvents(&NSArray::from_slice(&[event]));
        }

        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, event: &NSEvent) {
            let point = self.convertPoint_fromView(event.locationInWindow(), None);
            let bounds = self.bounds();
            let navigator_width = (bounds.size.width * 0.22).clamp(190.0, 260.0);
            let split = navigator_width + (bounds.size.width - navigator_width) * 0.55;
            let pane = if point.x >= split { PaneId::Editor } else { PaneId::TerminalA };
            let mut ui = self.ivars().ui.borrow_mut();
            ui.zoom.set_focus(pane);
            ui.dirty = true;
            drop(ui);
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
            self.commit_text(&objc_text(string));
        }

        #[unsafe(method(doCommandBySelector:))]
        unsafe fn do_command_by_selector(&self, selector: Sel) {
            if selector == sel!(insertNewline:) {
                self.commit_text("\r");
            } else if selector == sel!(deleteBackward:) {
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
    fn new(
        mtm: MainThreadMarker,
        frame: NSRect,
        pty: PtySession,
        options: Options,
        started_at: Instant,
    ) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(ViewIvars {
            renderer: RefCell::new(None),
            pty,
            ui: RefCell::new(UiState::new()),
            options,
            started_at,
            first_presented_at: Cell::new(None),
            window_id: Cell::new(0),
            scale_factor: Cell::new(1.0),
            tick_count: Cell::new(0),
            probe_index: Cell::new(0),
            probe_is_marked: Cell::new(false),
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
        self.ivars()
            .pty
            .resize(width.saturating_sub(260), height.saturating_sub(80))?;
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
        let terminal_snapshot = self.ivars().pty.snapshot();
        let terminal = terminal_snapshot
            .lock()
            .map_err(|_| anyhow!("stage=terminal.snapshot cause=poisoned-lock retryable=false"))?;
        let model = self.ivars().ui.borrow().model(&terminal);
        drop(terminal);

        let observation = {
            let mut renderer = self.ivars().renderer.borrow_mut();
            let renderer = renderer
                .as_mut()
                .ok_or_else(|| anyhow!("stage=wgpu.render cause=renderer-not-initialized"))?;
            renderer.resize(width, height);
            renderer.render(&model)?
        };
        self.after_present(observation, scale_factor)
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
            "\r" => "Return".to_owned(),
            "\u{7f}" => "Delete".to_owned(),
            _ => text.to_owned(),
        };
        ui.input_generation += 1;
        let generation = ui.input_generation;
        ui.pending_input.push_back((generation, Instant::now()));
        ui.visible_failure = result.err().map(|error| format!("Input failed: {error:#}"));
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

    fn update_accessibility_tree(&self) {
        let bounds = self.bounds();
        let navigator_width = (bounds.size.width * 0.22).clamp(190.0, 260.0);
        let canvas_width = bounds.size.width - navigator_width;
        let split = navigator_width + canvas_width * 0.55;
        let tab_height = 46.0;
        let canvas_height = bounds.size.height - tab_height - 30.0;
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

    // SAFETY: The implemented selector has the generated protocol signature.
    unsafe impl NSWindowDelegate for AppDelegate {
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
        window.setTitle(ns_string!("Herdr IDE Native Spike"));
        window.center();
        window.setContentMinSize(NSSize::new(760.0, 480.0));
        window.setDelegate(Some(ProtocolObject::from_ref(self)));

        let pty = PtySession::spawn()?;
        let view = RenderView::new(
            mtm,
            NSRect::new(
                NSPoint::new(0.0, 0.0),
                NSSize::new(WINDOW_WIDTH, WINDOW_HEIGHT),
            ),
            pty,
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
            "event=app.launched architecture={} lifecycle=appkit pixels=wgpu terminal=portable-pty",
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
    app.run();
    Ok(())
}
