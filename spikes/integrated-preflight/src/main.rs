#![cfg_attr(not(target_os = "macos"), allow(unused))]

#[cfg(not(target_os = "macos"))]
compile_error!("herdr-integrated-preflight is a macOS-only architecture spike");

#[cfg(target_os = "macos")]
mod app;
#[cfg(target_os = "macos")]
mod browser;
#[cfg(target_os = "macos")]
mod layout;
#[cfg(target_os = "macos")]
mod macos_app;
#[cfg(target_os = "macos")]
mod preflight;
#[cfg(target_os = "macos")]
mod pty;
#[cfg(target_os = "macos")]
mod render;

#[cfg(target_os = "macos")]
fn main() {
    if let Err(error) = run() {
        eprintln!("event=integrated.failed retryable=false error={error:#?}");
        std::process::exit(1);
    }
}

#[cfg(target_os = "macos")]
fn run() -> anyhow::Result<()> {
    match browser::bootstrap()? {
        browser::Bootstrap::Subprocess(code) => std::process::exit(code),
        browser::Bootstrap::Main(runtime) => app::run(runtime),
    }
}
