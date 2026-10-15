//! Moving the Herdr server another Hide app started onto this app's Herdr,
//! with its panes and their processes kept (Herdr's `server.live_handoff`).
//!
//! The server an app starts outlives the app, so once a newer app replaces
//! it the server goes on running the old app's Herdr from wherever the old
//! app was put: renamed aside or in the Trash. Every pane runs under that
//! program until the server ends, and the system names the old app when a
//! process in a pane asks for a permission. A packaged `hide connect`
//! therefore asks the server on its socket which program it runs, and when
//! that is another Hide app's Herdr it has the server hand its panes to this
//! app's Herdr. Herdr's own clients, the daemon's terminal attaches among
//! them, are told to reconnect.
//!
//! A server the operator runs from their own Herdr is never handed over, a
//! server newer than this app's Herdr is not given to an older one, and
//! Herdr hands over on Unix only. A handoff that does not happen leaves the
//! server as it was, as every earlier build did, so each outcome is a log
//! record and nothing the connect fails on.
//!
//! A device's node keeps a rule of its own ([`device_update`], PRD
//! device-herdr-update): Hide never starts a device's server, so the server
//! on the device's registered socket is the operator's, and it is handed to
//! the pinned copy the build brought whenever it runs an older stable
//! release of Herdr (D-04, D-06). A failed handoff is tried once per server
//! process and pin (D-13).

use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use hide_kit::device_herdr::{BehindCause, HerdrUpdate};
use hide_platform::fs::identity::file_id;
use hide_platform::ipc::LocalStream;
use hide_platform::process::{Image, image_of};

/// How long the server has to hand its panes over. Forty panes with
/// scrollback took about a second (2026-10-14); `hide connect` itself has
/// 25 s from the host.
const HANDOFF_WITHIN: Duration = Duration::from_secs(15);

/// How long a `ping` or this app's Herdr naming its version may take.
const ASK_WITHIN: Duration = Duration::from_secs(5);

/// What one look at the server came to.
#[derive(Debug, Eq, PartialEq)]
pub enum Outcome {
    /// No server answers on the socket.
    NoServer,
    /// The server runs this app's Herdr.
    Ours,
    /// The server runs a Herdr no Hide app holds: the operator's own.
    NotHides { program: PathBuf },
    /// The system cannot say which program the server runs, or the server
    /// cannot hand its panes over.
    Unsupported { reason: String },
    /// The server's Herdr is newer than this app's, or the two releases do
    /// not order, so the server keeps its own.
    Kept { server: String, ours: String },
    /// The panes now belong to a server running this app's Herdr.
    HandedOff {
        from_pid: u32,
        to_pid: u32,
        from_program: PathBuf,
        elapsed_ms: u64,
    },
    /// Something along the way failed; the server stays as it was unless
    /// the stage is `confirm`.
    Failed { stage: &'static str, detail: String },
}

impl Outcome {
    /// The log record, or `None` for the two answers every connect gives
    /// once nothing is left to move.
    pub fn record(&self) -> Option<serde_json::Value> {
        use serde_json::json;
        let record = match self {
            Outcome::NoServer | Outcome::Ours => return None,
            Outcome::NotHides { program } => {
                json!({"outcome": "not_hides", "program": program.display().to_string()})
            }
            Outcome::Unsupported { reason } => json!({"outcome": "unsupported", "reason": reason}),
            Outcome::Kept { server, ours } => {
                json!({"outcome": "kept", "server_version": server, "version": ours})
            }
            Outcome::HandedOff {
                from_pid,
                to_pid,
                from_program,
                elapsed_ms,
            } => json!({
                "outcome": "handed_off", "from_pid": from_pid, "to_pid": to_pid,
                "from_program": from_program.display().to_string(), "elapsed_ms": elapsed_ms,
            }),
            Outcome::Failed { stage, detail } => {
                json!({"outcome": "failed", "stage": stage, "message": detail})
            }
        };
        let mut record = record;
        record["component"] = "hide".into();
        record["kind"] = "herdr.handoff".into();
        Some(record)
    }
}

/// Moves the server on `socket` onto `ours`, this app's Herdr, when another
/// Hide app's Herdr runs it.
pub fn hand_off(ours: &Path, socket: &Path) -> Outcome {
    let ours_file = match file_id(ours) {
        Ok(file) => file,
        Err(error) => return failed("ours", error),
    };
    let (from_pid, image) = match program_on(socket) {
        Ok(Some(found)) => found,
        Ok(None) => return Outcome::NoServer,
        Err(error) if error.kind() == io::ErrorKind::Unsupported => {
            return Outcome::Unsupported {
                reason: error.to_string(),
            };
        }
        Err(error) => return failed("program", error),
    };
    if image.file == ours_file {
        return Outcome::Ours;
    }
    if !image
        .path
        .parent()
        .is_some_and(hide_kit::holds_package_runtime)
    {
        return Outcome::NotHides {
            program: image.path,
        };
    }
    let pong = match ask(socket, "ping", herdr_core::wire::ping_params())
        .and_then(herdr_core::wire::server_pong)
    {
        Ok(pong) => pong,
        Err(detail) => {
            return Outcome::Failed {
                stage: "ping",
                detail,
            };
        }
    };
    if !pong.live_handoff {
        return Outcome::Unsupported {
            reason: format!("Herdr {} cannot hand its panes over here", pong.version),
        };
    }
    let version = match version_of(ours) {
        Ok(version) => version,
        Err(detail) => {
            return Outcome::Failed {
                stage: "version",
                detail,
            };
        }
    };
    if !takes_over(&version, &pong.version) {
        return Outcome::Kept {
            server: pong.version,
            ours: version,
        };
    }
    match hand_over(ours, &ours_file, &version, socket) {
        Ok(Handed { to_pid, elapsed_ms }) => Outcome::HandedOff {
            from_pid,
            to_pid,
            from_program: image.path,
            elapsed_ms,
        },
        Err((stage, detail)) => Outcome::Failed { stage, detail },
    }
}

/// A server that now runs the Herdr it was handed to.
struct Handed {
    to_pid: u32,
    elapsed_ms: u64,
}

/// Has the server on `socket` hand its panes to `ours`, whose file is
/// `ours_file` and whose version is `version`, and confirms the server on
/// the socket runs it afterwards. A failure names its stage; the server
/// stays as it was unless the stage is `confirm`.
fn hand_over(
    ours: &Path,
    ours_file: &hide_platform::fs::identity::FileId,
    version: &str,
    socket: &Path,
) -> Result<Handed, (&'static str, String)> {
    let started = Instant::now();
    ours.to_str()
        .ok_or_else(|| format!("{} is not a path Herdr can take", ours.display()))
        .and_then(|ours| herdr_core::wire::live_handoff_params(ours, version))
        .and_then(|params| {
            hide_herdr_client::request_with_timeout(
                socket,
                "server.live_handoff",
                params,
                HANDOFF_WITHIN,
            )
            .map_err(|error| error.to_string())
        })
        .and_then(herdr_core::wire::live_handoff_done)
        .map_err(|detail| ("handoff", detail))?;
    match program_on(socket) {
        Ok(Some((to_pid, now))) if now.file == *ours_file => Ok(Handed {
            to_pid,
            elapsed_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        }),
        Ok(Some((pid, now))) => Err((
            "confirm",
            format!("the server (pid {pid}) runs {}", now.path.display()),
        )),
        Ok(None) => Err(("confirm", "no server answers after the handoff".to_owned())),
        Err(error) => Err(("confirm", error.to_string())),
    }
}

/// What a device's node does after its kit has applied: the server on the
/// device's registered `socket` is handed to `pinned`, the copy of Herdr the
/// build brought, when it runs an older stable release; `record` keeps the
/// handoffs that failed, so each is tried once per server process and pin.
/// Nothing here fails the kit pass: every outcome is an answer.
pub fn device_update(pinned: &Path, socket: &Path, record: &Path) -> HerdrUpdate {
    let failed = |stage: &str, reason: String| HerdrUpdate::Failed {
        stage: stage.to_owned(),
        reason,
    };
    let pinned_file = match file_id(pinned) {
        Ok(file) => file,
        Err(error) => return failed("pinned", error.to_string()),
    };
    let from_pid = match program_on(socket) {
        Ok(Some((pid, _))) => pid,
        Ok(None) => return HerdrUpdate::NoServer,
        Err(error) => return failed("program", error.to_string()),
    };
    let pong = match ask(socket, "ping", herdr_core::wire::ping_params())
        .and_then(herdr_core::wire::server_pong)
    {
        Ok(pong) => pong,
        Err(reason) => return failed("ping", reason),
    };
    let version = match version_of(pinned) {
        Ok(version) => version,
        Err(reason) => return failed("version", reason),
    };
    let server_version = pong.version;
    match device_step(&version, &server_version, pong.live_handoff) {
        DeviceStep::Current => return HerdrUpdate::Current { version },
        DeviceStep::Kept => {
            return HerdrUpdate::Kept {
                server_version,
                version,
            };
        }
        DeviceStep::CannotHandOff => {
            return HerdrUpdate::Behind {
                reason: format!("Herdr {server_version} cannot hand its panes over"),
                server_version,
                version,
                cause: BehindCause::CannotHandOff,
            };
        }
        DeviceStep::HandOver => {}
    }
    let attempt = match Attempt::of(socket, from_pid, pinned) {
        Ok(attempt) => attempt,
        Err(error) => return failed("program", error.to_string()),
    };
    let attempts = match Attempts::read(record) {
        Ok(attempts) => attempts,
        Err(reason) => return failed("record", reason),
    };
    if let Some(earlier) = attempts.failed(&attempt) {
        return HerdrUpdate::Behind {
            server_version,
            version,
            cause: BehindCause::HandoffFailed,
            reason: format!("an earlier handoff of this server failed: {earlier}"),
        };
    }
    match hand_over(pinned, &pinned_file, &version, socket) {
        Ok(Handed { to_pid, elapsed_ms }) => HerdrUpdate::HandedOff {
            server_version,
            version,
            from_pid,
            to_pid,
            elapsed_ms,
        },
        Err((stage, detail)) => {
            let reason = format!("{stage}: {detail}");
            let kept = attempts.add(attempt, reason.clone()).write(record);
            HerdrUpdate::Behind {
                server_version,
                version,
                cause: BehindCause::HandoffFailed,
                reason: match kept {
                    Ok(()) => reason,
                    Err(error) => format!("{reason}; the attempt was not recorded: {error}"),
                },
            }
        }
    }
}

/// What a device's server is due, from its Herdr's version, the pin's and
/// whether the server can hand its panes over.
#[derive(Debug, Eq, PartialEq)]
enum DeviceStep {
    /// The server already runs the pinned release, from whichever file.
    Current,
    /// A newer release, or one the pin does not order against: left alone.
    Kept,
    /// An older release that cannot hand its panes over.
    CannotHandOff,
    HandOver,
}

fn device_step(pinned: &str, server: &str, live_handoff: bool) -> DeviceStep {
    if pinned == server {
        return DeviceStep::Current;
    }
    match (release(pinned), release(server)) {
        (Some(pinned), Some(server)) if pinned > server => {
            if live_handoff {
                DeviceStep::HandOver
            } else {
                DeviceStep::CannotHandOff
            }
        }
        _ => DeviceStep::Kept,
    }
}

/// The most failed handoffs a device keeps (engineering rule 15): one per
/// server process, and a device runs only a few servers.
const ATTEMPTS_KEPT: usize = 16;

/// One handoff of one server process to one pinned copy.
#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
struct Attempt {
    socket: PathBuf,
    pid: u32,
    /// The process's start, so a pid a restarted server reuses is a new
    /// server (D-13).
    started: u64,
    pinned: PathBuf,
}

impl Attempt {
    fn of(socket: &Path, pid: u32, pinned: &Path) -> io::Result<Self> {
        Ok(Self {
            socket: socket.to_path_buf(),
            pid,
            started: hide_platform::process::start_time(pid)?,
            pinned: pinned.to_path_buf(),
        })
    }
}

#[derive(Debug, Default, serde::Deserialize, serde::Serialize)]
struct Attempts {
    failed: Vec<FailedAttempt>,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
struct FailedAttempt {
    #[serde(flatten)]
    attempt: Attempt,
    reason: String,
}

impl Attempts {
    /// The record at `path`, empty when there is none yet.
    fn read(path: &Path) -> Result<Self, String> {
        match std::fs::read(path) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map_err(|error| format!("{} is unreadable: {error}", path.display())),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(format!("{} could not be read: {error}", path.display())),
        }
    }

    /// Why `attempt` failed before, when it did.
    fn failed(&self, attempt: &Attempt) -> Option<&str> {
        self.failed
            .iter()
            .find(|failed| failed.attempt == *attempt)
            .map(|failed| failed.reason.as_str())
    }

    /// The record with `attempt` added and only the newest attempts kept.
    fn add(mut self, attempt: Attempt, reason: String) -> Self {
        self.failed.push(FailedAttempt { attempt, reason });
        let over = self.failed.len().saturating_sub(ATTEMPTS_KEPT);
        self.failed.drain(..over);
        self
    }

    fn write(&self, path: &Path) -> Result<(), String> {
        let bytes = serde_json::to_vec(self).map_err(|error| error.to_string())?;
        hide_platform::fs::atomic::write_file(path, &bytes, hide_platform::fs::Access::Private)
            .map(|_| ())
            .map_err(|error| format!("{}: {error}", path.display()))
    }
}

fn failed(stage: &'static str, error: io::Error) -> Outcome {
    Outcome::Failed {
        stage,
        detail: error.to_string(),
    }
}

/// The server answering on `socket` and the program it runs, `None` when
/// nothing answers there.
fn program_on(socket: &Path) -> io::Result<Option<(u32, Image)>> {
    let stream = match LocalStream::connect(socket) {
        Ok(stream) => stream,
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused
            ) =>
        {
            return Ok(None);
        }
        Err(error) => return Err(error),
    };
    let pid = stream.peer_pid()?;
    image_of(pid).map(|image| Some((pid, image)))
}

fn ask(
    socket: &Path,
    method: &str,
    params: Result<serde_json::Value, String>,
) -> Result<serde_json::Value, String> {
    hide_herdr_client::request_with_timeout(socket, method, params?, ASK_WITHIN)
        .map_err(|error| format!("{method}: {error}"))
}

/// The version this app's Herdr names for itself.
fn version_of(herdr: &Path) -> Result<String, String> {
    let mut command = std::process::Command::new(herdr);
    command.args(["status", "client", "--json"]);
    let finished = hide_platform::process::run_to_end(
        &mut command,
        ASK_WITHIN,
        &std::sync::atomic::AtomicBool::new(false),
    )
    .map_err(|error| format!("herdr status client: {error:?}"))?;
    if !finished.succeeded() {
        return Err(format!(
            "herdr status client exited {:?}: {}",
            finished.code,
            finished.stderr.trim()
        ));
    }
    let answer: serde_json::Value = serde_json::from_str(finished.stdout.trim())
        .map_err(|error| format!("herdr status client answered no JSON: {error}"))?;
    answer["version"]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| "herdr status client named no version".to_owned())
}

/// Whether this app's Herdr may take the server's panes: the same release,
/// or a later one. A newer server is not handed back to an older Herdr,
/// which may not read what it would be handed, and releases that do not
/// order (a preview beside a release) are left as they are.
fn takes_over(ours: &str, server: &str) -> bool {
    if ours == server {
        return true;
    }
    match (release(ours), release(server)) {
        (Some(ours), Some(server)) => ours > server,
        _ => false,
    }
}

fn release(version: &str) -> Option<(u64, u64, u64)> {
    let mut parts = version.split('.');
    let release = (
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
    );
    parts.next().is_none().then_some(release)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_same_or_a_later_release_takes_a_servers_panes() {
        for (ours, server, takes) in [
            ("0.9.3", "0.9.3", true),
            ("0.9.4", "0.9.3", true),
            ("0.10.0", "0.9.12", true),
            ("1.0.0", "0.9.3", true),
            ("0.9.3", "0.9.4", false),
            ("0.9.3", "0.10.0", false),
            ("0.9.4-preview.1", "0.9.3", false),
            ("0.9.3", "0.9.4-preview.1", false),
            ("0.9", "0.9.3", false),
        ] {
            assert_eq!(takes_over(ours, server), takes, "{ours} over {server}");
        }
    }

    #[test]
    fn a_device_server_is_handed_over_only_from_an_older_release_that_can() {
        for (pinned, server, live_handoff, step) in [
            ("0.9.3", "0.9.3", true, DeviceStep::Current),
            ("0.9.3", "0.9.3", false, DeviceStep::Current),
            ("0.9.3", "0.9.1", true, DeviceStep::HandOver),
            ("0.10.0", "0.9.12", true, DeviceStep::HandOver),
            ("0.9.3", "0.9.1", false, DeviceStep::CannotHandOff),
            ("0.9.3", "0.9.4", true, DeviceStep::Kept),
            ("0.9.3", "0.9.4", false, DeviceStep::Kept),
            ("0.9.3", "0.9.4-preview.1", true, DeviceStep::Kept),
            ("0.9.4-preview.1", "0.9.3", true, DeviceStep::Kept),
        ] {
            assert_eq!(
                device_step(pinned, server, live_handoff),
                step,
                "{server} under pin {pinned}, live handoff {live_handoff}"
            );
        }
    }

    #[test]
    fn a_failed_handoff_is_remembered_for_that_server_process_and_pin_only() {
        let dir = tempfile::tempdir().unwrap();
        let record = dir.path().join("handoff.json");
        let attempt = |pid, started, pinned: &str| Attempt {
            socket: PathBuf::from("/home/me/.config/herdr/herdr.sock"),
            pid,
            started,
            pinned: PathBuf::from(pinned),
        };
        let first = attempt(41, 7, "/root/herdr/aaaa/herdr");
        Attempts::read(&record)
            .unwrap()
            .add(first.clone(), "handoff: refused".to_owned())
            .write(&record)
            .unwrap();

        let read = Attempts::read(&record).unwrap();
        assert_eq!(read.failed(&first), Some("handoff: refused"));
        // A restarted server, even on the same pid, and a new pin are new.
        assert_eq!(read.failed(&attempt(41, 8, "/root/herdr/aaaa/herdr")), None);
        assert_eq!(read.failed(&attempt(41, 7, "/root/herdr/bbbb/herdr")), None);

        let mut many = read;
        for pid in 0..40 {
            many = many.add(attempt(pid, 1, "/root/herdr/aaaa/herdr"), "x".to_owned());
        }
        assert_eq!(many.failed.len(), ATTEMPTS_KEPT);
        assert_eq!(many.failed(&first), None, "the oldest attempts go first");
    }

    #[test]
    fn an_unreadable_record_is_refused_rather_than_read_as_empty() {
        let dir = tempfile::tempdir().unwrap();
        let record = dir.path().join("handoff.json");
        std::fs::write(&record, b"not json").unwrap();
        assert!(Attempts::read(&record).is_err());
    }
}
