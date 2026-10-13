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
            .get(&id)
            .is_some_and(|status| {
                status.request_id == payload.request_id && status.state != WriteState::Failed
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
            self.snapshot
                .ui_state
                .project_settings
                .get(&id)
                .cloned()
                .unwrap_or_else(|| ProjectSettings::new(&payload.device_id, &payload.root))
                .patched(payload.patch)
        })();
        match result {
            Err(code) => {
                // A busy refusal must not overwrite the active write's receipt.
                if code == "project_settings_busy" {
                    self.push_diagnostic(
                        code,
                        "Finish the pending Project settings write before another change",
                    );
                    return false;
                }
                self.push_diagnostic(code, "Project settings change refused");
                self.settings_write_status(
                    &id,
                    &payload.request_id,
                    WriteState::Failed,
                    Some(code),
                );
                true
            }
            Ok(record) => {
                if self.snapshot.ui_state.project_settings.get(&id) == Some(&record) {
                    self.settings_write_status(&id, &payload.request_id, WriteState::Saved, None);
                    return true;
                }
                self.project_settings_pending.insert(
                    id.clone(),
                    Pending {
                        record,
                        request_id: payload.request_id.clone(),
                    },
                );
                self.settings_write_status(&id, &payload.request_id, WriteState::Pending, None);
                if let Err(detail) = self.write_ui_state() {
                    self.push_diagnostic("project_settings.save_failed", detail);
                    self.project_settings_pending.remove(&id);
                    self.settings_write_status(
                        &id,
                        &payload.request_id,
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
        request: &str,
        state: WriteState,
        error: Option<&str>,
    ) {
        let statuses = &mut self.snapshot.ui_state.project_settings_writes;
        if !statuses.contains_key(id) && statuses.len() >= settings::LIMIT {
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
                request_id: request.to_owned(),
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
                self.settings_write_status(&id, &pending.request_id, WriteState::Saved, None);
                self.project_settings_changed(before.as_ref(), &pending.record);
            } else {
                self.settings_write_status(
                    &id,
                    &pending.request_id,
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
            self.refresh_issue_rows(Some(&after.device_id));
        }
    }
}
