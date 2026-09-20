//! Handing a pane's counts to Herdr as display-only metadata.
//!
//! `pane.report_metadata` is the whole integration: Herdr stores the tokens
//! on the pane, returns them in `PaneInfo`, and Hide's ordinary
//! `session.snapshot` carries them to the core. No event subscription or poll
//! is added by this path.
//!
//! The request goes over the socket through `hide-herdr-client`, the same
//! client the context-label plugin reports through, rather than through the
//! `herdr` CLI. The CLI's argument grammar is checked by nothing in this
//! repository, and the first release of this hook shelled out with the pane
//! id in a position the pinned CLI refuses, so every report exited 2, the
//! helper swallowed it, and every pane on the machine read as
//! `session_predates_install` for a week. The socket method is the contract
//! `scripts/check-herdr-contract.sh` pins, and the fake server test below
//! asserts the exact request it sends.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use hide_herdr_client::{ApiError, request_with_timeout};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::counters::PaneCounters;
use crate::runtime::{
    DONE_TOKEN, HOOK_SOURCE_NAME, HOOK_VERSION, HookEvent, INSTRUMENTED_TOKEN, WORKING_TOKEN,
};

/// How long one report may hold the agent's hook slot. A hook runs inside
/// the agent's turn, so a Herdr that does not answer costs the operator this
/// long and no more.
const REPORT_TIMEOUT: Duration = Duration::from_secs(2);

/// The socket the hook reports through.
///
/// Herdr exports `HERDR_SOCKET_PATH` to the panes of a server that was not
/// started on the default path, so a hook inside such a pane reaches the
/// server that owns it. Without the override the hook uses Herdr's default,
/// which is the same resolution the context-label plugin applies.
pub fn socket_path(home: &Path) -> PathBuf {
    std::env::var_os("HERDR_SOCKET_PATH")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".config/herdr/herdr.sock"))
}

/// The metadata source every hook report is filed under.
///
/// It is the marker name without its version: Herdr limits a source to
/// ASCII letters, digits, `:`, `.`, `_` and `-`, so the `@` of
/// [`crate::runtime::hook_source_id`] cannot travel here. The version is
/// carried by the [`INSTRUMENTED_TOKEN`] instead.
pub fn metadata_source() -> &'static str {
    HOOK_SOURCE_NAME
}

/// The `pane.report_metadata` parameters one report sends, as a value a test
/// can inspect.
///
/// Building them apart from sending them is what lets the contract be
/// asserted without a Herdr server (engineering rule 12).
pub fn report_params(pane_id: &str, counters: PaneCounters) -> serde_json::Value {
    json!({
        "pane_id": pane_id,
        "source": metadata_source(),
        "tokens": {
            INSTRUMENTED_TOKEN: HOOK_VERSION.to_string(),
            WORKING_TOKEN: counters.working.to_string(),
            DONE_TOKEN: counters.done.to_string(),
        },
    })
}

/// Reports one pane's counts to the Herdr behind `socket_path`.
pub fn report(socket_path: &Path, pane_id: &str, counters: PaneCounters) -> Result<(), ApiError> {
    request_with_timeout(
        socket_path,
        "pane.report_metadata",
        report_params(pane_id, counters),
        REPORT_TIMEOUT,
    )
    .map(|_| ())
}

/// The last report that did not reach Herdr, kept so the failure is a
/// sentence in `doctor` and in Settings rather than a pane that quietly
/// reads as uninstrumented (engineering rules 4 and 10).
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ReportFailure {
    pub pane_id: String,
    pub event: String,
    pub socket_path: String,
    pub error: String,
    pub at_unix_ms: u64,
}

impl ReportFailure {
    /// The one sentence both surfaces show.
    pub fn message(&self) -> String {
        format!(
            "The hook could not report to Herdr at {} for pane {} on {}: {}",
            self.socket_path, self.pane_id, self.event, self.error
        )
    }
}

fn failure_path(home: &Path) -> PathBuf {
    home.join(".hide")
        .join("agent-hooks")
        .join("last-report-failure.json")
}

/// Records the outcome of one report: a failure replaces the last one, a
/// success clears it, so the file describes the hook's current state rather
/// than its history.
pub fn record_outcome(
    home: &Path,
    pane_id: &str,
    event: HookEvent,
    socket_path: &Path,
    outcome: &Result<(), ApiError>,
) -> io::Result<()> {
    let path = failure_path(home);
    match outcome {
        Ok(()) => match fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        },
        Err(error) => {
            let failure = ReportFailure {
                pane_id: pane_id.to_owned(),
                event: event.name().to_owned(),
                socket_path: socket_path.display().to_string(),
                error: error.to_string(),
                at_unix_ms: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|elapsed| elapsed.as_millis() as u64)
                    .unwrap_or(0),
            };
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&path, serde_json::to_vec(&failure)?)
        }
    }
}

/// The last recorded failure, or none when the last report succeeded or no
/// report has run. A file that cannot be read back is reported as a failure
/// of its own rather than read as "no failure".
pub fn last_failure(home: &Path) -> Option<ReportFailure> {
    let path = failure_path(home);
    match fs::read(&path) {
        Ok(raw) => match serde_json::from_slice(&raw) {
            Ok(failure) => Some(failure),
            Err(error) => Some(ReportFailure {
                pane_id: String::new(),
                event: String::new(),
                socket_path: String::new(),
                error: format!("{} could not be read back: {error}", path.display()),
                at_unix_ms: 0,
            }),
        },
        Err(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixListener;

    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "hide-agent-hooks-report-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("scratch directory");
        root
    }

    #[test]
    fn the_report_names_the_pane_the_source_and_all_three_tokens() {
        let params = report_params(
            "w7B:pM",
            PaneCounters {
                working: 2,
                done: 5,
            },
        );
        assert_eq!(params["pane_id"], "w7B:pM");
        assert_eq!(params["source"], "hide-subagents");
        assert_eq!(
            params["tokens"],
            json!({
                "hide_hooks": HOOK_VERSION.to_string(),
                "hide_sub_working": "2",
                "hide_sub_done": "5"
            })
        );
    }

    #[test]
    fn the_source_and_token_names_are_ones_herdr_accepts() {
        // Herdr answers `invalid_metadata_source` for anything else; the
        // first hook shipped `hide-subagents@1` and every report was refused.
        assert!(
            metadata_source()
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b":._-".contains(&byte)),
            "metadata source may contain only ASCII letters, digits, colon, dot, underscore, and hyphen"
        );
        // `PaneReportMetadataParams.tokens` in contracts/herdr-api.schema.json.
        for name in [INSTRUMENTED_TOKEN, WORKING_TOKEN, DONE_TOKEN] {
            assert!(
                (1..=32).contains(&name.len())
                    && name
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-'),
                "{name} does not match ^[A-Za-z0-9_-]{{1,32}}$"
            );
        }
    }

    #[test]
    fn a_report_is_one_pane_report_metadata_request_and_a_success_clears_the_failure() {
        let root = scratch("request");
        // A Unix socket path is capped at SUN_LEN (104 bytes on macOS), and a
        // harness that points TMPDIR into its run directory exceeds it, so the
        // socket alone binds under the short system root, as the
        // hide-herdr-client socket tests do.
        let socket_root =
            Path::new("/tmp").join(format!("hide-agent-hooks-report-{}", std::process::id()));
        fs::create_dir_all(&socket_root).expect("socket directory");
        let socket = socket_root.join("herdr.sock");
        let _ = fs::remove_file(&socket);
        let listener = UnixListener::bind(&socket).expect("bind fake socket");
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let mut line = String::new();
            BufReader::new(stream.try_clone().expect("clone"))
                .read_line(&mut line)
                .expect("read request");
            let request: serde_json::Value = serde_json::from_str(&line).expect("request JSON");
            writeln!(
                stream,
                "{}",
                json!({"id": request["id"], "result": {"type": "ok"}})
            )
            .expect("write response");
            request
        });

        let outcome = report(
            &socket,
            "w1:p1",
            PaneCounters {
                working: 1,
                done: 0,
            },
        );
        assert!(outcome.is_ok(), "{outcome:?}");
        let request = server.join().expect("fake server joins");
        assert_eq!(request["method"], "pane.report_metadata");
        assert_eq!(
            request["params"],
            json!({
                "pane_id": "w1:p1",
                "source": "hide-subagents",
                "tokens": {"hide_hooks": "3", "hide_sub_working": "1", "hide_sub_done": "0"}
            })
        );

        let stale = Err(ApiError::Transport("earlier".to_owned()));
        record_outcome(&root, "w1:p1", HookEvent::SessionStart, &socket, &stale).expect("stale");
        assert!(last_failure(&root).is_some());
        record_outcome(&root, "w1:p1", HookEvent::Stop, &socket, &outcome).expect("record");
        assert_eq!(last_failure(&root), None);
        fs::remove_dir_all(&root).ok();
        fs::remove_file(&socket).ok();
        fs::remove_dir(&socket_root).ok();
    }

    #[test]
    fn a_report_nobody_answers_is_recorded_with_its_pane_event_and_socket() {
        let root = scratch("failure");
        let socket = root.join("absent.sock");
        let outcome = report(&socket, "w1:p1", PaneCounters::default());
        assert!(
            matches!(outcome, Err(ApiError::Transport(_))),
            "{outcome:?}"
        );

        record_outcome(&root, "w1:p1", HookEvent::SessionStart, &socket, &outcome).expect("record");
        let failure = last_failure(&root).expect("a failure is remembered");
        assert_eq!(failure.pane_id, "w1:p1");
        assert_eq!(failure.event, "SessionStart");
        assert_eq!(failure.socket_path, socket.display().to_string());
        assert!(
            failure.error.contains("connect failed"),
            "{}",
            failure.error
        );
        assert!(failure.message().contains("w1:p1"));

        record_outcome(&root, "w1:p1", HookEvent::Stop, &socket, &Ok(())).expect("clear");
        assert_eq!(last_failure(&root), None);
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn an_unreadable_failure_record_is_a_failure_not_a_clean_bill() {
        let root = scratch("unreadable");
        let path = failure_path(&root);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, b"not json").unwrap();
        let failure = last_failure(&root).expect("reported");
        assert!(failure.error.contains("could not be read back"));
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn the_socket_comes_from_the_environment_or_herdrs_default() {
        // The override is process-global, so this test only checks the
        // default; the override branch is one `var_os` read.
        if std::env::var_os("HERDR_SOCKET_PATH").is_none() {
            assert_eq!(
                socket_path(Path::new("/Users/example")),
                Path::new("/Users/example/.config/herdr/herdr.sock")
            );
        }
    }
}
