//! Drives the built binary against a temporary home, so what a command does
//! to the state directories is observed the way an operator would see it.
//! Nothing here reads or writes the real home.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use agent_context_labels::{LEGACY_PLUGIN_ID, PLUGIN_ID};
use tempfile::{TempDir, tempdir};

const BIN: &str = env!("CARGO_BIN_EXE_hide-agent-context-labels");

fn legacy_state(home: &Path) -> PathBuf {
    home.join(".local/state").join(LEGACY_PLUGIN_ID)
}

fn current_state(home: &Path) -> PathBuf {
    home.join(".local/state").join(PLUGIN_ID)
}

/// A home with only the previous id's state directory in it.
fn home_with_legacy_state() -> TempDir {
    let home = tempdir().unwrap();
    let legacy = legacy_state(home.path());
    std::fs::create_dir_all(&legacy).unwrap();
    std::fs::write(
        legacy.join("settings.json"),
        r#"{"automatic_summaries":false}"#,
    )
    .unwrap();
    home
}

/// The binary with an empty PATH, so no Herdr and no Codex is ever found.
fn command(home: &Path) -> Command {
    let mut command = Command::new(BIN);
    command
        .env_clear()
        .env("HOME", home)
        .env("PATH", "/usr/bin:/bin")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
}

#[test]
fn commands_other_than_watch_leave_legacy_state_where_it_is() {
    let cases: &[&[&str]] = &[
        &["--help"],
        &["not-a-command"],
        &["request-refresh"],
        &["set-automatic-summaries", "--enabled", "true"],
        &["verify-provider", "--provider", "claude"],
        &["analyze-stdin"],
        &["hook"],
    ];
    for args in cases {
        let home = home_with_legacy_state();
        let output = command(home.path()).args(*args).output().unwrap();
        assert!(
            legacy_state(home.path()).join("settings.json").exists(),
            "{args:?} moved the legacy state (status {:?}, stderr {})",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
        let log = current_state(home.path()).join("events.jsonl");
        if log.exists() {
            let log = std::fs::read_to_string(log).unwrap();
            assert!(!log.contains("state_migrated"), "{args:?}: {log}");
        }
    }
}

#[test]
fn help_and_an_invalid_command_create_no_state_at_all() {
    for args in [&["--help"][..], &["not-a-command"][..]] {
        let home = tempdir().unwrap();
        let _ = command(home.path()).args(args).output().unwrap();
        assert!(
            !home.path().join(".local").exists(),
            "{args:?} created state under the home"
        );
    }
}

/// Verification refuses rather than answering when the provider's CLI is not
/// there, and it names which state stopped it. The empty PATH above is what
/// makes that state the same on every machine.
#[test]
fn verify_provider_names_the_state_that_stopped_it_and_fails() {
    for provider in ["claude", "codex"] {
        let home = tempdir().unwrap();
        let output = command(home.path())
            .args(["verify-provider", "--provider", provider])
            .output()
            .unwrap();
        assert!(!output.status.success(), "{provider}");
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.contains(&format!("{provider}=not_installed")),
            "{provider}: {stdout}"
        );
    }
}

#[test]
fn the_watcher_moves_legacy_state_once_on_start() {
    let home = home_with_legacy_state();
    let mut watcher = command(home.path()).arg("watch").spawn().unwrap();
    let log = current_state(home.path()).join("events.jsonl");
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut seen = String::new();
    while Instant::now() < deadline {
        if let Ok(text) = std::fs::read_to_string(&log)
            && text.contains("watcher_started")
        {
            seen = text;
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let _ = watcher.kill();
    let _ = watcher.wait();
    assert!(
        seen.contains("watcher_started"),
        "watcher never started: {seen}"
    );
    assert!(
        seen.contains(r#""event":"state_migrated""#) && seen.contains(".local/state=moved"),
        "{seen}"
    );
    assert!(!legacy_state(home.path()).exists());
    assert_eq!(
        std::fs::read_to_string(current_state(home.path()).join("settings.json")).unwrap(),
        r#"{"automatic_summaries":false}"#
    );
}

/// A server launched outside a login shell has only the system PATH. The
/// shipped startup wrapper must still discover either macOS pnpm CLI layout.
/// Run the actual watcher against the existing external Codex protocol fixture;
/// neither an operator login nor a live Herdr socket is available to this test.
#[cfg(target_os = "macos")]
#[test]
fn startup_discovers_pnpm_providers_without_a_login_shell() {
    use std::os::unix::fs::symlink;

    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let codex_fixture = manifest
        .join("../../hide-ai/tests/fixtures/fake-app-server.py")
        .canonicalize()
        .unwrap();
    for location in ["Library/pnpm", "Library/pnpm/bin"] {
        let fixture = tempfile::Builder::new()
            .prefix("hcl-startup-")
            .tempdir_in("/tmp")
            .unwrap();
        let home = fixture.path().join("home with spaces");
        let scripts = fixture.path().join("plugins/agent-context-labels/scripts");
        let bin_dir = fixture.path().join("target/release");
        for directory in [
            &scripts,
            &bin_dir,
            &home.join(location),
            &home.join(".local/bin"),
        ] {
            std::fs::create_dir_all(directory).unwrap();
        }
        let startup = scripts.join("start-watcher.sh");
        std::fs::copy(manifest.join("scripts/start-watcher.sh"), &startup).unwrap();
        symlink(BIN, bin_dir.join("hide-agent-context-labels")).unwrap();
        symlink(&codex_fixture, home.join(location).join("codex")).unwrap();
        // Prevent a globally installed Claude CLI from being probed at all.
        symlink("/usr/bin/false", home.join(".local/bin/claude")).unwrap();

        let mut watcher = Command::new("/bin/sh")
            .arg(startup)
            .env_clear()
            .env("HOME", &home)
            .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
            .env("FAKE_ARGS_FILE", home.join("provider-args.json"))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let log = current_state(&home).join("events.jsonl");
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut ready = false;
        while Instant::now() < deadline {
            if let Ok(text) = std::fs::read_to_string(&log) {
                ready = text.lines().any(|line| {
                    serde_json::from_str::<serde_json::Value>(line).is_ok_and(|event| {
                        event["event"] == "ai_provider_availability"
                            && event["detail"]
                                .as_str()
                                .is_some_and(|detail| detail.split(';').any(|p| p == "codex=ready"))
                    })
                });
            }
            if ready || watcher.try_wait().is_ok_and(|status| status.is_some()) {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let _ = watcher.kill();
        let _ = watcher.wait();
        assert!(ready, "startup did not discover the provider in {location}");
        assert!(
            home.join("provider-args.json").is_file(),
            "startup used a provider outside the isolated pnpm install"
        );
    }
}

/// A short-path home that removes itself on drop, for the one test that needs
/// the watcher's wake socket path to fit in `SUN_LEN`.
#[cfg(unix)]
struct ShortHome(PathBuf);

#[cfg(unix)]
impl Drop for ShortHome {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A helper that reports whether a pid is still alive, using signal 0.
#[cfg(unix)]
fn alive(pid: i32) -> bool {
    Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

/// When the host that started the watcher exits, the watcher must exit too,
/// rather than reconnecting forever and holding its app-server (B11). The
/// watcher runs under an intermediate shell here; killing that shell reparents
/// the watcher, whose parent-pid poll then notices the host is gone, logs
/// `watcher_host_gone`, and exits. An empty PATH keeps Herdr and Codex out, so
/// nothing but the watcher's own loop is under test.
#[cfg(unix)]
#[test]
fn the_watcher_exits_when_its_host_is_gone() {
    // A short home: the watcher's wake socket path must fit in SUN_LEN (~104
    // bytes), which a `/var/folders` tempdir blows past, and this test needs
    // the event loop to actually run rather than fail at socket bind.
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let home = ShortHome(PathBuf::from(format!(
        "/tmp/hcl-host-death-{}-{unique}",
        std::process::id()
    )));
    std::fs::create_dir_all(&home.0).unwrap();
    let home = home.0.as_path();
    // The shell backgrounds the watcher, prints its pid, and waits, so the
    // shell is the watcher's parent and outlives it until we kill it.
    let script = format!(r#"{BIN} watch & echo $!; wait"#);
    let mut parent = Command::new("sh")
        .env_clear()
        .env("HOME", home)
        .env("PATH", "/usr/bin:/bin")
        .args(["-c", &script])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();

    use std::io::{BufRead, BufReader};
    let mut lines = BufReader::new(parent.stdout.take().unwrap()).lines();
    let watcher_pid: i32 = lines
        .next()
        .expect("the shell printed the watcher pid")
        .unwrap()
        .trim()
        .parse()
        .expect("watcher pid is a number");

    // Wait until the watcher is really up (it wrote its start line).
    let log = current_state(home).join("events.jsonl");
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if std::fs::read_to_string(&log)
            .map(|text| text.contains("watcher_started"))
            .unwrap_or(false)
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }

    // Kill the host. The watcher is now an orphan.
    let _ = parent.kill();
    let _ = parent.wait();

    // Within a few poll ticks (<= 5s each) the watcher notices and exits.
    let deadline = Instant::now() + Duration::from_secs(15);
    while alive(watcher_pid) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(100));
    }
    if alive(watcher_pid) {
        let _ = Command::new("kill")
            .args(["-9", &watcher_pid.to_string()])
            .status();
        panic!("the watcher outlived its host");
    }

    let seen = std::fs::read_to_string(&log).unwrap_or_default();
    assert!(
        seen.contains("watcher_host_gone"),
        "the watcher exited without logging the reason: {seen}"
    );
}
