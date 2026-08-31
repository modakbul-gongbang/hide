#[cfg(not(target_os = "macos"))]
compile_error!("herdr-integrated-preflight-helper is macOS-only");

#[cfg(target_os = "macos")]
fn main() {
    match run() {
        Ok(code) => std::process::exit(code.max(0)),
        Err(error) => {
            eprintln!("event=browser.helper.failed retryable=false error={error}");
            std::process::exit(1);
        }
    }
}

#[cfg(target_os = "macos")]
fn run() -> Result<i32, String> {
    let executable = std::env::current_exe()
        .map_err(|error| format!("stage=browser.helper.executable cause={error}"))?;
    let args = cef::args::Args::new();

    // CEF's macOS sandbox must be initialized before loading the framework.
    // Keep both scopes alive through CefExecuteProcess and let them drop in
    // loader-then-sandbox order before returning the subprocess exit code.
    let mut sandbox = cef::sandbox::Sandbox::new();
    sandbox.initialize(args.as_main_args());
    let loader = cef::library_loader::LibraryLoader::new(&executable, true);
    if !loader.load() {
        return Err("stage=browser.helper.cef-load cause=framework-load-failed".to_owned());
    }
    let _ = cef::api_hash(cef::sys::CEF_API_VERSION_LAST, 0);
    let code = cef::execute_process(
        Some(args.as_main_args()),
        None::<&mut cef::App>,
        std::ptr::null_mut(),
    );
    Ok(code)
}
