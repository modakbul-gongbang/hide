//! The per-pane subagent count the hook keeps between invocations.
//!
//! A hook script is stateless: it is started fresh for every event and knows
//! only its environment. The count therefore lives here, keyed by
//! `$HERDR_PANE_ID`, and is republished to Herdr after every event so the
//! core reads it back out of the pane tokens its ordinary snapshot already
//! carries (PRD D-53, and Technical structure).

use std::collections::HashSet;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::runtime::{HOOK_VERSION, HookEvent};

/// What one pane's session has spawned.
///
/// `blocked` is deliberately absent rather than zero: neither shipped runtime
/// reports a blocked subagent, and drawing a zero would claim knowledge the
/// adapter does not have (PRD B32, D-53).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct PaneCounters {
    #[serde(default)]
    pub working: u32,
    #[serde(default)]
    pub done: u32,
}

/// What the file of one pane holds: the counts, and who reported them.
///
/// Herdr keeps the tokens a report sets only as long as the server that took
/// it, so a handoff or a restart empties them while this file stays and the
/// pane ids stay. Which pane the file is for is therefore known, and what
/// else a restore must know is whether the agent running there now is the
/// one that reported: `version` is the hook version of the helper that wrote
/// the file, `agent` the adapter id of the runtime that reported, and
/// `session` the agent's own id of the session that did, when the hook's
/// input named it. A file an older helper wrote has none of them and is not
/// restored from.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
struct Record {
    #[serde(default)]
    working: u32,
    #[serde(default)]
    done: u32,
    #[serde(default)]
    version: Option<u32>,
    #[serde(default)]
    agent: Option<String>,
    #[serde(default)]
    session: Option<String>,
}

impl Record {
    fn counters(&self) -> PaneCounters {
        PaneCounters {
            working: self.working,
            done: self.done,
        }
    }

    /// Whether this file already says what an event of `who` would write.
    /// An event that names no session says nothing against the file's.
    fn names(&self, who: Reporter<'_>) -> bool {
        self.version == Some(HOOK_VERSION)
            && self.agent.as_deref() == who.agent
            && who
                .session
                .is_none_or(|session| self.session.as_deref() == Some(session))
    }
}

/// Who is reporting a pane's counts: the runtime's adapter id, and the
/// session the hook's input named, when it did.
///
/// A session the hook could not read stays unknown rather than carried over
/// from an earlier event of the pane, except by an event that changes no
/// count ([`Change::None`]), which is not evidence of a different session.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Reporter<'a> {
    pub agent: Option<&'a str>,
    pub session: Option<&'a str>,
}

/// What the pane's file said last, as a restore reads it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Restorable {
    pub counters: PaneCounters,
    pub version: u32,
    /// The adapter id of the runtime that reported.
    pub agent: String,
    /// The session that reported, when its hook's input named one.
    pub session: Option<String>,
}

/// What a restore can do with a pane's file.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Restore {
    /// No event ever counted this pane here.
    NoRecord,
    /// An older helper wrote the file, or a hook that did not know its own
    /// runtime: which agent reported is unknown, so nothing is guessed and the
    /// pane waits for its next event.
    Unpairable,
    /// What the pane's last report said.
    Report(Restorable),
}

/// Where the counts live. Under Hide's own directory, never the runtime's.
pub fn state_directory(home: &Path) -> PathBuf {
    home.join(".hide").join("agent-hooks").join("panes")
}

/// A pane id is Herdr's (`w7B:pM`), so it is folded into a file name rather
/// than used as one.
fn record_path(home: &Path, pane_id: &str) -> PathBuf {
    let safe: String = pane_id
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '-' || character == '_' {
                character
            } else {
                '_'
            }
        })
        .collect();
    state_directory(home).join(format!("{safe}.json"))
}

/// How long one event waits for another event of the same pane to finish its
/// update. Parallel subagents start and stop together, and an update read
/// before another one's write would lose that one.
const LOCK_WAIT: Duration = Duration::from_millis(1_000);

/// One change an event makes to a pane's counts.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Change {
    /// A new session took over the pane: its counts start again.
    Reset,
    /// One subagent started.
    Started,
    /// One subagent finished.
    Stopped,
    /// The turn ended with this many subagents still running: none for an
    /// agent whose subagents end with its turn, and the background ones Grok
    /// lists for a turn that leaves them running.
    Settled { running: u32 },
    /// The event changes no count; the pane's counts are only republished.
    None,
}

impl Change {
    /// The change a six-event runtime's hook makes.
    ///
    /// - `SessionStart` starts the pane over: a new session in a reused pane
    ///   must not inherit the last one's numbers.
    /// - `Stop` sweeps `working` to zero. The turn is over, so nothing this
    ///   session spawned is still running, and a `SubagentStop` that never
    ///   arrived cannot leave a count behind (PRD B31).
    pub fn of(event: HookEvent) -> Self {
        match event {
            HookEvent::SessionStart => Self::Reset,
            HookEvent::UserPromptSubmit | HookEvent::PreToolUse => Self::None,
            HookEvent::SubagentStart => Self::Started,
            HookEvent::SubagentStop => Self::Stopped,
            HookEvent::Stop => Self::Settled { running: 0 },
        }
    }
}

/// The pane's record as the last finished change left it, read under the
/// shared side of the lock, so a change being written is never read half
/// way; a record that cannot be read fails rather than answering zeros.
pub fn read_settled(home: &Path, pane_id: &str) -> io::Result<PaneCounters> {
    read_record(home, pane_id).map(|record| record.counters())
}

/// What a pane's last report said, read under the shared side of the lock
/// like [`read_settled`], for putting it back on a Herdr that lost it.
pub fn restore_of(home: &Path, pane_id: &str) -> io::Result<Restore> {
    match read_record(home, pane_id) {
        Ok(record) => Ok(match (&record.version, &record.agent) {
            (Some(version), Some(agent)) => Restore::Report(Restorable {
                counters: record.counters(),
                version: *version,
                agent: agent.clone(),
                session: record.session.clone(),
            }),
            _ => Restore::Unpairable,
        }),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Restore::NoRecord),
        Err(error) => Err(error),
    }
}

fn read_record(home: &Path, pane_id: &str) -> io::Result<Record> {
    let _held = hold(home, hide_platform::fs::lock::Mode::Shared)?;
    let raw = fs::read(record_path(home, pane_id))?;
    serde_json::from_slice(&raw).map_err(io::Error::from)
}

pub fn read(home: &Path, pane_id: &str) -> PaneCounters {
    let path = record_path(home, pane_id);
    fs::read_to_string(&path)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

/// Applies one [`Change`] under the pane's lock and returns the new counts.
/// The record is read, changed and written while the lock is held, so two
/// events of one pane never both start from the same count. The write is
/// not synced: a count describes running sessions and is rebuilt by their
/// next events, so it does not have to survive a crash, and a sync inside
/// the lock would make parallel subagent starts wait out the lock.
pub fn change(
    home: &Path,
    pane_id: &str,
    change: Change,
    who: Reporter<'_>,
) -> io::Result<PaneCounters> {
    let path = record_path(home, pane_id);
    if let Some(parent) = path.parent() {
        hide_platform::fs::private::create_dir_all(parent)?;
    }
    if change == Change::None {
        // An event that changes nothing leaves a file that already names this
        // reporter alone, so the common event stays a shared read. A pane with
        // no file yet, or one that names another version, agent or session,
        // gets one below: every pane that has reported has a file that says
        // who reported, which is what a restore reads.
        match read_record(home, pane_id) {
            Ok(record) if record.names(who) => return Ok(record.counters()),
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    let held = hold(home, hide_platform::fs::lock::Mode::Exclusive)?;
    let before = read_lenient(&path);
    let mut counters = before.counters();
    match change {
        Change::Reset => counters = PaneCounters::default(),
        Change::Started => counters.working = counters.working.saturating_add(1),
        Change::Stopped => {
            counters.working = counters.working.saturating_sub(1);
            counters.done = counters.done.saturating_add(1);
        }
        Change::Settled { running } => counters.working = running,
        Change::None => {}
    }
    let session = match change {
        // Not evidence of another session, so the one the file names stays
        // unless this event names its own, and only for the same agent.
        Change::None => who.session.map(str::to_owned).or_else(|| {
            before
                .session
                .clone()
                .filter(|_| before.agent.as_deref() == who.agent)
        }),
        _ => who.session.map(str::to_owned),
    };
    write_record(&path, counters, who, session)?;
    drop(held);
    Ok(counters)
}

/// Replaces a pane's counts with ones its agent keeps itself: OpenCode's
/// plugin follows its child sessions and sends the totals, not events. It
/// holds the same lock as [`change`], so a total and an event never
/// interleave.
pub fn store(
    home: &Path,
    pane_id: &str,
    counters: PaneCounters,
    who: Reporter<'_>,
) -> io::Result<()> {
    let path = record_path(home, pane_id);
    if let Some(parent) = path.parent() {
        hide_platform::fs::private::create_dir_all(parent)?;
    }
    let held = hold(home, hide_platform::fs::lock::Mode::Exclusive)?;
    write_record(&path, counters, who, who.session.map(str::to_owned))?;
    drop(held);
    Ok(())
}

/// The file as it is, or an empty record for one that is missing or cannot be
/// read: the next write replaces it whole, under the lock the caller holds.
fn read_lenient(path: &Path) -> Record {
    fs::read(path)
        .ok()
        .and_then(|raw| serde_json::from_slice(&raw).ok())
        .unwrap_or_default()
}

/// Writes a record in place, private to the account, under a held lock.
fn write_record(
    path: &Path,
    counters: PaneCounters,
    who: Reporter<'_>,
    session: Option<String>,
) -> io::Result<()> {
    let mut record = hide_platform::fs::private::open_or_create_file(path)?;
    record.set_len(0)?;
    record.write_all(&serde_json::to_vec(&Record {
        working: counters.working,
        done: counters.done,
        version: Some(HOOK_VERSION),
        agent: who.agent.map(str::to_owned),
        session,
    })?)
}

/// One lock file beside the records, not the record itself: Windows locks a
/// byte range against every other handle, so a record locked by one handle
/// could not be rewritten through another. It is the account's own, so no
/// other account can hold it and stall every count.
fn hold(
    home: &Path,
    mode: hide_platform::fs::lock::Mode,
) -> io::Result<hide_platform::fs::lock::Lock> {
    let lock = hide_platform::fs::private::open_or_create_file(&lock_path(home))?;
    match hide_platform::fs::lock::lock_file(lock, mode, LOCK_WAIT, &|| false)? {
        hide_platform::fs::lock::Waited::Locked(held) => Ok(held),
        _ => Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "another hook event held the pane counts",
        )),
    }
}

/// The lock every count update holds. It sits beside the records' folder,
/// so the sweep in [`retain`] never takes it for a record.
fn lock_path(home: &Path) -> PathBuf {
    home.join(".hide").join("agent-hooks").join("panes.lock")
}

/// Drops a pane's record. Used by the diagnosis when a pane is gone.
pub fn forget(home: &Path, pane_id: &str) -> io::Result<()> {
    match fs::remove_file(record_path(home, pane_id)) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

/// Drops every record whose pane no longer exists, and reports how many went.
///
/// A pane that Herdr has stopped listing cannot be running a session, so its
/// counts belong to one that is over and nothing can ask for them again (PRD
/// B31, D-53). Panes are the key rather than agents because the record is
/// written by `SessionStart`, which can run before the agent is listed;
/// sweeping by agent would race a session into losing its own count.
///
/// A record the sweep cannot read its name back from is left alone: this
/// directory is Hide's, but deleting a file on a guess is not a cleanup.
pub fn retain<'a>(
    home: &Path,
    live_pane_ids: impl IntoIterator<Item = &'a str>,
) -> io::Result<usize> {
    let keep: HashSet<PathBuf> = live_pane_ids
        .into_iter()
        .map(|pane_id| record_path(home, pane_id))
        .collect();
    let directory = state_directory(home);
    let entries = match fs::read_dir(&directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(error),
    };
    let mut dropped = 0;
    for entry in entries {
        let path = entry?.path();
        if path.extension().is_none_or(|extension| extension != "json") || keep.contains(&path) {
            continue;
        }
        fs::remove_file(&path)?;
        dropped += 1;
    }
    Ok(dropped)
}

#[cfg(test)]
mod tests {
    use super::*;

    const WHO: Reporter<'static> = Reporter {
        agent: Some("claude-code"),
        session: Some("session-a"),
    };

    fn home(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "hide-agent-hooks-counters-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn a_started_subagent_counts_until_it_stops_and_then_counts_as_done() {
        let root = home("lifecycle");
        let pane = "w7B:pM";
        assert_eq!(
            change(&root, pane, Change::of(HookEvent::SessionStart), WHO).unwrap(),
            PaneCounters::default()
        );
        change(&root, pane, Change::of(HookEvent::SubagentStart), WHO).unwrap();
        let two = change(&root, pane, Change::of(HookEvent::SubagentStart), WHO).unwrap();
        assert_eq!(
            two,
            PaneCounters {
                working: 2,
                done: 0
            }
        );
        let one = change(&root, pane, Change::of(HookEvent::SubagentStop), WHO).unwrap();
        assert_eq!(
            one,
            PaneCounters {
                working: 1,
                done: 1
            }
        );
        fs::remove_dir_all(&root).unwrap();
    }

    fn restorable(working: u32, done: u32, session: Option<&str>) -> Restore {
        Restore::Report(Restorable {
            counters: PaneCounters { working, done },
            version: HOOK_VERSION,
            agent: "claude-code".into(),
            session: session.map(str::to_owned),
        })
    }

    #[test]
    fn a_pane_that_reported_has_a_file_that_says_who_reported_what() {
        let root = home("restore");
        let pane = "w7B:pM";
        assert_eq!(restore_of(&root, pane).unwrap(), Restore::NoRecord);
        change(&root, pane, Change::of(HookEvent::SubagentStart), WHO).unwrap();
        change(&root, pane, Change::of(HookEvent::SubagentStart), WHO).unwrap();
        change(&root, pane, Change::of(HookEvent::SubagentStop), WHO).unwrap();
        assert_eq!(
            restore_of(&root, pane).unwrap(),
            restorable(1, 1, Some("session-a"))
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn an_event_that_names_another_session_replaces_the_one_the_file_names() {
        let root = home("restore-session");
        let pane = "w7B:pM";
        change(&root, pane, Change::Started, WHO).unwrap();
        let other = Reporter {
            session: Some("session-b"),
            ..WHO
        };
        change(&root, pane, Change::Started, other).unwrap();
        assert_eq!(
            restore_of(&root, pane).unwrap(),
            restorable(2, 0, Some("session-b"))
        );
        // A hook that could not read its input names no session, and the
        // count it changed is not the earlier session's to claim.
        let unknown = Reporter {
            session: None,
            ..WHO
        };
        change(&root, pane, Change::Started, unknown).unwrap();
        assert_eq!(restore_of(&root, pane).unwrap(), restorable(3, 0, None));
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn an_event_that_changes_nothing_keeps_the_session_for_the_same_agent_only() {
        let root = home("restore-none");
        let pane = "w7B:pM";
        change(&root, pane, Change::of(HookEvent::SessionStart), WHO).unwrap();
        let blind = Reporter {
            session: None,
            ..WHO
        };
        change(&root, pane, Change::of(HookEvent::PreToolUse), blind).unwrap();
        assert_eq!(
            restore_of(&root, pane).unwrap(),
            restorable(0, 0, Some("session-a"))
        );
        // Another runtime in the pane: the earlier session is not its own.
        let codex = Reporter {
            agent: Some("codex"),
            session: None,
        };
        change(&root, pane, Change::of(HookEvent::PreToolUse), codex).unwrap();
        assert_eq!(
            restore_of(&root, pane).unwrap(),
            Restore::Report(Restorable {
                counters: PaneCounters::default(),
                version: HOOK_VERSION,
                agent: "codex".into(),
                session: None,
            })
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn an_event_that_changes_nothing_still_leaves_a_file_for_a_pane_with_none() {
        let root = home("restore-first");
        let pane = "w7B:pM";
        assert_eq!(
            change(&root, pane, Change::of(HookEvent::PreToolUse), WHO).unwrap(),
            PaneCounters::default()
        );
        assert_eq!(
            restore_of(&root, pane).unwrap(),
            restorable(0, 0, Some("session-a"))
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_file_an_older_helper_wrote_is_unpairable_and_the_next_event_rewrites_it() {
        let root = home("restore-old");
        let pane = "w7B:pM";
        fs::create_dir_all(state_directory(&root)).unwrap();
        // Counts and nothing else, or a version without who reported.
        for old in [
            br#"{"working":2,"done":4}"#.as_slice(),
            br#"{"working":2,"done":4,"version":6}"#.as_slice(),
        ] {
            fs::write(record_path(&root, pane), old).unwrap();
            assert_eq!(restore_of(&root, pane).unwrap(), Restore::Unpairable);
            // The counts are still read the way they always were.
            assert_eq!(
                read_settled(&root, pane).unwrap(),
                PaneCounters {
                    working: 2,
                    done: 4
                }
            );
        }
        change(&root, pane, Change::of(HookEvent::PreToolUse), WHO).unwrap();
        assert_eq!(
            restore_of(&root, pane).unwrap(),
            restorable(2, 4, Some("session-a"))
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_file_that_cannot_be_read_is_an_error_and_not_an_empty_restore() {
        let root = home("restore-corrupt");
        let pane = "w7B:pM";
        fs::create_dir_all(state_directory(&root)).unwrap();
        fs::write(record_path(&root, pane), b"not a record").unwrap();
        assert_eq!(
            restore_of(&root, pane).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_turn_that_ends_sweeps_a_count_a_missing_stop_left_behind() {
        let root = home("sweep");
        let pane = "w7B:pM";
        change(&root, pane, Change::of(HookEvent::SessionStart), WHO).unwrap();
        change(&root, pane, Change::of(HookEvent::SubagentStart), WHO).unwrap();
        change(&root, pane, Change::of(HookEvent::SubagentStart), WHO).unwrap();
        let swept = change(&root, pane, Change::of(HookEvent::Stop), WHO).unwrap();
        assert_eq!(swept.working, 0, "no subagent outlives its turn");
        assert_eq!(swept.done, 0, "the sweep does not invent completions");
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_new_session_in_the_same_pane_does_not_inherit_the_last_ones_numbers() {
        let root = home("reset");
        let pane = "w7B:pM";
        change(&root, pane, Change::of(HookEvent::SubagentStart), WHO).unwrap();
        change(&root, pane, Change::of(HookEvent::SubagentStop), WHO).unwrap();
        assert_eq!(
            read(&root, pane),
            PaneCounters {
                working: 0,
                done: 1
            }
        );
        assert_eq!(
            change(&root, pane, Change::of(HookEvent::SessionStart), WHO).unwrap(),
            PaneCounters::default()
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_sweep_drops_the_records_of_panes_that_are_gone_and_keeps_the_rest() {
        let root = home("retain");
        change(&root, "w1:pA", Change::of(HookEvent::SubagentStart), WHO).unwrap();
        change(&root, "w1:pB", Change::of(HookEvent::SubagentStart), WHO).unwrap();
        change(&root, "w2:pC", Change::of(HookEvent::SubagentStart), WHO).unwrap();
        // A file the sweep did not write is not its business.
        fs::write(state_directory(&root).join("notes.txt"), b"kept").unwrap();

        assert_eq!(retain(&root, ["w1:pA", "w2:pC"]).unwrap(), 1);
        assert_eq!(read(&root, "w1:pA").working, 1);
        assert_eq!(read(&root, "w2:pC").working, 1);
        assert_eq!(
            read(&root, "w1:pB"),
            PaneCounters::default(),
            "a dead session's count is gone rather than waiting to be redrawn"
        );
        assert!(state_directory(&root).join("notes.txt").exists());

        // Idempotent: the same live set sweeps nothing the second time.
        assert_eq!(retain(&root, ["w1:pA", "w2:pC"]).unwrap(), 0);
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_sweep_before_any_hook_has_run_is_not_an_error() {
        let root = home("retain-empty");
        assert_eq!(retain(&root, ["w1:pA"]).unwrap(), 0);
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn parallel_starts_of_one_pane_all_count() {
        let root = home("parallel");
        let pane = "w7B:pM";
        let threads: Vec<_> = (0..8)
            .map(|_| {
                let root = root.clone();
                std::thread::spawn(move || change(&root, pane, Change::Started, WHO).unwrap())
            })
            .collect();
        for thread in threads {
            thread.join().unwrap();
        }
        assert_eq!(read(&root, pane).working, 8);
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_turn_that_leaves_background_subagents_running_keeps_them_counted() {
        let root = home("settled");
        let pane = "w7B:pM";
        for _ in 0..3 {
            change(&root, pane, Change::Started, WHO).unwrap();
        }
        let settled = change(&root, pane, Change::Settled { running: 2 }, WHO).unwrap();
        assert_eq!(settled.working, 2);
        let one = change(&root, pane, Change::Stopped, WHO).unwrap();
        assert_eq!((one.working, one.done), (1, 1));
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn two_panes_keep_separate_counts() {
        let root = home("panes");
        change(&root, "w1:pA", Change::of(HookEvent::SubagentStart), WHO).unwrap();
        change(&root, "w2:pB", Change::of(HookEvent::SubagentStart), WHO).unwrap();
        change(&root, "w2:pB", Change::of(HookEvent::SubagentStart), WHO).unwrap();
        assert_eq!(read(&root, "w1:pA").working, 1);
        assert_eq!(read(&root, "w2:pB").working, 2);
        fs::remove_dir_all(&root).unwrap();
    }
}
