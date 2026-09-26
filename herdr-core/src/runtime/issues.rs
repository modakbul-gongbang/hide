use super::*;
use crate::issues::{ISSUE_LIMIT, IssueCandidate, IssueLinkSnapshot, IssueReference};

pub(super) struct UnconfirmedIssueToken {
    pub value: String,
    pub observed: bool,
}

impl Runtime {
    /// Pure projection over the accepted catalog and metadata. No reader is
    /// scheduled unless the chosen identity changes or a user refreshes it.
    pub(super) fn sync_issues(&mut self) -> bool {
        let mut candidates = BTreeMap::new();
        let mut refresh = BTreeSet::new();
        let mut changed = false;
        let mut family_tokens: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
        for (pane, token) in &self.issue_tokens.panes {
            family_tokens
                .entry(pane.clone())
                .or_default()
                .insert(pane.clone(), token.clone());
        }
        for agent in &self.snapshot.navigator.agents {
            if let Some(token) = self.issue_tokens.panes.get(&agent.pane_id) {
                for ancestor in &agent.lineage_path_pane_ids {
                    family_tokens
                        .entry(ancestor.clone())
                        .or_default()
                        .insert(agent.pane_id.clone(), token.clone());
                }
            }
        }

        let sessions: BTreeMap<_, _> = self
            .snapshot
            .navigator
            .workspaces
            .iter()
            .flat_map(|workspace| {
                workspace.checkouts.iter().filter_map(|checkout| {
                    workspace::authoritative_session_space(
                        &self.last_session_spaces,
                        workspace,
                        &checkout.path,
                    )
                    .map(|space| (checkout.id.clone(), space.id.clone()))
                })
            })
            .collect();
        self.unconfirmed_issue_tokens.retain(|id, token| {
            if !self.last_session_spaces.iter().any(|space| &space.id == id) {
                return false;
            }
            if self.issue_tokens.workspaces.get(id) == Some(&token.value) {
                token.observed = true;
                true
            } else {
                // A delayed event for the failed write must be seen before a
                // later replacement/clear can confirm reconciliation.
                !token.observed
            }
        });
        for workspace in &mut self.snapshot.navigator.workspaces {
            if workspace.remote_target_id.is_some() || !workspace.is_git {
                continue;
            }
            let mut linked = BTreeSet::new();
            for checkout in &mut workspace.checkouts {
                if self
                    .issue_write_pending
                    .as_ref()
                    .is_some_and(|(_, id, _)| id == &checkout.id)
                {
                    if let Some(candidate) = self.issue_candidates.get(&checkout.id) {
                        candidates.insert(checkout.id.clone(), candidate.clone());
                    }
                    continue;
                }
                let physical: BTreeSet<_> = checkout
                    .tabs
                    .iter()
                    .flat_map(|tab| tab.panes.iter().map(|pane| pane.id.clone()))
                    .collect();
                let manual = sessions.get(&checkout.id).and_then(|id| {
                    self.issue_tokens.workspaces.get(id).filter(|value| {
                        !self
                            .unconfirmed_issue_tokens
                            .get(id)
                            .is_some_and(|token| token.value == **value)
                    })
                });
                let pane = physical
                    .iter()
                    .filter_map(|id| family_tokens.get(id))
                    .flat_map(|tokens| tokens.iter())
                    .min_by_key(|(id, _)| *id)
                    .map(|(_, token)| token);
                let repository = workspace.home_issues.repository.as_deref();
                let selected = manual
                    .map(|value| (value.clone(), "직접 연결".to_owned()))
                    .or_else(|| pane.map(|value| (value.clone(), "pane 토큰".into())))
                    .or_else(|| {
                        checkout
                            .branch_issue
                            .as_ref()
                            .map(|value| (value.clone(), "브랜치 설정".into()))
                    })
                    .or_else(|| {
                        checkout.pull_request.as_ref().and_then(|pr| {
                            pr.closing_issues
                                .first()
                                .map(|issue| (issue.token(), format!("PR #{} 본문", pr.number)))
                        })
                    })
                    .or_else(|| {
                        checkout
                            .branch
                            .as_deref()
                            .and_then(crate::issues::branch_number)
                            .map(|number| (format!("#{number}"), "브랜치 이름".into()))
                    });
                let candidate = selected.and_then(|(value, source)| {
                    match IssueReference::parse(&value, repository) {
                        Ok(reference) => Some(IssueCandidate { reference, source }),
                        Err(_) => {
                            // A missing repository will resolve after the first
                            // lookup; malformed external metadata is diagnostic.
                            if repository.is_some() || !value.starts_with('#') {
                                crate::diagnostic!(serde_json::json!({"component":"checkout_issue","kind":"reference.invalid","checkout_id":checkout.id}));
                            }
                            None
                        }
                    }
                });
                let issue = candidate.as_ref().and_then(|candidate| {
                    linked.insert(candidate.reference.clone());
                    if linked.len() > ISSUE_LIMIT {
                        return None;
                    }
                    workspace
                        .home_issues
                        .issues
                        .iter()
                        .find(|issue| issue.reference == candidate.reference)
                        .map(|issue| IssueLinkSnapshot {
                            issue: issue.clone(),
                            source: candidate.source.clone(),
                        })
                });
                if let Some(candidate) = candidate {
                    if self.issue_candidates.get(&checkout.id) != Some(&candidate) {
                        refresh.insert(workspace.path.clone());
                    }
                    candidates.insert(checkout.id.clone(), candidate);
                }
                if checkout.issue != issue {
                    checkout.issue = issue;
                    changed = true;
                }
            }
            if linked.len() > ISSUE_LIMIT && !workspace.home_issues.overflow {
                workspace.home_issues.overflow = true;
                changed = true;
            }
        }
        self.issue_candidates = candidates;
        for project in refresh {
            self.refresh_pull_requests(&project);
        }
        changed
    }
}

impl Runtime {
    /// Projects each local Git project's issues into the source-neutral task
    /// list the web reads, and names each checkout's task by its key. It runs
    /// after `sync_issues`, so a checkout's task is the issue it links to.
    pub(super) fn sync_tasks(&mut self) -> bool {
        let mut changed = false;
        for workspace in &mut self.snapshot.navigator.workspaces {
            let local_git = workspace.remote_target_id.is_none() && workspace.is_git;
            let status = workspace
                .checkouts
                .first()
                .map(|checkout| checkout.github.clone())
                .unwrap_or_default();
            let tasks = crate::tasks::github_tasks(local_git, &workspace.home_issues, &status);
            if workspace.tasks != tasks {
                workspace.tasks = tasks;
                changed = true;
            }
            for checkout in &mut workspace.checkouts {
                let key = checkout
                    .issue
                    .as_ref()
                    .map(|link| crate::tasks::github_key(&link.issue.reference));
                let closes: Vec<String> = checkout
                    .pull_request
                    .iter()
                    .flat_map(|pr| pr.closing_issues.iter().map(crate::tasks::github_key))
                    .filter(|closed| Some(closed) != key.as_ref())
                    .filter(|closed| workspace.tasks.tasks.iter().any(|task| &task.key == closed))
                    .collect();
                if checkout.task_key != key || checkout.closes_task_keys != closes {
                    checkout.task_key = key;
                    checkout.closes_task_keys = closes;
                    changed = true;
                }
            }
        }
        changed
    }

    pub(super) fn set_checkout_issue(&mut self, payload: SetCheckoutPurposePayload) -> bool {
        let target = self
            .snapshot
            .navigator
            .workspaces
            .iter()
            .filter(|workspace| workspace.remote_target_id.is_none() && workspace.is_git)
            .find_map(|workspace| {
                workspace
                    .checkouts
                    .iter()
                    .find(|checkout| {
                        checkout.id == payload.checkout_id
                            && checkout.is_worktree
                            && checkout.branch.is_some()
                    })
                    .map(|checkout| (workspace.clone(), checkout.clone()))
            });
        let Some((workspace, checkout)) = target else {
            self.push_diagnostic(
                "checkout_issue.target_missing",
                "Issue target checkout is unavailable",
            );
            return false;
        };
        let text = if payload.text.trim().is_empty() {
            String::new()
        } else {
            match IssueReference::parse(&payload.text, workspace.home_issues.repository.as_deref())
            {
                Ok(reference) => reference.token(),
                Err(error) => {
                    self.push_diagnostic("checkout_issue.invalid", error);
                    return false;
                }
            }
        };
        let id = match self.begin_task_operation(
            "checkout_issue",
            Some(workspace.path.clone()),
            checkout.branch.clone(),
            None,
            None,
        ) {
            Ok(id) => id,
            Err(error) => {
                self.push_diagnostic("checkout_issue.busy", error);
                return false;
            }
        };
        let session = workspace::authoritative_session_space(
            &self.last_session_spaces,
            &workspace,
            &checkout.path,
        )
        .map(|space| space.id.clone());
        let previous = session
            .as_ref()
            .and_then(|id| self.issue_tokens.workspaces.get(id))
            .cloned();
        let request = crate::live::PurposeTaskRequest {
            id,
            checkout_id: checkout.id.clone(),
            repository_root: workspace.path,
            branch: checkout.branch,
            session_workspace_id: session,
            purpose: text.clone(),
        };
        self.issue_write_pending = Some((id, checkout.id, text));
        let result = self
            .live
            .clone()
            .ok_or_else(|| "Live connection is unavailable".to_owned())
            .and_then(|context| crate::live::spawn_issue_write(context, request.clone(), previous));
        if let Err(error) = result {
            self.ingest_issue_operation_result(
                &request,
                Err(crate::live::IssueWriteFailure::unchanged(error)),
            );
        }
        true
    }

    pub(crate) fn ingest_issue_operation_result(
        &mut self,
        request: &crate::live::PurposeTaskRequest,
        result: Result<Option<crate::issues::IssueSnapshot>, crate::live::IssueWriteFailure>,
    ) -> bool {
        if !self
            .issue_write_pending
            .as_ref()
            .is_some_and(|(id, _, _)| *id == request.id)
        {
            return false;
        }
        self.issue_write_pending = None;
        match result {
            Ok(issue) => {
                if let Some(id) = &request.session_workspace_id {
                    self.unconfirmed_issue_tokens.remove(id);
                }
                if !self
                    .github
                    .projects
                    .iter()
                    .any(|project| project.root_path == request.repository_root)
                {
                    self.github
                        .projects
                        .push(crate::model::GithubProjectSnapshot {
                            root_path: request.repository_root.clone(),
                            issues: Default::default(),
                            status: Default::default(),
                            pull_requests: Vec::new(),
                            ..Default::default()
                        });
                }
                if let Some(id) = &request.session_workspace_id {
                    if request.purpose.is_empty() {
                        self.issue_tokens.workspaces.remove(id);
                    } else {
                        self.issue_tokens
                            .workspaces
                            .insert(id.clone(), request.purpose.clone());
                    }
                }
                for workspace in &mut self.snapshot.navigator.workspaces {
                    if let Some(checkout) = workspace
                        .checkouts
                        .iter_mut()
                        .find(|checkout| checkout.id == request.checkout_id)
                    {
                        checkout.branch_issue =
                            (!request.purpose.is_empty()).then(|| request.purpose.clone());
                        checkout.issue = issue.clone().map(|issue| IssueLinkSnapshot {
                            issue,
                            source: "직접 연결".into(),
                        });
                    }
                }
                if let Some(issue) = issue
                    && let Some(project) = self
                        .github
                        .projects
                        .iter_mut()
                        .find(|project| project.root_path == request.repository_root)
                {
                    project
                        .issues
                        .issues
                        .retain(|known| known.reference != issue.reference);
                    project.issues.issues.insert(0, issue);
                    project.issues.issues.truncate(ISSUE_LIMIT);
                }
                if let Some(operation) = self.snapshot.task_operation.as_mut() {
                    operation.phase = "ready".into();
                }
                self.refresh_pull_requests(&request.repository_root);
                self.refresh_worktrees();
                self.apply_pull_requests();
            }
            Err(error) => {
                if error.unconfirmed_token
                    && let Some(id) = &request.session_workspace_id
                {
                    self.unconfirmed_issue_tokens.insert(
                        id.clone(),
                        UnconfirmedIssueToken {
                            value: request.purpose.clone(),
                            observed: false,
                        },
                    );
                }
                self.push_diagnostic("checkout_issue.save_failed", error.detail);
                if let Some(operation) = self.snapshot.task_operation.as_mut() {
                    operation.phase = "failed".into();
                }
            }
        }
        true
    }
}
