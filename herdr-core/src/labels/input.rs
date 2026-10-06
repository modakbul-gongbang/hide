//! When the operator submitted something to a pane through Hide (PRD
//! overview-request-view D-19).
//!
//! Herdr's input calls carry no sender, so who wrote a person's message in a
//! conversation is decided from what Hide itself saw: the desktop's and a
//! device pane's keyboard (`Event::Key`) and the phone's reply
//! (`pane_input_submitted`). Only the moment of a submit is kept, never what
//! was typed. The runtime records under its own lock; the label workers read
//! from their coordinator threads; this record has its own small lock, taken
//! after the runtime's and never around it.
//!
//! Memory only and bounded: a restart starts empty, and a message written
//! before this process saw the pane is judged by its envelope and lineage
//! alone (B51).

use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;


/// Submits kept per pane, newest last.
pub(crate) const SUBMITS_PER_PANE: usize = 32;
/// Panes kept; the pane submitted to longest ago goes first.
pub(crate) const PANES: usize = 512;

/// One submit: when, and whether the agent was running then (a prompt it
/// queues is written to its conversation only when it takes it up).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Submit {
    /// Increases with every submit this process records.
    pub(crate) seq: u64,
    pub(crate) at_unix_ms: u64,
    pub(crate) while_working: bool,
}

#[derive(Default)]
struct Record {
    panes: HashMap<(String, String), VecDeque<Submit>>,
    /// Pane keys by their last submit, oldest first.
    order: VecDeque<(String, String)>,
    next_seq: u64,
}

pub(crate) struct OperatorInput {
    /// The label target of the core's own Herdr server: its node id.
    node: String,
    record: Mutex<Record>,
}

impl OperatorInput {
    pub(crate) fn new(node: &str) -> Self {
        Self {
            node: node.to_owned(),
            record: Mutex::default(),
        }
    }

    /// Records a submit to `pane_id` as the runtime names it: a device's pane
    /// is `remote:<device>:pane:<id>`, which a label worker knows as `<id>`
    /// under the target `device:<device>`.
    pub(crate) fn record(&self, pane_id: &str, at_unix_ms: u64, while_working: bool) {
        let key = label_key(&self.node, pane_id);
        let Ok(mut record) = self.record.lock() else {
            return;
        };
        record.next_seq += 1;
        let submit = Submit {
            seq: record.next_seq,
            at_unix_ms,
            while_working,
        };
        if let Some(position) = record.order.iter().position(|known| *known == key) {
            record.order.remove(position);
        } else if record.order.len() >= PANES
            && let Some(oldest) = record.order.pop_front()
        {
            record.panes.remove(&oldest);
        }
        record.order.push_back(key.clone());
        let submits = record.panes.entry(key).or_default();
        if submits.len() >= SUBMITS_PER_PANE {
            submits.pop_front();
        }
        submits.push_back(submit);
    }

    /// The submits recorded for a worker's pane, oldest first.
    pub(crate) fn submits(&self, target: &str, pane_id: &str) -> Vec<Submit> {
        let Ok(record) = self.record.lock() else {
            return Vec::new();
        };
        record
            .panes
            .get(&(target.to_owned(), pane_id.to_owned()))
            .map(|submits| submits.iter().copied().collect())
            .unwrap_or_default()
    }
}

fn label_key(node: &str, pane_id: &str) -> (String, String) {
    match pane_id
        .strip_prefix("remote:")
        .and_then(|rest| rest.split_once(":pane:"))
    {
        Some((device, pane)) => (format!("device:{device}"), pane.to_owned()),
        None => (node.to_owned(), pane_id.to_owned()),
    }
}

/// [`submits`] for the base64 a key event carries, decoded on the stack: the
/// keyboard path allocates nothing for it.
pub(crate) fn key_submits(bytes_base64: &str) -> bool {
    use base64::Engine as _;
    let mut bytes = [0_u8; TYPED_BURST + 3];
    if bytes_base64.len() > (TYPED_BURST / 3 + 1) * 4 {
        return false;
    }
    base64::engine::general_purpose::STANDARD
        .decode_slice(bytes_base64, &mut bytes)
        .is_ok_and(|length| submits(&bytes[..length]))
}

/// The longest chunk looked at; anything longer is a paste.
const TYPED_BURST: usize = 64;

/// Whether keyboard bytes submit what was typed: a carriage return that is
/// not part of an escape sequence (`ESC CR` is a newline in Claude Code) and
/// not inside a bracketed paste. A chunk longer than any typed burst is a
/// paste and is not looked at.
pub(crate) fn submits(bytes: &[u8]) -> bool {
    const PASTE_START: &[u8] = b"\x1b[200~";
    const PASTE_END: &[u8] = b"\x1b[201~";
    if bytes.len() > TYPED_BURST {
        return false;
    }
    let mut pasting = false;
    let mut index = 0;
    while index < bytes.len() {
        let rest = &bytes[index..];
        if rest.starts_with(PASTE_START) {
            pasting = true;
            index += PASTE_START.len();
            continue;
        }
        if rest.starts_with(PASTE_END) {
            pasting = false;
            index += PASTE_END.len();
            continue;
        }
        if bytes[index] == b'\r' && !pasting && (index == 0 || bytes[index - 1] != 0x1b) {
            return true;
        }
        index += 1;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::labels::store::LOCAL_TARGET;

    #[test]
    fn only_a_typed_return_is_a_submit() {
        assert!(submits(b"\r"));
        assert!(submits(b"yes\r"));
        assert!(!submits(b"\x1b\r"), "a newline in Claude Code");
        assert!(!submits(b"\x1b[200~line one\rline two\x1b[201~"));
        assert!(submits(b"\x1b[200~pasted\x1b[201~\r"));
        assert!(!submits(b"abc"));
        assert!(!submits(format!("{}\r", "a".repeat(80)).as_bytes()));
        assert!(key_submits(&crate::live::encode_base64(b"\r")));
        assert!(!key_submits(&crate::live::encode_base64(b"\x1b\r")));
        assert!(!key_submits(&crate::live::encode_base64(&[b'\r'; 200])));
        assert!(!key_submits("not base64!"));
    }

    #[test]
    fn a_device_pane_is_kept_under_its_worker_and_every_bound_holds() {
        let input = OperatorInput::new(LOCAL_TARGET);
        input.record("remote:mini:pane:w1-2", 10, false);
        input.record("p1", 20, true);
        assert_eq!(input.submits("device:mini", "w1-2")[0].at_unix_ms, 10);
        assert!(input.submits(LOCAL_TARGET, "w1-2").is_empty());
        assert!(input.submits(LOCAL_TARGET, "p1")[0].while_working);

        for at in 0..(SUBMITS_PER_PANE as u64 + 5) {
            input.record("p1", 100 + at, false);
        }
        let kept = input.submits(LOCAL_TARGET, "p1");
        assert_eq!(kept.len(), SUBMITS_PER_PANE);
        assert!(kept.windows(2).all(|pair| pair[0].seq < pair[1].seq));

        for pane in 0..PANES {
            input.record(&format!("many-{pane}"), 1, false);
        }
        assert!(
            input.submits("device:mini", "w1-2").is_empty(),
            "oldest pane goes first"
        );
        assert!(!input.submits(LOCAL_TARGET, "many-0").is_empty());
    }
}
