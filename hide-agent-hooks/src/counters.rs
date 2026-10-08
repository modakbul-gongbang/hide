//! The per-pane subagent count the hook keeps between invocations.
//!
//! A hook script is stateless: it is started fresh for every event and knows
//! only its environment. The count therefore lives here, keyed by
//! `$HERDR_PANE_ID`, and is republished to Herdr after every event so the
//! core reads it back out of the pane tokens its ordinary snapshot already
//! carries (PRD D-53, and Technical structure).

use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::runtime::HookEvent;

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

pub fn read(home: &Path, pane_id: &str) -> PaneCounters {
    let path = record_path(home, pane_id);
    fs::read_to_string(&path)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

/// Applies one hook event and returns the pane's new counts.
///
/// - `SessionStart` starts the pane over: a new session in a reused pane must
///   not inherit the last one's numbers.
/// - `Stop` sweeps `working` to zero. The turn is over, so nothing this
///   session spawned is still running, and a `SubagentStop` that never
///   arrived cannot leave a count behind (PRD B31).
pub fn apply(home: &Path, pane_id: &str, event: HookEvent) -> io::Result<PaneCounters> {
    change(home, pane_id, Change::of(event))
}

/// Applies one [`Change`] under the pane's lock and returns the new counts.
/// The record is read, changed and written while the lock is held, so two
/// events of one pane never both start from the same count.
pub fn change(home: &Path, pane_id: &str, change: Change) -> io::Result<PaneCounters> {
    if change == Change::None {
        return Ok(read(home, pane_id));
    }
    let path = record_path(home, pane_id);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    // One lock file beside the records, not the record itself: Windows locks
    // a byte range against every other handle, so a record locked by one
    // handle could not be rewritten through another.
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(lock_path(home))?;
    let held = match hide_platform::fs::lock::lock_file(
        lock,
        hide_platform::fs::lock::Mode::Exclusive,
        LOCK_WAIT,
        &|| false,
    )? {
        hide_platform::fs::lock::Waited::Locked(held) => held,
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "another hook event held the pane counts",
            ));
        }
    };
    let mut counters = read(home, pane_id);
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
    fs::write(&path, serde_json::to_vec(&counters)?)?;
    drop(held);
    Ok(counters)
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
            apply(&root, pane, HookEvent::SessionStart).unwrap(),
            PaneCounters::default()
        );
        apply(&root, pane, HookEvent::SubagentStart).unwrap();
        let two = apply(&root, pane, HookEvent::SubagentStart).unwrap();
        assert_eq!(
            two,
            PaneCounters {
                working: 2,
                done: 0
            }
        );
        let one = apply(&root, pane, HookEvent::SubagentStop).unwrap();
        assert_eq!(
            one,
            PaneCounters {
                working: 1,
                done: 1
            }
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_turn_that_ends_sweeps_a_count_a_missing_stop_left_behind() {
        let root = home("sweep");
        let pane = "w7B:pM";
        apply(&root, pane, HookEvent::SessionStart).unwrap();
        apply(&root, pane, HookEvent::SubagentStart).unwrap();
        apply(&root, pane, HookEvent::SubagentStart).unwrap();
        let swept = apply(&root, pane, HookEvent::Stop).unwrap();
        assert_eq!(swept.working, 0, "no subagent outlives its turn");
        assert_eq!(swept.done, 0, "the sweep does not invent completions");
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_new_session_in_the_same_pane_does_not_inherit_the_last_ones_numbers() {
        let root = home("reset");
        let pane = "w7B:pM";
        apply(&root, pane, HookEvent::SubagentStart).unwrap();
        apply(&root, pane, HookEvent::SubagentStop).unwrap();
        assert_eq!(
            read(&root, pane),
            PaneCounters {
                working: 0,
                done: 1
            }
        );
        assert_eq!(
            apply(&root, pane, HookEvent::SessionStart).unwrap(),
            PaneCounters::default()
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_sweep_drops_the_records_of_panes_that_are_gone_and_keeps_the_rest() {
        let root = home("retain");
        apply(&root, "w1:pA", HookEvent::SubagentStart).unwrap();
        apply(&root, "w1:pB", HookEvent::SubagentStart).unwrap();
        apply(&root, "w2:pC", HookEvent::SubagentStart).unwrap();
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
                std::thread::spawn(move || change(&root, pane, Change::Started).unwrap())
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
            change(&root, pane, Change::Started).unwrap();
        }
        let settled = change(&root, pane, Change::Settled { running: 2 }).unwrap();
        assert_eq!(settled.working, 2);
        let one = change(&root, pane, Change::Stopped).unwrap();
        assert_eq!((one.working, one.done), (1, 1));
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn two_panes_keep_separate_counts() {
        let root = home("panes");
        apply(&root, "w1:pA", HookEvent::SubagentStart).unwrap();
        apply(&root, "w2:pB", HookEvent::SubagentStart).unwrap();
        apply(&root, "w2:pB", HookEvent::SubagentStart).unwrap();
        assert_eq!(read(&root, "w1:pA").working, 1);
        assert_eq!(read(&root, "w2:pB").working, 2);
        fs::remove_dir_all(&root).unwrap();
    }
}
