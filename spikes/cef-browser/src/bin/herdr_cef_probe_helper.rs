#[cfg(not(target_os = "macos"))]
compile_error!("herdr-cef-probe-helper is a macOS-only CEF helper");

#[cfg(target_os = "macos")]
fn main() {
    use cef::{args::Args, *};

    let args = Args::new();
    let mut sandbox = cef::sandbox::Sandbox::new();
    sandbox.initialize(args.as_main_args());

    let loader = library_loader::LibraryLoader::new(
        &std::env::current_exe().expect("cannot resolve helper executable"),
        true,
    );
    if !loader.load() {
        eprintln!(
            "{{\"event\":\"probe.failed\",\"stage\":\"helper_framework_load\",\"error\":\"cannot load bundled CEF framework\"}}"
        );
        std::process::exit(1);
    }

    let _ = api_hash(sys::CEF_API_VERSION_LAST, 0);
    let exit_code = execute_process(
        Some(args.as_main_args()),
        None::<&mut App>,
        std::ptr::null_mut(),
    );
    if exit_code < 0 {
        eprintln!(
            "{{\"event\":\"probe.failed\",\"stage\":\"helper_execute\",\"exit_code\":{exit_code}}}"
        );
        std::process::exit(1);
    }
    std::process::exit(exit_code);
}
