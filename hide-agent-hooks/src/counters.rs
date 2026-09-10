//! The per-pane subagent count the hook keeps between invocations.
//!
//! A hook script is stateless: it is started fresh for every event and knows
//! only its environment. The count therefore lives here, keyed by
//! `$HERDR_PANE_ID`, and is republished to Herdr after every event so the
//! core reads it back out of the pane tokens its ordinary snapshot already
//! carries (PRD D-53, and Technical structure).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

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
    let mut counters = read(home, pane_id);
    match event {
        HookEvent::SessionStart => counters = PaneCounters::default(),
        HookEvent::SubagentStart => counters.working = counters.working.saturating_add(1),
        HookEvent::SubagentStop => {
            counters.working = counters.working.saturating_sub(1);
            counters.done = counters.done.saturating_add(1);
        }
        HookEvent::Stop => counters.working = 0,
    }
    let path = record_path(home, pane_id);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&path, serde_json::to_vec(&counters)?)?;
    Ok(counters)
}

/// Drops a pane's record. Used by the diagnosis when a pane is gone.
pub fn forget(home: &Path, pane_id: &str) -> io::Result<()> {
    match fs::remove_file(record_path(home, pane_id)) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
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
