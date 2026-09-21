//! Embeds `web/dist` into a release `hided` (PRD D-03).
//!
//! A release binary carries the web shell so `hide` runs from one file; it
//! fails to build without `web/dist`, because a release that silently served
//! a "UI not built" page would be a broken product, not a diagnostic. A debug
//! build embeds nothing and reads the directory at run time, so `pnpm build`
//! shows up without a cargo rebuild.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

fn main() {
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let dist = manifest.join("../web/dist");
    let out = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR")).join("ui_embed.rs");
    println!("cargo:rerun-if-changed=build.rs");
    let release = env::var("PROFILE").as_deref() == Ok("release");
    if !release {
        fs::write(&out, "pub static FILES: &[(&str, &[u8])] = &[];\n").expect("write ui_embed.rs");
        return;
    }
    if !dist.join("index.html").is_file() {
        panic!(
            "web/dist/index.html is missing at {}; run `pnpm --dir web build` before a release build of hided",
            dist.display()
        );
    }
    let mut files = Vec::new();
    collect(&dist, &dist, &mut files);
    files.sort();
    let mut source = String::from("pub static FILES: &[(&str, &[u8])] = &[\n");
    for (relative, absolute) in &files {
        println!("cargo:rerun-if-changed={}", absolute.display());
        source.push_str(&format!(
            "    ({:?}, include_bytes!({:?})),\n",
            relative,
            absolute.display().to_string()
        ));
    }
    source.push_str("];\n");
    fs::write(&out, source).expect("write ui_embed.rs");
}

fn collect(root: &Path, dir: &Path, files: &mut Vec<(String, PathBuf)>) {
    for entry in fs::read_dir(dir).expect("read web/dist") {
        let entry = entry.expect("dist entry");
        let path = entry.path();
        if path.is_dir() {
            collect(root, &path, files);
        } else if path.is_file() {
            let relative = path
                .strip_prefix(root)
                .expect("under dist")
                .to_string_lossy()
                .replace('\\', "/");
            files.push((relative, path));
        }
    }
}
