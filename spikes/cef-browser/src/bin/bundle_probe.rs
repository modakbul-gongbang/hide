#[cfg(not(target_os = "macos"))]
compile_error!("bundle-probe is a macOS-only bundle builder");

#[cfg(target_os = "macos")]
fn main() {
    if let Err(error) = run() {
        eprintln!("{{\"event\":\"bundle.failed\",\"error\":{error:?}}}");
        std::process::exit(1);
    }
}

#[cfg(target_os = "macos")]
fn run() -> Result<(), String> {
    use cef::build_util::mac::{BundleInfo, bundle};
    use semver::Version;

    let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let output = manifest_dir.join("target/bundle");
    let release = manifest_dir.join("target/release");
    let bundle_info = BundleInfo::new(
        "herdr-cef-probe",
        "dev.herdr.cef-native-child-probe",
        "Herdr CEF Native Child Probe",
        "English",
        Version::new(0, 1, 0),
    );
    let app = bundle(
        &output,
        &release,
        "herdr-cef-probe",
        "herdr-cef-probe-helper",
        None,
        bundle_info,
    )
    .map_err(|error| format!("CEF bundle assembly failed: {error}"))?;
    println!(
        "{{\"event\":\"bundle.ready\",\"app\":\"{}\"}}",
        app.display()
    );
    Ok(())
}
