//! The first-run wizard's progress (PRD first-run-onboarding D-07, B6): the
//! step it opens on, when its first step's apply has settled, the desktop
//! notification switch it and Settings > General share (D-21), and the
//! one-time introductions of the Factory and Devices screens (B43). The shell
//! draws the steps; each move is one event here, saved with the UI state.

use super::Runtime;
use crate::model::{AgentOnboarding, FIRST_RUN_VERSION, FirstRun, FirstRunStep, ScreenIntro};

/// The first step's Continue, waiting for the kit pass it queued on this Mac.
pub(super) struct FirstRunApply {
    /// The agents the step showed on.
    agents: Vec<String>,
    /// A browser tab's wizard ends with this step (B26).
    finish: bool,
}

impl Runtime {
    /// The step the wizard opens on, or `None` when it does not open: a Mac
    /// that was never asked, or progress saved by another flow version.
    fn first_run_step(&self) -> Option<FirstRunStep> {
        self.snapshot
            .ui_state
            .first_run
            .filter(|run| run.version == FIRST_RUN_VERSION)
            .map(|run| run.step)
    }

    /// Starts the wizard on a Mac whose kit record waits for the first-run
    /// agent choice; any progress already saved stands. The caller saves.
    pub(super) fn open_first_run(&mut self) -> bool {
        if self.snapshot.ui_state.first_run.is_some() {
            return false;
        }
        self.snapshot.ui_state.first_run = Some(FirstRun {
            version: FIRST_RUN_VERSION,
            step: FirstRunStep::Agents,
        });
        crate::diagnostic!(serde_json::json!({
            "component": "first_run",
            "kind": "first_run.opened",
            "version": FIRST_RUN_VERSION,
        }));
        true
    }

    fn move_first_run(&mut self, from: FirstRunStep, to: FirstRunStep) -> bool {
        self.snapshot.ui_state.first_run = Some(FirstRun {
            version: FIRST_RUN_VERSION,
            step: to,
        });
        self.persist_ui_state();
        crate::diagnostic!(serde_json::json!({
            "component": "first_run",
            "kind": "first_run.step",
            "from": from.as_str(),
            "to": to.as_str(),
        }));
        true
    }

    /// The operator left the step `from` (Continue, Not now, Skip for now, a
    /// project added). Only the step on screen moves, so a second press or a
    /// window still showing an earlier step changes nothing (engineering rule
    /// 11). The first step ends through its apply, never through this event.
    pub(super) fn advance_first_run(&mut self, from: FirstRunStep) -> bool {
        let to = match from {
            FirstRunStep::Notifications => FirstRunStep::Project,
            FirstRunStep::Project => FirstRunStep::Done,
            FirstRunStep::Agents | FirstRunStep::Done => {
                crate::diagnostic!(serde_json::json!({
                    "component": "first_run",
                    "kind": "first_run.advance_refused",
                    "from": from.as_str(),
                }));
                return false;
            }
        };
        if self.first_run_step() != Some(from) {
            return false;
        }
        self.move_first_run(from, to)
    }

    /// Whether the first step's Continue still waits for its kit pass; a
    /// second press meanwhile is the same intent (B10).
    pub(super) fn first_run_apply_in_flight(&self) -> bool {
        self.first_run_apply.is_some()
    }

    /// The first step's Continue queued its choice: the step ends once this
    /// Mac's kit pass says every chosen agent is in place (B12). With nothing
    /// queued, what the kit last said decides now.
    pub(super) fn wait_for_first_run_apply(&mut self, agents: Vec<String>, finish: bool) -> bool {
        if self.first_run_step() != Some(FirstRunStep::Agents) {
            return false;
        }
        self.first_run_apply = Some(FirstRunApply { agents, finish });
        self.settle_first_run_apply()
    }

    /// Decides the first step after this Mac's kit report was stored. A
    /// report while work is still queued predates the pass Continue asked for
    /// and decides nothing. A record that did not save asks again (B6), and a
    /// chosen agent whose piece failed keeps the step on screen with its
    /// reason (B12); otherwise the wizard moves on.
    pub(super) fn settle_first_run_apply(&mut self) -> bool {
        if self.local_kit_pending.is_some() || self.first_run_apply.is_none() {
            return false;
        }
        let Some(apply) = self.first_run_apply.take() else {
            return false;
        };
        if self.first_run_step() != Some(FirstRunStep::Agents) {
            return false;
        }
        if self.snapshot.ui_state.agent_onboarding == Some(AgentOnboarding::Pending) {
            crate::diagnostic!(serde_json::json!({
                "component": "first_run",
                "kind": "first_run.apply_unsaved",
            }));
            return false;
        }
        let node = self.node.clone();
        let failed: Vec<String> = self
            .kit_view(node.as_str())
            .agents
            .iter()
            .filter(|row| apply.agents.contains(&row.id) && row.enabled)
            .filter(|row| {
                [Some(&row.skill), row.hook.as_ref(), row.herdr.as_ref()]
                    .into_iter()
                    .flatten()
                    .any(|piece| piece.state == hide_kit::ComponentState::Failed)
            })
            .map(|row| row.id.clone())
            .collect();
        if !failed.is_empty() {
            crate::diagnostic!(serde_json::json!({
                "component": "first_run",
                "kind": "first_run.apply_failed",
                "agents": failed,
            }));
            return false;
        }
        let to = if apply.finish {
            FirstRunStep::Done
        } else {
            FirstRunStep::Notifications
        };
        self.move_first_run(FirstRunStep::Agents, to)
    }

    /// The desktop notification switch (Settings > General, the wizard's
    /// second step). Whether the system lets Hide show one is the desktop
    /// host's to read; this is only the operator's choice.
    pub(super) fn set_desktop_notifications(&mut self, enabled: bool) -> bool {
        if self.snapshot.ui_state.desktop_notifications == enabled {
            return false;
        }
        self.snapshot.ui_state.desktop_notifications = enabled;
        self.persist_ui_state();
        crate::diagnostic!(serde_json::json!({
            "component": "first_run",
            "kind": "desktop_notifications.set",
            "enabled": enabled,
        }));
        true
    }

    /// A screen's one-line introduction was shown; it is not shown again.
    pub(super) fn mark_screen_intro_seen(&mut self, screen: ScreenIntro) -> bool {
        if !self.snapshot.ui_state.screen_intros_seen.insert(screen) {
            return false;
        }
        self.persist_ui_state();
        true
    }
}
