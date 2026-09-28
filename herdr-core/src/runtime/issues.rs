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
        let sources = self.snapshot.ui_state.project_issue_sources.clone();
        let mut local_links = BTreeMap::new();
        for workspace in &mut self.snapshot.navigator.workspaces {
            if workspace.remote_target_id.is_some() || !workspace.is_git {
                continue;
            }
            let status = workspace
                .checkouts
                .first()
                .map(|checkout| checkout.github.clone())
                .unwrap_or_default();
            let local_source = crate::tasks::source_kind(
                sources.get(&workspace.path).map(String::as_str),
                workspace.is_git,
                &workspace.home_issues,
                &status,
            ) == crate::tasks::SourceKind::Local;
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
                // A Local project's link is the same chain read as a local id
                // (`L-3`, or the number a branch starts with); a pull
                // request's closing issues are GitHub's and do not apply.
                if local_source {
                    let number = manual
                        .or(pane)
                        .or(checkout.branch_issue.as_ref())
                        .and_then(|value| crate::local_issues::parse_id(value))
                        .or_else(|| {
                            checkout
                                .branch
                                .as_deref()
                                .and_then(crate::issues::branch_number)
                        });
                    if let Some(number) = number {
                        local_links.insert(checkout.id.clone(), number);
                    }
                    if checkout.issue.is_some() {
                        checkout.issue = None;
                        changed = true;
                    }
                    continue;
                }
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
        self.local_issue_links = local_links;
        for project in refresh {
            self.refresh_pull_requests(&project);
        }
        changed
    }
}

impl Runtime {
    /// Projects each local project's issues, from the source it reads
    /// (`tasks::source_kind`), into the source-neutral task list the web
    /// reads, and names each checkout's task by its key. It runs after
    /// `sync_issues`, so a checkout's task is the issue it links to.
    pub(super) fn sync_tasks(&mut self) -> bool {
        let mut changed = false;
        let sources = &self.snapshot.ui_state.project_issue_sources;
        let store = &self.local_issues;
        for workspace in &mut self.snapshot.navigator.workspaces {
            let status = workspace
                .checkouts
                .first()
                .map(|checkout| checkout.github.clone())
                .unwrap_or_default();
            let choice = sources.get(&workspace.path).map(String::as_str);
            let kind = crate::tasks::source_kind(
                choice,
                workspace.is_git,
                &workspace.home_issues,
                &status,
            );
            let tasks = if workspace.remote_target_id.is_some() {
                // A device's projects keep their issues on that device.
                crate::tasks::ProjectTasksSnapshot::default()
            } else if kind == crate::tasks::SourceKind::Local {
                let read = match store {
                    Err(reason) => crate::tasks::LocalRead::Failed(reason),
                    Ok(store) => crate::tasks::LocalRead::Ready(store.project(&workspace.path)),
                };
                crate::tasks::local_tasks(&workspace.path, read, choice.is_some())
            } else {
                crate::tasks::github_tasks(&workspace.home_issues, &status, choice.is_some())
            };
            if workspace.tasks != tasks {
                workspace.tasks = tasks;
                changed = true;
            }
            let local =
                workspace.remote_target_id.is_none() && kind == crate::tasks::SourceKind::Local;
            for checkout in &mut workspace.checkouts {
                let (key, closes) = if local {
                    let key = self
                        .local_issue_links
                        .get(&checkout.id)
                        .map(|number| crate::tasks::local_key(&workspace.path, *number))
                        .filter(|key| workspace.tasks.tasks.iter().any(|task| &task.key == key));
                    (key, Vec::new())
                } else {
                    let key = checkout
                        .issue
                        .as_ref()
                        .map(|link| crate::tasks::github_key(&link.issue.reference));
                    let closes: Vec<String> = checkout
                        .pull_request
                        .iter()
                        .flat_map(|pr| pr.closing_issues.iter().map(crate::tasks::github_key))
                        .filter(|closed| Some(closed) != key.as_ref())
                        .filter(|closed| {
                            workspace.tasks.tasks.iter().any(|task| &task.key == closed)
                        })
                        .collect();
                    (key, closes)
                };
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
        match self.link_checkout_issue(&payload.checkout_id, &payload.text) {
            Ok(()) => true,
            Err((kind, message)) => {
                self.push_diagnostic(kind, message);
                false
            }
        }
    }

    /// The link Hide keeps between a checkout and an issue: the branch's issue
    /// link and its Workspace token, written on a worker. `Err` names the
    /// diagnostic kind and why nothing was started; the write's own answer
    /// lands in `ingest_issue_operation_result`.
    pub(super) fn link_checkout_issue(
        &mut self,
        checkout_id: &str,
        text: &str,
    ) -> Result<(), (&'static str, String)> {
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
                        checkout.id == checkout_id
                            && checkout.is_worktree
                            && checkout.branch.is_some()
                    })
                    .map(|checkout| (workspace.clone(), checkout.clone()))
            });
        let Some((workspace, checkout)) = target else {
            return Err((
                "checkout_issue.target_missing",
                "Issue target checkout is unavailable".to_owned(),
            ));
        };
        let local_source = self.project_source_kind(&workspace) == crate::tasks::SourceKind::Local;
        let text = if text.trim().is_empty() {
            String::new()
        } else if local_source {
            let known = crate::local_issues::parse_id(text).filter(|number| {
                matches!(&self.local_issues, Ok(store) if store.issue(&workspace.path, *number).is_some())
            });
            match known {
                Some(number) => crate::local_issues::display_id(number),
                None => {
                    return Err((
                        "checkout_issue.invalid",
                        format!("No local issue {} in {}", text.trim(), workspace.label),
                    ));
                }
            }
        } else {
            IssueReference::parse(text, workspace.home_issues.repository.as_deref())
                .map(|reference| reference.token())
                .map_err(|error| ("checkout_issue.invalid", error))?
        };
        let id = self
            .begin_task_operation(
                "checkout_issue",
                Some(workspace.path.clone()),
                checkout.branch.clone(),
                None,
                None,
            )
            .map_err(|error| ("checkout_issue.busy", error))?;
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
            .and_then(|context| {
                if local_source {
                    crate::live::spawn_local_issue_write(context, request.clone(), previous)
                } else {
                    crate::live::spawn_issue_write(context, request.clone(), previous)
                }
            });
        if let Err(error) = result {
            self.ingest_issue_operation_result(
                &request,
                Err(crate::live::IssueWriteFailure::unchanged(error)),
            );
        }
        Ok(())
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
        if self.pr_link_checkout.as_deref() == Some(request.checkout_id.as_str()) {
            self.pr_link_checkout = None;
            self.settle_pr_link_write(result.as_ref().err().map(|error| error.detail.clone()));
        }
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

/// A Local issue as its panel reads it: the body and when it was made, from
/// the store the runtime already holds, so the read needs no worker.
fn local_issue_detail(
    store: &Result<crate::local_issues::LocalIssueStore, String>,
    key: &str,
) -> Result<crate::tasks::TaskDetail, String> {
    let (path, number) =
        crate::tasks::parse_local_key(key).ok_or_else(|| format!("{key} is not a local issue"))?;
    match store {
        Err(reason) => Err(reason.clone()),
        Ok(store) => store
            .issue(path, number)
            .map(|issue| crate::tasks::TaskDetail {
                body: issue.body.clone(),
                created_at_unix_ms: Some(issue.created_at_unix_ms),
                ..Default::default()
            })
            .ok_or_else(|| {
                format!(
                    "로컬 이슈 {}가 없습니다.",
                    crate::local_issues::display_id(number)
                )
            }),
    }
}

impl Runtime {
    /// The source a registered local project reads now (`tasks::source_kind`).
    pub(super) fn project_source_kind(
        &self,
        workspace: &WorkspaceSnapshot,
    ) -> crate::tasks::SourceKind {
        let status = workspace
            .checkouts
            .first()
            .map(|checkout| checkout.github.clone())
            .unwrap_or_default();
        crate::tasks::source_kind(
            self.snapshot
                .ui_state
                .project_issue_sources
                .get(&workspace.path)
                .map(String::as_str),
            workspace.is_git,
            &workspace.home_issues,
            &status,
        )
    }

    pub(super) fn local_project_by_id(&self, workspace_id: &str) -> Option<WorkspaceSnapshot> {
        self.snapshot
            .navigator
            .workspaces
            .iter()
            .find(|workspace| workspace.id == workspace_id && workspace.remote_target_id.is_none())
            .cloned()
    }

    fn next_issue_work_id(&mut self) -> u64 {
        self.next_issue_work_id = self.next_issue_work_id.wrapping_add(1).max(1);
        self.next_issue_work_id
    }

    /// Runs `work` on its own thread and hands its answer to `ingest` under
    /// the lock; `Err` when there is no worker to run it on (a core built
    /// without one, as the tests build it).
    pub(super) fn spawn_issue_worker<T: Send + 'static>(
        &self,
        name: &str,
        work: impl FnOnce() -> T + Send + 'static,
        ingest: impl FnOnce(&mut Runtime, T) -> bool + Send + 'static,
    ) -> Result<(), String> {
        let context = self
            .worker_context
            .clone()
            .ok_or_else(|| "No worker is available for this request".to_owned())?;
        thread::Builder::new()
            .name(format!("herdr-core-{name}"))
            .spawn(move || {
                let answer = work();
                let Some(runtime) = context.runtime.upgrade() else {
                    return;
                };
                let changed = match runtime.lock() {
                    Ok(mut guard) => ingest(&mut guard, answer),
                    Err(_) => return,
                };
                drop(runtime);
                if changed {
                    context.notifier.notify();
                }
            })
            .map(|_| ())
            .map_err(|error| format!("{name} worker could not start: {error}"))
    }

    /// Settings › Issues: a project's source, or `auto` for its default.
    pub(super) fn set_issue_source(&mut self, payload: IssueSourceSetPayload) -> bool {
        let sources = &mut self.snapshot.ui_state.project_issue_sources;
        let changed = match payload.source.as_str() {
            "auto" => sources.remove(&payload.project_path).is_some(),
            crate::tasks::GITHUB | crate::tasks::LOCAL => {
                sources.insert(payload.project_path.clone(), payload.source.clone())
                    != Some(payload.source.clone())
            }
            other => {
                self.set_error(
                    "issue_source.unknown",
                    format!("No issue source named {other}"),
                    false,
                );
                return true;
            }
        };
        if !changed {
            return false;
        }
        crate::diagnostic!(serde_json::json!({
            "component": "issues", "kind": "issue_source.set",
            "project": payload.project_path, "source": payload.source,
        }));
        self.persist_ui_state();
        self.apply_pull_requests();
        true
    }

    /// Settings › Issues: how starting work from an issue behaves.
    pub(super) fn set_issue_settings(&mut self, payload: IssueSettingsSetPayload) -> bool {
        if let Some(agent) = payload.default_agent.as_deref()
            && !matches!(agent, "claude" | "codex" | "terminal")
        {
            self.set_error(
                "issue_settings.unknown_agent",
                format!("No agent named {agent}"),
                false,
            );
            return true;
        }
        let settings = &mut self.snapshot.ui_state.issue_settings;
        let before = settings.clone();
        if let Some(value) = payload.ai_worktree_name {
            settings.ai_worktree_name = value;
        }
        if let Some(value) = payload.default_agent {
            settings.default_agent = value;
        }
        if let Some(value) = payload.closes_instruction {
            settings.closes_instruction = value;
        }
        if *settings == before {
            return false;
        }
        self.persist_ui_state();
        true
    }

    fn settle_issue_create(&mut self, id: u64, result: Result<String, String>) -> bool {
        let Some(slot) = self
            .snapshot
            .issue_work
            .create
            .as_mut()
            .filter(|slot| slot.id == id)
        else {
            return false;
        };
        match result {
            Ok(key) => {
                slot.phase = "ready".into();
                slot.task_key = Some(key);
                slot.message = None;
            }
            Err(message) => {
                slot.phase = "failed".into();
                slot.message = Some(message.clone());
                self.push_diagnostic("issue_create.failed", message);
            }
        }
        true
    }

    /// `issue_create`: a new issue in the project's source. A Local one is
    /// in the store at once and written by the coordinator; a GitHub one is
    /// `gh issue create` on a worker.
    pub(super) fn create_issue(&mut self, payload: IssueCreatePayload) -> bool {
        let Some(workspace) = self.local_project_by_id(&payload.workspace_id) else {
            self.set_error(
                "issue_create.unknown_project",
                "A new issue needs a project on this Mac",
                false,
            );
            return true;
        };
        if self
            .snapshot
            .issue_work
            .create
            .as_ref()
            .is_some_and(|slot| slot.phase == "working")
        {
            self.set_error(
                "issue_create.busy",
                "Another issue is still being created",
                true,
            );
            return true;
        }
        let id = self.next_issue_work_id();
        self.snapshot.issue_work.create = Some(crate::model::IssueCreateSnapshot {
            id,
            workspace_id: workspace.id.clone(),
            phase: "working".into(),
            task_key: None,
            message: None,
        });
        let checked = crate::local_issues::validated_title(&payload.title).and_then(|title| {
            crate::local_issues::validated_body(&payload.body).map(|body| (title, body))
        });
        let (title, body) = match checked {
            Ok(checked) => checked,
            Err(message) => return self.settle_issue_create(id, Err(message)),
        };
        crate::diagnostic!(serde_json::json!({
            "component": "issues", "kind": "issue_create.requested",
            "id": id, "project": workspace.path,
        }));
        match self.project_source_kind(&workspace) {
            crate::tasks::SourceKind::Local => {
                let created = match self.local_issues.as_mut() {
                    Err(reason) => Err(format!(
                        "로컬 이슈 파일을 읽지 못해 새 이슈를 만들 수 없습니다: {reason}"
                    )),
                    Ok(store) => store
                        .create(&workspace.path, &title, &body, unix_milliseconds())
                        .map(|number| crate::tasks::local_key(&workspace.path, number)),
                };
                if created.is_ok() {
                    self.persist_local_issues();
                }
                self.settle_issue_create(id, created);
                self.apply_pull_requests();
                true
            }
            crate::tasks::SourceKind::Github => {
                let root = PathBuf::from(&workspace.path);
                let project_path = workspace.path.clone();
                let spawned = self.spawn_issue_worker(
                    "issue-create",
                    move || crate::github::create_issue(&root, &title, &body),
                    move |runtime, result| runtime.ingest_created_issue(id, &project_path, result),
                );
                if let Err(message) = spawned {
                    return self.settle_issue_create(id, Err(message));
                }
                true
            }
        }
    }

    pub(crate) fn ingest_created_issue(
        &mut self,
        id: u64,
        project_path: &str,
        result: Result<crate::issues::IssueSnapshot, String>,
    ) -> bool {
        let issue = match result {
            Ok(issue) => issue,
            Err(message) => return self.settle_issue_create(id, Err(message)),
        };
        let key = crate::tasks::github_key(&issue.reference);
        self.remember_created_issue(project_path, issue);
        self.settle_issue_create(id, Ok(key))
    }

    /// A GitHub issue Hide just made shows at once; the next read confirms it.
    pub(super) fn remember_created_issue(
        &mut self,
        project_path: &str,
        issue: crate::issues::IssueSnapshot,
    ) {
        if let Some(project) = self
            .github
            .projects
            .iter_mut()
            .find(|project| project.root_path == project_path)
        {
            project
                .issues
                .issues
                .retain(|known| known.reference != issue.reference);
            project.issues.issues.insert(0, issue);
            project.issues.issues.truncate(ISSUE_LIMIT);
        }
        self.refresh_pull_requests(project_path);
        self.apply_pull_requests();
    }

    /// `issue_detail_request`: an issue's body, labels, author, assignees and
    /// latest comments, for its panel and the Start dialog (PRD
    /// overview-lenses-issues D-40). A Local issue is read from the store at
    /// once; a GitHub one is one `gh issue view` on a worker, off the lock.
    pub(super) fn request_issue_detail(&mut self, payload: IssueDetailRequestPayload) -> bool {
        let Some(workspace) = self.local_project_by_id(&payload.workspace_id) else {
            return false;
        };
        let key = payload.task_key;
        if key.starts_with("local:") {
            let answer = local_issue_detail(&self.local_issues, &key);
            self.snapshot.issue_work.detail =
                Some(crate::model::IssueDetailSnapshot::answered(key, answer));
            return true;
        }
        let Some(reference) = crate::tasks::parse_github_key(&key) else {
            let answer = Err(format!("{key} is not an issue"));
            self.snapshot.issue_work.detail =
                Some(crate::model::IssueDetailSnapshot::answered(key, answer));
            return true;
        };
        crate::diagnostic!(serde_json::json!({
            "component": "issues", "kind": "issue_detail.requested", "task": key,
        }));
        self.snapshot.issue_work.detail =
            Some(crate::model::IssueDetailSnapshot::reading(key.clone()));
        let root = PathBuf::from(&workspace.path);
        let worker_key = key.clone();
        if let Err(message) = self.spawn_issue_worker(
            "issue-detail",
            move || crate::github::issue_detail(&root, &reference),
            move |runtime, result| runtime.ingest_issue_detail(&worker_key, result),
        ) {
            self.snapshot.issue_work.detail = Some(crate::model::IssueDetailSnapshot::answered(
                key,
                Err(message),
            ));
        }
        true
    }

    pub(crate) fn ingest_issue_detail(
        &mut self,
        key: &str,
        result: Result<crate::tasks::TaskDetail, String>,
    ) -> bool {
        let Some(slot) = self
            .snapshot
            .issue_work
            .detail
            .as_mut()
            .filter(|slot| slot.task_key == key && slot.phase == "reading")
        else {
            return false;
        };
        if let Err(reason) = &result {
            crate::diagnostic!(serde_json::json!({
                "component": "issues", "kind": "issue_detail.failed",
                "task": key, "reason": reason,
            }));
        }
        *slot = crate::model::IssueDetailSnapshot::answered(key.to_owned(), result);
        true
    }

    /// `worktree_name_suggest`: the background AI names the worktree. Off
    /// when Settings › Issues says so; the dialog's own name stands then.
    pub(super) fn suggest_worktree_name(&mut self, payload: WorktreeNameSuggestPayload) -> bool {
        if !self.snapshot.ui_state.issue_settings.ai_worktree_name {
            return false;
        }
        let settings = self.ai_settings.clone().unwrap_or_default();
        let request_id = payload.request_id.clone();
        self.snapshot.issue_work.name = Some(crate::model::WorktreeNameSnapshot {
            request_id: request_id.clone(),
            phase: "working".into(),
            name: None,
            message: None,
        });
        let worker_request = request_id.clone();
        if let Err(message) = self.spawn_issue_worker(
            "worktree-name",
            move || {
                crate::ai::suggest_worktree_name(
                    &settings,
                    &worker_request,
                    &payload.prefix,
                    &payload.title,
                    &payload.body,
                )
            },
            move |runtime, result| runtime.ingest_worktree_name(&request_id, result),
        ) {
            self.snapshot.issue_work.name = Some(crate::model::WorktreeNameSnapshot {
                request_id: payload.request_id,
                phase: "failed".into(),
                name: None,
                message: Some(message),
            });
        }
        true
    }

    pub(crate) fn ingest_worktree_name(
        &mut self,
        request_id: &str,
        result: Result<String, String>,
    ) -> bool {
        let Some(slot) = self
            .snapshot
            .issue_work
            .name
            .as_mut()
            .filter(|slot| slot.request_id == request_id)
        else {
            return false;
        };
        crate::diagnostic!(serde_json::json!({
            "component": "issues", "kind": "worktree_name.answered",
            "ok": result.is_ok(),
        }));
        match result {
            Ok(name) => {
                slot.phase = "ready".into();
                slot.name = Some(name);
            }
            Err(message) => {
                slot.phase = "failed".into();
                slot.message = Some(message);
            }
        }
        true
    }

    /// `issue_set_open`: closes or reopens a Local issue. A GitHub issue is
    /// closed by the pull request that fixes it, on GitHub.
    pub(super) fn set_issue_open(&mut self, payload: IssueSetOpenPayload) -> bool {
        let Some((path, number)) = crate::tasks::parse_local_key(&payload.task_key) else {
            self.set_error(
                "issue_set_open.not_local",
                "Only a local issue is closed from Hide",
                false,
            );
            return true;
        };
        let Ok(store) = self.local_issues.as_mut() else {
            self.set_error(
                "issue_set_open.unavailable",
                "Local issues are not readable right now",
                true,
            );
            return true;
        };
        if !store.set_open(path, number, payload.open, unix_milliseconds()) {
            return false;
        }
        self.persist_local_issues();
        self.apply_pull_requests();
        true
    }

    /// `local_issue_update`: a Local issue's title and body, edited in its
    /// panel (PRD overview-lenses-issues D-41). The store changes under the
    /// lock and is written by the save thread; the answer, or why it was
    /// refused, is in `issue_work.update` for the web's request id. There is
    /// no GitHub counterpart.
    pub(super) fn update_local_issue(&mut self, payload: LocalIssueUpdatePayload) -> bool {
        let now = unix_milliseconds();
        let updated = match (
            crate::tasks::parse_local_key(&payload.task_key),
            self.local_issues.as_mut(),
        ) {
            (None, _) => Err("Only a local issue is edited in Hide".to_owned()),
            (Some(_), Err(reason)) => Err(format!(
                "로컬 이슈 파일을 읽지 못해 고칠 수 없습니다: {reason}"
            )),
            (Some((path, number)), Ok(store)) => {
                store.update(path, number, &payload.title, &payload.body, now)
            }
        };
        crate::diagnostic!(serde_json::json!({
            "component": "issues", "kind": "local_issue.update",
            "request": payload.request_id, "task": payload.task_key, "ok": updated.is_ok(),
        }));
        if updated == Ok(true) {
            self.persist_local_issues();
            self.apply_pull_requests();
        }
        self.snapshot.issue_work.update = Some(crate::model::IssueUpdateSnapshot {
            request_id: payload.request_id,
            task_key: payload.task_key,
            phase: if updated.is_ok() { "ready" } else { "failed" }.into(),
            message: updated.err(),
        });
        true
    }

    /// Writes the Local store off the lock: one save thread at a time, and
    /// a change made while it writes is written after it (`write_ui_state`).
    pub(super) fn persist_local_issues(&mut self) {
        let Some(path) = self.local_issues_path.clone() else {
            return;
        };
        let Some(context) = self.worker_context.clone() else {
            // A runtime without a worker has no shared mutex to stay off.
            if let Ok(store) = &self.local_issues
                && let Err(reason) = crate::local_issues::save(&path, store)
            {
                self.push_diagnostic("local_issues.save_failed", reason);
            }
            return;
        };
        self.local_issues_save_pending = true;
        if self.local_issues_save_active {
            return;
        }
        self.local_issues_save_active = true;
        let spawned = thread::Builder::new()
            .name("hide-local-issues-save".into())
            .spawn(move || {
                let Some(runtime) = context.runtime.upgrade() else {
                    return;
                };
                loop {
                    let store = {
                        let mut guard = runtime.lock().unwrap_or_else(|error| error.into_inner());
                        let store = match (&guard.local_issues, guard.local_issues_save_pending) {
                            (Ok(store), true) => store.clone(),
                            _ => {
                                guard.local_issues_save_active = false;
                                return;
                            }
                        };
                        guard.local_issues_save_pending = false;
                        store
                    };
                    if let Err(reason) = crate::local_issues::save(&path, &store) {
                        runtime
                            .lock()
                            .unwrap_or_else(|error| error.into_inner())
                            .push_diagnostic(
                                "local_issues.save_failed",
                                format!("Local issues were not saved; they stay for this session: {reason}"),
                            );
                        context.notifier.notify();
                    }
                }
            });
        if let Err(error) = spawned {
            self.local_issues_save_active = false;
            self.push_diagnostic(
                "local_issues.save_failed",
                format!("The local issue save worker could not start: {error}"),
            );
        }
    }
}
