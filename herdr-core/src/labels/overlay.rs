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
use hide_session::turns::Waiting;

use super::analysis::LabelEnd;
use super::facts::SessionFacts;
use super::store::{self, PaneRecord};
use crate::request_view::RowFacts;
use crate::sidebar::{AgentLabel, SessionAgentPayload, SessionSnapshotPayload};

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
    native_session_id: Option<String>,
    native_source_path: Option<String>,
    proven_reference: Option<String>,
    /// `None` while the operator has turned agent summaries off (D-11): the
    /// row stands on its facts alone.
    summary: Option<Summary>,
    facts: RowFacts,
    /// What the session's last complete read says the agent waits for, with
    /// the Herdr state it was read under.
    turn: Option<(u64, Option<Waiting>)>,
    user_turn: Option<(u64, hide_session::turns::UserTurnFact)>,
}

#[derive(Clone, Debug, PartialEq)]
struct Summary {
    goal: Option<String>,
    line: String,
    end: Option<LabelEnd>,
}

impl LabelOverlay {
    pub(crate) fn of_records<'a>(
        records: impl IntoIterator<Item = (&'a String, &'a PaneRecord)>,
        labels_shown: bool,
        summaries: bool,
    ) -> Self {
        let panes = records
            .into_iter()
            .map(|(pane_id, record)| {
                let label = labels_shown.then(|| ProvenLabel {
                    owner: record.owner.clone(),
                    native_session_id: record.native_session_id.clone(),
                    native_source_path: record.native_source_path.clone(),
                    proven_reference: record.proven_reference.clone(),
                    summary: summaries.then(|| Summary {
                        goal: record.goal.clone(),
                        line: record.line.clone(),
                        end: record.end,
                    }),
                    facts: row_facts(&record.facts),
                    turn: record.turn_read(),
                    user_turn: record.user_turn(),
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

    /// What the agent waits for in its current Herdr state, as its session
    /// read says (PRD codex-plan-approval-hold D-06, D-07): `None` when no
    /// read of the session it runs now was made for that state, or the
    /// records read do not settle it. The caller decides what not knowing
    /// means for an agent whose read reports no turns.
    pub(crate) fn waiting(&self, agent: &SessionAgentPayload) -> Option<Waiting> {
        let pane_id = agent.pane_id.as_deref().or(agent.id.as_deref())?;
        let label = self.panes.get(pane_id)?.label.as_ref()?;
        let reference = agent.agent_session.as_ref().and_then(|session| {
            label_reference_token(
                agent.agent.as_deref().unwrap_or_default(),
                &session.kind,
                &session.value,
            )
        });
        if !label.proves(reference.as_deref()) {
            return None;
        }
        let (seq, waiting) = label.turn?;
        (agent.state_change_seq == Some(seq))
            .then_some(waiting)
            .flatten()
    }

    pub(crate) fn apply(&self, payload: &mut SessionSnapshotPayload) {
        for agent in &mut payload.agents {
            let awaiting_operator = matches!(
                self.waiting(agent),
                Some(Waiting::Question | Waiting::PlanApproval)
            );
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
            let mut facts = label.facts.clone();
            facts.native_session_id = label
                .native_session_id
                .as_ref()
                .filter(|id| {
                    label_reference_token(agent.agent.as_deref().unwrap_or_default(), "id", id)
                        .as_deref()
                        == label.owner.as_deref()
                })
                .cloned();
            facts.native_reference = facts.native_session_id.as_ref().and_then(|_| {
                if agent.agent.as_deref() == Some("pi") {
                    label.native_source_path.as_ref().map(|path| {
                        crate::sidebar::SessionAgentSessionPayload {
                            kind: "path".into(),
                            value: path.clone(),
                        }
                    })
                } else {
                    agent.agent_session.clone()
                }
            });
            facts.awaiting_operator = awaiting_operator;
            facts.user_turn = label.user_turn.as_ref().and_then(|(seq, fact)| {
                (agent.state_change_seq == Some(*seq)).then(|| fact.clone())
            });
            if let Some(summary) = &label.summary {
                let working = agent.agent_status.as_deref() == Some("working");
                let asking = summary.end == Some(LabelEnd::Question);
                let line = (!summary.line.trim().is_empty()).then(|| summary.line.clone());
                // The sidebar's two lines keep their meaning: the line is the
                // reply asked for when the turn ended on a question, and the
                // progress otherwise.
                agent.label = Some(AgentLabel {
                    task: summary.goal.clone(),
                    progress: line.clone().filter(|_| !asking),
                    expected_reply: line.clone().filter(|_| asking),
                    question: asking && !working,
                });
                facts.end = summary.end;
                facts.line = line;
            }
            agent.facts = Some(facts);
        }
    }
}

/// The part of a session's facts a row is built from.
fn row_facts(facts: &SessionFacts) -> RowFacts {
    RowFacts {
        native_reference: None,
        native_session_id: None,
        native_title: facts.custom_title.clone().or_else(|| facts.title.clone()),
        operator_request: facts.operator_request.clone(),
        other_request: facts.other_request.clone(),
        reply: facts.reply.clone(),
        created_prs: facts
            .created_prs
            .iter()
            .map(|pr| (pr.repository.clone(), pr.number, pr.sighted_at_unix_ms))
            .collect(),
        end: None,
        line: None,
        awaiting_operator: false,
        user_turn: None,
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
