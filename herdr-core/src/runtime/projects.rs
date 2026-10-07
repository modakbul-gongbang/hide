use super::*;

const MINIMUM_PURPOSE_HERDR_VERSION: (u64, u64, u64) = (0, 9, 1);

/// How long a project's pull requests are trusted before the core asks again.
const GITHUB_REREAD: std::time::Duration = std::time::Duration::from_secs(5 * 60);

/// How long a project waits after its first failed read; each further failure
/// in a row doubles it, up to `GITHUB_REREAD`.
const GITHUB_RETRY_FIRST: std::time::Duration = std::time::Duration::from_secs(30);

/// A project's place in the re-read cycle. The wait runs from the answer, not
/// from the ask: counted from the ask, a pass over many projects that
/// outlasts it would be overtaken by its own next ask and never land.
///
/// `failures` is how many reads in a row ended without the project's pull
/// requests (the answer's `pull_requests_read` was false), so a read that
/// failed is asked again sooner than one that worked and a success starts the
/// count over.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum GithubClock {
    /// A read is asked for and has not answered.
    Waiting { failures: u32 },
    /// The read answered at this instant.
    Answered {
        at: std::time::Instant,
        failures: u32,
    },
}

impl GithubClock {
    fn failures(self) -> u32 {
        match self {
            Self::Waiting { failures } | Self::Answered { failures, .. } => failures,
        }
    }
}

/// The wait after an answer that came after `failures` failed reads in a row:
/// the full re-read for a success, else 30 s doubling per failure and stopping
/// at the full re-read.
fn github_wait(failures: u32) -> std::time::Duration {
    match failures {
        0 => GITHUB_REREAD,
        failures => GITHUB_RETRY_FIRST
            .saturating_mul(1u32.checked_shl(failures - 1).unwrap_or(u32::MAX))
            .min(GITHUB_REREAD),
    }
}

/// The most sighted addresses remembered at once as having set off their read;
/// one past it sets off nothing and is a reported shortfall.
const SIGHTED_ASKED_LIMIT: usize = 64;

/// A pull request address a session printed that set off a read, or waits to
/// (`read_sighted_pull_requests`).
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct Sighted {
    /// Until when it is recent enough to read for, on the clock the re-read
    /// runs on.
    fresh_until: std::time::Instant,
    /// The projects whose ask was unanswered when it was printed, each owed a
    /// read once that answer lands if the answer does not hold it.
    waiting: BTreeSet<String>,
}

/// What a sighting may do to one project now.
enum SightedRead {
    /// The project's last read answered and worked, and nothing has asked
    /// since: read it again now.
    Now,
    /// An ask is outstanding (a read in flight, the first not yet answered,
    /// or an answer the clock has not taken yet): asking again would overtake
    /// it, so wait for its answer.
    AfterAnswer,
    /// The last read failed; the project's own retry comes within five
    /// minutes and reads the pull request then.
    Never,
}

/// The most local Git projects the core reads GitHub for. Every registered one
/// is read, and each costs `gh` calls every `GITHUB_REREAD`; a count past this
/// is a reported shortfall, not a larger number.
const GITHUB_PROJECT_LIMIT: usize = 64;

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

/// A project's size as its facts line reads it: the catalog's sum once every
/// part is measured, the reason when a part could not be, and `measuring`
/// while the project is the one named for measuring and neither has come
/// back yet.
fn project_disk(
    project: Option<&crate::model::ProjectWorktreesSnapshot>,
    named: bool,
) -> crate::model::ProjectDiskSnapshot {
    let total_bytes = project.and_then(|project| project.disk_total_bytes);
    let unavailable_reason = project.and_then(|project| project.disk_unavailable_reason.clone());
    let free_bytes = project.and_then(|project| {
        project
            .worktrees
            .iter()
            .map(|worktree| &worktree.disk)
            .chain(std::iter::once(&project.shared_git_disk))
            .find_map(|disk| disk.volume_free_bytes)
    });
    // The layers of every checkout that was measured: one that could not be
    // read leaves the subtotal, not the whole breakdown.
    let layers = project.and_then(|project| {
        let mut sum = crate::model::ProjectDiskLayersSnapshot {
            shared_git: project.shared_git_disk.total_bytes.unwrap_or(0),
            ..Default::default()
        };
        let mut any = false;
        for layers in project
            .worktrees
            .iter()
            .filter_map(|worktree| worktree.disk.layers.as_ref())
        {
            any = true;
            sum.build_cache += layers.build_cache.bytes;
            sum.dependencies += layers.dependencies.bytes;
            sum.other += layers.other.bytes;
            sum.source += layers.source_bytes;
        }
        any.then_some(sum)
    });
    crate::model::ProjectDiskSnapshot {
        measuring: named && total_bytes.is_none() && unavailable_reason.is_none(),
        total_bytes,
        unavailable_reason,
        free_bytes,
        confirmed_bytes: project.and_then(|project| project.disk_confirmed_bytes),
        layers,
    }
}

impl Runtime {
    /// What the changes view needs read, or `None` while nothing on screen
    /// shows it. The checkout in front may be on this machine or on a device;
    /// either is read by its own host. The diffs the front Workspace's View
    /// areas show ride the same read (PRD S7 A5).
    pub fn changes_request(&mut self) -> Option<crate::changes::ChangesRequest> {
        let (workspace_id, checkout_id, root_path) = self.changes_target()?;
        let root = self.document_root(&workspace_id, &checkout_id).ok()?;
        let channel = self
            .changes_channel(&root.device_id)
            .map(crate::changes::ChannelRef);
        let (selected_path, selected_committed) = self.changes_selection();
        // The base comes from the checkout row, so the committed group and
        // the card's `↑A ↓B` are measured against the same branch; a device
        // checkout has none, and its host uses the repository default.
        let base_branch = self
            .catalog_checkout(&workspace_id, &checkout_id)
            .and_then(|(_, checkout)| checkout.base_branch.clone());
        Some(crate::changes::ChangesRequest {
            root,
            channel,
            root_path,
            selected_path,
            selected_committed,
            base_branch,
            diffs: self.visible_view_diffs(),
        })
    }

    /// The row whose diff the next read takes, and its group: the diff in
    /// front, otherwise the row History has selected.
    fn changes_selection(&self) -> (Option<String>, bool) {
        let active_diff = self.active_diff_tab();
        let selected_path = active_diff
            .map(|tab| tab.path.clone())
            .or_else(|| self.snapshot.changes.selected_path.clone());
        let selected_committed = active_diff
            .and_then(|tab| tab.diff_committed)
            .unwrap_or(self.snapshot.changes.selected_committed);
        (selected_path, selected_committed)
    }

    /// The device and folder the changes view is about now, the fence every
    /// answer passes.
    pub(crate) fn changes_key(&self) -> Option<crate::changes::ChangesKey> {
        self.changes_target()?;
        self.front_changes_key()
    }

    /// The device and folder History describes for the checkout in front,
    /// whether or not anything shows it now.
    fn front_changes_key(&self) -> Option<crate::changes::ChangesKey> {
        let (workspace_id, checkout_id) = self.front_checkout()?;
        let (workspace, _) = self.catalog_checkout(workspace_id, checkout_id)?;
        Some(crate::changes::ChangesKey {
            device_id: workspace.device_id.clone(),
            root_path: self.focused_changes_root_path()?,
        })
    }

    fn active_diff_tab(&self) -> Option<&EditorTabSnapshot> {
        self.snapshot
            .editor
            .active_tab_id
            .as_deref()
            .and_then(|tab_id| {
                self.snapshot
                    .editor
                    .tabs
                    .iter()
                    .find(|tab| tab.id == tab_id && tab.kind == EditorTabKind::Diff)
            })
    }

    fn changes_target(&self) -> Option<(String, String, String)> {
        let section_visible = |section| {
            self.snapshot.ui_state.right_panel_visible
                && self.snapshot.ui_state.right_panel_section == section
        };
        if !section_visible(RightPanelSection::Changes)
            && !section_visible(RightPanelSection::Explorer)
            && self.active_diff_tab().is_none()
            && self.visible_view_diffs().is_empty()
            && !self.new_tab_visible()
        {
            return None;
        }
        let (workspace_id, checkout_id) = self.front_checkout_owned()?;
        let root_path = self.focused_changes_root_path()?;
        Some((workspace_id, checkout_id, root_path))
    }

    /// The host the changes view reads through. A device's helper is asked
    /// for once when the view first needs it; a helper that failed is not
    /// asked again on every refresh, and the view says why until the
    /// operator's next action retries it.
    fn changes_channel(
        &mut self,
        device_id: &str,
    ) -> Result<Arc<dyn crate::host_access::HostChannel>, String> {
        if device_id != workspace::LOCAL_DEVICE_ID && !self.device_hosts.contains_key(device_id) {
            self.start_device_host(device_id);
        }
        match self.device_hosts.get(device_id).map(|host| &host.phase) {
            _ if device_id == workspace::LOCAL_DEVICE_ID => Ok(Arc::clone(&self.local_host)),
            Some(hosts::HostPhase::Ready { host, .. }) if host.closed_reason().is_none() => {
                Ok(Arc::clone(host))
            }
            _ => Err(self
                .host_snapshot(device_id)
                .message
                .unwrap_or_else(|| "The device helper is not ready".to_owned())),
        }
    }

    /// A registration can name a folder inside its Git checkout. The
    /// navigator workspace path may have been projected as the repository
    /// root after Herdr occupies it, so use the registration's own path.
    /// Linked worktree rows use their own checkout root.
    pub(super) fn focused_changes_root_path(&self) -> Option<String> {
        let (workspace_id, checkout_id) = self.front_checkout()?;
        let (workspace, checkout) = self.catalog_checkout(workspace_id, checkout_id)?;
        if workspace.registered
            && let Some(registered) = self
                .snapshot
                .ui_state
                .workspace_registrations
                .iter()
                .find(|registration| registration.id == workspace.id)
            && hide_platform::path::wire_relative(&checkout.path, &registered.path).is_ok()
        {
            return Some(registered.path.clone());
        }
        Some(checkout.path.clone())
    }

    /// Keep the History identity in the same snapshot frame as checkout focus.
    /// All focus routes call this after assigning the focused workspace and
    /// checkout; remote checkouts clear the identity immediately.
    pub(super) fn sync_changes_root_path(&mut self) {
        self.snapshot.navigator.changes_root_path = self.focused_changes_root_path();
        // The same folder on another device is another checkout: a projection
        // read on the one left behind is dropped in this same frame rather
        // than shown under the new device until the next read lands (B22).
        let front = self.front_changes_key();
        if self.changes_published_key.is_some() && self.changes_published_key != front {
            self.snapshot
                .changes
                .set(crate::model::ChangesSnapshot::default());
            self.changes_published_key = None;
        }
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
                generation: self
                    .worktree_project_generations
                    .get(&workspace.path)
                    .copied()
                    .unwrap_or(0),
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
                        crate::github::preferred_per_branch(&project.pull_requests)
                            .into_iter()
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
            removals: self.worktree_removals,
        }
    }

    /// Stores a catalog read that started when `removals` local removals had
    /// settled. A checkout a later removal dropped stays dropped: the read
    /// ran before Git forgot it, and only a read that started after the
    /// removal is news about that path.
    pub fn ingest_worktrees(
        &mut self,
        catalog: crate::model::WorktreeCatalogSnapshot,
        removals: u64,
    ) -> bool {
        self.ingest_worktrees_answer(catalog, removals, true)
    }

    pub(crate) fn ingest_worktrees_answer(
        &mut self,
        mut catalog: crate::model::WorktreeCatalogSnapshot,
        removals: u64,
        current: bool,
    ) -> bool {
        // Keep the visible values on an Overview refresh. An initial read has
        // no values to preserve, so accept its slightly old answer and let the
        // already queued current read correct it; otherwise a moving request
        // can starve the first catalog entirely.
        if !current && !self.worktree_catalog.projects.is_empty() {
            return false;
        }
        self.removed_worktrees
            .retain(|(settled, _)| *settled > removals);
        for project in &mut catalog.projects {
            project.worktrees.retain(|worktree| {
                !self
                    .removed_worktrees
                    .iter()
                    .any(|(_, path)| *path == worktree.path)
            });
        }
        let loading = self.snapshot.git_worktrees_loading && !current;
        let changed =
            self.worktree_catalog != catalog || self.snapshot.git_worktrees_loading != loading;
        self.local_worktree_paths = catalog
            .projects
            .iter()
            .map(|project| {
                project
                    .worktrees
                    .iter()
                    .map(|worktree| session::PathRules::Local.read(&worktree.path))
                    .collect()
            })
            .collect();
        self.worktree_catalog = catalog;
        self.snapshot.git_worktrees_loading = loading;
        self.refresh_worktree_projection();
        // A HEAD the reader moved decides again which pull request a checkout
        // holds, and what hangs off that (its issues, its request rows).
        self.apply_pull_requests();
        self.refresh_worktree_projection();
        changed
    }

    /// The local Git project a cleanup names, or `None` for anything else.
    fn cleanup_workspace(&self, workspace_id: &str) -> Option<&crate::model::WorkspaceSnapshot> {
        self.snapshot.navigator.workspaces.iter().find(|workspace| {
            workspace.id == workspace_id && workspace.remote_target_id.is_none() && workspace.is_git
        })
    }

    fn checkout_facts(
        &self,
        workspace: &crate::model::WorkspaceSnapshot,
    ) -> Vec<live::cleanup::CheckoutFacts> {
        use live::cleanup::AgentUse;
        // A delegated agent can run on a connected device, so its state is
        // read from the device's own rows too.
        let agents: HashMap<&str, &crate::model::SidebarAgentSnapshot> = self
            .snapshot
            .navigator
            .agents
            .iter()
            .chain(
                self.snapshot
                    .status
                    .remote
                    .iter()
                    .filter_map(|status| status.session.as_ref())
                    .flat_map(|session| session.agents.iter()),
            )
            .map(|agent| (agent.pane_id.as_str(), agent))
            .collect();
        let state = |agent: &crate::model::SidebarAgentSnapshot| {
            AgentUse::of(&agent.demand, agent.blocked, &agent.activity)
        };
        workspace
            .checkouts
            .iter()
            .map(|checkout| {
                let panes: Vec<String> = checkout
                    .tabs
                    .iter()
                    .flat_map(|tab| tab.panes.iter())
                    .map(|pane| pane.id.clone())
                    .collect();
                let mut facts = live::cleanup::CheckoutFacts {
                    path: PathBuf::from(&checkout.path),
                    ..Default::default()
                };
                for pane in &panes {
                    let Some(agent) = agents.get(pane.as_str()) else {
                        facts.terminal_panes.push(pane.clone());
                        continue;
                    };
                    match state(agent) {
                        AgentUse::Quiet => {}
                        AgentUse::Working => facts.agent_working += 1,
                        AgentUse::Waiting => facts.agent_waiting += 1,
                        AgentUse::Unknown => facts.agent_unknown += 1,
                    }
                    // A delegated agent keeps its parent's checkout in use
                    // wherever it runs, and a device that went away leaves its
                    // last state, not an idle one. Ones in this checkout are
                    // counted above, and an unknown one is not vouched for
                    // either way. The children are walked rather than the
                    // close list, which leaves out unreachable devices.
                    let mut seen: HashSet<&str> = HashSet::new();
                    let mut pending: Vec<&str> = agent
                        .lineage_child_pane_ids
                        .iter()
                        .map(String::as_str)
                        .collect();
                    while let Some(id) = pending.pop() {
                        if !seen.insert(id) {
                            continue;
                        }
                        let Some(descendant) = agents.get(id) else {
                            continue;
                        };
                        pending
                            .extend(descendant.lineage_child_pane_ids.iter().map(String::as_str));
                        if !panes.iter().any(|own| own == id)
                            && matches!(state(descendant), AgentUse::Working | AgentUse::Waiting)
                        {
                            facts.descendants_busy += 1;
                        }
                    }
                }
                facts.panes = panes;
                facts
            })
            .collect()
    }

    /// Only the in-use facts of a local Git project's checkouts, for a read
    /// that happens once per move: none of the folder or size maps
    /// [`Self::cleanup_input`] copies.
    pub(crate) fn cleanup_facts(
        &self,
        workspace_id: &str,
    ) -> Option<Vec<live::cleanup::CheckoutFacts>> {
        Some(self.checkout_facts(self.cleanup_workspace(workspace_id)?))
    }

    /// What the cleanup worker needs to know about a local Git project's
    /// checkouts, copied under the lock so the worker reads nothing of the
    /// runtime while it inspects the disk and Herdr.
    pub(crate) fn cleanup_input(&self, workspace_id: &str) -> Option<live::cleanup::ReviewInput> {
        let workspace = self.cleanup_workspace(workspace_id)?;
        let worktrees = self
            .worktree_catalog
            .project(&workspace.path)
            .into_iter()
            .flat_map(|project| &project.worktrees);
        Some(live::cleanup::ReviewInput {
            workspace_id: workspace.id.clone(),
            root: PathBuf::from(&workspace.path),
            current: self
                .focused_local_checkout()
                .filter(|(focused, _)| focused.id == workspace.id)
                .map(|(_, checkout)| PathBuf::from(&checkout.path)),
            checkouts: self.checkout_facts(workspace),
            folders: worktrees
                .clone()
                .filter(|worktree| !worktree.disk.folders.is_empty())
                .map(|worktree| (PathBuf::from(&worktree.path), worktree.disk.folders.clone()))
                .collect(),
            bytes: worktrees
                .filter_map(|worktree| {
                    Some((PathBuf::from(&worktree.path), worktree.disk.total_bytes?))
                })
                .collect(),
        })
    }

    /// The worker of cleanup `id` has ended, however it ended.
    pub(crate) fn cleanup_worker_finished(&mut self, id: u64) {
        if self.cleanup_worker == Some(id) {
            self.cleanup_worker = None;
        }
    }

    pub(crate) fn ingest_cleanup(&mut self, answer: live::cleanup::CleanupSnapshot) -> bool {
        if self
            .cleanup
            .as_ref()
            .is_none_or(|current| current.id != answer.id)
        {
            return false;
        }
        // What was removed is news for the catalog and the sizes once the run
        // has finished, not on every progress step.
        let removed = answer.phase == "complete"
            && (answer
                .rows
                .iter()
                .any(|row| row.result.as_deref() == Some("removed"))
                || answer
                    .cell_results
                    .iter()
                    .any(|cell| cell.outcome == "removed"));
        self.cleanup = Some(answer);
        if removed {
            self.refresh_worktrees();
            self.remeasure_disk();
        }
        self.refresh_worktree_projection();
        true
    }

    pub(super) fn review_cleanup(&mut self, workspace_id: &str) -> bool {
        if self.cleanup_worker.is_some()
            || self
                .cleanup
                .as_ref()
                .is_some_and(|r| matches!(r.phase.as_str(), "loading" | "removing"))
        {
            return false;
        }
        let Some(input) = self.cleanup_input(workspace_id) else {
            crate::diagnostic!(serde_json::json!({
                "component": "cleanup",
                "kind": "review.not_local_git",
                "workspace_id": workspace_id,
            }));
            return false;
        };
        self.next_cleanup_id = self.next_cleanup_id.wrapping_add(1).max(1);
        let mut review = live::cleanup::CleanupSnapshot {
            id: self.next_cleanup_id,
            workspace_id: workspace_id.to_owned(),
            repository_root: hide_platform::path::to_wire_lossy(&input.root),
            phase: "loading".into(),
            ..Default::default()
        };
        self.cleanup = Some(review.clone());
        // The sizes the sheet shows come from the reader the facts line uses;
        // reviewing again measures again.
        self.measure_project_disk(workspace_id);
        let started = self.live.clone().ok_or_else(|| "A live Herdr connection is required to verify what is in use. Connect and review again.".into())
            .and_then(|context| live::cleanup::spawn_review(context, review.id, input));
        if started.is_ok() {
            self.cleanup_worker = Some(review.id);
        }
        if let Err(message) = started {
            review.phase = "failed".into();
            review.usage_error = Some(message.clone());
            review.message = Some(message);
            self.cleanup = Some(review);
        }
        self.refresh_worktree_projection();
        true
    }

    pub(super) fn confirm_cleanup(&mut self, payload: CleanupConfirmPayload) -> bool {
        // A review worker that has published its answer may not have ended
        // yet; a confirmation waits for it, so its end can never free the
        // lane of the worker this one starts.
        if self.cleanup_worker.is_some() {
            return false;
        }
        let Some(review) = self.cleanup.clone().filter(|r| {
            r.id == payload.id && r.phase == "review" && r.usage_error.is_none() && r.usage_ready
        }) else {
            return false;
        };
        // A confirmation names at most every row and each row's two layers;
        // anything past that is not from this review, and is not filtered.
        let limit = review.rows.len().saturating_mul(2);
        let mut paths: Vec<_> = payload
            .paths
            .into_iter()
            .take(limit)
            .filter(|path| {
                review
                    .rows
                    .iter()
                    .any(|row| row.path == *path && row.exclusion.is_none() && row.in_use.is_none())
            })
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        paths.sort();
        // A worktree goes with everything in it, so its own cells are not
        // emptied one by one.
        let mut cells: Vec<live::cleanup::CellChoice> = Vec::new();
        for cell in payload.cells.into_iter().take(limit) {
            let Some(layer) = crate::disk_layers::Layer::from_code(&cell.layer) else {
                continue;
            };
            let choice = live::cleanup::CellChoice {
                path: cell.path,
                layer,
            };
            if !paths.contains(&choice.path)
                && !cells.contains(&choice)
                && review
                    .rows
                    .iter()
                    .any(|row| row.path == choice.path && row.in_use.is_none())
            {
                cells.push(choice);
            }
        }
        if paths.is_empty() && cells.is_empty() {
            return false;
        }
        let Some(input) = self.cleanup_input(&review.workspace_id) else {
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
                live::cleanup::spawn_confirm(context, review.clone(), input, paths, cells)
            });
        if started.is_ok() {
            self.cleanup_worker = Some(review.id);
        }
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
            .map(|p| hide_platform::path::to_wire_lossy(&p.root))
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
            // One per branch, as the catalog always carried; a checkout's own
            // pull request is on its row (`associate_pull_requests`).
            project.pull_requests = github_project
                .map(|g| {
                    crate::github::preferred_per_branch(&g.pull_requests)
                        .into_iter()
                        .cloned()
                        .collect()
                })
                .unwrap_or_default();
            project.pull_request_window = crate::github::PULL_REQUEST_LIMIT.into();
            for worktree in &mut project.worktrees {
                let panes = pane_rows.get(&worktree.path);
                worktree.pane_count = panes.map_or(0, HashSet::len);
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
                worktree.pull_request = github_project.and_then(|project| {
                    crate::github::pull_request_for_checkout(
                        &project.pull_requests,
                        worktree.branch.as_deref(),
                        worktree.head_sha.as_deref(),
                    )
                    .cloned()
                });
                worktree.deletion_gate = crate::worktrees::deletion_gate(
                    worktree,
                    worktree.branch == project.base_branch && project.base_branch.is_some(),
                    worktree.pane_count,
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

        let link_summaries = self
            .snapshot
            .link_summaries
            .as_ref()
            .map(|summaries| &summaries.projects);
        for workspace in &mut self.snapshot.navigator.workspaces {
            if workspace.remote_target_id.is_some() {
                continue;
            }
            workspace::apply_worktrees(workspace, &self.worktree_catalog);
            // The commit a checkout is on is what ties a settled pull request
            // to it, so a catalog read that moved a HEAD decides again here,
            // and whether its work landed follows the same read.
            let project = github.project(&workspace.path);
            associate_pull_requests(workspace, project);
            super::links::associate_landed(
                workspace,
                link_summaries.and_then(|summaries| summaries.get(&workspace.id)),
            );
            // Git refreshes the persistent purpose sources. Restore the
            // agent/PR fallback in this same projection before publishing it.
            crate::sidebar::sync_checkout_purposes(
                std::slice::from_mut(workspace),
                &self.snapshot.navigator.agents,
            );
            let named = self.disk_project.as_deref() == Some(workspace.path.as_str());
            workspace.disk = project_disk(self.worktree_catalog.project(&workspace.path), named);
            workspace.cleanup = self
                .cleanup
                .as_ref()
                .filter(|review| review.workspace_id == workspace.id)
                .cloned();
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
        self.refresh_card();
        self.refresh_browser_inventory_scope()
            || before_catalog != self.worktree_catalog
            || before_navigator != self.snapshot.navigator
            || before_git != self.snapshot.git_worktrees
            || before_remote != self.snapshot.git_worktrees_remote
    }

    pub fn worktree_catalog(&self) -> crate::model::WorktreeCatalogSnapshot {
        self.worktree_catalog.clone()
    }

    /// Every local Git project, whether or not a screen that shows its pull
    /// requests is open: the sidebar row is such a screen. At most
    /// `GITHUB_PROJECT_LIMIT` of them: the project in front first, then those
    /// a screen named, then the rest, each group in path order. The chosen are
    /// returned in path order so the request does not move when the projects'
    /// activity order does; the second value is how many local Git projects
    /// there are in all.
    fn github_projects(&self) -> (Vec<&WorkspaceSnapshot>, usize) {
        let in_front = self
            .focused_local_checkout()
            .map(|(workspace, _)| workspace.path.as_str());
        let mut projects: Vec<&WorkspaceSnapshot> = self
            .snapshot
            .navigator
            .workspaces
            .iter()
            .filter(|workspace| workspace.remote_target_id.is_none() && workspace.is_git)
            .collect();
        let total = projects.len();
        projects.sort_by(|left, right| {
            let rank = |workspace: &WorkspaceSnapshot| {
                if Some(workspace.path.as_str()) == in_front {
                    0
                } else if self.github_wanted.contains(&workspace.path) {
                    1
                } else {
                    2
                }
            };
            (rank(left), &left.path).cmp(&(rank(right), &right.path))
        });
        projects.truncate(GITHUB_PROJECT_LIMIT);
        projects.sort_by(|left, right| left.path.cmp(&right.path));
        (projects, total)
    }

    pub fn github_request(&self) -> crate::github::GithubRequest {
        crate::github::GithubRequest {
            projects: self
                .github_projects()
                .0
                .into_iter()
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

    /// Asks again for each project whose last answer is `github_wait` old (the full
    /// re-read after a success, sooner after a failed read),
    /// by moving its generation; the reader re-reads exactly the projects
    /// whose generation moved. A project is not asked again while its last ask
    /// is unanswered, so a slow pass is never overtaken by the next one. A
    /// project seen for the first time is not asked for here: the reader has
    /// no answer for it, so its first read is already due.
    pub(crate) fn reread_stale_github(&mut self, now: std::time::Instant) {
        let (projects, total) = self.github_projects();
        let paths: Vec<String> = projects
            .into_iter()
            .map(|workspace| workspace.path.clone())
            .collect();
        let over_limit = total.saturating_sub(GITHUB_PROJECT_LIMIT);
        if over_limit != self.github_over_limit {
            self.github_over_limit = over_limit;
            if over_limit > 0 {
                crate::diagnostic!(serde_json::json!({
                    "component": "github",
                    "kind": "projects.over_limit",
                    "limit": GITHUB_PROJECT_LIMIT,
                    "local_git_projects": total,
                    "not_read": over_limit,
                }));
            }
        }
        let registered: HashSet<&str> = self
            .snapshot
            .navigator
            .workspaces
            .iter()
            .map(|workspace| workspace.path.as_str())
            .collect();
        self.github_wanted
            .retain(|path| registered.contains(path.as_str()));
        let known = |path: &String| paths.contains(path);
        self.github_clock.retain(|path, _| known(path));
        self.github_answered.retain(|path, _| known(path));
        self.github_read_generation.retain(|path, _| known(path));
        self.github_settled.retain(known);
        // A sighting that grew old while its project's answer was owed (no
        // window attached, no read) sets off nothing when the answer lands.
        self.github_sighted
            .retain(|_, kept| now <= kept.fresh_until);
        for kept in self.github_sighted.values_mut() {
            kept.waiting.retain(known);
        }
        for path in paths {
            let before = self.github_clock.get(&path).copied();
            if let Some(failed) = self.github_answered.remove(&path) {
                let failures = if failed {
                    before.map_or(0, GithubClock::failures).saturating_add(1)
                } else {
                    0
                };
                self.github_clock
                    .insert(path.clone(), GithubClock::Answered { at: now, failures });
                if self.sighted_owed_after_answer(&path, failed) {
                    self.ask_github_again(&path);
                }
                continue;
            }
            match before {
                None => {
                    self.github_clock
                        .insert(path, GithubClock::Waiting { failures: 0 });
                }
                Some(GithubClock::Answered { at, failures })
                    if now.duration_since(at) >= github_wait(failures) =>
                {
                    if failures > 0 {
                        crate::diagnostic!(serde_json::json!({
                            "component": "github",
                            "kind": "read.retry",
                            "project": path,
                            "attempt": failures,
                            "delay_ms": github_wait(failures).as_millis(),
                        }));
                    }
                    self.github_clock
                        .insert(path.clone(), GithubClock::Waiting { failures });
                    self.bump_github_generation(&path);
                }
                Some(_) => {}
            }
        }
    }

    /// Reads GitHub again for each project a session just printed the address
    /// of a pull request in that the last answer does not hold, so a pull
    /// request an agent opens reaches its row without waiting for the
    /// five-minute re-read. The read is the existing one: the project's
    /// generation moves and its clock waits for the answer, as the stale
    /// re-read does, so a project is still never asked again while an ask is
    /// unanswered; a sighting that finds one waits for that answer and reads
    /// only if the answer does not hold it, and a project whose last read
    /// failed is left to its own retry. An address sets off its read once
    /// while it is recent; one the read still does not return (another
    /// repository, past the newest 200) is not asked for again. The read is
    /// quiet: the row does not show `loading` for it.
    pub(crate) fn read_sighted_pull_requests(
        &mut self,
        sighted: &[crate::labels::worker::SightedPullRequest],
        (now, now_unix_ms): (std::time::Instant, u64),
    ) {
        self.github_sighted
            .retain(|_, kept| now <= kept.fresh_until);
        let candidates = self.sighted_candidates();
        let mut read = BTreeSet::new();
        for sighting in sighted {
            let address = (sighting.repository.clone(), sighting.number);
            // Every pull request read is keyed here by its address, so a
            // sighting it holds needs no read.
            let Some(fresh_for) =
                crate::labels::worker::sighting_fresh_for(sighting.at_unix_ms, now_unix_ms)
            else {
                continue;
            };
            if self.pull_request_times.contains_key(&address)
                || self.github_sighted.contains_key(&address)
            {
                continue;
            }
            let projects = self.sighted_projects(&candidates, sighting);
            if projects.is_empty() {
                // Not remembered: an address no read project owns must not
                // take a place a project's own pull request needs.
                crate::diagnostic!(serde_json::json!({
                    "component": "github",
                    "kind": "read.sighted_unowned",
                    "repository": sighting.repository,
                    "number": sighting.number,
                    "pane_id": sighting.pane_id,
                }));
                continue;
            }
            if self.github_sighted.len() >= SIGHTED_ASKED_LIMIT {
                crate::diagnostic!(serde_json::json!({
                    "component": "github",
                    "kind": "read.sighted_over_limit",
                    "limit": SIGHTED_ASKED_LIMIT,
                    "repository": sighting.repository,
                    "number": sighting.number,
                }));
                continue;
            }
            let mut kept = Sighted {
                fresh_until: now + fresh_for,
                waiting: BTreeSet::new(),
            };
            let mut read_now = Vec::new();
            for path in projects {
                match self.sighted_read(&path) {
                    SightedRead::Now => read_now.push(path),
                    SightedRead::AfterAnswer => {
                        kept.waiting.insert(path);
                    }
                    SightedRead::Never => {}
                }
            }
            crate::diagnostic!(serde_json::json!({
                "component": "github",
                "kind": "read.sighted",
                "repository": sighting.repository,
                "number": sighting.number,
                "pane_id": sighting.pane_id,
                "read": read_now,
                "after_answer": kept.waiting,
            }));
            read.extend(read_now);
            self.github_sighted.insert(address, kept);
        }
        for path in read {
            self.ask_github_again(&path);
        }
    }

    /// The read local projects by path, each with the repository its pull
    /// requests name, if it has one yet.
    fn sighted_candidates(&self) -> Vec<(String, Option<String>)> {
        self.github_projects()
            .0
            .into_iter()
            .map(|workspace| {
                let repository = self.github.project(&workspace.path).and_then(|project| {
                    project
                        .pull_requests
                        .iter()
                        .find_map(|pull_request| pull_request_address(&pull_request.url))
                        .map(|(repository, _)| repository)
                });
                (workspace.path.clone(), repository)
            })
            .collect()
    }

    /// The read local projects a sighted pull request belongs to: those whose
    /// pull requests are in its repository; when none is, the project of the
    /// pane whose session printed it, unless that project's pull requests
    /// name another repository. A repository's first pull request has no
    /// earlier one to name it, and the session that made it works in it; a
    /// fork's checkout whose agent opens a pull request upstream is therefore
    /// not read for it.
    fn sighted_projects(
        &self,
        candidates: &[(String, Option<String>)],
        sighting: &crate::labels::worker::SightedPullRequest,
    ) -> Vec<String> {
        let matching: Vec<String> = candidates
            .iter()
            .filter(|(_, repository)| repository.as_deref() == Some(sighting.repository.as_str()))
            .map(|(path, _)| path.clone())
            .collect();
        if !matching.is_empty() {
            return matching;
        }
        let printed_in = self
            .snapshot
            .navigator
            .workspaces
            .iter()
            .filter(|workspace| workspace.remote_target_id.is_none())
            .find(|workspace| {
                workspace.checkouts.iter().any(|checkout| {
                    checkout
                        .tabs
                        .iter()
                        .any(|tab| tab.panes.iter().any(|pane| pane.id == sighting.pane_id))
                })
            })
            .map(|workspace| workspace.path.as_str());
        candidates
            .iter()
            .find(|(path, repository)| Some(path.as_str()) == printed_in && repository.is_none())
            .map(|(path, _)| vec![path.clone()])
            .unwrap_or_default()
    }

    fn sighted_read(&self, path: &str) -> SightedRead {
        // Whoever asked (the clock, a refresh, the request view, a sighting),
        // an ask is outstanding while the generation asked for is not the one
        // last answered, and until the clock takes an answer that landed.
        let asked = self.github_generations.get(path).copied().unwrap_or(0);
        let outstanding = self.github_read_generation.get(path) != Some(&asked)
            || self.github_answered.contains_key(path);
        match self.github_clock.get(path) {
            _ if outstanding => SightedRead::AfterAnswer,
            Some(GithubClock::Answered { failures: 0, .. })
                if self.github_settled.contains(path) =>
            {
                SightedRead::Now
            }
            Some(GithubClock::Answered { failures, .. }) if *failures > 0 => SightedRead::Never,
            _ => SightedRead::AfterAnswer,
        }
    }

    /// Takes `path` off every sighting waiting for its answer, and whether one
    /// of them is owed a read: the answer worked and does not hold it.
    fn sighted_owed_after_answer(&mut self, path: &str, failed: bool) -> bool {
        let times = &self.pull_request_times;
        let mut owed = false;
        for (address, kept) in &mut self.github_sighted {
            if kept.waiting.remove(path) && !failed && !times.contains_key(address) {
                owed = true;
            }
        }
        owed
    }

    /// Asks for `path` again and waits for that answer, as the stale re-read
    /// does, so nothing asks again while it is unanswered.
    fn ask_github_again(&mut self, path: &str) {
        self.github_clock
            .insert(path.to_owned(), GithubClock::Waiting { failures: 0 });
        self.bump_github_generation(path);
    }

    /// Whether `path` is read for GitHub and no read has answered for it yet.
    fn github_awaiting(&self, path: &str) -> bool {
        !self.github_settled.contains(path)
            && self
                .github_projects()
                .0
                .iter()
                .any(|workspace| workspace.path == path)
    }

    /// Restores what the last run saved, as stale until a read replaces it,
    /// and keeps the store to save every later answer into.
    pub(crate) fn install_github_store(
        &mut self,
        store: crate::github_store::GithubStore,
        restored: Option<crate::model::GithubSnapshot>,
    ) {
        if let Some(restored) = restored {
            self.github = restored;
            self.apply_pull_requests();
            self.refresh_worktree_projection();
        }
        self.github_store = Some(store);
    }

    pub fn ingest_github(&mut self, github: crate::model::GithubSnapshot) -> bool {
        self.ingest_github_answer(github, true)
    }

    pub(crate) fn ingest_github_answer(
        &mut self,
        github: crate::model::GithubSnapshot,
        current: bool,
    ) -> bool {
        if !current {
            return false;
        }
        // An empty answer to an empty request says nothing: the navigator has
        // not arrived yet, and taking it would erase the answer restored from
        // the last run and the file it was saved in.
        let requested = self.github_request();
        if requested.projects.is_empty() && github.projects.is_empty() {
            return false;
        }
        // The answer is for the request as it stands now, so every project in
        // it has been read once, including one that returned nothing.
        let mut newly_settled = false;
        for project in requested.projects {
            let path = project.root.to_string_lossy().into_owned();
            // The read's own provenance, taken before a failed one is filled
            // in from the previous answer below.
            let failed = github
                .project(&path)
                .is_some_and(|read| !read.pull_requests_read);
            // The reader hands back its cached entry for every project in the
            // request, so only a project whose generation moved since the
            // last answer was actually read; counting the others would
            // restart their wait, or climb a failing project's backoff,
            // because a neighbour was asked.
            if self
                .github_read_generation
                .insert(path.clone(), project.generation)
                != Some(project.generation)
            {
                self.github_answered.insert(path.clone(), failed);
            }
            newly_settled |= self.github_settled.insert(path);
        }
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
            // Dependencies and sub-issues that could not be read this pass keep the ones
            // read before, the same way a failed lookup keeps its answer.
            if project.issues.dependencies_failure.is_some()
                && let Some(previous) = self.github.project(&project.root_path)
            {
                for issue in &mut project.issues.issues {
                    if let Some(known) = previous
                        .issues
                        .issues
                        .iter()
                        .find(|known| known.reference == issue.reference)
                    {
                        issue.blocked_by = known.blocked_by.clone();
                        issue.sub_issues = known.sub_issues.clone();
                    }
                }
            }
        }
        if self.github == merged {
            // Nothing to show changed, but a project that returned nothing
            // stops loading.
            if newly_settled {
                self.apply_pull_requests();
            }
            return newly_settled;
        }
        self.github = merged;
        if let Some(store) = &self.github_store {
            store.save(self.github.clone());
        }
        self.apply_pull_requests();
        self.refresh_worktree_projection();
        true
    }

    fn bump_github_generation(&mut self, project_path: &str) {
        let generation = self
            .github_generations
            .entry(project_path.to_owned())
            .or_insert(0);
        *generation = generation.wrapping_add(1);
    }

    pub fn refresh_pull_requests(&mut self, project_path: &str) {
        self.bump_github_generation(project_path);
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

    pub(super) fn refresh_project_worktrees(&mut self, project_path: &str) {
        let generation = self
            .worktree_project_generations
            .entry(project_path.to_owned())
            .or_insert(0);
        *generation = generation.wrapping_add(1);
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
            && matches!(removal.phase.as_str(), "checking" | "closing" | "removing")
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
        if let Some(device) = payload
            .device_id
            .clone()
            .filter(|device| device != workspace::LOCAL_DEVICE_ID)
        {
            return self.remove_device_worktree(&device, payload);
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
        if !self.worktree_removal_allowed(&worktree, &payload) {
            return true;
        }
        let id = self.begin_worktree_removal(
            None,
            repository_root,
            protected_base_branch,
            worktree,
            &payload,
        );
        self.start_worktree_preflight(id, None, payload.close_descendant_pane_ids)
    }

    /// `remove_worktree` for a device's linked worktree (PRD S5.5 B28, B29):
    /// the same gate, receipt and phases as this machine's, with the device's
    /// Herdr closing its panes and its helper rechecking and removing.
    fn remove_device_worktree(&mut self, device: &str, payload: RemoveWorktreePayload) -> bool {
        let target = self.device_worktrees.get(device).and_then(|worktrees| {
            worktrees.projects.values().find_map(|project| {
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
            })
        });
        let Some((repository_root, protected_base_branch, worktree)) = target else {
            self.set_error(
                "worktree.remove_unknown",
                format!("Worktree is no longer listed: {}", payload.checkout_path),
                true,
            );
            self.request_device_worktrees(device, true);
            return true;
        };
        if !self.worktree_removal_allowed(&worktree, &payload) {
            return true;
        }
        if let Err(message) = self.device_worktree_target(device) {
            self.set_error(
                "worktree.remove_unavailable",
                format!("Deleting a worktree on this device needs its connection: {message}"),
                true,
            );
            return true;
        }
        let id = self.begin_worktree_removal(
            Some(device),
            repository_root,
            protected_base_branch,
            worktree,
            &payload,
        );
        self.start_worktree_preflight(
            id,
            Some(device.to_owned()),
            payload.close_descendant_pane_ids,
        )
    }

    fn start_worktree_preflight(
        &mut self,
        id: u64,
        device: Option<String>,
        outside: Vec<String>,
    ) -> bool {
        let context = match device.as_deref() {
            Some(device) => self.device_worktree_target(device),
            None => self.local_worktree_target(),
        };
        let result =
            context.and_then(|context| live::spawn_worktree_preflight(context, id, outside));
        if let Err(reason) = result {
            self.ingest_worktree_preflight_result(id, Vec::new(), Err(reason));
        }
        true
    }

    /// Only a successful host measurement admits any descendant or local close.
    pub(crate) fn ingest_worktree_preflight_result(
        &mut self,
        id: u64,
        outside: Vec<String>,
        result: Result<(), String>,
    ) -> bool {
        let Some(removal) = self
            .snapshot
            .worktree_removal
            .as_mut()
            .filter(|row| row.id == id && row.phase == "checking")
        else {
            return false;
        };
        if let Err(message) = result {
            removal.phase = "failed".to_owned();
            removal.message = Some(message);
            let device = removal.device_id.clone();
            match device.as_deref() {
                Some(device) => {
                    self.request_device_worktrees(device, true);
                }
                None => self.refresh_worktrees(),
            };
            return true;
        }
        removal.phase = "closing".to_owned();
        let device = removal.device_id.clone();
        let path = removal.checkout_path.clone();
        self.close_worktree_then_remove(id, device, path, outside)
    }

    /// The panes of a checkout a removal closes, by the ids its own Herdr
    /// uses: this machine's as they are, a device's without their scope.
    fn checkout_pane_ids(&self, device: Option<&str>, checkout_path: &str) -> Vec<String> {
        let workspaces = match device {
            Some(device) => self
                .snapshot
                .status
                .remote
                .iter()
                .find(|status| status.target_id == device)
                .and_then(|status| status.session.as_ref())
                .map(|session| session.workspaces.as_slice())
                .unwrap_or_default(),
            None => self.snapshot.navigator.workspaces.as_slice(),
        };
        workspaces
            .iter()
            .flat_map(|workspace| &workspace.checkouts)
            .filter(|checkout| checkout.path == checkout_path)
            .flat_map(|checkout| &checkout.tabs)
            .flat_map(|tab| &tab.panes)
            .filter_map(|pane| match device {
                Some(device) => super::remote_pane_source_id(device, &pane.id).map(str::to_owned),
                None => Some(pane.id.clone()),
            })
            .collect()
    }

    /// After the confirmed removal is recorded: the operator's chosen
    /// descendants outside the checkout close first when there are any
    /// (PRD close-agent-subtree D-36), then the checkout's own panes.
    fn close_worktree_then_remove(
        &mut self,
        id: u64,
        device: Option<String>,
        checkout_path: String,
        outside: Vec<String>,
    ) -> bool {
        if outside.is_empty() {
            return self.close_worktree_panes(id, device, checkout_path);
        }
        let inside = self
            .checkout_pane_ids(device.as_deref(), &checkout_path)
            .into_iter()
            .map(|pane| match device.as_deref() {
                Some(device) => crate::session_sync::remote_pane_id(device, &pane),
                None => pane,
            })
            .collect::<HashSet<_>>();
        self.close_descendants_before_removal(
            &inside,
            outside,
            super::tree_close::TreeFinal::Worktree {
                removal_id: id,
                device,
                checkout_path,
            },
        )
    }

    /// Closes a recorded removal's checkout panes on the close worker, which
    /// then carries the removal out.
    pub(super) fn close_worktree_panes(
        &mut self,
        id: u64,
        device: Option<String>,
        checkout_path: String,
    ) -> bool {
        let pane_ids = self.checkout_pane_ids(device.as_deref(), &checkout_path);
        crate::diagnostic!(serde_json::json!({
            "component": "worktree_removal",
            "kind": "close_requested",
            "target": device.as_deref().unwrap_or(workspace::LOCAL_DEVICE_ID),
            "id": id,
            "pane_count": pane_ids.len(),
        }));
        let context = match device.as_deref() {
            Some(device) => self.device_worktree_target(device),
            None => self.local_worktree_target(),
        };
        let context = match context {
            Ok(context) => context,
            Err(message) => {
                return self.ingest_worktree_close_result(
                    id,
                    &[],
                    Err(format!(
                        "Deleting a worktree needs a live Herdr connection: {message}"
                    )),
                );
            }
        };
        if let Err(message) = live::spawn_worktree_close(context, id, checkout_path, pane_ids) {
            self.ingest_worktree_close_result(id, &[], Err(message));
        }
        true
    }

    /// The gate at the moment of confirmation: the main worktree is refused,
    /// and a folder whose loss the operator did not accept is refused with
    /// what changed, since the confirmation they saw may be older than the row.
    fn worktree_removal_allowed(
        &mut self,
        worktree: &crate::model::WorktreeSnapshot,
        payload: &RemoveWorktreePayload,
    ) -> bool {
        if let Some(reason) = worktree.deletion_gate.blocked_reason.as_ref() {
            self.set_error("worktree.remove_blocked", reason.clone(), true);
            return false;
        }
        if worktree.ignored_repositories != payload.expected_ignored_repositories {
            self.set_error("worktree.remove_unaccepted", "Not deleted: the ignored repository list changed. Refresh and review every repository before choosing Discard.".to_owned(), true);
            return false;
        }
        if let Some(label) = worktree.deletion_gate.discard_label.as_ref()
            && !payload.discard_changes
        {
            self.set_error(
                "worktree.remove_unaccepted",
                format!("Not deleted: deleting it would lose work. Tick \"{label}\" to delete it anyway."),
                true,
            );
            return false;
        }
        true
    }

    /// Records the confirmed removal the close worker then carries out, with
    /// the operator's choices as the gate allowed them.
    fn begin_worktree_removal(
        &mut self,
        device: Option<&str>,
        repository_root: String,
        protected_base_branch: Option<String>,
        worktree: crate::model::WorktreeSnapshot,
        payload: &RemoveWorktreePayload,
    ) -> u64 {
        let gate = &worktree.deletion_gate;
        let delete_branch = payload.delete_branch && gate.can_delete_branch;
        self.next_worktree_removal_id = self.next_worktree_removal_id.wrapping_add(1).max(1);
        let id = self.next_worktree_removal_id;
        self.snapshot.worktree_removal = Some(crate::model::WorktreeRemovalSnapshot {
            id,
            device_id: device.map(str::to_owned),
            repository_root,
            checkout_path: payload.checkout_path.clone(),
            expected_head_sha: worktree.head_sha.clone(),
            expected_branch: worktree.branch.clone(),
            // A worktree already on the base was confirmed with that warning;
            // the recheck guards only against the branch becoming the base.
            protected_base_branch: protected_base_branch
                .filter(|base| worktree.branch.as_deref() != Some(base.as_str())),
            delete_branch,
            force_delete_branch: delete_branch && gate.branch_warning.is_some(),
            discard_changes: payload.discard_changes && gate.discard_label.is_some(),
            expected_ignored_repositories: payload.expected_ignored_repositories.clone(),
            branch: worktree.branch,
            phase: "checking".to_owned(),
            message: None,
        });
        id
    }

    /// One local registration choice; no filesystem or Herdr work under the lock.
    pub(super) fn set_primary_checkout(&mut self, payload: SetPrimaryCheckoutPayload) -> bool {
        let registration_index = self
            .snapshot
            .ui_state
            .workspace_registrations
            .iter()
            .position(|row| row.id == payload.workspace_id);
        let project = self
            .snapshot
            .navigator
            .workspaces
            .iter()
            .find(|row| row.id == payload.workspace_id);
        let refusal = match (registration_index, project) {
            (Some(index), _)
                if self.snapshot.ui_state.workspace_registrations[index].device_id
                    != workspace::LOCAL_DEVICE_ID =>
            {
                Some("remote_read_only")
            }
            (None, _) => Some("unregistered"),
            (_, None) => Some("missing_workspace"),
            (_, Some(project)) if !project.is_git => Some("plain_folder"),
            (_, Some(project))
                if !project
                    .checkouts
                    .iter()
                    .any(|checkout| checkout.id == payload.checkout_id && checkout.exists) =>
            {
                Some("missing_checkout")
            }
            _ => None,
        };
        if let Some(reason) = refusal {
            self.push_diagnostic(
                format!("workspace.primary_{reason}"),
                format!(
                    "Project {} checkout {}",
                    payload.workspace_id, payload.checkout_id
                ),
            );
            return true;
        }
        if project.is_some_and(|project| {
            project
                .checkouts
                .iter()
                .any(|checkout| checkout.id == payload.checkout_id && checkout.is_primary)
        }) {
            return false;
        }
        let registration = &mut self.snapshot.ui_state.workspace_registrations
            [registration_index.expect("validated registration")];
        if registration.primary_checkout_id.as_ref() == Some(&payload.checkout_id) {
            return false;
        }
        registration.primary_checkout_id = Some(payload.checkout_id.clone());
        for project in self
            .snapshot
            .navigator
            .workspaces
            .iter_mut()
            .chain(self.last_accepted_catalog.iter_mut().flatten())
            .filter(|project| project.id == payload.workspace_id)
        {
            workspace::apply_primary_checkout(project, Some(&payload.checkout_id));
        }
        crate::project_context::sort_projects(
            &mut self.snapshot.navigator.workspaces,
            &self.snapshot.navigator.agents,
        );
        self.refresh_inactive_groups();
        self.persist_ui_state();
        true
    }

    /// Pins or unpins a project (D-07, D-08). The row moves at once and the
    /// registration is persisted on the existing off-lock save; the same
    /// value again changes nothing and writes nothing. A row Herdr shows
    /// without a registration is registered by its pin (PRD
    /// sidebar-context-menus D-14), so pinning it stays one event.
    pub(super) fn set_workspace_pinned(&mut self, payload: WorkspacePinSetPayload) -> bool {
        if !self
            .snapshot
            .ui_state
            .workspace_registrations
            .iter()
            .any(|registration| registration.id == payload.workspace_id)
            && let Some(registration) = self.unregistered_row_registration(&payload.workspace_id)
        {
            if !payload.pinned {
                // Nothing is pinned without a registration: already the target state.
                return false;
            }
            return self.register_pinned(registration);
        }
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
        let device = registration.device_id.clone();
        if device != workspace::LOCAL_DEVICE_ID {
            // A device's rows are derived from its registrations, so the row
            // moves when its session is derived again.
            self.refresh_device_catalog(&device);
        }
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

    /// The registration a row Herdr shows without one would take: the id,
    /// label, root and device the row already carries, so nothing is read
    /// from disk under the lock and the next catalog rebuild adopts it under
    /// the same id. A device's row qualifies only once its helper grouped it
    /// under its root (`device_catalog::project_id`); before that its id
    /// names one Herdr workspace, which no registration can keep.
    fn unregistered_row_registration(
        &self,
        workspace_id: &str,
    ) -> Option<Result<crate::model::WorkspaceRegistration, String>> {
        let row_registration = |row: &crate::model::WorkspaceSnapshot, device_id: &str| {
            crate::model::WorkspaceRegistration {
                primary_checkout_id: None,
                id: row.id.clone(),
                label: row.label.clone(),
                path: row.path.clone(),
                device_id: device_id.to_owned(),
                pinned: true,
                home: false,
            }
        };
        if let Some(row) =
            self.snapshot.navigator.workspaces.iter().find(|row| {
                row.id == workspace_id && !row.registered && row.remote_target_id.is_none()
            })
        {
            return Some(Ok(row_registration(row, workspace::LOCAL_DEVICE_ID)));
        }
        self.snapshot.status.remote.iter().find_map(|status| {
            let row = status
                .session
                .as_ref()?
                .workspaces
                .iter()
                .find(|row| row.id == workspace_id && !row.registered)?;
            let grouped =
                crate::device_catalog::project_id(&status.target_id, Path::new(&row.path));
            Some(if grouped == row.id {
                Ok(row_registration(row, &status.target_id))
            } else {
                Err(status.target_id.clone())
            })
        })
    }

    /// Registers a row Herdr shows without a registration and pins it, in
    /// the event that asked for the pin. A device's row its helper has not
    /// grouped yet stays as it is, with the reason in the log: the operator
    /// can pin it again once the device's catalog is ready.
    fn register_pinned(
        &mut self,
        registration: Result<crate::model::WorkspaceRegistration, String>,
    ) -> bool {
        let registration = match registration {
            Ok(registration) => registration,
            Err(device) => {
                self.push_diagnostic(
                    "workspace.pin_ungrouped",
                    format!("A project row on {device} is not grouped by its helper yet, so it was not registered"),
                );
                return true;
            }
        };
        // A removal still closing this row's panes would retire the row
        // this registration keeps; the two requests contradict each other.
        if self.workspace_removals_in_flight.contains(&registration.id) {
            self.push_diagnostic(
                "workspace.pin_during_removal",
                format!(
                    "Project {} is being removed, so it was not pinned",
                    registration.id
                ),
            );
            return true;
        }
        let id = registration.id.clone();
        let device = registration.device_id.clone();
        self.snapshot
            .ui_state
            .workspace_registrations
            .push(registration);
        if device != workspace::LOCAL_DEVICE_ID {
            self.refresh_device_catalog(&device);
        }
        for row in self
            .snapshot
            .navigator
            .workspaces
            .iter_mut()
            .chain(self.last_accepted_catalog.iter_mut().flatten())
            .filter(|row| row.id == id)
        {
            row.registered = true;
            row.temporary = false;
            row.pinned = true;
            for checkout in &mut row.checkouts {
                checkout.temporary = false;
            }
        }
        let agents = self.snapshot.navigator.agents.clone();
        crate::project_context::sort_projects(&mut self.snapshot.navigator.workspaces, &agents);
        self.refresh_inactive_groups();
        self.persist_ui_state();
        crate::diagnostic!(serde_json::json!({
            "component": "registration", "kind": "workspace.registered_by_pin",
            "workspace_id": id, "target": device,
        }));
        self.push_diagnostic("workspace.pinned", format!("Project {id}"));
        true
    }

    /// `Remove project…` (D-09, D-11). A project with no pane loses its
    /// registration at once, as before. One with panes has them closed on a
    /// worker thread that waits for Herdr to confirm; only that confirmation
    /// removes the registration, so a timeout leaves the project registered
    /// with the reason in the error banner and a retry starts from whatever
    /// panes remain. A row Herdr shows without a registration (PRD
    /// sidebar-context-menus D-14) has only its panes to close; the row then
    /// leaves with Herdr's workspace.
    pub(super) fn remove_workspace(&mut self, payload: RemoveWorkspacePayload) -> bool {
        let registered = self
            .snapshot
            .ui_state
            .workspace_registrations
            .iter()
            .find(|registration| registration.id == payload.workspace_id)
            .map(|registration| registration.device_id.clone());
        let Some(owner) =
            registered.or_else(|| self.unregistered_row_device(&payload.workspace_id))
        else {
            // Already gone: the target state is reached, and a repeat of a
            // completed removal stays quiet (docs/UI_BEHAVIOR.md, registration removal).
            return false;
        };
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
        let device = Some(owner).filter(|device| device != workspace::LOCAL_DEVICE_ID);
        if !payload.close_descendant_pane_ids.is_empty() {
            // The operator's chosen descendants outside the project close
            // first; the removal starts only once they have (PRD
            // close-agent-subtree D-36). The mark keeps a repeat quiet.
            let (_, inside) = self.project_panes(&payload.workspace_id, device.as_deref());
            let inside = inside
                .into_iter()
                .map(|pane| match device.as_deref() {
                    Some(device) => crate::session_sync::remote_pane_id(device, &pane),
                    None => pane,
                })
                .collect::<HashSet<_>>();
            self.workspace_removals_in_flight
                .insert(payload.workspace_id.clone());
            return self.close_descendants_before_removal(
                &inside,
                payload.close_descendant_pane_ids,
                super::tree_close::TreeFinal::Project {
                    workspace_id: payload.workspace_id,
                    device,
                },
            );
        }
        self.close_project_panes(payload.workspace_id, device)
    }

    /// A project's checkout folders and the panes in them, by the ids its
    /// own Herdr uses.
    fn project_panes(
        &self,
        workspace_id: &str,
        device: Option<&str>,
    ) -> (Vec<String>, Vec<String>) {
        // A device's project lists its panes in that device's session, by
        // scoped ids; Herdr there closes them by its own.
        let rows = match device {
            Some(device) => self
                .snapshot
                .status
                .remote
                .iter()
                .find(|status| status.target_id == device)
                .and_then(|status| status.session.as_ref())
                .map(|session| session.workspaces.as_slice())
                .unwrap_or_default(),
            None => self.snapshot.navigator.workspaces.as_slice(),
        };
        rows.iter()
            .filter(|workspace| workspace.id == workspace_id)
            .flat_map(|workspace| &workspace.checkouts)
            .fold(
                (Vec::new(), Vec::new()),
                |(mut paths, mut panes), checkout| {
                    paths.push(checkout.path.clone());
                    panes.extend(checkout.tabs.iter().flat_map(|tab| &tab.panes).filter_map(
                        |pane| match device {
                            Some(device) => {
                                super::remote_pane_source_id(device, &pane.id).map(str::to_owned)
                            }
                            None => Some(pane.id.clone()),
                        },
                    ));
                    (paths, panes)
                },
            )
    }

    /// Closes a project's panes on the close worker, which answers whether
    /// Herdr confirmed them gone; only that removes the registration.
    pub(super) fn close_project_panes(
        &mut self,
        workspace_id: String,
        device: Option<String>,
    ) -> bool {
        let (checkout_paths, pane_ids) = self.project_panes(&workspace_id, device.as_deref());
        if pane_ids.is_empty() {
            return self.retire_workspace_registration(&workspace_id);
        }
        crate::diagnostic!(serde_json::json!({
            "component": "registration", "kind": "remove.close_requested",
            "workspace_id": workspace_id, "pane_ids": pane_ids,
            "target": device.as_deref().unwrap_or(workspace::LOCAL_DEVICE_ID),
        }));
        let context = match device.as_deref() {
            Some(device) => self.device_worktree_target(device),
            None => self.local_worktree_target(),
        };
        let context = match context {
            Ok(context) => context,
            Err(message) => {
                self.set_error(
                    "workspace.remove_failed",
                    format!("Removing a project with open panes needs its device's Herdr connection: {message}"),
                    true,
                );
                return true;
            }
        };
        self.workspace_removals_in_flight
            .insert(workspace_id.clone());
        if let Err(message) =
            live::spawn_workspace_close(context, workspace_id.clone(), checkout_paths, pane_ids)
        {
            self.ingest_workspace_close_result(&workspace_id, Err(message));
        }
        true
    }

    /// The device of a row Herdr shows without a registration, when one has
    /// this id: this machine's navigator first, then each device's session.
    fn unregistered_row_device(&self, workspace_id: &str) -> Option<String> {
        if self
            .snapshot
            .navigator
            .workspaces
            .iter()
            .any(|row| row.id == workspace_id && !row.registered && row.remote_target_id.is_none())
        {
            return Some(workspace::LOCAL_DEVICE_ID.to_owned());
        }
        self.snapshot.status.remote.iter().find_map(|status| {
            status
                .session
                .as_ref()?
                .workspaces
                .iter()
                .any(|row| row.id == workspace_id && !row.registered)
                .then(|| status.target_id.clone())
        })
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
                let registered = self
                    .snapshot
                    .ui_state
                    .workspace_registrations
                    .iter()
                    .any(|registration| registration.id == workspace_id);
                self.set_error(
                    "workspace.remove_failed",
                    if registered {
                        format!("{message}. The project stays registered; remove it again to close the panes that remain.")
                    } else {
                        format!("{message}. Remove the project again to close the panes that remain.")
                    },
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
            // Only this machine's registrations: a device's project at the
            // same absolute path is another folder (B2).
            .filter(|registration| registration.device_id == workspace::LOCAL_DEVICE_ID)
            .filter(|registration| self.workspace_removals_in_flight.contains(&registration.id))
            .find(|registration| Path::new(&registration.path) == requested)
            .map(|registration| registration.id.clone())
    }

    /// Whether a creation still running names the folder `workspace_id`
    /// registers; creation is keyed by the requested path, not the id.
    pub(super) fn workspace_creation_in_flight_for(&self, workspace_id: &str) -> bool {
        if self.workspace_creations_in_flight.is_empty() {
            return false;
        }
        self.snapshot
            .ui_state
            .workspace_registrations
            .iter()
            // Creations in flight are this machine's folders only (B2).
            .filter(|registration| {
                registration.id == workspace_id
                    && registration.device_id == workspace::LOCAL_DEVICE_ID
            })
            .any(|registration| {
                self.workspace_creations_in_flight
                    .iter()
                    .any(|path| Path::new(path) == Path::new(&registration.path))
            })
    }

    /// Drops the registration and its row. Files, worktrees and Herdr
    /// workspaces are never touched here.
    pub(super) fn retire_workspace_registration(&mut self, workspace_id: &str) -> bool {
        let local_path = self
            .snapshot
            .navigator
            .workspaces
            .iter()
            .find(|workspace| workspace.id == workspace_id && workspace.remote_target_id.is_none())
            .map(|workspace| workspace.path.clone());
        let device = self
            .snapshot
            .ui_state
            .workspace_registrations
            .iter()
            .find(|registration| registration.id == workspace_id)
            .map(|registration| registration.device_id.clone())
            .filter(|device| device != workspace::LOCAL_DEVICE_ID);
        let registered = self
            .snapshot
            .ui_state
            .workspace_registrations
            .iter()
            .find(|registration| registration.id == workspace_id)
            .map(|registration| (registration.device_id.clone(), registration.path.clone()));
        let before = self.snapshot.ui_state.workspace_registrations.len();
        self.snapshot
            .ui_state
            .workspace_registrations
            .retain(|registration| registration.id != workspace_id);
        if before == self.snapshot.ui_state.workspace_registrations.len() {
            return false;
        }
        // The project's checkouts leave the recent list with it; their ids
        // are read before the catalog drops the rows that name them. A
        // device that is not connected has no rows to read, so the
        // registered folder's own checkout is named by its folder as well.
        let mut checkout_ids: Vec<String> = self
            .catalog_workspaces()
            .filter(|workspace| workspace.id == workspace_id)
            .flat_map(|workspace| &workspace.checkouts)
            .map(|checkout| checkout.id.clone())
            .collect();
        if let Some((device_id, path)) =
            registered.filter(|(device, _)| device != workspace::LOCAL_DEVICE_ID)
        {
            checkout_ids.push(crate::device_catalog::checkout_id(&device_id, &path));
        }
        self.forget_recent_checkouts(|held| checkout_ids.contains(&held.checkout_id));
        if let Some(path) = local_path {
            self.worktree_project_generations.remove(&path);
        }
        if let Some(device) = device {
            self.refresh_device_catalog(&device);
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

    /// `closed_pane_ids` are the panes Herdr confirmed gone; the navigator can
    /// still list them until Herdr's close events are applied, so only a pane
    /// outside that set counts as one that appeared during the confirmation.
    pub fn ingest_worktree_close_result(
        &mut self,
        id: u64,
        closed_pane_ids: &[String],
        result: Result<(), String>,
    ) -> bool {
        let Some(active) = self.snapshot.worktree_removal.as_ref() else {
            return false;
        };
        if active.id != id || active.phase != "closing" {
            return false;
        }
        let mut result = result;
        if result.is_ok() {
            let device = active.device_id.as_deref();
            let catalog = match device {
                Some(device) => self
                    .device_worktrees
                    .get(device)
                    .map(|worktrees| worktrees.projects.values().collect::<Vec<_>>())
                    .unwrap_or_default(),
                None => self.worktree_catalog.projects.iter().collect(),
            };
            // The catalog is the reader's latest answer. Closing a checkout's
            // last pane can end the only workspace that asked for its
            // repository, and the next answer then has no project for it,
            // which says nothing about the worktree, so only a repository the
            // reader still answers for is compared. The host checks the
            // registration, HEAD and branch again before it removes anything.
            let identity_changed = catalog
                .into_iter()
                .find(|project| project.root_path == active.repository_root)
                .is_some_and(|project| {
                    project
                        .worktrees
                        .iter()
                        .find(|worktree| worktree.path == active.checkout_path)
                        .is_none_or(|worktree| {
                            worktree.head_sha != active.expected_head_sha
                                || worktree.branch != active.expected_branch
                                || worktree.deletion_gate.blocked_reason.is_some()
                        })
                });
            // The close worker names a device's panes by Herdr's own ids.
            let workspaces = match device {
                Some(device) => self
                    .snapshot
                    .status
                    .remote
                    .iter()
                    .find(|status| status.target_id == device)
                    .and_then(|status| status.session.as_ref())
                    .map(|session| session.workspaces.as_slice())
                    .unwrap_or_default(),
                None => self.snapshot.navigator.workspaces.as_slice(),
            };
            let pane_reappeared = workspaces
                .iter()
                .flat_map(|workspace| &workspace.checkouts)
                .filter(|checkout| checkout.path == active.checkout_path)
                .flat_map(|checkout| &checkout.tabs)
                .flat_map(|tab| &tab.panes)
                .any(|pane| {
                    let id = match device {
                        Some(device) => super::remote_pane_source_id(device, &pane.id),
                        None => Some(pane.id.as_str()),
                    };
                    id.is_none_or(|id| !closed_pane_ids.iter().any(|closed| closed == id))
                });
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
                removal.phase = "removing".to_owned();
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

    /// The removal the close worker may now execute: the active request, once
    /// every pane is confirmed gone and the identity still matched.
    pub(crate) fn confirmed_worktree_removal(
        &self,
        id: u64,
    ) -> Option<hide_host::worktrees::ConfirmedRemoval> {
        self.worktree_removal_request(id, "removing")
    }

    pub(crate) fn worktree_preflight_request(
        &self,
        id: u64,
    ) -> Option<hide_host::worktrees::ConfirmedRemoval> {
        self.worktree_removal_request(id, "checking")
    }

    fn worktree_removal_request(
        &self,
        id: u64,
        phase: &str,
    ) -> Option<hide_host::worktrees::ConfirmedRemoval> {
        let removal = self.snapshot.worktree_removal.as_ref()?;
        (removal.id == id && removal.phase == phase).then(|| {
            hide_host::worktrees::ConfirmedRemoval {
                repository_root: removal.repository_root.clone(),
                checkout_path: removal.checkout_path.clone(),
                expected_head_sha: removal.expected_head_sha.clone(),
                expected_branch: removal.expected_branch.clone(),
                protected_base_branch: removal.protected_base_branch.clone(),
                delete_branch: removal
                    .delete_branch
                    .then(|| removal.branch.clone())
                    .flatten(),
                force_delete_branch: removal.force_delete_branch,
                discard_changes: removal.discard_changes,
                expected_ignored_repositories: removal.expected_ignored_repositories.clone(),
            }
        })
    }

    /// Settles the active removal with what Git did. Only the worker that ran
    /// the removal calls this, so a result can never name another request.
    pub(crate) fn ingest_worktree_removal_result(
        &mut self,
        id: u64,
        result: Result<String, String>,
    ) -> bool {
        let Some(removal) = self
            .snapshot
            .worktree_removal
            .as_mut()
            .filter(|removal| removal.id == id && removal.phase == "removing")
        else {
            crate::diagnostic!(serde_json::json!({
                "component": "worktree_removal",
                "kind": "result_without_request",
                "id": id,
            }));
            return false;
        };
        let (phase, message) = match result {
            Ok(message) => ("finished", message),
            Err(message) => ("failed", message),
        };
        removal.phase = phase.to_owned();
        removal.message = Some(message);
        let checkout_path = removal.checkout_path.clone();
        let removal_device = removal.device_id.clone();
        if phase == "finished" {
            // The removed checkout leaves the recent list in the frame its
            // row leaves the catalog.
            let device = removal_device
                .clone()
                .unwrap_or_else(|| workspace::LOCAL_DEVICE_ID.to_owned());
            let gone = self.catalog_checkout_ids_at(&device, &checkout_path);
            self.forget_recent_checkouts(|held| gone.contains(&held.checkout_id));
        }
        match removal_device {
            Some(device) => {
                self.request_device_worktrees(&device, true);
            }
            // The row leaves in this same frame. Git dropped the registration
            // in the repository's own Git directory, which is that project's
            // freshness key, so the reader re-reads that project alone; every
            // other project keeps its last answer.
            None if phase == "finished" => self.drop_removed_worktree(checkout_path),
            // A refused removal re-reads too: whatever stopped it (a moved
            // HEAD, a dirty file) is news the catalog should show.
            None => self.refresh_worktrees(),
        }
        true
    }

    /// Takes a removed checkout out of the catalog and the rows built on it,
    /// and keeps it out of any read that started before the removal settled.
    fn drop_removed_worktree(&mut self, checkout_path: String) {
        self.worktree_removals = self.worktree_removals.wrapping_add(1);
        for project in &mut self.worktree_catalog.projects {
            project
                .worktrees
                .retain(|worktree| worktree.path != checkout_path);
        }
        self.removed_worktrees
            .push((self.worktree_removals, checkout_path));
        self.refresh_worktree_projection();
    }

    /// This machine's Herdr and file host, for a worktree task here.
    pub(super) fn local_worktree_target(&self) -> Result<live::WorktreeTarget, String> {
        self.live
            .as_ref()
            .map(|context| live::WorktreeTarget::local(context, Arc::clone(&self.local_host)))
            .ok_or_else(|| "A live Herdr connection is required".to_owned())
    }

    /// A device's Herdr and file helper, for a worktree task there. Both
    /// have to be up: Herdr creates and closes the panes, the helper checks
    /// and removes on the device's disk.
    pub(super) fn device_worktree_target(
        &mut self,
        device: &str,
    ) -> Result<live::WorktreeTarget, String> {
        let control = self
            .remote_controls
            .get(device)
            .cloned()
            .ok_or_else(|| "The device's Herdr connection is unavailable".to_owned())?;
        let host = self.device_channel(device)?;
        Ok(live::WorktreeTarget::device(&control, host))
    }

    pub fn disk_request(&self) -> crate::disk::DiskRequest {
        let mut shared_git = Vec::new();
        let mut paths = if self.snapshot.ui_state.right_panel_visible
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
                        shared_git.push(PathBuf::from(shared));
                    }
                    Some(paths)
                })
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        // The project a web Overview named is measured beside the right
        // panel's; the reader lets the deepest root own its subtree and
        // counts each inode once, so a path both ask for is one measurement.
        if let Some(project) = self
            .disk_project
            .as_deref()
            .and_then(|path| self.worktree_catalog.project(path))
        {
            paths.extend(project.worktrees.iter().map(|w| PathBuf::from(&w.path)));
            if let Some(shared) = project.shared_git_path.as_ref().map(PathBuf::from) {
                shared_git.push(shared.clone());
                paths.push(shared);
            }
            paths.sort();
            paths.dedup();
            shared_git.sort();
            shared_git.dedup();
        }
        crate::disk::DiskRequest {
            paths,
            shared_git,
            generation: self.disk_generation,
        }
    }

    /// Names the local Git project a web Overview shows for measuring, for
    /// every window of the daemon, and measures it again. A project that is
    /// not a local Git project here has no size to measure; the refusal is a
    /// diagnostic, and the Overview simply draws no size.
    pub(super) fn measure_project_disk(&mut self, workspace_id: &str) -> bool {
        let Some(path) = self
            .snapshot
            .navigator
            .workspaces
            .iter()
            .find(|workspace| {
                workspace.id == workspace_id
                    && workspace.remote_target_id.is_none()
                    && workspace.is_git
            })
            .map(|workspace| workspace.path.clone())
        else {
            crate::diagnostic!(serde_json::json!({
                "component": "disk",
                "kind": "project_measure.not_local_git",
                "workspace_id": workspace_id,
            }));
            return false;
        };
        self.disk_project = Some(path);
        self.disk_generation = self.disk_generation.wrapping_add(1);
        self.refresh_worktree_projection();
        true
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
        let awaiting: HashSet<String> = self
            .github_projects()
            .0
            .into_iter()
            .map(|workspace| workspace.path.clone())
            .filter(|path| !self.github_settled.contains(path))
            .collect();
        let link_summaries = self
            .snapshot
            .link_summaries
            .as_ref()
            .map(|summaries| &summaries.projects);
        for workspace in self.snapshot.navigator.workspaces.iter_mut() {
            let project = github.project(&workspace.path);
            let status = project
                .map(|project| project.status.clone())
                .unwrap_or_else(|| crate::model::GithubStatusSnapshot {
                    loading: awaiting.contains(&workspace.path),
                    ..Default::default()
                });
            let home_issues = project
                .map(|project| project.issues.clone())
                .unwrap_or_default();
            if workspace.home_issues != home_issues {
                workspace.home_issues = home_issues;
                changed = true;
            }
            let pull_requests = match project {
                Some(project) if workspace.is_git && workspace.remote_target_id.is_none() => {
                    super::pull_requests::shown_pull_requests(
                        &project.pull_requests,
                        &workspace.checkouts,
                        unix_milliseconds(),
                    )
                }
                _ => Vec::new(),
            };
            if workspace.pull_requests != pull_requests {
                workspace.pull_requests = pull_requests;
                changed = true;
            }
            for checkout in workspace.checkouts.iter_mut() {
                if checkout.github != status {
                    checkout.github = status.clone();
                    changed = true;
                }
            }
            changed |= associate_pull_requests(workspace, project);
            changed |= super::links::associate_landed(
                workspace,
                link_summaries.and_then(|summaries| summaries.get(&workspace.id)),
            );
        }
        let times = pull_request_times(&github);
        if *self.pull_request_times != times {
            // A sighted address the answer now holds needs no place kept.
            self.github_sighted
                .retain(|address, _| !times.contains_key(address));
            self.pull_request_times = std::sync::Arc::new(times);
            if let Some(services) = self.label_services.as_ref() {
                services.wake_local();
            }
        }
        changed |= crate::sidebar::sync_checkout_purposes(
            &mut self.snapshot.navigator.workspaces,
            &self.snapshot.navigator.agents,
        );
        changed |= self.sync_issues();
        changed |= self.sync_tasks();
        changed |= self.sync_request_rows();
        self.feed_links();
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
                // An absent answer is loading until the first read of this
                // project has answered; a project the core does not read
                // (past the limit) has no work in flight.
                .unwrap_or(crate::model::GithubStatusSnapshot {
                    loading: self.github_awaiting(&workspace.path),
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
        self.start_worktree_task(payload, false)
    }

    /// `create_worktree`, on a new branch or, `existing_branch`, on a branch
    /// that already exists here or on `origin` (a pull request's, PRD
    /// overview-lenses-prs D-46), which only this machine's repositories do.
    pub(super) fn start_worktree_task(
        &mut self,
        payload: CreateWorktreePayload,
        existing_branch: bool,
    ) -> bool {
        // The kind reaches `agent.start`, which runs it in the new pane's
        // shell, so only the providers Hide can start are accepted.
        if let Some(kind) = payload.agent_kind.as_deref()
            && !matches!(kind, "claude" | "codex")
        {
            self.set_error(
                "worktree.create_unknown_agent",
                format!("No agent provider named {kind}"),
                false,
            );
            return true;
        }
        let branch = payload.branch.trim().to_owned();
        if branch.is_empty() {
            self.set_error(
                "worktree.create_invalid_branch",
                "Branch is required",
                false,
            );
            return true;
        }
        let device = payload
            .device_id
            .clone()
            .filter(|device| device != workspace::LOCAL_DEVICE_ID);
        let listed = match device.as_deref() {
            Some(device) => self
                .device_worktrees
                .get(device)
                .and_then(|worktrees| worktrees.projects.get(&payload.repository_root)),
            None => self.worktree_catalog.project(&payload.repository_root),
        };
        // A repository whose worktrees have not been read yet is not one
        // without branches: a device's are read after its helper answers.
        let Some(listed) = listed else {
            self.set_error(
                "worktree.create_unread",
                format!(
                    "The worktrees of {} have not been read yet; try again in a moment",
                    payload.repository_root
                ),
                true,
            );
            if let Some(device) = device.as_deref() {
                self.request_device_worktrees(device, false);
            }
            return true;
        };
        let has_branch = listed
            .worktrees
            .iter()
            .any(|row| row.branch.is_some() && row.head_sha.is_some());
        if !has_branch {
            self.set_error(
                "worktree.create_without_branches",
                "Create the repository's first branch before creating a worktree",
                true,
            );
            return true;
        }
        // The link is written into the new worktree's own metadata, in the
        // form the linking chain reads back (`sync_issues`).
        let issue = match payload.task_key.as_deref() {
            None => None,
            Some(key) => match crate::tasks::issue_token(key) {
                Some(token) => Some(token),
                None => {
                    self.set_error(
                        "worktree.create_unknown_task",
                        format!("No task named {key}"),
                        false,
                    );
                    return true;
                }
            },
        };
        let model = match agent_choice::chosen_model(
            payload.agent_kind.as_deref(),
            payload.model.as_deref(),
        ) {
            Ok(model) => model,
            Err(message) => {
                self.set_error("worktree.create_unknown_model", message, false);
                return true;
            }
        };
        let prompt = payload.agent_kind.as_ref().and(payload.prompt.clone());
        if let Some(message) = prompt
            .as_deref()
            .and_then(|prompt| live::prompt_argument(prompt).err())
        {
            self.set_error("worktree.create_invalid_prompt", message, false);
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
        self.remember_agent_choice(payload.agent_kind.as_deref(), model.as_deref());
        self.set_task_agent_launch(
            id,
            prompt,
            agent_choice::agent_arguments(model.as_deref(), &[]),
        );
        if let Some(operation) = self.snapshot.task_operation.as_mut() {
            operation.device_id = device.clone();
        }
        let request = live::WorktreeTaskRequest {
            id,
            repository_root: payload.repository_root,
            branch,
            base_branch: payload.base_branch,
            agent_kind: payload.agent_kind,
            focus: true,
            purpose: payload.purpose,
            issue: if device.is_none() { issue } else { None },
        };
        let context = match device.as_deref() {
            Some(device) => self.device_worktree_target(device),
            None => self.local_worktree_target(),
        };
        let context = match context {
            Ok(context) => context,
            Err(message) => {
                return self
                    .ingest_task_operation_result(id, Err(format!("create worktree: {message}")));
            }
        };
        let spawned = match (existing_branch, device.is_some()) {
            (true, true) => Err(
                "create worktree: an existing branch is checked out only on this Mac".to_owned(),
            ),
            (true, false) => live::spawn_existing_branch_worktree(context, request),
            (false, _) => live::spawn_worktree_create(context, request),
        };
        if let Err(message) = spawned {
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
        // A device checkout's purpose goes to its owner workspace; its other
        // workspaces only hold tabs there (D-10).
        let session_workspace_id = if remote_target_id.is_some() {
            checkout.owner_workspace_id.clone()
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
        let request_id = payload.request_id.clone();
        let agent_kind = match payload.provider.as_str() {
            "terminal" => None,
            "claude" | "codex" => Some(payload.provider.clone()),
            other => {
                self.set_request_error(
                    "agent_start.unknown_provider",
                    format!("No agent provider named {other}"),
                    false,
                    request_id.as_deref(),
                );
                return true;
            }
        };
        let model =
            match agent_choice::chosen_model(agent_kind.as_deref(), payload.model.as_deref()) {
                Ok(model) => model,
                Err(message) => {
                    self.set_request_error(
                        "agent_start.unknown_model",
                        message,
                        false,
                        request_id.as_deref(),
                    );
                    return true;
                }
            };
        let resume = match payload.resume_session_id.as_deref() {
            None => None,
            Some(session_id) => match agent_kind.as_deref().and_then(|kind| {
                resume_session_arguments(kind, session_id).filter(|_| payload.prompt.is_none())
            }) {
                Some(arguments) => Some(arguments),
                None => {
                    self.set_request_error(
                        "agent_start.invalid_resume",
                        "This session cannot be resumed".to_owned(),
                        false,
                        request_id.as_deref(),
                    );
                    return true;
                }
            },
        };
        let prompt = agent_kind.as_ref().and(payload.prompt.clone());
        // A prompt the agent's command line cannot carry is refused before a
        // tab or Home sync, so nothing is left behind.
        if let Some(message) = prompt
            .as_deref()
            .and_then(|prompt| live::prompt_argument(prompt).err())
        {
            self.set_request_error(
                "agent_start.invalid_prompt",
                message,
                false,
                request_id.as_deref(),
            );
            return true;
        }
        let device = payload
            .device_id
            .clone()
            .filter(|device| !device.is_empty())
            .unwrap_or_else(|| workspace::LOCAL_DEVICE_ID.to_owned());
        // A start names one place: a device's Home, or one of its checkouts.
        let checkout_path = match (payload.home, payload.checkout_path.clone()) {
            (true, None) => {
                return self.start_in_home(&device, agent_kind, model, prompt, request_id);
            }
            (false, Some(path)) if !path.is_empty() => path,
            _ => {
                self.set_request_error(
                    "agent_start.invalid_target",
                    "A start names either Home or one checkout",
                    false,
                    request_id.as_deref(),
                );
                return true;
            }
        };
        let local = device == workspace::LOCAL_DEVICE_ID;
        let found = if local {
            self.local_checkout_tab(&checkout_path)
        } else {
            self.device_checkout_tab(&device, &checkout_path)
        };
        let (workspace_path, label, host) = match found {
            Ok(found) => found,
            Err((kind, message)) => {
                self.set_request_error(kind, message, false, request_id.as_deref());
                return true;
            }
        };
        let id = match self.begin_task_operation(
            "agent_start",
            Some(workspace_path),
            None,
            None,
            agent_kind.clone(),
        ) {
            Ok(id) => id,
            Err(message) => {
                self.set_request_error("task_operation.busy", message, true, request_id.as_deref());
                return true;
            }
        };
        if let Some(operation) = self.snapshot.task_operation.as_mut() {
            operation.request_id = request_id;
            operation.device_id = (!local).then(|| device.clone());
        }
        self.remember_agent_choice(agent_kind.as_deref(), model.as_deref());
        let mut arguments = resume.unwrap_or_default();
        arguments.extend(agent_choice::agent_arguments(model.as_deref(), &[]));
        self.set_task_agent_launch(id, prompt, arguments);
        let request = live::CheckoutTabRequest {
            id,
            checkout_path,
            label,
            host,
        };
        let target = if local {
            self.live
                .as_ref()
                .map(live::TabTarget::local)
                .ok_or("start an agent: a live Herdr connection is required")
        } else {
            self.remote_controls
                .get(&device)
                .map(live::TabTarget::device)
                .ok_or("start an agent: the device's Herdr connection is unavailable")
        };
        let target = match target {
            Ok(target) => target,
            Err(message) => return self.ingest_task_operation_result(id, Err(message.into())),
        };
        if let Err(message) = live::spawn_checkout_tab_create(target, request) {
            return self.ingest_task_operation_result(id, Err(message));
        }
        true
    }

    /// The project path, next tab label and tab host of a local checkout, or
    /// the refusal a start reports for a path the navigator does not list.
    fn local_checkout_tab(
        &self,
        checkout_path: &str,
    ) -> Result<(String, String, TabHost), (&'static str, String)> {
        let unknown = || {
            (
                "overview.unknown_checkout",
                format!("Checkout is not listed: {checkout_path}"),
            )
        };
        let (workspace_id, checkout_id) =
            self.local_checkout_ids(checkout_path).ok_or_else(unknown)?;
        let workspace = self
            .snapshot
            .navigator
            .workspaces
            .iter()
            .find(|workspace| workspace.id == workspace_id)
            .ok_or_else(unknown)?;
        let checkout = workspace
            .checkouts
            .iter()
            .find(|checkout| checkout.id == checkout_id)
            .ok_or_else(unknown)?;
        let host = self
            .local_tab_host(&workspace_id, &checkout_id)
            .ok_or_else(unknown)?;
        Ok((
            workspace.path.clone(),
            checkout.next_tab_label.clone(),
            host,
        ))
    }

    /// The same for a checkout on `device`, from that device's session: the
    /// tab goes to the checkout's owner there, opened first when none is
    /// (PRD home-device-rail D-22, checkout-workspace-binding D-07).
    fn device_checkout_tab(
        &self,
        device: &str,
        checkout_path: &str,
    ) -> Result<(String, String, TabHost), (&'static str, String)> {
        let session = self
            .snapshot
            .status
            .remote
            .iter()
            .find(|remote| remote.target_id == device)
            .and_then(|remote| remote.session.as_ref())
            .ok_or_else(|| {
                (
                    "agent_start.device_unavailable",
                    format!("{device} is not connected"),
                )
            })?;
        let (project, checkout) = session
            .workspaces
            .iter()
            .find_map(|project| {
                project
                    .checkouts
                    .iter()
                    .find(|checkout| checkout.path == checkout_path)
                    .map(|checkout| (project, checkout))
            })
            .ok_or_else(|| {
                (
                    "overview.unknown_checkout",
                    format!("Checkout is not listed on {device}: {checkout_path}"),
                )
            })?;
        if checkout.owner_workspace_id.is_none() && checkout.unconfirmed {
            return Err((
                "remote.control.checkout_unconfirmed",
                format!("{checkout_path} on {device} is not confirmed yet; no tab was opened"),
            ));
        }
        Ok((
            project.path.clone(),
            checkout.next_tab_label.clone(),
            tab_host(project, checkout, device),
        ))
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

    /// Where a new tab in one local checkout goes: its owner Herdr workspace,
    /// or that owner opened first (PRD checkout-workspace-binding D-07). A
    /// workspace that merely holds the checkout's tabs is never the answer
    /// (D-08).
    pub(super) fn local_tab_host(&self, project_id: &str, checkout_id: &str) -> Option<TabHost> {
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
        Some(tab_host(project, checkout, workspace::LOCAL_DEVICE_ID))
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
            issue: None,
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
    pub fn ingest_changes(&mut self, answer: crate::changes::ChangesAnswer) -> bool {
        if answer.key != self.changes_key() {
            return false;
        }
        let mut changes = answer.changes;
        // A failed read of the checkout already on screen keeps the list its
        // last successful read confirmed, marked stale with the reason,
        // rather than replacing it with nothing (S5.5 B22).
        if let Some(reason) = changes.unavailable_reason.clone()
            && self.changes_published_key == answer.key
            && self.snapshot.changes.unavailable_reason.is_none()
            && self.snapshot.changes.root_path == changes.root_path
        {
            let mut kept = crate::model::ChangesSnapshot::clone(&self.snapshot.changes);
            kept.stale_reason = Some(reason);
            changes = kept;
        }
        // An answer lands one wake after its request, and with View areas a
        // file in front leaves the next request asking for the row History
        // shows. An answer for a row the operator has left since would put
        // that row back, the next request would ask for it again, and the
        // two rows would chase each other with a read and a frame on every
        // tick while nothing is driven. Its list is still the newest, so only
        // the row and its diff stay as they are. The closed view's empty
        // projection is taken whole, so its diff text leaves the wire, and
        // an older client's reading is left as it was.
        if self.separate_view_areas()
            && answer.key.is_some()
            && answer.selection != self.changes_selection()
        {
            changes.selected_path = self.snapshot.changes.selected_path.clone();
            changes.selected_committed = self.snapshot.changes.selected_committed;
            changes.diff = self.snapshot.changes.diff.clone();
        }
        self.changes_published_key = answer.key;
        // The one comparison of the section: it takes a new edit number, and
        // is sent again, only when this read changed it.
        if self.snapshot.changes == changes {
            return false;
        }
        // A failure is logged when it first shows or changes. One the
        // operator cannot act on, a folder that is not a repository, is shown
        // nowhere else (issue 570).
        let failure = |changes: &crate::model::ChangesSnapshot| {
            changes
                .unavailable_reason
                .clone()
                .or_else(|| changes.stale_reason.clone())
        };
        if let Some(error) = failure(&changes)
            && failure(&self.snapshot.changes).as_ref() != Some(&error)
        {
            crate::diagnostic!(serde_json::json!({
                "component": "changes",
                "kind": "changes.unavailable",
                "device": self.changes_published_key.as_ref().map(|key| key.device_id.as_str()),
                "not_a_repository": changes.not_a_repository,
                "stale": changes.stale_reason.is_some(),
                "error": error,
            }));
        }
        self.snapshot.changes.set(changes);
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

/// Where a new tab in `checkout` of `project` on `device_id` goes: the owner
/// the session names, else the owner to open.
pub(super) fn tab_host(
    project: &WorkspaceSnapshot,
    checkout: &crate::model::CheckoutSnapshot,
    device_id: &str,
) -> TabHost {
    match &checkout.owner_workspace_id {
        Some(owner) => TabHost::Workspace(owner.clone()),
        None => TabHost::Open(owner_open(project, checkout, device_id)),
    }
}

/// How `checkout` of `project` gets its owner opened. A workspace Hide opens
/// is named as the sidebar names the checkout: a linked worktree by its
/// branch, the primary checkout and a plain folder by the project (D-14).
pub(super) fn owner_open(
    project: &WorkspaceSnapshot,
    checkout: &crate::model::CheckoutSnapshot,
    device_id: &str,
) -> OwnerOpen {
    let label = if checkout.is_worktree {
        &checkout.label
    } else {
        &project.label
    };
    OwnerOpen::for_checkout(
        device_id,
        &checkout.path,
        &project.path,
        project.is_git,
        label,
    )
}

/// Gives each checkout the one pull request that is its own work
/// (`github::pull_request_for_checkout`), and reports whether any changed.
/// Both places that learn something new about a checkout, GitHub's list and
/// the worktree reader's HEAD, come through here.
fn associate_pull_requests(
    workspace: &mut crate::model::WorkspaceSnapshot,
    project: Option<&crate::model::GithubProjectSnapshot>,
) -> bool {
    let mut changed = false;
    for checkout in &mut workspace.checkouts {
        let found = project.and_then(|project| {
            crate::github::pull_request_for_checkout(
                &project.pull_requests,
                checkout.branch.as_deref(),
                checkout.head_sha(),
            )
        });
        // Cloned only when it moved: this runs on every session update.
        if checkout.pull_request.as_ref() != found {
            checkout.pull_request = found.cloned();
            changed = true;
        }
    }
    changed
}

/// A pull request's lowercase `owner/name` and number, from its address.
fn pull_request_address(url: &str) -> Option<(String, u64)> {
    hide_session::pull_request_addresses(url)
        .into_iter()
        .next()
        .map(|(repository, number)| (repository.to_ascii_lowercase(), number))
}

/// When GitHub made each pull request read, by the address's lowercase
/// `owner/name` and number.
fn pull_request_times(
    github: &crate::model::GithubSnapshot,
) -> crate::labels::facts::PullRequestTimes {
    github
        .projects
        .iter()
        .flat_map(|project| project.pull_requests.iter())
        .filter_map(|pull_request| {
            let created = pull_request.created_at_unix_ms?;
            Some((pull_request_address(&pull_request.url)?, created))
        })
        .collect()
}

/// The arguments that resume `session_id` in `kind`'s CLI, or none for an id
/// that is not one plain token: it reaches the agent's command line, where
/// one that began with `-` would read as an option.
fn resume_session_arguments(kind: &str, session_id: &str) -> Option<Vec<String>> {
    let plain = session_id
        .bytes()
        .next()
        .is_some_and(|byte| byte.is_ascii_alphanumeric())
        && session_id.len() <= 128
        && session_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_');
    if !plain {
        return None;
    }
    crate::recent_closed::resume_arguments(&crate::recent_closed::ClosedAgent {
        kind: kind.to_owned(),
        session_id: Some(session_id.to_owned()),
    })
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
