//! What a worker's labels lay onto a session projection.
//!
//! The coordinator takes one overlay from its worker each time it publishes
//! and the runtime keeps the last one, so a projection that reaches the
//! runtime by another path (a workspace created or an agent closed, each
//! with its own fresh Herdr projection) keeps the same labels and elapsed
//! times instead of drawing provider names until the next publish. Each
//! label still carries the session it was proven for and is laid only while
//! the pane's current reference proves it (D-04), so a stale overlay can
//! never put one session's words on another.

use std::collections::HashMap;

use hide_session::label_reference_token;

use super::store::{self, PaneRecord};
use crate::sidebar::{AgentLabel, SessionSnapshotPayload};

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct LabelOverlay {
    panes: HashMap<String, OverlayPane>,
}

#[derive(Clone, Debug, PartialEq)]
struct OverlayPane {
    changed_unix_ms: u64,
    /// `None` while this daemon does not generate labels (another daemon on
    /// the same Herdr holds the generator role, D-10): only elapsed time is
    /// laid.
    label: Option<ProvenLabel>,
}

#[derive(Clone, Debug, PartialEq)]
struct ProvenLabel {
    owner: Option<String>,
    proven_reference: Option<String>,
    task: Option<String>,
    progress: String,
    expected_reply: String,
    question: bool,
}

impl LabelOverlay {
    pub(crate) fn of_records<'a>(
        records: impl IntoIterator<Item = (&'a String, &'a PaneRecord)>,
        labels_shown: bool,
    ) -> Self {
        let panes = records
            .into_iter()
            .map(|(pane_id, record)| {
                let label = labels_shown.then(|| ProvenLabel {
                    owner: record.owner.clone(),
                    proven_reference: record.proven_reference.clone(),
                    task: record.task.clone(),
                    progress: record.progress.clone(),
                    expected_reply: record.expected_reply.clone(),
                    question: record.question,
                });
                (
                    pane_id.clone(),
                    OverlayPane {
                        changed_unix_ms: record.changed_unix_ms,
                        label,
                    },
                )
            })
            .collect();
        Self { panes }
    }

    pub(crate) fn apply(&self, payload: &mut SessionSnapshotPayload) {
        for agent in &mut payload.agents {
            let Some(pane_id) = agent.pane_id.as_deref().or(agent.id.as_deref()) else {
                continue;
            };
            let Some(pane) = self.panes.get(pane_id) else {
                continue;
            };
            agent.changed_at_unix_ms = Some(pane.changed_unix_ms);
            let Some(label) = pane.label.as_ref() else {
                continue;
            };
            let reference = agent.agent_session.as_ref().and_then(|session| {
                label_reference_token(
                    agent.agent.as_deref().unwrap_or_default(),
                    &session.kind,
                    &session.value,
                )
            });
            if !label.proves(reference.as_deref()) {
                continue;
            }
            let non_empty = |value: &str| (!value.trim().is_empty()).then(|| value.to_owned());
            agent.label = Some(AgentLabel {
                task: label.task.clone(),
                progress: non_empty(&label.progress),
                expected_reply: non_empty(&label.expected_reply),
                question: label.question && agent.agent_status.as_deref() != Some("working"),
            });
        }
    }
}

impl ProvenLabel {
    fn proves(&self, reference: Option<&str>) -> bool {
        store::proves(
            self.owner.as_deref(),
            self.proven_reference.as_deref(),
            reference,
        )
    }
}
