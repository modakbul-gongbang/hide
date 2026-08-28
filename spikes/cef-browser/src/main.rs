#![cfg_attr(not(target_os = "macos"), allow(unused))]

#[cfg(not(target_os = "macos"))]
compile_error!("herdr-cef-probe is a macOS-only architecture probe");

#[cfg(target_os = "macos")]
mod macos_app;
#[cfg(target_os = "macos")]
mod probe;

#[cfg(target_os = "macos")]
fn main() {
    if let Err(error) = probe::run() {
        eprintln!("{{\"event\":\"probe.failed\",\"stage\":\"main\",\"error\":{error:?}}}");
        std::process::exit(1);
    }
}
