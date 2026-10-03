//! Keeps every agent row's request block current (`crate::request_view`).
//!
//! A session's rows are rebuilt fresh on every update, so the block is laid
//! on them in the same pipeline, before they are compared with the rows
//! already shown; an unchanged update stays unchanged. It is laid again
//! where its other inputs move: after GitHub's pull requests land, after
//! the read axis moves, and after the cross-device lineage pass. Each is a
//! pass over rows in hand, under the lock, with no I/O; the verb records are
//! saved through the coalesced UI-state save.

use super::*;
use crate::request_view::{self, RowPlace};

/// How often, while the request view is shown, a project whose linked pull
/// request has checks running is read again (D-32).
const PENDING_CHECKS_REREAD: std::time::Duration = std::time::Duration::from_secs(60);

impl Runtime {
    /// Lays the block on this Mac's rows, whose pull requests come from
    /// their checkout's branch and their session.
    pub(super) fn lay_local_requests(&mut self, agents: &mut [SidebarAgentSnapshot]) {
        let places: HashMap<&str, (Option<&str>, &str)> = self
            .snapshot
            .navigator
            .workspaces
            .iter()
            .filter(|workspace| workspace.remote_target_id.is_none())
            .flat_map(|workspace| {
                workspace.checkouts.iter().flat_map(move |checkout| {
                    checkout.tabs.iter().flat_map(move |tab| {
                        tab.panes.iter().map(move |pane| {
                            (
                                pane.id.as_str(),
                                (checkout.branch.as_deref(), workspace.path.as_str()),
                            )
                        })
                    })
                })
            })
            .collect();
        let verbs = &mut self.snapshot.ui_state.request_verbs;
        let mut changed = request_view::apply(
            agents,
            |pane| {
                places.get(pane).map(|(branch, root_path)| RowPlace {
                    branch: *branch,
                    root_path,
                })
            },
            &self.github,
            verbs,
            unix_milliseconds(),
        );
        changed |= request_view::prune_verbs(verbs, |pane| !pane.starts_with("remote:"), agents);
        if changed {
            self.persist_ui_state();
        }
    }

    /// Lays the block on a device's rows; their pull requests are not read
    /// (PRD Non-goals), so they carry none.
    pub(super) fn lay_remote_requests(
        &mut self,
        target_id: &str,
        agents: &mut [SidebarAgentSnapshot],
    ) {
        let verbs = &mut self.snapshot.ui_state.request_verbs;
        let mut changed = request_view::apply(
            agents,
            |_| None,
            &crate::model::GithubSnapshot::default(),
            verbs,
            unix_milliseconds(),
        );
        let prefix = remote_pane_id_prefix(target_id);
        changed |= request_view::prune_verbs(verbs, |pane| pane.starts_with(&prefix), agents);
        if changed {
            self.persist_ui_state();
        }
    }

    /// Lays the block again on every row already shown. Returns whether a
    /// row changed.
    pub(super) fn sync_request_rows(&mut self) -> bool {
        let mut agents = self.snapshot.navigator.agents.clone();
        self.lay_local_requests(&mut agents);
        let mut changed = agents != self.snapshot.navigator.agents;
        self.snapshot.navigator.agents = agents;
        let targets: Vec<String> = self
            .snapshot
            .status
            .remote
            .iter()
            .filter(|remote| remote.session.is_some())
            .map(|remote| remote.target_id.clone())
            .collect();
        for target_id in targets {
            let Some(mut agents) = self
                .snapshot
                .status
                .remote
                .iter()
                .find(|remote| remote.target_id == target_id)
                .and_then(|remote| remote.session.as_ref())
                .map(|session| session.agents.clone())
            else {
                continue;
            };
            self.lay_remote_requests(&target_id, &mut agents);
            if let Some(session) = self
                .snapshot
                .status
                .remote
                .iter_mut()
                .find(|remote| remote.target_id == target_id)
                .and_then(|remote| remote.session.as_mut())
                && session.agents != agents
            {
                session.agents = agents;
                changed = true;
            }
        }
        changed
    }

    /// A window began or stopped showing the request view.
    pub(super) fn observe_request_view(&mut self, observing: bool, now: std::time::Instant) {
        if observing && !self.request_view_observed {
            // Opening the view reads its projects already; the first re-read
            // is a minute later.
            self.pending_checks_read_at = Some(now);
        }
        self.request_view_observed = observing;
    }

    /// Once a minute while the request view is shown, asks GitHub again for
    /// each local project a row's pull request has checks running in, so a
    /// finished run moves its row without a new turn (D-32, B12). Nothing is
    /// asked while no window shows the view or no checks are running.
    pub(crate) fn reread_pending_checks(&mut self, now: std::time::Instant) {
        if !self.request_view_observed
            || self
                .pending_checks_read_at
                .is_some_and(|at| now.duration_since(at) < PENDING_CHECKS_REREAD)
        {
            return;
        }
        self.pending_checks_read_at = Some(now);
        let running: BTreeSet<String> = self
            .snapshot
            .navigator
            .agents
            .iter()
            .filter_map(|row| row.request.as_ref())
            .flat_map(|request| request.pull_requests.iter())
            .filter(|pull_request| {
                pull_request.live
                    && !pull_request.badge.is_settled()
                    && pull_request.checks == crate::model::PullRequestChecks::Pending
            })
            .filter_map(|pull_request| {
                self.github
                    .projects
                    .iter()
                    .find(|project| {
                        project
                            .pull_requests
                            .iter()
                            .any(|known| known.url == pull_request.url)
                    })
                    .map(|project| project.root_path.clone())
            })
            .collect();
        for project in running {
            self.refresh_pull_requests(&project);
        }
    }

    /// The operator opened a finished row in the request view: the pane is
    /// read, as a focus would make it, without moving the focus (D-29). A
    /// row asking a question keeps its demand.
    pub(super) fn open_result(&mut self, pane_id: &str) -> bool {
        let mut records = std::mem::take(&mut self.snapshot.ui_state.pane_read_records);
        let mut changes = Vec::new();
        if self
            .snapshot
            .navigator
            .agents
            .iter()
            .any(|row| row.pane_id == pane_id)
        {
            let mut agents = self.snapshot.navigator.agents.clone();
            changes = crate::sidebar::apply_read_state(&mut agents, &mut records, Some(pane_id));
        } else {
            for remote in &self.snapshot.status.remote {
                if let Some(session) = remote.session.as_ref()
                    && session.agents.iter().any(|row| row.pane_id == pane_id)
                {
                    let mut agents = session.agents.clone();
                    changes =
                        crate::sidebar::apply_read_state(&mut agents, &mut records, Some(pane_id));
                }
            }
        }
        self.snapshot.ui_state.pane_read_records = records;
        if changes.is_empty() {
            return false;
        }
        self.record_read_record_changes(&changes);
        // The rows take the new record with the focus each server reports.
        let mut changed = self.refresh_pane_read_state();
        let mut records = std::mem::take(&mut self.snapshot.ui_state.pane_read_records);
        for remote in &mut self.snapshot.status.remote {
            let Some(session) = remote.session.as_mut() else {
                continue;
            };
            let before = session.agents.clone();
            crate::sidebar::apply_read_state(
                &mut session.agents,
                &mut records,
                session.focused_pane_id.as_deref(),
            );
            changed |= before != session.agents;
        }
        self.snapshot.ui_state.pane_read_records = records;
        changed | self.sync_request_rows()
    }
}
