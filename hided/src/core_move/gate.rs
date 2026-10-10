//! A core started on a move's copy waits for the move to commit (PRD
//! core-host-node-move amendment 3): while its handover is pending it takes
//! only the link that carries the move's intent, and every worker that acts
//! outside the machine holds until that link is taken. A core with no
//! pending handover is open from the start.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::json;
use tokio::sync::watch;

use super::handover::{self, Activation, HandoverState};

pub struct MoveGate {
    state_dir: PathBuf,
    /// The pending move's intent; `None` once open.
    intent: std::sync::Mutex<Option<String>>,
    open: watch::Sender<bool>,
}

impl MoveGate {
    /// The gate for a core starting on `state_dir`.
    pub fn for_start(state_dir: &Path) -> Result<Arc<Self>, String> {
        let pending = match handover::read(state_dir)? {
            Some(record) if record.state == HandoverState::Pending => Some(record.intent),
            Some(record) if record.state == HandoverState::Active => None,
            Some(record) => {
                return Err(format!(
                    "this state folder's core left it in move {}; it starts no core",
                    record.intent
                ));
            }
            None => None,
        };
        let open = pending.is_none();
        Ok(Arc::new(Self {
            state_dir: state_dir.to_path_buf(),
            intent: std::sync::Mutex::new(pending),
            open: watch::Sender::new(open),
        }))
    }

    pub fn is_open(&self) -> bool {
        *self.open.borrow()
    }

    /// Hears the gate open; it never closes again.
    pub fn subscribe(&self) -> watch::Receiver<bool> {
        self.open.subscribe()
    }

    /// The pending move's intent, while the gate is closed.
    pub fn pending_intent(&self) -> Option<String> {
        self.intent
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// Whether a node's link that names `move_intent` may be taken now: a
    /// pending core takes only its own move's link, and taking it is the
    /// commit, recorded before the node is told.
    pub fn admit(&self, node: &str, move_intent: Option<&str>) -> Result<(), &'static str> {
        let mut pending = self
            .intent
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(intent) = pending.clone() else {
            return Ok(());
        };
        if move_intent != Some(intent.as_str()) {
            return Err(if move_intent.is_none() {
                "core_pending"
            } else {
                "move_unknown"
            });
        }
        match handover::activate(&self.state_dir, &intent) {
            Ok(Activation::Activated | Activation::AlreadyActive) => {
                *pending = None;
                drop(pending);
                herdr_core::diagnostic!(json!({
                    "component": "core_move",
                    "kind": "move.committed",
                    "intent": intent,
                    "node": node,
                }));
                self.open.send_replace(true);
                Ok(())
            }
            Ok(Activation::NotThisMove) => Err("move_unknown"),
            Err(message) => {
                herdr_core::diagnostic!(json!({
                    "component": "core_move",
                    "kind": "move.activation_failed",
                    "intent": intent,
                    "message": message,
                }));
                Err("move_unavailable")
            }
        }
    }
}
