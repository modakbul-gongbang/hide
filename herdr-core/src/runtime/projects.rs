use super::*;

const MINIMUM_PURPOSE_HERDR_VERSION: (u64, u64, u64) = (0, 9, 1);

fn herdr_version_supports_purpose(version: Option<&str>) -> bool {
    parse_herdr_version(version.unwrap_or_default())
        .is_some_and(|version| version >= MINIMUM_PURPOSE_HERDR_VERSION)
}

fn parse_herdr_version(version: &str) -> Option<(u64, u64, u64)> {
    let version = version.strip_prefix('v').unwrap_or(version);
    let numeric = version.split(['-', '+']).next()?;
    let mut parts = numeric.split('.');
    let parsed = (
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
    );
    parts.next().is_none().then_some(parsed)
}

fn remote_purpose_unavailable_reason(version: Option<&str>) -> String {
    match version {
        Some(version) => format!(
            "Set purpose requires Herdr 0.9.1 or newer on the remote device; {version} is installed"
        ),
        None => "Set purpose requires Herdr 0.9.1 or newer on the remote device; its version is unavailable"
            .to_owned(),
    }
}

impl Runtime {
    pub fn changes_request(&self) -> Option<crate::changes::ChangesRequest> {
        let changes_list_visible = self.snapshot.ui_state.right_panel_visible
            && self.snapshot.ui_state.right_panel_section == RightPanelSection::Changes;
        let explorer_visible = self.snapshot.ui_state.right_panel_visible
            && self.snapshot.ui_state.right_panel_section == RightPanelSection::Explorer;
        let active_diff = self
            .snapshot
            .editor
            .active_tab_id
            .as_deref()
            .and_then(|tab_id| {
                self.snapshot
                    .editor
                    .tabs
                    .iter()
                    .find(|tab| tab.id == tab_id && tab.kind == EditorTabKind::Diff)
            });
        if !changes_list_visible && !explorer_visible && active_diff.is_none() {
            return None;
        }
        let root_path = self.snapshot.navigator.root_path.as_ref()?;
        let selected_path = active_diff
            .map(|tab| tab.path.clone())
            .or_else(|| self.snapshot.changes.selected_path.clone());
        let selected_committed = active_diff
            .and_then(|tab| tab.diff_committed)
            .unwrap_or(self.snapshot.changes.selected_committed);
        Some(crate::changes::ChangesRequest {
            root_path: PathBuf::from(root_path),
            selected_path,
            selected_committed,
            // The base comes from the checkout row, so the committed group and
            // the card's `↑A ↓B` are measured against the same branch.
            base_branch: self
                .focused_local_checkout()
                .and_then(|(_, checkout)| checkout.base_branch.clone()),
        })
    }

    pub fn worktrees_request(&self) -> crate::worktrees::WorktreeRequest {
        let projects = self
            .snapshot
            .navigator
            .workspaces
            .iter()
            .filter(|workspace| workspace.remote_target_id.is_none() && workspace.is_git)
            .map(|workspace| crate::worktrees::WorktreeProjectRequest {
                root_path: PathBuf::from(&workspace.path),
                base_override: self
                    .snapshot
                    .ui_state
                    .project_base_branches
                    .get(&workspace.path)
                    .cloned(),
                bases: self
                    .github
                    .project(&workspace.path)
                    .map(|project| {
                        project
                            .pull_requests
                            .iter()
                            .map(|pull_request| {
                                (
                                    pull_request.head_branch.clone(),
                                    pull_request.base_branch.clone(),
                                )
                            })
                            .collect()
                    })
                    .unwrap_or_default(),
            })
            .collect();
        crate::worktrees::WorktreeRequest {
            projects,
            generation: self.worktree_generation,
        }
    }

    pub fn ingest_worktrees(&mut self, catalog: crate::model::WorktreeCatalogSnapshot) -> bool {
        let changed = self.worktree_catalog != catalog || self.snapshot.git_worktrees_loading;
        self.worktree_catalog = catalog;
        self.snapshot.git_worktrees_loading = false;
        self.refresh_worktree_projection();
        changed
    }

    pub(crate) fn cleanup_current_path(&self) -> Option<String> {
        self.focused_local_checkout()
            .map(|(_, checkout)| checkout.path.clone())
    }

    pub(crate) fn ingest_cleanup(&mut self, answer: live::cleanup::CleanupSnapshot) -> bool {
        if self
            .cleanup
            .as_ref()
            .is_none_or(|current| current.id != answer.id)
        {
            return false;
        }
        let removed = answer
            .rows
            .iter()
            .any(|row| row.result.as_deref() == Some("removed"));
        self.cleanup = Some(answer);
        if removed {
            self.refresh_worktrees();
            self.remeasure_disk();
        }
        self.refresh_worktree_projection();
        true
    }

    pub(super) fn review_cleanup(&mut self) -> bool {
        if self
            .cleanup
            .as_ref()
            .is_some_and(|r| matches!(r.phase.as_str(), "loading" | "removing"))
        {
            return false;
        }
        let Some((workspace, checkout)) = self.focused_local_checkout() else {
            return false;
        };
        let root = workspace.path.clone();
        let current = checkout.path.clone();
        self.next_cleanup_id = self.next_cleanup_id.wrapping_add(1).max(1);
        let mut review = live::cleanup::CleanupSnapshot {
            id: self.next_cleanup_id,
            repository_root: root,
            phase: "loading".into(),
            ..Default::default()
        };
        self.cleanup = Some(review.clone());
        let started = self.live.clone().ok_or_else(|| "A live Herdr connection is required to verify worktree usage. Connect and review again.".into())
            .and_then(|context| live::cleanup::spawn(context, review.clone(), None, current));
        if let Err(message) = started {
            review.phase = "failed".into();
            review.message = Some(message);
            self.cleanup = Some(review);
        }
        self.refresh_worktree_projection();
        true
    }

    pub(super) fn confirm_cleanup(&mut self, payload: CleanupConfirmPayload) -> bool {
        let Some(review) = self
            .cleanup
            .clone()
            .filter(|r| r.id == payload.id && r.phase == "review")
        else {
            return false;
        };
        let paths: Vec<_> = payload
            .paths
            .into_iter()
            .filter(|path| {
                review
                    .rows
                    .iter()
                    .any(|row| row.path == *path && row.exclusion.is_none())
            })
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        if paths.is_empty() {
            return false;
        }
        let Some(current) = self.cleanup_current_path() else {
            return false;
        };
        self.cleanup.as_mut().unwrap().phase = "removing".into();
        let started = self
            .live
            .clone()
            .ok_or_else(|| {
                "The Herdr connection is unavailable. Reconnect and review again.".into()
            })
            .and_then(|context| {
                live::cleanup::spawn(context, review.clone(), Some(paths), current)
            });
        if let Err(message) = started {
            let active = self.cleanup.as_mut().unwrap();
            active.phase = "failed".into();
            active.message = Some(message);
        }
        self.refresh_worktree_projection();
        true
    }

    pub(super) fn refresh_worktree_projection(&mut self) -> bool {
        let before_catalog = self.worktree_catalog.clone();
        let before_navigator = self.snapshot.navigator.clone();
        let before_git = self.snapshot.git_worktrees.clone();
        let before_remote = self.snapshot.git_worktrees_remote;

        let pane_rows = self
            .snapshot
            .navigator
            .workspaces
            .iter()
            .flat_map(|workspace| workspace.checkouts.iter())
            .map(|checkout| {
                (
                    checkout.path.clone(),
                    checkout
                        .tabs
                        .iter()
                        .flat_map(|tab| tab.panes.iter())
                        .map(|pane| pane.id.clone())
                        .collect::<HashSet<_>>(),
                )
            })
            .collect::<HashMap<_, _>>();
        let running = self
            .snapshot
            .navigator
            .agents
            .iter()
            .filter(|agent| agent.activity == "working")
            .map(|agent| agent.pane_id.as_str())
            .collect::<HashSet<_>>();
        // The agent line reads the rows the sidebar already projected and the
        // instrumentation the pane header already resolved, so Overview
        // repeats neither judgement (PRD B34, B35, engineering rule 7).
        let agent_chips = self
            .snapshot
            .navigator
            .agents
            .iter()
            .map(|agent| (agent.pane_id.clone(), crate::sidebar::agent_chip(agent)))
            .collect::<HashMap<_, _>>();
        let pane_instrumentation = self
            .snapshot
            .navigator
            .workspaces
            .iter()
            .flat_map(|workspace| workspace.checkouts.iter())
            .flat_map(|checkout| checkout.tabs.iter())
            .flat_map(|tab| tab.panes.iter())
            .filter_map(|pane| Some((pane.id.clone(), pane.children.clone()?)))
            .collect::<HashMap<_, _>>();
        let requested_github: HashSet<_> = self
            .github_request()
            .projects
            .iter()
            .map(|p| p.root.to_string_lossy().into_owned())
            .collect();
        let github = self.github.clone();
        let disk_usage = self.disk_usage.clone();

        for project in &mut self.worktree_catalog.projects {
            let github_project = github.project(&project.root_path);
            project.github = github_project.map(|g| g.status.clone()).unwrap_or_else(|| {
                crate::model::GithubStatusSnapshot {
                    loading: requested_github.contains(&project.root_path),
                    ..Default::default()
                }
            });
            project.pull_requests = github_project
                .map(|g| g.pull_requests.clone())
                .unwrap_or_default();
            project.pull_request_window = crate::github::PULL_REQUEST_LIMIT.into();
            for worktree in &mut project.worktrees {
                let panes = pane_rows.get(&worktree.path);
                worktree.pane_count = panes.map_or(0, HashSet::len);
                worktree.running_agent_count = panes.map_or(0, |pane_ids| {
                    pane_ids
                        .iter()
                        .filter(|pane_id| running.contains(pane_id.as_str()))
                        .count()
                });
                worktree.agent_line = worktree_agent_line(
                    panes,
                    &agent_chips,
                    &pane_instrumentation,
                    &self.snapshot.navigator.agents,
                );
                worktree.disk = disk_usage
                    .iter()
                    .find(|disk| disk.path.as_deref() == Some(worktree.path.as_str()))
                    .cloned()
                    .unwrap_or_default();
                worktree.github = github_project
                    .map(|project| project.status.clone())
                    .unwrap_or_default();
                worktree.pull_request = worktree.branch.as_deref().and_then(|branch| {
                    github_project?
                        .pull_requests
                        .iter()
                        .find(|pull_request| pull_request.head_branch == branch)
                        .cloned()
                });
                worktree.deletion_gate = crate::worktrees::deletion_gate(
                    worktree,
                    worktree.branch == project.base_branch && project.base_branch.is_some(),
                    false,
                    worktree.pane_count,
                    worktree.running_agent_count,
                );
            }
            project.shared_git_disk = disk_usage
                .iter()
                .find(|d| d.path.is_some() && d.path == project.shared_git_path)
                .cloned()
                .unwrap_or_default();
            let components: Vec<_> = project
                .worktrees
                .iter()
                .map(|w| &w.disk)
                .chain(std::iter::once(&project.shared_git_disk))
                .collect();
            project.disk_total_bytes = project
                .shared_git_path
                .as_ref()
                .and_then(|_| components.iter().map(|d| d.total_bytes).sum());
            let confirmed: Vec<_> = components.iter().filter_map(|d| d.total_bytes).collect();
            project.disk_confirmed_bytes = (!confirmed.is_empty()).then(|| confirmed.iter().sum());
            project.linked_disk_bytes = project
                .worktrees
                .iter()
                .filter(|w| !w.is_main)
                .map(|w| w.disk.total_bytes)
                .sum();
            project.disk_unavailable_reason = components
                .iter()
                .find_map(|d| d.unavailable_reason.clone())
                .or_else(|| {
                    project
                        .shared_git_path
                        .is_none()
                        .then(|| "Shared Git directory is unavailable. Refresh Overview.".into())
                });
            project.worktrees.sort_by(|left, right| {
                right
                    .is_main
                    .cmp(&left.is_main)
                    .then_with(|| (right.pane_count > 0).cmp(&(left.pane_count > 0)))
                    .then_with(|| {
                        right
                            .last_commit_unix_seconds
                            .unwrap_or(0)
                            .cmp(&left.last_commit_unix_seconds.unwrap_or(0))
                    })
                    .then_with(|| left.path.cmp(&right.path))
            });
        }

        for workspace in &mut self.snapshot.navigator.workspaces {
            if workspace.remote_target_id.is_some() {
                continue;
            }
            workspace::apply_worktrees(workspace, &self.worktree_catalog);
        }
        crate::project_context::sort_projects(
            &mut self.snapshot.navigator.workspaces,
            &self.snapshot.navigator.agents,
        );
        self.refresh_inactive_groups();

        let focused_project = self
            .snapshot
            .navigator
            .focused_checkout_id
            .as_deref()
            .and_then(|focused_id| {
                self.snapshot.navigator.workspaces.iter().find(|workspace| {
                    workspace
                        .checkouts
                        .iter()
                        .any(|checkout| checkout.id == focused_id)
                })
            });
        self.snapshot.git_worktrees_remote =
            focused_project.is_some_and(|workspace| workspace.remote_target_id.is_some());
        self.snapshot.git_worktrees = focused_project
            .filter(|workspace| workspace.remote_target_id.is_none())
            .and_then(|workspace| self.worktree_catalog.project(&workspace.path))
            .cloned();
        if let Some(project) = self.snapshot.git_worktrees.as_mut() {
            project.cleanup = self
                .cleanup
                .as_ref()
                .filter(|review| review.repository_root == project.root_path)
                .cloned();
        }
        self.refresh_card();
        before_catalog != self.worktree_catalog
            || before_navigator != self.snapshot.navigator
            || before_git != self.snapshot.git_worktrees
            || before_remote != self.snapshot.git_worktrees_remote
    }

    pub fn worktree_catalog(&self) -> crate::model::WorktreeCatalogSnapshot {
        self.worktree_catalog.clone()
    }

    pub fn github_request(&self) -> crate::github::GithubRequest {
        if !self.github_lookup_requested() && self.sidebar_github_projects.is_empty() {
            return crate::github::GithubRequest::default();
        }
        crate::github::GithubRequest {
            projects: self
                .snapshot
                .navigator
                .workspaces
                .iter()
                .filter(|workspace| workspace.remote_target_id.is_none() && workspace.is_git)
                .filter(|workspace| {
                    (self.github_lookup_requested()
                        && self
                            .focused_local_checkout()
                            .is_some_and(|(focused, _)| focused.path == workspace.path))
                        || self.sidebar_github_projects.contains(&workspace.path)
                })
                .map(|workspace| crate::github::GithubProjectRequest {
                    links: workspace
                        .checkouts
                        .iter()
                        .filter_map(|checkout| self.issue_candidates.get(&checkout.id))
                        .map(|candidate| candidate.reference.clone())
                        .collect::<BTreeSet<_>>()
                        .into_iter()
                        .take(crate::issues::ISSUE_LIMIT)
                        .collect(),
                    root: PathBuf::from(&workspace.path),
                    generation: self
                        .github_generations
                        .get(&workspace.path)
                        .copied()
                        .unwrap_or(0),
                })
                .collect(),
        }
    }

    pub(super) fn github_lookup_requested(&self) -> bool {
        self.snapshot.ui_state.right_panel_visible
            && matches!(
                self.snapshot.ui_state.right_panel_section,
                RightPanelSection::Overview
            )
    }

    pub fn ingest_github(&mut self, github: crate::model::GithubSnapshot) -> bool {
        // A failed lookup must not erase the answer it failed to replace: the
        // card shows the previous pull requests with `as of` beside them, so a
        // stale project keeps its results and only its status changes.
        let mut merged = github;
        for project in &mut merged.projects {
            if (project.status.stale || project.status.unavailable_reason.is_some())
                && let Some(previous) = self.github.project(&project.root_path)
            {
                let incomplete = !project.pull_requests_read || !project.issues_read;
                if !project.pull_requests_read
                    && (previous.pull_requests_read
                        || previous.status.last_success_at_unix_ms.is_some())
                {
                    project.pull_requests = previous.pull_requests.clone();
                    project.pull_requests_read = true;
                }
                if !project.issues_read
                    && (previous.issues_read || previous.status.last_success_at_unix_ms.is_some())
                {
                    project.issues = previous.issues.clone();
                    project.issues_read = true;
                }
                if incomplete {
                    project.status.stale = true;
                    project.status.last_success_at_unix_ms =
                        previous.status.last_success_at_unix_ms;
                }
            }
        }
        if self.github == merged {
            return false;
        }
        self.github = merged;
        self.apply_pull_requests();
        self.refresh_worktree_projection();
        true
    }

    pub fn refresh_pull_requests(&mut self, project_path: &str) {
        let generation = self
            .github_generations
            .entry(project_path.to_owned())
            .or_insert(0);
        *generation = generation.wrapping_add(1);
        if let Some(cached) = self
            .github
            .projects
            .iter_mut()
            .find(|p| p.root_path == project_path)
        {
            cached.status.loading = true;
        }
    }

    pub fn refresh_worktrees(&mut self) {
        self.worktree_generation = self.worktree_generation.wrapping_add(1);
        self.snapshot.git_worktrees_loading = true;
    }

    pub(super) fn open_git_worktree(&mut self, checkout_path: String) -> bool {
        let target = self.worktree_catalog.projects.iter().find_map(|project| {
            project
                .worktrees
                .iter()
                .find(|worktree| worktree.path == checkout_path)
                .map(|worktree| (project.root_path.clone(), worktree.clone()))
        });
        let Some((repository_root, worktree)) = target else {
            self.set_error(
                "worktree.open_unknown",
                format!("Worktree is no longer listed: {checkout_path}"),
                true,
            );
            self.refresh_worktrees();
            return true;
        };
        if worktree.missing {
            self.ingest_worktree_open_result(
                checkout_path,
                Err("Worktree is missing on disk".to_owned()),
            );
            return true;
        }
        let newest_pane = self
            .snapshot
            .navigator
            .workspaces
            .iter()
            .flat_map(|workspace| workspace.checkouts.iter())
            .filter(|checkout| checkout.path == checkout_path)
            .flat_map(|checkout| checkout.tabs.iter())
            .flat_map(|tab| tab.panes.iter())
            .max_by_key(|pane| pane.activity_at_unix_ms.unwrap_or(0))
            .map(|pane| pane.id.clone());
        let Some(context) = self.live.as_ref().cloned() else {
            self.ingest_worktree_open_result(
                checkout_path,
                Err("Opening a worktree needs a live Herdr connection".to_owned()),
            );
            return true;
        };
        if let Err(message) =
            live::spawn_worktree_open(context, checkout_path.clone(), repository_root, newest_pane)
        {
            self.ingest_worktree_open_result(checkout_path, Err(message));
        }
        true
    }

    pub(super) fn set_git_worktree_base(&mut self, project_path: String, branch: String) -> bool {
        let listed = self
            .worktree_catalog
            .project(&project_path)
            .is_some_and(|project| {
                project
                    .worktrees
                    .iter()
                    .any(|worktree| worktree.branch.as_deref() == Some(branch.as_str()))
            });
        if !listed {
            self.set_error(
                "worktree.base_unavailable",
                format!("Branch {branch} is no longer checked out in this repository"),
                true,
            );
            self.refresh_worktrees();
            return true;
        }
        if self
            .snapshot
            .ui_state
            .project_base_branches
            .insert(project_path, branch)
            .is_some()
        {
            // Replacing and inserting both persist below; the return value is
            // deliberately not used as the change detector because the same
            // branch is an idempotent no-op at the reader boundary.
        }
        self.persist_ui_state();
        self.refresh_worktrees();
        true
    }

    pub(super) fn remove_git_worktree(&mut self, payload: RemoveWorktreePayload) -> bool {
        if let Some(removal) = self.snapshot.worktree_removal.as_ref()
            && matches!(removal.phase.as_str(), "closing" | "ready")
        {
            if removal.checkout_path == payload.checkout_path {
                return false;
            }
            self.set_error(
                "worktree.remove_busy",
                format!(
                    "Finish removing {} before deleting another worktree",
                    removal.checkout_path
                ),
                true,
            );
            return true;
        }
        let target = self.worktree_catalog.projects.iter().find_map(|project| {
            project
                .worktrees
                .iter()
                .find(|worktree| worktree.path == payload.checkout_path)
                .map(|worktree| {
                    (
                        project.root_path.clone(),
                        project.base_branch.clone(),
                        worktree.clone(),
                    )
                })
        });
        let Some((repository_root, protected_base_branch, worktree)) = target else {
            self.set_error(
                "worktree.remove_unknown",
                format!("Worktree is no longer listed: {}", payload.checkout_path),
                true,
            );
            self.refresh_worktrees();
            return true;
        };
        if let Some(reason) = worktree.deletion_gate.blocked_reason.as_ref() {
            self.set_error("worktree.remove_blocked", reason.clone(), true);
            return true;
        }
        let pane_ids = self
            .snapshot
            .navigator
            .workspaces
            .iter()
            .flat_map(|workspace| workspace.checkouts.iter())
            .filter(|checkout| checkout.path == payload.checkout_path)
            .flat_map(|checkout| checkout.tabs.iter())
            .flat_map(|tab| tab.panes.iter())
            .map(|pane| pane.id.clone())
            .collect::<Vec<_>>();
        self.next_worktree_removal_id = self.next_worktree_removal_id.wrapping_add(1).max(1);
        let id = self.next_worktree_removal_id;
        self.snapshot.worktree_removal = Some(crate::model::WorktreeRemovalSnapshot {
            id,
            repository_root,
            checkout_path: payload.checkout_path.clone(),
            expected_head_sha: worktree.head_sha.clone(),
            expected_branch: worktree.branch.clone(),
            protected_base_branch,
            branch: worktree.branch,
            delete_branch: payload.delete_branch && worktree.deletion_gate.can_delete_branch,
            phase: "closing".to_owned(),
            message: None,
        });
        eprintln!(
            "{}",
            serde_json::json!({
                "component":"worktree_removal",
                "stage":"close_requested",
                "id":id,
                "path":payload.checkout_path,
                "pane_ids":pane_ids,
            })
        );
        let Some(context) = self.live.as_ref().cloned() else {
            return self.ingest_worktree_close_result(
                id,
                Err("Deleting a worktree needs a live Herdr connection".to_owned()),
            );
        };
        if let Err(message) =
            live::spawn_worktree_close(context, id, payload.checkout_path, pane_ids)
        {
            self.ingest_worktree_close_result(id, Err(message));
        }
        true
    }

    /// Pins or unpins a registered project (D-07, D-08). The row moves at
    /// once and the registration is persisted on the existing off-lock save;
    /// the same value again changes nothing and writes nothing.
    pub(super) fn set_workspace_pinned(&mut self, payload: WorkspacePinSetPayload) -> bool {
        let Some(registration) = self
            .snapshot
            .ui_state
            .workspace_registrations
            .iter_mut()
            .find(|registration| registration.id == payload.workspace_id)
        else {
            self.set_error(
                "workspace.pin_unregistered",
                format!(
                    "Project {} is not registered, so it cannot be pinned",
                    payload.workspace_id
                ),
                false,
            );
            return true;
        };
        if registration.pinned == payload.pinned {
            return false;
        }
        registration.pinned = payload.pinned;
        for workspace in self
            .snapshot
            .navigator
            .workspaces
            .iter_mut()
            .chain(self.last_accepted_catalog.iter_mut().flatten())
            .filter(|workspace| workspace.id == payload.workspace_id)
        {
            workspace.pinned = payload.pinned;
        }
        // Re-sort in place, the way an activity change does: the catalog
        // carries the flag on its next rebuild, but the row moves now.
        let agents = self.snapshot.navigator.agents.clone();
        crate::project_context::sort_projects(&mut self.snapshot.navigator.workspaces, &agents);
        self.refresh_inactive_groups();
        self.persist_ui_state();
        self.push_diagnostic(
            if payload.pinned {
                "workspace.pinned"
            } else {
                "workspace.unpinned"
            },
            format!("Project {}", payload.workspace_id),
        );
        true
    }

    /// `Remove project…` (D-09, D-11). A project with no pane loses its
    /// registration at once, as before. One with panes has them closed on a
    /// worker thread that waits for Herdr to confirm; only that confirmation
    /// removes the registration, so a timeout leaves the project registered
    /// with the reason in the error banner and a retry starts from whatever
    /// panes remain.
    pub(super) fn remove_workspace(&mut self, payload: RemoveWorkspacePayload) -> bool {
        if !self
            .snapshot
            .ui_state
            .workspace_registrations
            .iter()
            .any(|registration| registration.id == payload.workspace_id)
        {
            // Already gone: the target state is reached, and a repeat of a
            // completed removal stays quiet (DESIGN.md, registration removal).
            return false;
        }
        if self
            .workspace_removals_in_flight
            .contains(&payload.workspace_id)
        {
            return false;
        }
        // The mirror of the create-side refusal: a creation still running
        // for this folder will push the registration back after the retire.
        if self.workspace_creation_in_flight_for(&payload.workspace_id) {
            self.set_error(
                "workspace.create_in_flight",
                "This project is still being added; wait for its first pane, then remove it",
                false,
            );
            return true;
        }
        let (checkout_paths, pane_ids) = self
            .snapshot
            .navigator
            .workspaces
            .iter()
            .filter(|workspace| workspace.id == payload.workspace_id)
            .flat_map(|workspace| &workspace.checkouts)
            .fold(
                (Vec::new(), Vec::new()),
                |(mut paths, mut panes), checkout| {
                    paths.push(checkout.path.clone());
                    panes.extend(
                        checkout
                            .tabs
                            .iter()
                            .flat_map(|tab| &tab.panes)
                            .map(|pane| pane.id.clone()),
                    );
                    (paths, panes)
                },
            );
        if pane_ids.is_empty() {
            return self.retire_workspace_registration(&payload.workspace_id);
        }
        crate::diagnostic!(serde_json::json!({
            "component": "registration", "kind": "remove.close_requested",
            "workspace_id": payload.workspace_id, "pane_ids": pane_ids,
        }));
        let Some(context) = self.live.as_ref().cloned() else {
            self.set_error(
                "workspace.remove_failed",
                "Removing a project with open panes needs a live Herdr connection",
                true,
            );
            return true;
        };
        self.workspace_removals_in_flight
            .insert(payload.workspace_id.clone());
        if let Err(message) = live::spawn_workspace_close(
            context,
            payload.workspace_id.clone(),
            checkout_paths,
            pane_ids,
        ) {
            self.ingest_workspace_close_result(&payload.workspace_id, Err(message));
        }
        true
    }

    /// The worker's answer to `remove_workspace`: Herdr confirmed every pane
    /// gone, or it did not in time. Only the first removes the registration.
    pub fn ingest_workspace_close_result(
        &mut self,
        workspace_id: &str,
        result: Result<(), String>,
    ) -> bool {
        if !self.workspace_removals_in_flight.remove(workspace_id) {
            return false;
        }
        match result {
            Ok(()) => self.retire_workspace_registration(workspace_id),
            Err(message) => {
                crate::diagnostic!(serde_json::json!({
                    "component": "registration", "kind": "remove.close_failed",
                    "workspace_id": workspace_id, "error": message,
                }));
                self.set_error(
                    "workspace.remove_failed",
                    format!("{message}. The project stays registered; remove it again to close the panes that remain."),
                    true,
                );
                true
            }
        }
    }

    /// The registration id a removal is still closing panes for, when
    /// `path` names that same folder. Ids are path-keyed, so the folder and
    /// the registration cannot be told apart by id alone. The registration is
    /// still listed while the close runs, so its stored path is the
    /// comparison; nothing is resolved on disk under the lock.
    pub(super) fn workspace_removal_in_flight_for(&self, path: &str) -> Option<String> {
        if self.workspace_removals_in_flight.is_empty() {
            return None;
        }
        let requested = Path::new(path);
        self.snapshot
            .ui_state
            .workspace_registrations
            .iter()
            .filter(|registration| self.workspace_removals_in_flight.contains(&registration.id))
            .find(|registration| Path::new(&registration.path) == requested)
            .map(|registration| registration.id.clone())
    }

    /// Whether a creation still running names the folder `workspace_id`
    /// registers; creation is keyed by the requested path, not the id.
    fn workspace_creation_in_flight_for(&self, workspace_id: &str) -> bool {
        if self.workspace_creations_in_flight.is_empty() {
            return false;
        }
        self.snapshot
            .ui_state
            .workspace_registrations
            .iter()
            .filter(|registration| registration.id == workspace_id)
            .any(|registration| {
                self.workspace_creations_in_flight
                    .iter()
                    .any(|path| Path::new(path) == Path::new(&registration.path))
            })
    }

    /// Drops the registration and its row. Files, worktrees and Herdr
    /// workspaces are never touched here.
    fn retire_workspace_registration(&mut self, workspace_id: &str) -> bool {
        let before = self.snapshot.ui_state.workspace_registrations.len();
        self.snapshot
            .ui_state
            .workspace_registrations
            .retain(|registration| registration.id != workspace_id);
        if before == self.snapshot.ui_state.workspace_registrations.len() {
            return false;
        }
        // The focused checkout and the selected pane leave with the project;
        // kept, they would name a checkout no catalog carries and the sync
        // would report `pane.projection_unavailable` every tick instead of
        // moving to the next project, exactly as after a Herdr restart
        // (`consume_restore_hint`).
        let focus_leaves_with_workspace = self
            .snapshot
            .navigator
            .workspaces
            .iter()
            .filter(|workspace| workspace.id == workspace_id)
            .flat_map(|workspace| workspace.checkouts.iter())
            .any(|checkout| {
                Some(checkout.id.as_str()) == self.snapshot.navigator.focused_checkout_id.as_deref()
            });
        // Retire the accepted projection directly. Rebuilding the
        // filesystem catalog here ran git while holding the mutex.
        self.snapshot
            .navigator
            .workspaces
            .retain(|workspace| workspace.id != workspace_id);
        if focus_leaves_with_workspace {
            self.snapshot.ui_state.focused_checkout_id = None;
            self.snapshot.navigator.focused_checkout_id = None;
            self.snapshot.ui_state.selected_pane_id = None;
            self.snapshot.terminal.pane_id = None;
            self.snapshot.focused.pane_id = None;
            self.clear_terminal_projection();
            if self
                .snapshot
                .status
                .last_error
                .as_ref()
                .is_some_and(|error| error.kind == "pane.projection_unavailable")
            {
                self.snapshot.status.last_error = None;
            }
        }
        if let Some(catalog) = &mut self.last_accepted_catalog {
            catalog.retain(|workspace| workspace.id != workspace_id);
        }
        self.snapshot
            .ui_state
            .collapsed_workspace_ids
            .retain(|id| id != workspace_id);
        self.refresh_inactive_groups();
        self.resync_navigator_focus();
        self.persist_current_ui_state();
        self.push_diagnostic(
            "workspace.unregistered",
            format!("Unregistered workspace {workspace_id} without touching its files"),
        );
        true
    }

    pub fn ingest_worktree_close_result(&mut self, id: u64, result: Result<(), String>) -> bool {
        let Some(active) = self.snapshot.worktree_removal.as_ref() else {
            return false;
        };
        if active.id != id || active.phase != "closing" {
            return false;
        }
        let mut result = result;
        if result.is_ok() {
            let current = self
                .worktree_catalog
                .projects
                .iter()
                .flat_map(|project| &project.worktrees)
                .find(|worktree| worktree.path == active.checkout_path);
            let identity_changed = current.is_none_or(|worktree| {
                worktree.head_sha != active.expected_head_sha
                    || worktree.branch != active.expected_branch
                    || worktree.deletion_gate.blocked_reason.is_some()
            });
            let pane_reappeared = self
                .snapshot
                .navigator
                .workspaces
                .iter()
                .flat_map(|workspace| &workspace.checkouts)
                .any(|checkout| checkout.path == active.checkout_path && checkout.has_panes);
            if identity_changed || pane_reappeared {
                result = Err(if pane_reappeared {
                    "A pane appeared in the worktree while deletion was being confirmed".to_owned()
                } else {
                    "The worktree identity or deletion gate changed while panes were closing"
                        .to_owned()
                });
            }
        }
        let removal = self.snapshot.worktree_removal.as_mut().unwrap();
        match result {
            Ok(()) => {
                removal.phase = "ready".to_owned();
                removal.message = None;
            }
            Err(message) => {
                removal.phase = "failed".to_owned();
                removal.message = Some(message);
            }
        }
        true
    }

    pub fn ingest_worktree_open_result(
        &mut self,
        checkout_path: String,
        result: Result<(), String>,
    ) -> bool {
        let message = result.err();
        let mut changed = false;
        for project in &mut self.worktree_catalog.projects {
            if let Some(worktree) = project
                .worktrees
                .iter_mut()
                .find(|worktree| worktree.path == checkout_path)
                && worktree.open_error != message
            {
                worktree.open_error = message.clone();
                changed = true;
            }
        }
        for workspace in &mut self.snapshot.navigator.workspaces {
            for checkout in &mut workspace.checkouts {
                if checkout.path == checkout_path
                    && let Some(worktree) = checkout.worktree.as_mut()
                    && worktree.open_error != message
                {
                    worktree.open_error = message.clone();
                    changed = true;
                }
            }
        }
        if message.is_some() {
            self.refresh_worktrees();
        }
        changed
    }

    pub(super) fn finish_worktree_removal(
        &mut self,
        payload: WorktreeRemovalFinishedPayload,
    ) -> bool {
        let Some(removal) = self.snapshot.worktree_removal.as_mut() else {
            self.set_error(
                "worktree.remove_result_without_request",
                "A worktree removal result arrived without an active request",
                false,
            );
            return true;
        };
        if removal.id != payload.id || removal.phase != "ready" {
            self.set_error(
                "worktree.remove_result_stale",
                format!(
                    "Worktree removal result {} is not the active ready request",
                    payload.id
                ),
                false,
            );
            return true;
        }
        removal.phase = if payload.removed {
            "finished"
        } else {
            "failed"
        }
        .to_owned();
        removal.message = Some(payload.message);
        if payload.removed {
            self.refresh_worktrees();
        }
        true
    }

    pub fn disk_request(&self) -> crate::disk::DiskRequest {
        let paths = if self.snapshot.ui_state.right_panel_visible
            && matches!(
                self.snapshot.ui_state.right_panel_section,
                RightPanelSection::Overview
            ) {
            self.focused_local_checkout()
                .and_then(|(workspace, _)| {
                    // A folder project has no worktree list, and it still has
                    // a size the Overview states (PRD B16): the folder itself.
                    if !workspace.is_git {
                        return Some(vec![PathBuf::from(&workspace.path)]);
                    }
                    let project = self.worktree_catalog.project(&workspace.path)?;
                    let mut paths: Vec<_> = project
                        .worktrees
                        .iter()
                        .map(|w| PathBuf::from(&w.path))
                        .collect();
                    if let Some(shared) = &project.shared_git_path {
                        paths.push(PathBuf::from(shared));
                    }
                    Some(paths)
                })
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        crate::disk::DiskRequest {
            paths,
            generation: self.disk_generation,
        }
    }

    pub fn ingest_disk_usage(&mut self, disk: Vec<crate::model::DiskUsageSnapshot>) -> bool {
        if self.disk_usage == disk {
            return false;
        }
        self.disk_usage = disk;
        self.refresh_worktree_projection();
        true
    }

    pub fn remeasure_disk(&mut self) {
        self.disk_generation = self.disk_generation.wrapping_add(1);
        self.refresh_card();
    }

    pub(super) fn focused_local_checkout(&self) -> Option<(&WorkspaceSnapshot, &CheckoutSnapshot)> {
        let focused_checkout_id = self.snapshot.navigator.focused_checkout_id.as_deref()?;
        self.snapshot
            .navigator
            .workspaces
            .iter()
            .filter(|workspace| workspace.remote_target_id.is_none())
            .find_map(|workspace| {
                workspace
                    .checkouts
                    .iter()
                    .find(|checkout| checkout.id == focused_checkout_id)
                    .map(|checkout| (workspace, checkout))
            })
    }

    pub(super) fn apply_pull_requests(&mut self) -> bool {
        let mut changed = false;
        let github = self.github.clone();
        let git_requested = self.github_lookup_requested();
        for workspace in self.snapshot.navigator.workspaces.iter_mut() {
            let project = github.project(&workspace.path);
            let status = project
                .map(|project| project.status.clone())
                .unwrap_or_else(|| crate::model::GithubStatusSnapshot {
                    loading: workspace.is_git
                        && workspace.remote_target_id.is_none()
                        && (self.sidebar_github_projects.contains(&workspace.path)
                            || git_requested),
                    ..Default::default()
                });
            let home_issues = project
                .map(|project| project.issues.clone())
                .unwrap_or_default();
            if workspace.home_issues != home_issues {
                workspace.home_issues = home_issues;
                changed = true;
            }
            for checkout in workspace.checkouts.iter_mut() {
                if checkout.github != status {
                    checkout.github = status.clone();
                    changed = true;
                }
                let pull_request = checkout.branch.as_deref().and_then(|branch| {
                    project?
                        .pull_requests
                        .iter()
                        .find(|pull_request| pull_request.head_branch == branch)
                        .cloned()
                });
                if checkout.pull_request != pull_request {
                    checkout.pull_request = pull_request;
                    changed = true;
                }
            }
        }
        changed |= crate::sidebar::sync_checkout_purposes(
            &mut self.snapshot.navigator.workspaces,
            &self.snapshot.navigator.agents,
        );
        changed |= self.sync_issues();
        changed
    }

    pub fn refresh_card(&mut self) -> bool {
        let card = self.projected_card();
        if self.snapshot.card == card {
            return false;
        }
        let retired = self
            .snapshot
            .card
            .panes
            .iter()
            .filter(|old| {
                !card.panes.iter().any(|new| {
                    new.pane_id == old.pane_id && new.parent_pane_id == old.parent_pane_id
                })
            })
            .count();
        if retired > 0 {
            crate::diagnostic!(serde_json::json!({
                "component": "checkout_context", "kind": "context.retired", "count": retired,
            }));
        }
        self.snapshot.card = card;
        true
    }

    pub(super) fn projected_card(&self) -> crate::model::CheckoutCardSnapshot {
        let Some((workspace, checkout)) = self.focused_local_checkout() else {
            return crate::model::CheckoutCardSnapshot::default();
        };
        let github = if workspace.is_git {
            self.github
                .project(&workspace.path)
                .map(|project| project.status.clone())
                // An absent answer is loading only while the Git section
                // requests it. Explorer also renders this card but starts
                // no lookup, so absence there must not imply work in flight.
                .unwrap_or(crate::model::GithubStatusSnapshot {
                    loading: self.github_lookup_requested(),
                    ..crate::model::GithubStatusSnapshot::default()
                })
        } else {
            crate::model::GithubStatusSnapshot::default()
        };
        let disk = self
            .disk_usage
            .iter()
            .find(|disk| disk.path.as_deref() == Some(checkout.path.as_str()))
            .cloned()
            .unwrap_or_default();
        let disk_measuring = checkout.is_worktree && disk.path.is_none();
        crate::model::CheckoutCardSnapshot {
            checkout_id: Some(checkout.id.clone()),
            panes: crate::project_context::checkout_panes(
                checkout,
                &self.snapshot.navigator.workspaces,
                &self.snapshot.navigator.agents,
            ),
            github,
            disk,
            disk_measuring,
            deletion_gate: checkout
                .worktree
                .as_ref()
                .map(|worktree| worktree.deletion_gate.clone()),
        }
    }

    pub(super) fn create_project_worktree(&mut self, payload: CreateWorktreePayload) -> bool {
        let branch = payload.branch.trim().to_owned();
        if branch.is_empty() {
            self.set_error(
                "worktree.create_invalid_branch",
                "Branch is required",
                false,
            );
            return true;
        }
        let has_branch = self
            .worktree_catalog
            .project(&payload.repository_root)
            .is_some_and(|project| {
                project
                    .worktrees
                    .iter()
                    .any(|row| row.branch.is_some() && row.head_sha.is_some())
            });
        if !has_branch {
            self.set_error(
                "worktree.create_without_branches",
                "Create the repository's first branch before creating a worktree",
                true,
            );
            return true;
        }
        let id = match self.begin_task_operation(
            "worktree_create",
            Some(payload.repository_root.clone()),
            Some(branch.clone()),
            payload.base_branch.clone(),
            payload.agent_kind.clone(),
        ) {
            Ok(id) => id,
            Err(message) => {
                self.set_error("task_operation.busy", message, true);
                return true;
            }
        };
        let request = live::WorktreeTaskRequest {
            id,
            repository_root: payload.repository_root,
            branch,
            base_branch: payload.base_branch,
            agent_kind: payload.agent_kind,
            focus: true,
            purpose: payload.purpose,
        };
        let Some(context) = self.live.as_ref().cloned() else {
            return self.ingest_task_operation_result(
                id,
                Err("create worktree: a live Herdr connection is required".into()),
            );
        };
        if let Err(message) = live::spawn_worktree_create(context, request) {
            return self.ingest_task_operation_result(id, Err(message));
        }
        true
    }

    /// Writes the live Herdr token first and the branch-description mirror
    /// second on the task-operation worker. The runtime lock is held only for
    /// validating the target and publishing the receipt.
    pub(super) fn set_checkout_purpose(&mut self, payload: SetCheckoutPurposePayload) -> bool {
        let text = payload.text.trim().to_owned();
        let target = self
            .snapshot
            .navigator
            .workspaces
            .iter()
            .find_map(|workspace| {
                workspace
                    .checkouts
                    .iter()
                    .find(|checkout| checkout.id == payload.checkout_id)
                    .map(|checkout| (None, workspace.clone(), checkout.clone()))
            })
            .or_else(|| {
                self.snapshot.status.remote.iter().find_map(|status| {
                    status
                        .session
                        .as_ref()?
                        .workspaces
                        .iter()
                        .find_map(|workspace| {
                            workspace
                                .checkouts
                                .iter()
                                .find(|checkout| checkout.id == payload.checkout_id)
                                .map(|checkout| {
                                    (
                                        Some(status.target_id.clone()),
                                        workspace.clone(),
                                        checkout.clone(),
                                    )
                                })
                        })
                })
            });
        let Some((remote_target_id, workspace, checkout)) = target else {
            let id = match self.begin_task_operation("checkout_purpose", None, None, None, None) {
                Ok(id) => id,
                Err(message) => {
                    self.set_error("task_operation.busy", message, true);
                    return true;
                }
            };
            self.purpose_operation_target = Some(PurposeOperationTarget {
                id,
                checkout_id: payload.checkout_id,
                remote_target_id: None,
            });
            return self.fail_purpose_operation(
                id,
                "The checkout is no longer available",
                "checkout_purpose.unknown_checkout",
            );
        };
        let repository_root = workspace.path.clone();
        let checkout_path = checkout.path.clone();
        let branch = checkout.branch.clone();
        let persisted_branch = remote_target_id.is_none().then(|| branch.clone()).flatten();
        let id = match self.begin_task_operation(
            "checkout_purpose",
            Some(repository_root.clone()),
            persisted_branch.clone(),
            None,
            None,
        ) {
            Ok(id) => id,
            Err(message) => {
                self.set_error("task_operation.busy", message, true);
                return true;
            }
        };
        if let Some(operation) = self.snapshot.task_operation.as_mut() {
            operation.path = Some(checkout_path.clone());
        }
        self.purpose_operation_target = Some(PurposeOperationTarget {
            id,
            checkout_id: payload.checkout_id.clone(),
            remote_target_id: remote_target_id.clone(),
        });
        if text.chars().count() > 80 || text.contains(['\n', '\r']) {
            return self.fail_purpose_operation(
                id,
                "Purpose must be one line of 80 characters or fewer",
                "checkout_purpose.invalid",
            );
        }
        if let Some(target_id) = remote_target_id.as_deref() {
            let version = self
                .snapshot
                .status
                .remote
                .iter()
                .find(|remote| remote.target_id == target_id)
                .and_then(|remote| remote.herdr_version.as_deref());
            if !herdr_version_supports_purpose(version) {
                return self.fail_purpose_operation(
                    id,
                    remote_purpose_unavailable_reason(version),
                    "checkout_purpose.remote_unsupported",
                );
            }
        }
        let session_workspace_id = if remote_target_id.is_some() {
            workspace.session_workspace_ids.last().cloned()
        } else {
            workspace::authoritative_session_space(
                &self.last_session_spaces,
                &workspace,
                &checkout_path,
            )
            .map(|space| space.id.clone())
        };
        if branch.is_none() && session_workspace_id.is_none() {
            return self.fail_purpose_operation(
                id,
                "This detached checkout has no live Herdr workspace to hold a purpose",
                "checkout_purpose.no_target",
            );
        }
        let request = live::PurposeTaskRequest {
            id,
            checkout_id: payload.checkout_id,
            repository_root,
            branch: persisted_branch,
            session_workspace_id,
            purpose: text,
        };
        let spawn_result = if let Some(target_id) = remote_target_id.as_deref() {
            self.remote_controls
                .get(target_id)
                .cloned()
                .ok_or_else(|| "The remote Herdr connection is unavailable".to_owned())
                .and_then(|context| live::spawn_remote_purpose_write(context, request))
        } else {
            self.live
                .as_ref()
                .cloned()
                .ok_or_else(|| "A live Herdr connection is required".to_owned())
                .and_then(|context| live::spawn_purpose_write(context, request))
        };
        if let Err(message) = spawn_result {
            return self.ingest_purpose_operation_result(id, Err(message));
        }
        true
    }

    /// The Overview's `Open in History` and its `N files` chip: focus the
    /// checkout and switch the panel section together. The checkout has to be
    /// a local one the navigator lists; the section has to be one the panel
    /// has. Either failing is an error the operator did not cause and cannot
    /// act on, so it goes to the log and nothing on screen moves.
    pub(super) fn overview_open_section(&mut self, payload: OverviewOpenSectionPayload) -> bool {
        let Some(section) = RightPanelSection::parse(&payload.section) else {
            self.set_error(
                "overview.unknown_section",
                format!("Right panel has no section {}", payload.section),
                false,
            );
            return true;
        };
        let Some((workspace_id, checkout_id)) = self.local_checkout_ids(&payload.checkout_path)
        else {
            self.set_error(
                "overview.unknown_checkout",
                format!("Checkout is not listed: {}", payload.checkout_path),
                false,
            );
            return true;
        };
        if self.snapshot.navigator.focused_checkout_id.as_deref() != Some(checkout_id.as_str()) {
            self.focus_checkout(&workspace_id, &checkout_id);
        }
        self.snapshot.ui_state.right_panel_visible = true;
        self.snapshot.ui_state.right_panel_section = section;
        self.persist_ui_state();
        true
    }

    /// `New agent here ▸ Terminal only / Claude / Codex` and the empty
    /// group's `Start agent…`: one tab with the checkout as its cwd, in the
    /// checkout's own Herdr workspace, through the task operation slot the
    /// worktree sheet already reports through. The shell starts the provider
    /// in the pane the slot names, so `terminal` needs nothing more from it.
    pub(super) fn agent_start_in_checkout(&mut self, payload: AgentStartInCheckoutPayload) -> bool {
        let agent_kind = match payload.provider.as_str() {
            "terminal" => None,
            "claude" | "codex" => Some(payload.provider.clone()),
            other => {
                self.set_error(
                    "agent_start.unknown_provider",
                    format!("No agent provider named {other}"),
                    false,
                );
                return true;
            }
        };
        let Some((workspace_id, checkout_id)) = self.local_checkout_ids(&payload.checkout_path)
        else {
            self.set_error(
                "overview.unknown_checkout",
                format!("Checkout is not listed: {}", payload.checkout_path),
                false,
            );
            return true;
        };
        let (workspace_path, checkout_label, next_tab_label) = {
            let workspace = self
                .snapshot
                .navigator
                .workspaces
                .iter()
                .find(|workspace| workspace.id == workspace_id)
                .expect("local_checkout_ids named a listed workspace");
            let checkout = workspace
                .checkouts
                .iter()
                .find(|checkout| checkout.id == checkout_id)
                .expect("local_checkout_ids named a listed checkout");
            (
                workspace.path.clone(),
                checkout.label.clone(),
                checkout.next_tab_label.clone(),
            )
        };
        let session_workspace_id = self.reusable_session_workspace_id(&workspace_id, &checkout_id);
        let label = session_workspace_id
            .as_ref()
            .map(|_| next_tab_label)
            .unwrap_or_else(|| format!("hide {checkout_label}"));
        let id = match self.begin_task_operation(
            "agent_start",
            Some(workspace_path),
            None,
            None,
            agent_kind,
        ) {
            Ok(id) => id,
            Err(message) => {
                self.set_error("task_operation.busy", message, true);
                return true;
            }
        };
        let request = live::CheckoutTabRequest {
            id,
            checkout_path: payload.checkout_path,
            label,
            session_workspace_id,
        };
        let Some(context) = self.live.as_ref().cloned() else {
            return self.ingest_task_operation_result(
                id,
                Err("start an agent: a live Herdr connection is required".into()),
            );
        };
        if let Err(message) = live::spawn_checkout_tab_create(context, request) {
            return self.ingest_task_operation_result(id, Err(message));
        }
        true
    }

    /// The workspace and checkout ids a local checkout path names, or `None`
    /// for a path the navigator does not list locally.
    fn local_checkout_ids(&self, checkout_path: &str) -> Option<(String, String)> {
        self.snapshot
            .navigator
            .workspaces
            .iter()
            .filter(|workspace| workspace.remote_target_id.is_none())
            .find_map(|workspace| {
                workspace
                    .checkouts
                    .iter()
                    .find(|checkout| checkout.path == checkout_path)
                    .map(|checkout| (workspace.id.clone(), checkout.id.clone()))
            })
    }

    /// Returns a live Herdr workspace that belongs only to this Hide project.
    ///
    /// A Herdr workspace can contain panes from several repository roots. Its
    /// label and future tabs then belong to none of those projects reliably,
    /// so opening another tab there would carry a neighboring project's name
    /// and keep mixing the two catalogs. Prefer the checkout's visible tab,
    /// then its other tabs, then the project's remaining session workspaces,
    /// but reuse a candidate only while this project is its sole owner.
    pub(super) fn reusable_session_workspace_id(
        &self,
        project_id: &str,
        checkout_id: &str,
    ) -> Option<String> {
        let project = self
            .snapshot
            .navigator
            .workspaces
            .iter()
            .find(|workspace| workspace.id == project_id)?;
        let checkout = project
            .checkouts
            .iter()
            .find(|checkout| checkout.id == checkout_id)?;
        let mut candidates = Vec::new();
        if let Some(workspace_id) = self
            .visible_tab_ids
            .get(checkout_id)
            .and_then(|tab_id| {
                self.snapshot
                    .pane_layouts
                    .iter()
                    .find(|layout| &layout.tab_id == tab_id)
            })
            .map(|layout| layout.workspace_id.clone())
        {
            candidates.push(workspace_id);
        }
        candidates.extend(
            checkout
                .tabs
                .iter()
                .filter_map(|tab| tab.workspace_id.clone()),
        );
        candidates.extend(project.session_workspace_ids.iter().cloned());
        candidates.into_iter().find(|candidate| {
            project.session_workspace_ids.contains(candidate)
                && self
                    .snapshot
                    .navigator
                    .workspaces
                    .iter()
                    .filter(|workspace| workspace.session_workspace_ids.contains(candidate))
                    .all(|workspace| workspace.id == project_id)
        })
    }

    pub(super) fn migrate_main_branch(&mut self, payload: MigrateMainBranchPayload) -> bool {
        let Some(project) = self.worktree_catalog.project(&payload.repository_root) else {
            self.set_error(
                "branch_migrate.unknown_project",
                "Repository status is unavailable",
                true,
            );
            return true;
        };
        let Some(main) = project.worktrees.iter().find(|row| row.is_main) else {
            self.set_error(
                "branch_migrate.missing_main",
                "The main worktree is unavailable",
                true,
            );
            return true;
        };
        if main.dirty {
            self.set_error(
                "branch_migrate.dirty",
                "Commit or discard uncommitted changes before moving the branch",
                true,
            );
            return true;
        }
        let branch = main.branch.clone();
        let id = match self.begin_task_operation(
            "branch_migrate",
            Some(payload.repository_root.clone()),
            branch.clone(),
            Some(payload.base_branch.clone()),
            None,
        ) {
            Ok(id) => id,
            Err(message) => {
                self.set_error("task_operation.busy", message, true);
                return true;
            }
        };
        let request = live::WorktreeTaskRequest {
            id,
            repository_root: payload.repository_root,
            branch: branch.unwrap_or_default(),
            base_branch: Some(payload.base_branch),
            agent_kind: None,
            focus: false,
            purpose: None,
        };
        let Some(context) = self.live.as_ref().cloned() else {
            return self.ingest_task_operation_result(
                id,
                Err("move branch: a live Herdr connection is required".into()),
            );
        };
        if let Err(message) = live::spawn_branch_migration(context, request) {
            return self.ingest_task_operation_result(id, Err(message));
        }
        true
    }

    /// Accepts a projection only while it still describes the checkout the
    /// runtime is asking about, so a slow read against a checkout the operator
    /// has already left cannot overwrite the current one.
    pub fn ingest_changes(&mut self, changes: crate::model::ChangesSnapshot) -> bool {
        let expected = self
            .changes_request()
            .map(|request| request.root_path.to_string_lossy().into_owned());
        if expected != changes.root_path {
            return false;
        }
        if self.snapshot.changes == changes {
            return false;
        }
        self.snapshot.changes = changes;
        true
    }

    /// Herdr closes a workspace with its last pane, and a project that exists
    /// only as that Herdr workspace would vanish from the sidebar with it.
    /// The user asked to close a pane, not to forget the project, so the
    /// project is registered at its repository path first. The path-keyed
    /// project id is unchanged by this, and the row stays selectable with
    /// its "start new terminal" control once Herdr's workspace is gone.
    pub(super) fn retain_project_before_last_pane_closes(&mut self, pane_id: &str) {
        let Some(project) = self.snapshot.navigator.workspaces.iter().find(|workspace| {
            workspace.remote_target_id.is_none()
                && workspace
                    .checkouts
                    .iter()
                    .flat_map(|checkout| checkout.tabs.iter())
                    .flat_map(|tab| tab.panes.iter())
                    .any(|pane| pane.id == pane_id)
        }) else {
            return;
        };
        let pane_count = project
            .checkouts
            .iter()
            .flat_map(|checkout| checkout.tabs.iter())
            .map(|tab| tab.panes.len())
            .sum::<usize>();
        if project.registered || pane_count != 1 {
            return;
        }
        let registration =
            match workspace::registration(&project.path, &project.repo_name, &project.device_id) {
                Ok(registration) => registration,
                Err(message) => {
                    self.set_error("workspace.retain_failed", message, false);
                    return;
                }
            };
        if self
            .snapshot
            .ui_state
            .workspace_registrations
            .iter()
            .any(|existing| existing.id == registration.id)
        {
            return;
        }
        self.push_diagnostic(
            "workspace.retained",
            format!(
                "Registered {} at {} so closing its last pane keeps the project listed",
                registration.label, registration.path
            ),
        );
        self.snapshot
            .ui_state
            .workspace_registrations
            .push(registration);
        // The project id is path-keyed, so registering changes nothing the
        // sidebar shows right now; the sync that follows Herdr's
        // workspace_closed rebuilds the catalog off the runtime lock.
        self.persist_current_ui_state();
    }
}

#[cfg(test)]
mod purpose_version_tests {
    use super::*;

    #[test]
    fn remote_purpose_requires_the_pinned_minimum_version() {
        assert!(!herdr_version_supports_purpose(None));
        assert!(!herdr_version_supports_purpose(Some("0.9.0")));
        assert!(herdr_version_supports_purpose(Some("0.9.1")));
        assert!(herdr_version_supports_purpose(Some("v0.10.0")));
        assert!(herdr_version_supports_purpose(Some("1.0.0-beta.1")));
        assert!(!herdr_version_supports_purpose(Some("unknown")));
    }
}
