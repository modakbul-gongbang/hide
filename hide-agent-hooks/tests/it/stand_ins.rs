//! Programs a test runs against a product deadline, ready before the
//! deadline starts (issue 813). This file is also built into hide-kit's unit
//! tests (`hide-kit/src/tests.rs`), whose stand-ins meet the kit's deadlines.
//!
//! macOS checks a file the first time it is started, once per file, and the
//! checks of every process on the machine wait in one line: on 2026-10-09 at
//! load 7, a new script's first start took about 90 ms and its second 6 ms,
//! twenty new scripts started together finished one after another with the
//! last at 1.8 s, and a hard link to a file already started began at once. A
//! test that copied the helper or wrote its stand-in afresh queued behind
//! every new file the machine's other builds and tests started, inside the
//! hook's budget or the kit's 5 s deadlines, and under load the wait outgrew
//! them. Linux has its own refusal: a file another thread's fork still holds
//! open for writing does not start (`ETXTBSY`).
//!
//! So a program is one file per build, written once and never changed, and
//! started once per process before a test is given it; a test's folder holds
//! hard links to it, which are the same file and so share its check.

use std::collections::HashSet;
use std::io::ErrorKind;
use std::path::Path;
#[cfg(unix)]
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{LazyLock, Mutex, PoisonError};
use std::time::{Duration, Instant};

use hide_platform::fs::identity::FileId;
use hide_platform::process::OwnedChild;

/// How long a program's first start may take. A hang guard, not a deadline:
/// the wait is the machine's line of first-start checks, which grows with
/// every other build and test running beside this one, and it is paid once
/// per file per process, before any deadline a test measures starts. It is
/// nextest's slow-test period (`.config/nextest.toml`).
const READY_GUARD: Duration = Duration::from_secs(60);

/// The most stand-ins one store keeps. A body is fixed text, so a store grows
/// only when a body is edited, by one file per edit until the build folder is
/// cleaned; a body that named a test's own folder would add a file on every
/// run, and this cap fails that test instead.
#[cfg(unix)]
const STORE_CAP: usize = 64;

/// The files this process has already started once.
static READY: LazyLock<Mutex<HashSet<FileId>>> = LazyLock::new(Mutex::default);

/// The stand-in program whose text is `body`, kept in `store` and ready to
/// run. A stand-in finds its test's files from `HOME`, which the program
/// under test hands on to it, never from a path written into its body.
#[cfg(unix)]
pub fn stand_in(store: &Path, body: &str) -> PathBuf {
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;

    std::fs::create_dir_all(store).unwrap();
    // FNV-1a, so a body keeps its name across toolchains; a name two bodies
    // shared would fail the comparison below rather than run the wrong one.
    let name = body.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
    });
    let path = store.join(format!("{name:016x}"));
    if !path.exists() {
        let kept = std::fs::read_dir(store)
            .unwrap()
            .filter(|entry| {
                !entry
                    .as_ref()
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .starts_with('.')
            })
            .count();
        assert!(
            kept < STORE_CAP,
            "{} holds {kept} stand-ins: a stand-in's body must not name a test's folder, \
             and after many edited bodies the folder can be removed",
            store.display()
        );
        let mut file = tempfile::NamedTempFile::new_in(store).unwrap();
        file.write_all(body.as_bytes()).unwrap();
        // Read-only, so a test that wrote to its link would fail rather than
        // change the program every other test runs.
        file.as_file()
            .set_permissions(std::fs::Permissions::from_mode(0o555))
            .unwrap();
        match file.persist_noclobber(&path) {
            Ok(_) => {}
            // Another test process kept the same body first.
            Err(error) if error.error.kind() == ErrorKind::AlreadyExists => {}
            Err(error) => panic!("{} could not be kept: {}", path.display(), error.error),
        }
    }
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        body,
        "{} is not the stand-in its name says",
        path.display()
    );
    ready(&path);
    path
}

/// Starts `program` once in this process, unless it already was, and waits
/// for it to end. It runs with `--help` and a `HOME` in a folder of its own,
/// so a stand-in that writes under or beside its `HOME` writes into that
/// folder.
pub fn ready(program: &Path) {
    let id = hide_platform::fs::identity::file_id(program).unwrap();
    let mut started = READY.lock().unwrap_or_else(PoisonError::into_inner);
    if started.contains(&id) {
        return;
    }
    let scratch = tempfile::tempdir().unwrap();
    let deadline = Instant::now() + READY_GUARD;
    loop {
        let mut command = Command::new(program);
        command
            .arg("--help")
            .env(
                hide_platform::host::HOME_VARIABLE,
                scratch.path().join("home"),
            )
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        match OwnedChild::spawn(&mut command) {
            Ok(mut child) => {
                child.capture_until(deadline, 1).unwrap_or_else(|error| {
                    panic!(
                        "{} did not end its first start: {error:?}",
                        program.display()
                    )
                });
                break;
            }
            Err(error)
                if error.kind() == ErrorKind::ExecutableFileBusy && Instant::now() < deadline =>
            {
                std::thread::yield_now();
            }
            Err(error) => panic!("{} could not start: {error}", program.display()),
        }
    }
    started.insert(id);
}
