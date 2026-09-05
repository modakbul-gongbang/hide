use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    const SCHEMA_PATH: &str = "../contracts/herdr-api.schema.json";
    // The pinned-runtime manifest lives beside the macOS resources because the
    // app bundle ships it verbatim, but it is the single source of the pin for
    // every consumer: this build script, the Swift shell, and build-app.sh.
    const RUNTIME_MANIFEST_PATH: &str = "../macos/Sources/HerdrMacOS/Resources/herdr-bundle.json";

    println!("cargo:rerun-if-changed={SCHEMA_PATH}");
    println!("cargo:rerun-if-changed={RUNTIME_MANIFEST_PATH}");
    println!("cargo:rerun-if-changed=build.rs");

    let schema_text = fs::read_to_string(SCHEMA_PATH)
        .unwrap_or_else(|error| panic!("failed to read canonical Herdr API schema: {error}"));
    let schema: serde_json::Value = serde_json::from_str(&schema_text)
        .unwrap_or_else(|error| panic!("canonical Herdr API schema is invalid JSON: {error}"));
    let protocol = schema
        .get("protocol")
        .and_then(serde_json::Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .filter(|value| *value > 0)
        .unwrap_or_else(|| {
            panic!("canonical Herdr API schema must contain a positive uint32 protocol")
        });
    let schema_version = schema
        .get("schema_version")
        .and_then(serde_json::Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .filter(|value| *value > 0)
        .unwrap_or_else(|| {
            panic!("canonical Herdr API schema must contain a positive uint32 schema_version")
        });

    let manifest_text = fs::read_to_string(RUNTIME_MANIFEST_PATH).unwrap_or_else(|error| {
        panic!("failed to read the pinned Herdr runtime manifest: {error}")
    });
    let manifest: serde_json::Value = serde_json::from_str(&manifest_text)
        .unwrap_or_else(|error| panic!("pinned Herdr runtime manifest is invalid JSON: {error}"));
    let bundled_version = manifest
        .get("version")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| {
            !value.is_empty()
                && value.split('.').count() >= 2
                && value
                    .split('.')
                    .all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()))
        })
        .unwrap_or_else(|| {
            panic!(
                "pinned Herdr runtime manifest must contain a dotted numeric version, \
                 e.g. \"0.8.2\"; found {:?}",
                manifest.get("version")
            )
        });

    let generated = format!(
        "pub const HERDR_PROTOCOL_REVISION: u32 = {protocol};\n\
         pub const HERDR_API_SCHEMA_VERSION: u32 = {schema_version};\n\
         pub const BUNDLED_HERDR_VERSION: &str = \"{bundled_version}\";\n"
    );
    let output = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR is set by Cargo"))
        .join("herdr_contract.rs");
    fs::write(output, generated).expect("failed to write generated Herdr contract constants");
}
