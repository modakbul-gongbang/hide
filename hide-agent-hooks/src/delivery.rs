//! Prompt intake has a single total deadline. Pull is read-only; only a
//! successful stdout flush permits confirmation. All child processes are
//! owned and their bounded stdout is drained while they run.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use hide_platform::fs::{self, private};
use hide_platform::process::OwnedChild;
use serde::{Deserialize, Serialize};

const OUTPUT_LIMIT: usize = 64 * 1024;
const CONTEXT_LIMIT: usize = 8 * 1024;
const CONFIRM_RESERVE: Duration = Duration::from_millis(400);

#[derive(Deserialize)]
pub struct Intake {
    pub context: String,
    pub ids: Vec<String>,
}

pub fn pull(deadline: Instant) -> Result<Option<Intake>, &'static str> {
    let pull_deadline = deadline.checked_sub(CONFIRM_RESERVE).ok_or("deadline")?;
    let answer = run_cli(&["inbox", "--hook"], pull_deadline)?;
    let intake: Intake = serde_json::from_value(answer).map_err(|_| "format")?;
    if intake.context.len() > CONTEXT_LIMIT
        || intake.ids.len() > 5
        || intake
            .ids
            .iter()
            .any(|id| id.is_empty() || id.len() > 256 || id.chars().any(char::is_control))
    {
        return Err("format");
    }
    if intake.ids.is_empty() {
        Ok(None)
    } else {
        Ok(Some(intake))
    }
}

pub fn confirm(intake: &Intake, deadline: Instant) -> Result<(), &'static str> {
    let mut arguments = vec!["inbox", "--confirm"];
    arguments.extend(intake.ids.iter().map(String::as_str));
    let answer = run_cli(&arguments, deadline)?;
    let confirmed = answer
        .get("confirmed")
        .and_then(serde_json::Value::as_array)
        .ok_or("confirm")?;
    if confirmed.len() != intake.ids.len()
        || !confirmed
            .iter()
            .zip(&intake.ids)
            .all(|(actual, expected)| actual.as_str() == Some(expected))
    {
        return Err("confirm");
    }
    Ok(())
}

fn run_cli(arguments: &[&str], deadline: Instant) -> Result<serde_json::Value, &'static str> {
    if Instant::now() >= deadline {
        return Err("deadline");
    }
    let executable = std::env::current_exe().map_err(|_| "cli")?;
    let sibling = executable
        .parent()
        .ok_or("cli")?
        .join(format!("hide{}", std::env::consts::EXE_SUFFIX));
    // Reuse the installed sibling rather than trusting a different checkout
    // earlier on PATH. A missing part follows the manual-inbox fallback.
    let mut command = Command::new(sibling);
    command
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = OwnedChild::spawn(&mut command).map_err(|_| "cli")?;
    let stdout = child.take_stdout().ok_or("cli")?;
    let reader = thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout
            .take(OUTPUT_LIMIT as u64 + 1)
            .read_to_end(&mut bytes)
            .map(|_| bytes)
    });
    let status = loop {
        if Instant::now() >= deadline {
            break Err("deadline");
        }
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) => thread::sleep(Duration::from_millis(5)),
            Err(_) => break Err("cli"),
        }
    };
    let _ = child.kill_tree();
    let bytes = reader.join().map_err(|_| "cli")?.map_err(|_| "cli")?;
    let status = status?;
    let value: serde_json::Value = if bytes.len() <= OUTPUT_LIMIT {
        serde_json::from_slice(&bytes).map_err(|_| "format")?
    } else {
        return Err("format");
    };
    if !status.success() || value["ok"] != true {
        return Err(match value["reason"].as_str() {
            Some("ledger_unavailable") => "ledger",
            Some("capacity") => "capacity",
            Some("agent_pane_required" | "caller_identity_conflict" | "caller_context_changed") => {
                "identity"
            }
            _ => "cli",
        });
    }
    value.get("result").cloned().ok_or("format")
}

#[derive(Deserialize, Serialize, Default)]
struct Diagnostics {
    last: BTreeMap<String, u64>,
}

/// Eight fixed causes, private storage and a nonblocking cross-process lock.
/// A hook with no pane can still record its failure without recording its cwd.
pub fn diagnose(home: &Path, cause: &'static str) -> bool {
    const CAUSES: [&str; 8] = [
        "cli", "deadline", "format", "confirm", "ledger", "capacity", "identity", "stdout",
    ];
    if !CAUSES.contains(&cause) {
        return false;
    }
    let path = diagnostic_path(home);
    let result = (|| -> std::io::Result<bool> {
        let parent = path
            .parent()
            .ok_or_else(|| std::io::Error::other("diagnostic parent unavailable"))?;
        private::create_dir_all(parent)?;
        let file = private::open_own_file(&parent.join("delivery-diagnostics.lock"), true)?;
        let fs::lock::Waited::Locked(_lock) =
            fs::lock::lock_file(file, fs::lock::Mode::Exclusive, Duration::ZERO, &|| false)?
        else {
            return Ok(false);
        };
        let mut state = match private::open_own_file(&path, false) {
            Ok(file) => {
                let mut bytes = Vec::new();
                file.take(2049).read_to_end(&mut bytes)?;
                if bytes.len() > 2048 {
                    return Err(std::io::Error::other("diagnostic capacity"));
                }
                serde_json::from_slice::<Diagnostics>(&bytes).map_err(std::io::Error::other)?
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Diagnostics::default(),
            Err(error) => return Err(error),
        };
        if state.last.len() > 8
            || state
                .last
                .keys()
                .any(|cause| !CAUSES.contains(&cause.as_str()))
        {
            return Err(std::io::Error::other("diagnostic capacity"));
        }
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(std::io::Error::other)?
            .as_millis() as u64;
        if state
            .last
            .get(cause)
            .is_some_and(|last| now.saturating_sub(*last) < 10 * 60_000)
        {
            return Ok(false);
        }
        if state.last.len() >= 8 && !state.last.contains_key(cause) {
            return Err(std::io::Error::other("diagnostic capacity"));
        }
        state.last.insert(cause.to_owned(), now);
        let bytes = serde_json::to_vec(&state).map_err(std::io::Error::other)?;
        fs::atomic::write_file(&path, &bytes, fs::Access::Private)?;
        Ok(true)
    })();
    // Emit only after the cross-process claim is persisted. Damaged or
    // unavailable throttle storage cannot authorize an unthrottled log.
    if matches!(result, Ok(true)) {
        eprintln!(
            "{}",
            serde_json::json!({"component":"delivery_hook","kind":"intake.failed","code":cause,"diagnostic_saved":result.is_ok()})
        );
        true
    } else {
        false
    }
}

fn diagnostic_path(home: &Path) -> PathBuf {
    crate::counters::state_directory(home)
        .parent()
        .expect("counter directory has a parent")
        .join("delivery-diagnostics.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostic_claim_precedes_output_and_repeated_cause_is_suppressed() {
        let home = tempfile::tempdir().unwrap();
        assert!(diagnose(home.path(), "cli"));
        assert!(!diagnose(home.path(), "cli"));
        assert!(diagnose(home.path(), "deadline"));
        let stored = std::fs::read(diagnostic_path(home.path())).unwrap();
        let state: Diagnostics = serde_json::from_slice(&stored).unwrap();
        assert_eq!(state.last.len(), 2);
        assert!(hide_platform::fs::private::is_private(&diagnostic_path(home.path())).unwrap());
        assert!(
            !String::from_utf8(stored)
                .unwrap()
                .contains(&home.path().display().to_string())
        );
    }

    #[test]
    fn corrupt_or_unavailable_diagnostic_storage_never_bypasses_throttle() {
        for damaged in [
            b"damaged private bytes".as_slice(),
            b"{\"last\":{\"unknown\":0}}".as_slice(),
        ] {
            let home = tempfile::tempdir().unwrap();
            let path = diagnostic_path(home.path());
            private::create_dir_all(path.parent().unwrap()).unwrap();
            fs::atomic::write_file(&path, damaged, fs::Access::Private).unwrap();
            for _ in 0..3 {
                assert!(!diagnose(home.path(), "cli"));
            }
            assert_eq!(std::fs::read(&path).unwrap(), damaged);
        }
        let home = tempfile::tempdir().unwrap();
        let path = diagnostic_path(home.path());
        private::create_dir_all(path.parent().unwrap()).unwrap();
        // A real invalid store shape fails before an output claim.
        private::create_dir_all(&path).unwrap();
        assert!(!diagnose(home.path(), "cli"));
        assert!(!diagnose(home.path(), "unexpected"));
        assert!(path.is_dir());
    }
}
