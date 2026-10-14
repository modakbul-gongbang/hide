//! Settings writes use the existing coalesced state writer. The screen and
//! provider consumers retain the confirmed record until that exact save
//! acknowledges it. Closing a dialog changes neither the write nor its scope.

use super::*;
use crate::project_settings::{
    self as settings, BaseBranchChoice, NewWorkLocation, ProjectSettings, WriteState, WriteStatus,
};
use serde::Deserialize;

pub(super) struct Pending {
    pub(super) record: ProjectSettings,
    request_id: String,
    patch: settings::Patch,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SetPayload {
    device_id: String,
    root: String,
    request_id: String,
    patch: settings::Patch,
}

impl Runtime {
    /// Device and canonical Project root must both name the catalog entry.
    /// A disconnected remote entry remains readable, never writable through
    /// a local path with the same spelling.
    fn settings_workspace(
        &self,
        device: &str,
        root: &str,
    ) -> Result<&WorkspaceSnapshot, &'static str> {
        if device == self.node.as_str() {
            return self
                .snapshot
                .navigator
                .workspaces
                .iter()
                .find(|workspace| {
                    workspace.remote_target_id.is_none()
                        && workspace.device_id == device
                        && workspace.path == root
                        && !workspace.is_home
                })
                .ok_or("project_settings_project_missing");
        }
        self.linked_node(device)
            .map_err(|_| "project_settings_device_unavailable")?;
        let remote = self
            .snapshot
            .status
            .remote
            .iter()
            .find(|remote| remote.target_id == device)
            .ok_or("project_settings_device_unavailable")?;
        if remote.catalog.state != "ready" {
            return Err("project_settings_project_unavailable");
        }
        remote
            .session
            .as_ref()
            .and_then(|session| {
                session
                    .workspaces
                    .iter()
                    .find(|workspace| workspace.path == root && !workspace.is_home)
            })
            .ok_or("project_settings_project_missing")
    }

    pub(super) fn set_project_settings(&mut self, payload: SetPayload) -> bool {
        let id = settings::key(&payload.device_id, &payload.root);
        if payload.request_id.is_empty() || payload.request_id.len() > 128 {
            self.push_diagnostic(
                "project_settings.invalid_request",
                "Project settings request id is missing or too long",
            );
            return false;
        }
        // Replaying the same caller intent has no write or provider effect.
        if self
            .snapshot
            .ui_state
            .project_settings_writes
            .values()
            .any(|status| {
                status.device_id == payload.device_id
                    && status.root == payload.root
                    && status.request_id == payload.request_id
                    && status.state != WriteState::Failed
            })
        {
            return false;
        }
        let result = (|| {
            if self.project_settings_pending.contains_key(&id) {
                return Err("project_settings_busy");
            }
            if !self.snapshot.ui_state.project_settings.contains_key(&id)
                && self.snapshot.ui_state.project_settings.len()
                    + self.project_settings_pending.len()
                    >= settings::LIMIT
            {
                return Err("project_settings_limit");
            }
            let workspace = self.settings_workspace(&payload.device_id, &payload.root)?;
            if !workspace.is_git
                && (payload.patch.base_branch.is_some()
                    || payload.patch.new_work == Some(NewWorkLocation::Worktree))
            {
                return Err("project_settings_not_git");
            }
            if let Some(BaseBranchChoice::Branch { branch }) = &payload.patch.base_branch
                && !workspace.branches.contains(branch)
            {
                return Err("project_settings_branch_unavailable");
            }
            let mut record = self
                .snapshot
                .ui_state
                .project_settings
                .get(&id)
                .cloned()
                .unwrap_or_else(|| ProjectSettings::new(&payload.device_id, &payload.root))
                .patched(payload.patch.clone())?;
            if self
                .snapshot
                .ui_state
                .project_settings
                .get(&id)
                .and_then(|record| record.repository.as_ref())
                != record.repository.as_ref()
            {
                for checkout in &workspace.checkouts {
                    let reference = checkout
                        .issue
                        .as_ref()
                        .map(|link| link.issue.reference.clone())
                        .or_else(|| {
                            checkout
                                .task_key
                                .as_deref()
                                .and_then(crate::tasks::parse_github_key)
                        })
                        .or_else(|| {
                            self.issue_candidates
                                .get(&checkout.id)
                                .map(|candidate| candidate.reference.clone())
                        });
                    if let Some(reference) = reference {
                        let scope_id = settings::link_scope_key(
                            &checkout.path,
                            checkout.branch.as_deref(),
                            reference.number,
                        );
                        if !record.link_scopes.contains_key(&scope_id)
                            && record.link_scopes.len() >= crate::issues::ISSUE_LIMIT
                        {
                            return Err("project_settings_link_limit");
                        }
                        record
                            .link_scopes
                            .entry(scope_id)
                            .or_insert(settings::LinkScope {
                                branch: checkout.branch.clone(),
                                reference,
                            });
                    }
                }
            }
            Ok(record)
        })();
        match result {
            Err(code) => {
                // A busy refusal must not overwrite the active write's receipt.
                if code == "project_settings_busy" {
                    self.push_diagnostic(
                        code,
                        "Finish the pending Project settings write before another change",
                    );
                    let refusal = format!("{id}:{}", payload.request_id);
                    self.settings_write_status(&refusal, &payload, WriteState::Failed, Some(code));
                    return true;
                }
                self.push_diagnostic(code, "Project settings change refused");
                self.settings_write_status(&id, &payload, WriteState::Failed, Some(code));
                true
            }
            Ok(record) => {
                if self.snapshot.ui_state.project_settings.get(&id) == Some(&record) {
                    self.settings_write_status(&id, &payload, WriteState::Saved, None);
                    return true;
                }
                self.project_settings_pending.insert(
                    id.clone(),
                    Pending {
                        record,
                        request_id: payload.request_id.clone(),
                        patch: payload.patch.clone(),
                    },
                );
                self.settings_write_status(&id, &payload, WriteState::Pending, None);
                if let Err(detail) = self.write_ui_state() {
                    self.push_diagnostic("project_settings.save_failed", detail);
                    self.project_settings_pending.remove(&id);
                    self.settings_write_status(
                        &id,
                        &payload,
                        WriteState::Failed,
                        Some("project_settings_save_failed"),
                    );
                }
                true
            }
        }
    }

    fn settings_write_status(
        &mut self,
        id: &str,
        payload: &SetPayload,
        state: WriteState,
        error: Option<&str>,
    ) {
        // One notifier frame may contain several completed writes. Keep the
        // displaced caller's outcome so coalescing cannot hide its completion.
        if let Some(previous) = self
            .snapshot
            .ui_state
            .project_settings_writes
            .get(id)
            .cloned()
            && previous.request_id != payload.request_id
            && previous.state != WriteState::Pending
        {
            self.settings_write_status(
                &format!("{id}:{}", previous.request_id),
                &SetPayload {
                    device_id: previous.device_id,
                    root: previous.root,
                    request_id: previous.request_id,
                    patch: previous.patch,
                },
                previous.state,
                previous.error.as_deref(),
            );
        }
        // Retry moves that same intent back to the canonical live receipt.
        self.snapshot
            .ui_state
            .project_settings_writes
            .remove(&format!("{id}:{}", payload.request_id));
        let statuses = &mut self.snapshot.ui_state.project_settings_writes;
        if !statuses.contains_key(id) && statuses.len() >= settings::LIMIT * 2 {
            // Completed receipts are replaceable; an active caller retains its
            // receipt until its write settles. The pending map has the same cap.
            if let Some(retired) = statuses
                .iter()
                .find(|(_, status)| status.state != WriteState::Pending)
                .map(|(id, _)| id.clone())
            {
                statuses.remove(&retired);
            } else {
                self.push_diagnostic(
                    "project_settings.receipt_limit",
                    "Project settings receipt limit reached",
                );
                return;
            }
        }
        statuses.insert(
            id.to_owned(),
            WriteStatus {
                device_id: payload.device_id.clone(),
                root: payload.root.clone(),
                request_id: payload.request_id.clone(),
                patch: payload.patch.clone(),
                state,
                error: error.map(str::to_owned),
            },
        );
    }

    /// Only records present in the exact saved frame settle. A newer intent
    /// not included in that frame stays pending for the writer's next turn.
    pub(super) fn acknowledge_project_settings_save(
        &mut self,
        saved: &UiStateSnapshot,
        success: bool,
    ) -> bool {
        let settled: Vec<_> = self
            .project_settings_pending
            .iter()
            .filter(|(id, pending)| saved.project_settings.get(*id) == Some(&pending.record))
            .map(|(id, _)| id.clone())
            .collect();
        let changed = !settled.is_empty();
        for id in settled {
            let pending = self
                .project_settings_pending
                .remove(&id)
                .expect("settled pending Project settings");
            if success {
                let before = self
                    .snapshot
                    .ui_state
                    .project_settings
                    .insert(id.clone(), pending.record.clone());
                self.settings_write_status(
                    &id,
                    &SetPayload {
                        device_id: pending.record.device_id.clone(),
                        root: pending.record.root.clone(),
                        request_id: pending.request_id.clone(),
                        patch: pending.patch.clone(),
                    },
                    WriteState::Saved,
                    None,
                );
                self.project_settings_changed(before.as_ref(), &pending.record);
            } else {
                self.settings_write_status(
                    &id,
                    &SetPayload {
                        device_id: pending.record.device_id.clone(),
                        root: pending.record.root.clone(),
                        request_id: pending.request_id.clone(),
                        patch: pending.patch.clone(),
                    },
                    WriteState::Failed,
                    Some("project_settings_save_failed"),
                );
            }
        }
        changed
    }

    fn project_settings_changed(
        &mut self,
        before: Option<&ProjectSettings>,
        after: &ProjectSettings,
    ) {
        let repository_changed =
            before.and_then(|record| record.repository.as_ref()) != after.repository.as_ref();
        let base_changed =
            before.and_then(|record| record.base_branch.as_ref()) != after.base_branch.as_ref();
        if repository_changed {
            let live = self
                .link_projects()
                .into_iter()
                .map(|project| project.key)
                .collect::<std::collections::BTreeSet<_>>();
            self.links_work
                .connection_epochs
                .retain(|id, _| live.contains(id));
            let id = hide_project::project_id(&after.device_id, Path::new(&after.root));
            let epoch = self
                .links_work
                .connection_epochs
                .get(&id)
                .copied()
                .unwrap_or(0)
                .saturating_add(1)
                .max(unix_milliseconds());
            self.links_work.connection_epochs.insert(id, epoch);
            self.invalidate_session_issue_repository(&after.device_id, &after.root);
            // The one search slot spans repository targets. Keep its running
            // worker bounded, but refuse its old answer and queued old intent.
            self.github_search.pending = None;
            if let Some(active) = self.github_search.active.as_mut() {
                active.invalidated = true;
            }
            if let Some(slot) = self.snapshot.issue_work.search.take() {
                self.snapshot.issue_work.search = Some(crate::model::GithubSearchSnapshot::failed(
                    slot.request_id,
                    slot.query,
                    "project_settings_repository_changed",
                ));
            }
            if let Some(slot) = self
                .snapshot
                .pr_work
                .feedback
                .as_mut()
                .filter(|slot| slot.device_id == after.device_id && slot.root == after.root)
            {
                slot.phase = "failed".into();
                slot.body = None;
                slot.failed_checks.clear();
                slot.change_requests.clear();
                slot.message = Some("project_settings_repository_changed".into());
            }
        }
        if after.device_id == self.node.as_str() {
            if repository_changed {
                self.github
                    .projects
                    .retain(|project| project.root_path != after.root);
                self.github_settled.remove(&after.root);
                self.refresh_pull_requests(&after.root);
            }
            self.apply_pull_requests();
            if base_changed {
                self.refresh_project_worktrees(&after.root);
            }
        } else {
            if repository_changed {
                if let Some(github) = self.device_github.get_mut(&after.device_id) {
                    github
                        .projects
                        .retain(|project| project.root_path != after.root);
                }
                self.device_github_read_generation
                    .remove(&(after.device_id.clone(), after.root.clone()));
                self.refresh_device_pull_requests(&after.device_id, &after.root);
            }
            if base_changed {
                if let Some(entry) = self.device_worktrees.get_mut(&after.device_id) {
                    entry.discard_answer |= entry.in_flight.is_some();
                }
                self.request_device_worktrees(&after.device_id, true);
            }
            self.refresh_issue_rows(Some(&after.device_id));
        }
        if repository_changed {
            self.feed_links();
        }
    }
}
