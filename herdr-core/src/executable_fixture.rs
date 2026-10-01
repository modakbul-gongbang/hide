//! Writes a script a test then runs as a program.
//!
//! Linux refuses to exec a file that any process holds open for writing
//! (`ETXTBSY`, "Text file busy"). A fixture written with `std::fs::write` holds
//! such a descriptor for a moment, and a test on another thread that spawns a
//! child in that moment copies it into the child, where it stays until the
//! child's own exec closes it. Run in that gap, the fixture fails to start: on
//! the ubuntu runner `run_gh` returned an error within milliseconds of the
//! suite starting, while a thousand tests were spawning beside it. macOS has
//! no such refusal, so the suite never showed it there.
//!
//! The file is therefore written by a child process: this process never holds
//! a write descriptor to it, so no spawn can carry one away, and the writer has
//! exited, its descriptor with it, before the fixture is run.

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

/// Writes `contents` to `path` as an owner-executable file.
pub(crate) fn write_executable(path: &Path, contents: &str) {
    let mut writer = Command::new("/bin/sh")
        .args(["-c", "cat > \"$1\" && chmod 700 \"$1\"", "sh"])
        .arg(path)
        .stdin(Stdio::piped())
        .spawn()
        .expect("start the executable fixture writer");
    writer
        .stdin
        .take()
        .expect("piped stdin")
        .write_all(contents.as_bytes())
        .expect("send the executable fixture");
    let status = writer
        .wait()
        .expect("wait for the executable fixture writer");
    assert!(
        status.success(),
        "writing {} failed: {status}",
        path.display()
    );
}
