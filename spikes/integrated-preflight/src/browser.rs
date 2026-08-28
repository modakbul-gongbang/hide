use std::cell::RefCell;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use cef::{args::Args, sys::cef_window_handle_t, *};
use objc2::{Message, rc::Retained};
use objc2_app_kit::{NSAutoresizingMaskOptions, NSResponder, NSView};

use crate::macos_app;

const CEF_VERSION: &str = "151.8.0+151.3.24";

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BrowserMode {
    Closed,
    Open(BrowserConfig),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BrowserConfig {
    pub url: String,
    pub profile_dir: PathBuf,
    pub remote_debugging_port: i32,
}

pub enum Bootstrap {
    Subprocess(i32),
    Main(Option<BrowserRuntime>),
}

pub struct BrowserRuntime {
    _loader: library_loader::LibraryLoader,
}

#[derive(Default)]
struct BrowserState {
    config: Option<BrowserConfig>,
    context_ready: bool,
    create_requested: bool,
    parent: Option<Retained<NSView>>,
    child: Option<Retained<NSView>>,
    browser: Option<Browser>,
    closing: bool,
}

thread_local! {
    static STATE: RefCell<BrowserState> = RefCell::new(BrowserState::default());
}

pub fn parse_browser_mode(arguments: impl IntoIterator<Item = String>) -> Result<BrowserMode> {
    let mut closed = false;
    let mut url = None;
    let mut profile = None;
    let mut debug_port = None;
    let mut preflight_mode = None;
    let mut preflight_url = None;
    let mut preflight_profile = None;
    let mut preflight_debug_port = None;
    let mut arguments = arguments.into_iter().skip(1);
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--browser-closed" => closed = true,
            "--browser-url" => url = Some(required_next(&mut arguments, "--browser-url")?),
            "--browser-profile" => {
                profile = Some(PathBuf::from(required_next(
                    &mut arguments,
                    "--browser-profile",
                )?))
            }
            "--remote-debugging-port" => {
                let raw = required_next(&mut arguments, "--remote-debugging-port")?;
                debug_port = Some(raw.parse::<i32>().with_context(|| {
                    format!("stage=browser.config option=--remote-debugging-port value={raw:?}")
                })?);
            }
            "--t1-browser-mode" => {
                preflight_mode = Some(required_next(&mut arguments, "--t1-browser-mode")?);
            }
            "--t1-browser-url" => {
                preflight_url = Some(required_next(&mut arguments, "--t1-browser-url")?);
            }
            "--t1-browser-profile" => {
                preflight_profile = Some(PathBuf::from(required_next(
                    &mut arguments,
                    "--t1-browser-profile",
                )?));
            }
            "--t1-remote-debugging-port" => {
                let raw = required_next(&mut arguments, "--t1-remote-debugging-port")?;
                preflight_debug_port = Some(raw.parse::<i32>().with_context(|| {
                    format!("stage=browser.config option=--t1-remote-debugging-port value={raw:?}")
                })?);
            }
            _ => {}
        }
    }

    if let Some(mode) = preflight_mode {
        match mode.as_str() {
            "browser-closed" => return Ok(BrowserMode::Closed),
            "browser-included" => {
                url = preflight_url;
                profile = preflight_profile;
                debug_port = preflight_debug_port;
            }
            _ => {
                return Err(anyhow!(
                    "stage=browser.config option=--t1-browser-mode cause=invalid-value value={mode:?}"
                ));
            }
        }
    }

    if closed {
        if url.is_some() || profile.is_some() || debug_port.is_some() {
            return Err(anyhow!(
                "stage=browser.config cause=closed-mode-conflicts-with-browser-options"
            ));
        }
        return Ok(BrowserMode::Closed);
    }
    let Some(url) = url else {
        return Ok(BrowserMode::Closed);
    };
    if !url.starts_with("http://127.0.0.1:") {
        return Err(anyhow!(
            "stage=browser.config option=--browser-url cause=literal-ipv4-loopback-required"
        ));
    }
    let profile_dir = profile.ok_or_else(|| {
        anyhow!("stage=browser.config option=--browser-profile cause=required-when-open")
    })?;
    if !profile_dir.is_absolute() {
        return Err(anyhow!(
            "stage=browser.config option=--browser-profile cause=absolute-path-required"
        ));
    }
    let remote_debugging_port = debug_port.ok_or_else(|| {
        anyhow!("stage=browser.config option=--remote-debugging-port cause=required-when-open")
    })?;
    if !(1024..=65535).contains(&remote_debugging_port) {
        return Err(anyhow!(
            "stage=browser.config option=--remote-debugging-port cause=out-of-range"
        ));
    }
    Ok(BrowserMode::Open(BrowserConfig {
        url,
        profile_dir,
        remote_debugging_port,
    }))
}

fn required_next(
    arguments: &mut impl Iterator<Item = String>,
    option: &'static str,
) -> Result<String> {
    arguments
        .next()
        .ok_or_else(|| anyhow!("stage=browser.config option={option} cause=missing-value"))
}

pub fn bootstrap() -> Result<Bootstrap> {
    let mode = parse_browser_mode(std::env::args())?;
    let BrowserMode::Open(config) = mode else {
        eprintln!("event=browser.closed helpers_expected=0 cef_initialized=false");
        return Ok(Bootstrap::Main(None));
    };

    std::fs::create_dir_all(&config.profile_dir).with_context(|| {
        format!(
            "stage=browser.profile.mkdir path={}",
            config.profile_dir.display()
        )
    })?;
    let executable = std::env::current_exe().context("stage=browser.executable")?;
    let loader = library_loader::LibraryLoader::new(&executable, false);
    if !loader.load() {
        return Err(anyhow!(
            "stage=browser.cef.load cause=framework-load-failed"
        ));
    }
    let _ = api_hash(sys::CEF_API_VERSION_LAST, 0);
    let args = Args::new();
    let command_line = args
        .as_cmd_line()
        .ok_or_else(|| anyhow!("stage=browser.command-line cause=cef-parse-failed"))?;
    let process_type =
        CefString::from(&command_line.switch_value(Some(&CefString::from("type")))).to_string();
    if process_type.is_empty() {
        macos_app::initialize().map_err(|error| anyhow!("stage=appkit.cef-class cause={error}"))?;
    }
    let process_exit = execute_process(
        Some(args.as_main_args()),
        None::<&mut App>,
        std::ptr::null_mut(),
    );
    if !process_type.is_empty() {
        return Ok(Bootstrap::Subprocess(process_exit.max(0)));
    }
    if process_exit != -1 {
        return Err(anyhow!(
            "stage=browser.process-route cause=unexpected-code code={process_exit}"
        ));
    }

    let profile = absolute_utf8(&config.profile_dir)?;
    let log_path = config.profile_dir.join("cef-debug.log");
    let log_path = absolute_utf8(&log_path)?;
    STATE.with(|state| state.borrow_mut().config = Some(config.clone()));
    let settings = Settings {
        no_sandbox: 0,
        multi_threaded_message_loop: 0,
        cache_path: CefString::from(profile),
        root_cache_path: CefString::from(profile),
        persist_session_cookies: 1,
        remote_debugging_port: config.remote_debugging_port,
        log_file: CefString::from(log_path),
        log_severity: LogSeverity::WARNING,
        ..Default::default()
    };
    let mut app = IntegratedCefApp::new();
    if initialize(
        Some(args.as_main_args()),
        Some(&settings),
        Some(&mut app),
        std::ptr::null_mut(),
    ) != 1
    {
        return Err(anyhow!("stage=browser.cef.initialize cause=returned-false"));
    }
    eprintln!(
        "event=browser.cef.initialized version={CEF_VERSION} cdp=127.0.0.1:{} profile={}",
        config.remote_debugging_port,
        config.profile_dir.display()
    );
    Ok(Bootstrap::Main(Some(BrowserRuntime { _loader: loader })))
}

fn absolute_utf8(path: &Path) -> Result<&str> {
    path.to_str()
        .ok_or_else(|| anyhow!("stage=browser.path cause=non-utf8 path={}", path.display()))
}

impl BrowserRuntime {
    pub fn shutdown(&mut self) -> Result<()> {
        if browser_is_open() {
            return Err(anyhow!(
                "stage=browser.cef.shutdown cause=browser-still-open retryable=false"
            ));
        }
        STATE.with(|state| *state.borrow_mut() = BrowserState::default());
        eprintln!("event=browser.cef.shutdown begin=true");
        shutdown();
        eprintln!("event=browser.cef.shutdown complete=true");
        Ok(())
    }
}

pub fn attach_parent(parent: &NSView) {
    STATE.with(|state| state.borrow_mut().parent = Some(parent.retain()));
    eprintln!("event=browser.create.deferred gate=cef-context-ready");
    create_if_ready();
}

pub fn detach_native_child_for_window_close() {
    let child = STATE.with(|state| {
        let mut state = state.borrow_mut();
        state.parent = None;
        state.child.take()
    });
    if let Some(child) = child {
        child.removeFromSuperview();
        eprintln!("event=browser.native-child.detached reason=top-level-window-close");
    } else {
        eprintln!(
            "event=browser.native-child.detach-skipped cause=child-unavailable retryable=false"
        );
    }
}

pub fn layout_native_child() {
    STATE.with(|state| {
        let state = state.borrow();
        let (Some(parent), Some(child), Some(browser)) =
            (&state.parent, &state.child, &state.browser)
        else {
            return;
        };
        child.setFrame(parent.bounds());
        if let Some(host) = browser.host() {
            host.notify_move_or_resize_started();
        }
        let frame = child.frame();
        let scale = child
            .window()
            .map(|window| window.backingScaleFactor())
            .unwrap_or(0.0);
        eprintln!(
            "event=browser.layout width={:.0} height={:.0} scale={scale:.2}",
            frame.size.width, frame.size.height
        );
    });
}

pub fn focus_browser() -> bool {
    STATE.with(|state| {
        let state = state.borrow();
        let Some(child) = &state.child else {
            return false;
        };
        if let Some(host) = state.browser.as_ref().and_then(Browser::host) {
            host.set_focus(1);
        }
        child
            .window()
            .map(|window| window.makeFirstResponder(Some(child)))
            .unwrap_or(false)
    })
}

pub fn blur_browser() {
    STATE.with(|state| {
        if let Some(host) = state.borrow().browser.as_ref().and_then(Browser::host) {
            host.set_focus(0);
        }
    });
}

pub fn owns_first_responder() -> bool {
    STATE.with(|state| {
        let state = state.borrow();
        let Some(child) = &state.child else {
            return false;
        };
        let Some(window) = child.window() else {
            return false;
        };
        let target = Retained::as_ptr(child).cast::<NSResponder>();
        let mut responder = window.firstResponder();
        while let Some(current) = responder {
            if Retained::as_ptr(&current) == target {
                return true;
            }
            responder = unsafe { current.nextResponder() };
        }
        false
    })
}

pub fn browser_is_open() -> bool {
    STATE.with(|state| state.borrow().browser.is_some())
}

pub fn current_config() -> Option<BrowserConfig> {
    STATE.with(|state| state.borrow().config.clone())
}

pub fn request_app_quit() {
    STATE.with(|state| state.borrow_mut().closing = true);
    if browser_is_open() {
        request_close(false);
    } else {
        quit_message_loop();
        eprintln!("event=browser.message-loop.quit requested=true owner=cef no_browser=true");
    }
}

pub fn is_closing() -> bool {
    STATE.with(|state| state.borrow().closing)
}

fn request_close(force: bool) {
    let host = STATE.with(|state| state.borrow().browser.as_ref().and_then(Browser::host));
    if let Some(host) = host {
        host.close_browser(i32::from(force));
    }
}

fn create_if_ready() {
    let request = STATE.with(|state| {
        let mut state = state.borrow_mut();
        if !state.context_ready || state.create_requested {
            return None;
        }
        let parent = state.parent.clone()?;
        let config = state.config.clone()?;
        state.create_requested = true;
        Some((parent, config))
    });
    let Some((parent, config)) = request else {
        return;
    };
    let bounds = parent.bounds();
    let parent_handle: cef_window_handle_t = Retained::as_ptr(&parent).cast_mut().cast();
    let cef_bounds = Rect {
        x: 0,
        y: 0,
        width: bounds.size.width.round().max(1.0) as i32,
        height: bounds.size.height.round().max(1.0) as i32,
    };
    let window_info = WindowInfo::default().set_as_child(parent_handle, &cef_bounds);
    let mut client = IntegratedClient::new();
    let created = browser_host_create_browser(
        Some(&window_info),
        Some(&mut client),
        Some(&CefString::from(config.url.as_str())),
        Some(&BrowserSettings::default()),
        None,
        None,
    );
    if created != 1 {
        eprintln!("event=browser.create.failed retryable=false cause=cef-returned-false");
        STATE.with(|state| state.borrow_mut().create_requested = false);
    } else {
        eprintln!("event=browser.create.requested owner=cef-native-child-nsview");
    }
}

wrap_app! {
    struct IntegratedCefApp;
    impl App {
        fn browser_process_handler(&self) -> Option<BrowserProcessHandler> {
            Some(IntegratedBrowserProcessHandler::new())
        }
    }
}

wrap_browser_process_handler! {
    struct IntegratedBrowserProcessHandler;
    impl BrowserProcessHandler {
        fn on_context_initialized(&self) {
            STATE.with(|state| state.borrow_mut().context_ready = true);
            eprintln!("event=browser.context.ready");
            create_if_ready();
        }
    }
}

wrap_client! {
    struct IntegratedClient;
    impl Client {
        fn life_span_handler(&self) -> Option<LifeSpanHandler> {
            Some(IntegratedLifeSpanHandler::new())
        }
    }
}

wrap_life_span_handler! {
    struct IntegratedLifeSpanHandler;
    impl LifeSpanHandler {
        fn on_after_created(&self, browser: Option<&mut Browser>) {
            let Some(browser) = browser.cloned() else {
                eprintln!("event=browser.create.failed retryable=false cause=missing-browser");
                return;
            };
            let Some(host) = browser.host() else {
                eprintln!("event=browser.create.failed retryable=false cause=missing-host");
                return;
            };
            let pointer = host.window_handle().cast::<NSView>();
            let Some(child) = (unsafe { pointer.as_ref() }) else {
                eprintln!("event=browser.create.failed retryable=false cause=missing-native-child");
                return;
            };
            let child = child.retain();
            child.setAutoresizingMask(
                NSAutoresizingMaskOptions::ViewWidthSizable
                    | NSAutoresizingMaskOptions::ViewHeightSizable,
            );
            STATE.with(|state| {
                let mut state = state.borrow_mut();
                state.child = Some(child);
                state.browser = Some(browser);
            });
            layout_native_child();
            crate::app::browser_ready();
            eprintln!("event=browser.ready native_child_attached=true");
        }

        fn do_close(&self, _browser: Option<&mut Browser>) -> i32 {
            eprintln!("event=browser.close.ready action=allow-cef-standard-window-close");
            0
        }

        fn on_before_close(&self, _browser: Option<&mut Browser>) {
            let closing = STATE.with(|state| {
                let mut state = state.borrow_mut();
                state.browser = None;
                state.child = None;
                state.closing
            });
            if closing {
                quit_message_loop();
                crate::app::stop_native_message_loop();
                eprintln!("event=browser.message-loop.quit requested=true owner=cef");
            }
            eprintln!("event=browser.closed closing={closing} quit_deferred={closing}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn browser_closed_is_cef_dormant() {
        assert_eq!(
            parse_browser_mode(args(&["app", "--browser-closed"])).unwrap(),
            BrowserMode::Closed
        );
    }

    #[test]
    fn browser_open_requires_literal_ipv4_loopback() {
        let error = parse_browser_mode(args(&[
            "app",
            "--browser-url",
            "http://localhost:9000/",
            "--browser-profile",
            "/tmp/herdr-profile",
            "--remote-debugging-port",
            "9222",
        ]))
        .unwrap_err();
        assert!(error.to_string().contains("literal-ipv4-loopback-required"));
    }

    #[test]
    fn open_mode_is_explicit_and_complete() {
        let mode = parse_browser_mode(args(&[
            "app",
            "--browser-url",
            "http://127.0.0.1:9000/",
            "--browser-profile",
            "/tmp/herdr-profile",
            "--remote-debugging-port",
            "9222",
        ]))
        .unwrap();
        assert!(matches!(mode, BrowserMode::Open(_)));
    }

    #[test]
    fn preflight_closed_mode_keeps_cef_dormant_with_included_config_present() {
        let mode = parse_browser_mode(args(&[
            "app",
            "--t1-browser-mode",
            "browser-closed",
            "--t1-browser-url",
            "http://127.0.0.1:9000/",
            "--t1-browser-profile",
            "/tmp/herdr-profile",
            "--t1-remote-debugging-port",
            "9222",
        ]))
        .unwrap();
        assert_eq!(mode, BrowserMode::Closed);
    }

    #[test]
    fn preflight_included_mode_uses_its_owned_config() {
        let mode = parse_browser_mode(args(&[
            "app",
            "--t1-browser-mode",
            "browser-included",
            "--t1-browser-url",
            "http://127.0.0.1:9000/",
            "--t1-browser-profile",
            "/tmp/herdr-profile",
            "--t1-remote-debugging-port",
            "9222",
        ]))
        .unwrap();
        assert!(matches!(mode, BrowserMode::Open(_)));
    }
}
