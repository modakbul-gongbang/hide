use crate::macos_app;
use cef::{args::Args, sys::cef_window_handle_t, *};
use objc2::{MainThreadMarker, MainThreadOnly, Message, rc::Retained};
use objc2_app_kit::{
    NSApp, NSApplicationActivationPolicy, NSAutoresizingMaskOptions, NSBackingStoreType, NSView,
    NSWindow, NSWindowStyleMask,
};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};
use std::{
    cell::RefCell,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

const CEF_VERSION: &str = "151.8.0+151.3.24";
const WINDOW_WIDTH: i32 = 1024;
const WINDOW_HEIGHT: i32 = 700;

struct BrowserConfig {
    url: String,
    profile_dir: PathBuf,
    remote_debugging_port: i32,
}

impl BrowserConfig {
    fn from_command_line(command_line: &CommandLine) -> Result<Self, String> {
        let url = required_switch(command_line, "url")?;
        if !url.starts_with("http://127.0.0.1:") {
            return Err(
                "--url must use the literal IPv4 loopback host 127.0.0.1 and an explicit port"
                    .to_owned(),
            );
        }

        let profile_dir = PathBuf::from(required_switch(command_line, "probe-profile-dir")?);
        if !profile_dir.is_absolute() {
            return Err("--probe-profile-dir must be an absolute owned fixture path".to_owned());
        }
        std::fs::create_dir_all(&profile_dir)
            .map_err(|error| format!("cannot create profile directory: {error}"))?;

        let remote_debugging_port = required_switch(command_line, "remote-debugging-port")?
            .parse::<i32>()
            .map_err(|_| "--remote-debugging-port must be an integer".to_owned())?;
        if !(1024..=65535).contains(&remote_debugging_port) {
            return Err("--remote-debugging-port must be between 1024 and 65535".to_owned());
        }

        Ok(Self {
            url,
            profile_dir,
            remote_debugging_port,
        })
    }
}

fn required_switch(command_line: &CommandLine, name: &str) -> Result<String, String> {
    let name = CefString::from(name);
    let value = CefString::from(&command_line.switch_value(Some(&name))).to_string();
    if value.is_empty() {
        Err(format!("missing required --{}=<value>", name.to_string()))
    } else {
        Ok(value)
    }
}

pub fn run() -> Result<(), String> {
    let executable = std::env::current_exe()
        .map_err(|error| format!("cannot resolve current executable: {error}"))?;
    let loader = library_loader::LibraryLoader::new(&executable, false);
    if !loader.load() {
        return Err("cannot load bundled Chromium Embedded Framework.framework".to_owned());
    }
    // Every cef-rs API wrapper must be initialized against the loaded CEF ABI
    // before the first CEF function call, including command-line creation.
    let _ = api_hash(sys::CEF_API_VERSION_LAST, 0);

    let args = Args::new();
    let command_line = args
        .as_cmd_line()
        .ok_or_else(|| "CEF could not parse process command line".to_owned())?;

    let process_type =
        CefString::from(&command_line.switch_value(Some(&CefString::from("type")))).to_string();
    if process_type.is_empty() {
        macos_app::initialize().map_err(str::to_owned)?;
    }

    let process_exit = execute_process(
        Some(args.as_main_args()),
        None::<&mut App>,
        std::ptr::null_mut(),
    );
    if !process_type.is_empty() {
        if process_exit < 0 {
            return Err(format!(
                "CEF subprocess {process_type} returned invalid exit code {process_exit}"
            ));
        }
        return Ok(());
    }
    if process_exit != -1 {
        return Err(format!(
            "CEF browser process routing returned unexpected code {process_exit}"
        ));
    }

    let config = BrowserConfig::from_command_line(&command_line)?;
    let profile_dir = absolute_utf8(&config.profile_dir)?;
    let log_file = config.profile_dir.join("cef-debug.log");
    let log_file = absolute_utf8(&log_file)?;

    eprintln!(
        "{{\"event\":\"probe.start\",\"cef_version\":\"{CEF_VERSION}\",\"pid\":{},\"debug_port\":{}}}",
        std::process::id(),
        config.remote_debugging_port
    );

    let settings = Settings {
        no_sandbox: 0,
        cache_path: CefString::from(profile_dir),
        root_cache_path: CefString::from(profile_dir),
        persist_session_cookies: 1,
        remote_debugging_port: config.remote_debugging_port,
        log_file: CefString::from(log_file),
        log_severity: LogSeverity::WARNING,
        ..Default::default()
    };

    let lifecycle = ProbeLifecycle::new();
    let mut application = ProbeApp::new(config.url, lifecycle);
    if initialize(
        Some(args.as_main_args()),
        Some(&settings),
        Some(&mut application),
        std::ptr::null_mut(),
    ) != 1
    {
        return Err("cef_initialize returned failure".to_owned());
    }

    run_message_loop();
    shutdown();
    eprintln!("{{\"event\":\"probe.shutdown.complete\"}}");
    Ok(())
}

fn absolute_utf8(path: &Path) -> Result<&str, String> {
    path.to_str()
        .ok_or_else(|| "owned fixture path is not valid UTF-8".to_owned())
}

wrap_app! {
    struct ProbeApp {
        url: String,
        lifecycle: Arc<Mutex<ProbeLifecycle>>,
    }

    impl App {
        fn browser_process_handler(&self) -> Option<BrowserProcessHandler> {
            Some(ProbeBrowserProcessHandler::new(
                self.url.clone(),
                self.lifecycle.clone(),
                RefCell::new(None),
            ))
        }
    }
}

wrap_browser_process_handler! {
    struct ProbeBrowserProcessHandler {
        url: String,
        lifecycle: Arc<Mutex<ProbeLifecycle>>,
        window: RefCell<Option<Retained<NSWindow>>>,
    }

    impl BrowserProcessHandler {
        fn on_context_initialized(&self) {
            let (window, content_view) = match create_parent_window() {
                Ok(value) => value,
                Err(error) => {
                    eprintln!(
                        "{{\"event\":\"probe.failed\",\"stage\":\"appkit_parent\",\"error\":{error:?}}}"
                    );
                    quit_message_loop();
                    return;
                }
            };

            let parent_view: cef_window_handle_t =
                Retained::as_ptr(&content_view).cast_mut().cast();
            let bounds = Rect {
                x: 0,
                y: 0,
                width: WINDOW_WIDTH,
                height: WINDOW_HEIGHT,
            };
            let window_info = WindowInfo::default().set_as_child(parent_view, &bounds);
            let mut client = ProbeClient::new(self.lifecycle.clone());
            let created = browser_host_create_browser(
                Some(&window_info),
                Some(&mut client),
                Some(&CefString::from(self.url.as_str())),
                Some(&BrowserSettings::default()),
                None,
                None,
            );
            if created != 1 {
                eprintln!(
                    "{{\"event\":\"probe.failed\",\"stage\":\"cef_child_create\",\"error\":\"browser_host_create_browser returned false\"}}"
                );
                quit_message_loop();
                return;
            }

            window.makeKeyAndOrderFront(None);
            *self.window.borrow_mut() = Some(window);
            eprintln!(
                "{{\"event\":\"probe.browser.create_requested\",\"owner\":\"cef_native_child_nsview\"}}"
            );
        }
    }
}

struct ProbeLifecycle {
    browsers: Vec<Browser>,
}

impl ProbeLifecycle {
    fn new() -> Arc<Mutex<Self>> {
        Arc::new(Mutex::new(Self {
            browsers: Vec::new(),
        }))
    }
}

wrap_client! {
    struct ProbeClient {
        lifecycle: Arc<Mutex<ProbeLifecycle>>,
    }

    impl Client {
        fn life_span_handler(&self) -> Option<LifeSpanHandler> {
            Some(ProbeLifeSpanHandler::new(self.lifecycle.clone()))
        }
    }
}

wrap_life_span_handler! {
    struct ProbeLifeSpanHandler {
        lifecycle: Arc<Mutex<ProbeLifecycle>>,
    }

    impl LifeSpanHandler {
        fn on_after_created(&self, browser: Option<&mut Browser>) {
            let Some(browser) = browser.cloned() else {
                eprintln!(
                    "{{\"event\":\"probe.failed\",\"stage\":\"browser_after_created\",\"error\":\"browser missing\"}}"
                );
                quit_message_loop();
                return;
            };
            let Some(host) = browser.host() else {
                eprintln!(
                    "{{\"event\":\"probe.failed\",\"stage\":\"browser_after_created\",\"error\":\"host missing\"}}"
                );
                quit_message_loop();
                return;
            };

            let view_ptr = host.window_handle().cast::<NSView>();
            let Some(view) = (unsafe { view_ptr.as_ref() }) else {
                eprintln!(
                    "{{\"event\":\"probe.failed\",\"stage\":\"browser_after_created\",\"error\":\"CEF native child NSView missing\"}}"
                );
                quit_message_loop();
                return;
            };
            let view = view.retain();
            view.setAutoresizingMask(
                NSAutoresizingMaskOptions::ViewWidthSizable
                    | NSAutoresizingMaskOptions::ViewHeightSizable,
            );

            // SAFETY: CEF returned a live retained NSView on its UI thread; querying
            // its current AppKit parent does not mutate the hierarchy.
            let parent_attached = unsafe { view.superview() }.is_some();
            let frame = view.frame();
            let scale = view
                .window()
                .map(|window| window.backingScaleFactor())
                .unwrap_or(0.0);
            let first_responder = view
                .window()
                .map(|window| window.makeFirstResponder(Some(&view)))
                .unwrap_or(false);

            let mut lifecycle = self.lifecycle.lock().expect("lifecycle mutex poisoned");
            lifecycle.browsers.push(browser);
            eprintln!(
                "{{\"event\":\"probe.browser.ready\",\"native_child_attached\":{parent_attached},\"first_responder\":{first_responder},\"frame_points\":[{:.0},{:.0}],\"backing_scale\":{scale:.2}}}",
                frame.size.width,
                frame.size.height
            );
        }

        fn on_before_close(&self, browser: Option<&mut Browser>) {
            let mut lifecycle = self.lifecycle.lock().expect("lifecycle mutex poisoned");
            if let Some(browser) = browser {
                lifecycle
                    .browsers
                    .retain_mut(|candidate| candidate.is_same(Some(browser)) == 0);
            }
            let remaining = lifecycle.browsers.len();
            eprintln!(
                "{{\"event\":\"probe.browser.closed\",\"remaining\":{remaining}}}"
            );
            if remaining == 0 {
                quit_message_loop();
            }
        }
    }
}

fn create_parent_window() -> Result<(Retained<NSWindow>, Retained<NSView>), String> {
    let main_thread = MainThreadMarker::new()
        .ok_or_else(|| "AppKit parent window creation is not on main thread".to_owned())?;
    let application = NSApp(main_thread);
    let activation_changed =
        application.setActivationPolicy(NSApplicationActivationPolicy::Regular);
    let activation_policy = application.activationPolicy();
    if activation_policy != NSApplicationActivationPolicy::Regular {
        return Err(format!(
            "cannot set regular AppKit activation policy: changed={activation_changed}, actual={}",
            activation_policy.0
        ));
    }

    let rect = NSRect::new(
        NSPoint::new(0.0, 0.0),
        NSSize::new(WINDOW_WIDTH as f64, WINDOW_HEIGHT as f64),
    );
    let style = NSWindowStyleMask::Titled
        | NSWindowStyleMask::Closable
        | NSWindowStyleMask::Miniaturizable
        | NSWindowStyleMask::Resizable;
    let window = unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(
            NSWindow::alloc(main_thread),
            rect,
            style,
            NSBackingStoreType::Buffered,
            false,
        )
    };
    window.setTitle(&NSString::from_str("Herdr CEF Native Child Probe"));
    window.center();
    let content_view = window
        .contentView()
        .ok_or_else(|| "AppKit window has no content view".to_owned())?;
    application.activate();

    Ok((window, content_view))
}
