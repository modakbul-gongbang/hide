#[cfg(not(target_os = "macos"))]
compile_error!("bundle-integrated-preflight is macOS-only");

#[cfg(target_os = "macos")]
fn main() {
    if let Err(error) = run() {
        eprintln!("event=bundle.failed error={error:?}");
        std::process::exit(1);
    }
}

#[cfg(target_os = "macos")]
fn run() -> Result<(), String> {
    use cef::build_util::mac::{BundleInfo, bundle};
    use semver::Version;

    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let app = bundle(
        &root.join("target/bundle"),
        &root.join("target/release"),
        "herdr-integrated-preflight",
        "herdr-integrated-preflight-helper",
        None,
        BundleInfo::new(
            "herdr-integrated-preflight",
            "dev.herdr.integrated-preflight",
            "Herdr Integrated Preflight",
            "English",
            Version::new(0, 1, 0),
        ),
    )
    .map_err(|error| format!("CEF bundle assembly failed: {error}"))?;
    println!("event=bundle.ready app={}", app.display());
    Ok(())
}
