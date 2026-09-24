//! Applies ordered Herdr topology events to the authoritative replica.

use super::*;

#[derive(Clone)]
pub(crate) struct SessionReplica {
    pub(crate) state: ProjectionState,
    /// The last projection whose dependency ranges were confirmed. Pending
    /// topology is kept in `state`, while this copy is what the runtime is
    /// allowed to see. That lets an independent workspace move forward while
    /// a different workspace still waits for its layout or replacement focus.
    pub(crate) published_state: ProjectionState,
    pub(crate) pending_layouts: BTreeSet<String>,
    pub(crate) pending_workspace_closures: BTreeSet<String>,
    pub(crate) pending_active_tab_focuses: BTreeSet<String>,
    /// How many events this replica has applied since its snapshot. Herdr's
    /// stream carries no sequence, so this is the only position a diagnostic
    /// can name.
    pub(crate) applied_events: u64,
}

/// How an event that disagrees with the replica is treated.
///
/// A subscription is opened before the snapshot it starts from, so the
/// stream cannot miss an event; the price is that an event emitted before
/// the snapshot was taken arrives as well, and it describes a change the
/// snapshot already holds. For a short window after the snapshot the
/// snapshot therefore wins: an event it contradicts (a pane created that it
/// already lists, a pane closed that it no longer lists) is dropped with a
/// diagnostic rather than declared malformed. After the window an event
/// that contradicts the replica is a real divergence, and the replica is
/// rebuilt from a fresh snapshot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ApplyMode {
    Strict,
    Reconcile,
}

#[derive(Debug)]
pub(crate) struct ApplyOutcome {
    pub(crate) publish: bool,
    pub(crate) refresh_agents: bool,
    pub(crate) refresh_worktrees: bool,
}

/// Every top-level `session.snapshot` field the replica reads. The contract
/// test asserts the pinned Herdr declares each one required, so a pin whose
/// snapshot cannot feed the replica fails in CI rather than at the user's
/// first launch with "snapshot is missing agents".
#[cfg(test)]
pub(crate) const SNAPSHOT_FIELDS_THE_REPLICA_READS: [&str; 7] = [
    "version",
    "protocol",
    "workspaces",
    "tabs",
    "panes",
    "layouts",
    "agents",
];

impl SessionReplica {
    #[cfg(test)]
    pub(crate) fn from_snapshot(snapshot: &Value) -> Result<Self, SessionFetchError> {
        Self::from_decoded(wire::snapshot(snapshot.clone())?)
    }

    pub(crate) fn from_decoded(state: ProjectionState) -> Result<Self, SessionFetchError> {
        let replica = Self {
            published_state: state.clone(),
            state,
            pending_layouts: BTreeSet::new(),
            pending_workspace_closures: BTreeSet::new(),
            pending_active_tab_focuses: BTreeSet::new(),
            applied_events: 0,
        };
        replica.validate()?;
        replica.validate_active_tabs()?;
        Ok(replica)
    }

    pub(crate) fn project(&self) -> SessionSnapshotPayload {
        self.published_state.project()
    }

    fn pending_workspace_ids(&self) -> HashSet<String> {
        let mut blocked = HashSet::new();
        for workspace_id in self
            .pending_workspace_closures
            .iter()
            .chain(self.pending_active_tab_focuses.iter())
        {
            blocked.insert(workspace_id.clone());
        }
        for tab_id in &self.pending_layouts {
            self.state
                .tabs
                .iter()
                .find(|tab| &tab.tab_id == tab_id)
                .or_else(|| {
                    self.published_state
                        .tabs
                        .iter()
                        .find(|tab| &tab.tab_id == tab_id)
                })
                .map(|tab| blocked.insert(tab.workspace_id.clone()));
        }
        blocked
    }

    fn merge_by_workspace<T: Clone>(
        current: &[T],
        published: &[T],
        blocked: &HashSet<String>,
        workspace_id: impl Fn(&T) -> &str,
    ) -> Vec<T> {
        let mut merged = Vec::with_capacity(current.len() + published.len());
        for item in current {
            if blocked.contains(workspace_id(item)) {
                continue;
            } else {
                merged.push(item.clone());
            }
        }
        for item in published {
            if blocked.contains(workspace_id(item)) {
                merged.push(item.clone());
            }
        }
        merged
    }

    fn workspace_for_pane<'a>(state: &'a ProjectionState, pane_id: &str) -> Option<&'a str> {
        state
            .panes
            .iter()
            .find(|pane| pane.pane_id == pane_id)
            .map(|pane| pane.workspace_id.as_str())
    }

    fn focused_pane_for_partial_publish(&self, blocked: &HashSet<String>) -> Option<String> {
        match self.state.focused_pane_id.as_deref() {
            Some(pane_id) => match Self::workspace_for_pane(&self.state, pane_id) {
                Some(workspace_id) if !blocked.contains(workspace_id) => Some(pane_id.to_owned()),
                Some(_) => self.published_state.focused_pane_id.clone(),
                None => None,
            },
            None => self
                .published_state
                .focused_pane_id
                .as_deref()
                .filter(|pane_id| {
                    Self::workspace_for_pane(&self.published_state, pane_id)
                        .is_some_and(|workspace_id| blocked.contains(workspace_id))
                })
                .map(str::to_owned),
        }
    }

    fn focused_workspace_for_partial_publish(&self, blocked: &HashSet<String>) -> Option<String> {
        match self.state.focused_workspace_id.as_deref() {
            Some(workspace_id) if !blocked.contains(workspace_id) => Some(workspace_id.to_owned()),
            Some(_) => self.published_state.focused_workspace_id.clone(),
            None => self
                .published_state
                .focused_workspace_id
                .as_deref()
                .filter(|workspace_id| blocked.contains(*workspace_id))
                .map(str::to_owned),
        }
    }

    fn partial_published_state(&self) -> ProjectionState {
        let blocked = self.pending_workspace_ids();
        ProjectionState {
            focused_pane_id: self.focused_pane_for_partial_publish(&blocked),
            focused_workspace_id: self.focused_workspace_for_partial_publish(&blocked),
            workspaces: Self::merge_by_workspace(
                &self.state.workspaces,
                &self.published_state.workspaces,
                &blocked,
                |workspace| workspace.workspace_id.as_str(),
            ),
            tabs: Self::merge_by_workspace(
                &self.state.tabs,
                &self.published_state.tabs,
                &blocked,
                |tab| tab.workspace_id.as_str(),
            ),
            panes: Self::merge_by_workspace(
                &self.state.panes,
                &self.published_state.panes,
                &blocked,
                |pane| pane.workspace_id.as_str(),
            ),
            layouts: Self::merge_by_workspace(
                &self.state.layouts,
                &self.published_state.layouts,
                &blocked,
                |layout| layout.workspace_id.as_str(),
            ),
            agents: Self::merge_by_workspace(
                &self.state.agents,
                &self.published_state.agents,
                &blocked,
                |agent| agent.workspace_id.as_str(),
            ),
        }
    }

    pub(crate) fn refresh_published_state(&mut self) -> Result<bool, SessionFetchError> {
        if self.ready_to_publish() {
            self.validate()?;
            let changed = self.published_state != self.state;
            self.published_state = self.state.clone();
            return Ok(changed);
        }
        let next = self.partial_published_state();
        let changed = next != self.published_state;
        self.published_state = next;
        Ok(changed)
    }

    pub(crate) fn project_remote(
        &self,
        target_id: &str,
    ) -> Result<(RemoteSessionSnapshot, Vec<crate::sidebar::AgentExclusion>), SessionFetchError>
    {
        let agent_projection = crate::sidebar::project_agents(self.project());
        let mut agents = agent_projection.agents;

        let state = &self.published_state;
        let mut pane_layouts = Vec::with_capacity(state.layouts.len());
        for layout in &state.layouts {
            let area_width = f64::from(layout.area.width);
            let area_height = f64::from(layout.area.height);
            if area_width <= 0.0 || area_height <= 0.0 {
                return Err(SessionFetchError::Malformed(format!(
                    "remote Herdr layout {} has an empty area",
                    layout.tab_id
                )));
            }
            let mut frames = Vec::with_capacity(layout.panes.len());
            for pane in &layout.panes {
                let frame = RemotePaneLayoutFrame {
                    pane_id: pane.pane_id.clone(),
                    x: (f64::from(pane.rect.x) - f64::from(layout.area.x)) / area_width,
                    y: (f64::from(pane.rect.y) - f64::from(layout.area.y)) / area_height,
                    width: f64::from(pane.rect.width) / area_width,
                    height: f64::from(pane.rect.height) / area_height,
                };
                if frame.x < 0.0
                    || frame.y < 0.0
                    || frame.width <= 0.0
                    || frame.height <= 0.0
                    || frame.x + frame.width > 1.0
                    || frame.y + frame.height > 1.0
                {
                    return Err(SessionFetchError::Malformed(format!(
                        "remote Herdr layout {} contains an out-of-bounds pane {}",
                        layout.tab_id, pane.pane_id
                    )));
                }
                frames.push(frame);
            }
            pane_layouts.push(RemotePaneLayoutSnapshot {
                workspace_id: remote_workspace_id(target_id, &layout.workspace_id),
                tab_id: remote_tab_id(target_id, &layout.tab_id),
                focused_pane_id: remote_pane_id(target_id, &layout.focused_pane_id),
                zoomed: layout.zoomed,
                frames: frames
                    .into_iter()
                    .map(|mut frame| {
                        frame.pane_id = remote_pane_id(target_id, &frame.pane_id);
                        frame
                    })
                    .collect(),
            });
        }

        let mut workspaces = state
            .workspaces
            .iter()
            .map(|workspace| {
                let workspace_id = remote_workspace_id(target_id, &workspace.workspace_id);
                let checkout_id = remote_checkout_id(target_id, &workspace.workspace_id);
                let workspace_panes = state
                    .panes
                    .iter()
                    .filter(|pane| pane.workspace_id == workspace.workspace_id)
                    .collect::<Vec<_>>();
                let pane_path = workspace_panes
                    .iter()
                    .find_map(|pane| pane.cwd.as_deref().filter(|cwd| !cwd.trim().is_empty()))
                    .map(str::to_owned);
                let path = workspace
                    .worktree
                    .as_ref()
                    .map(|worktree| worktree.checkout_path.clone())
                    .or(pane_path)
                    .unwrap_or_default();
                let repo_name = workspace
                    .worktree
                    .as_ref()
                    .map(|worktree| worktree.repo_name.clone())
                    .unwrap_or_else(|| workspace.label.clone());
                let next_tab_label = crate::model::next_tab_label(
                    state
                        .tabs
                        .iter()
                        .filter(|tab| tab.workspace_id == workspace.workspace_id)
                        .map(|tab| tab.label.as_str()),
                );
                let tabs = state
                    .tabs
                    .iter()
                    .filter(|tab| tab.workspace_id == workspace.workspace_id)
                    .map(|tab| {
                        let panes = workspace_panes
                            .iter()
                            .filter(|pane| pane.tab_id == tab.tab_id)
                            .map(|pane| {
                                let agent =
                                    agents.iter().find(|agent| agent.pane_id == pane.pane_id);
                                PaneSnapshot {
                                    id: remote_pane_id(target_id, &pane.pane_id),
                                    content: crate::pane_content::PaneContent::from_tokens(
                                        &pane.tokens,
                                        true,
                                    ),
                                    herdr_label: non_blank(pane.label.as_deref()),
                                    terminal_title: non_blank(
                                        pane.terminal_title_stripped
                                            .as_deref()
                                            .or(pane.terminal_title.as_deref()),
                                    ),
                                    workspace_label: non_blank(Some(workspace.label.as_str())),
                                    cwd: pane.cwd.clone().unwrap_or_else(|| path.clone()),
                                    status_label: agent
                                        .map(|agent| agent.status_label.clone())
                                        .unwrap_or_else(|| "Attached".to_owned()),
                                    requires_close_confirmation: agent
                                        .is_some_and(|agent| agent.requires_close_confirmation),
                                    requires_close_status_check: agent
                                        .is_some_and(|agent| agent.requires_close_status_check),
                                    identity_label: agent.map(|agent| agent.identity_label.clone()),
                                    activity_at_unix_ms: state
                                        .agents
                                        .iter()
                                        .find(|source| source.pane_id == pane.pane_id)
                                        .and_then(wire::agent_activity)
                                        .and_then(|activity| activity.parse().ok()),
                                    fork: crate::runtime::pane_fork_snapshot(agent),
                                    // Ports describe this machine's listeners,
                                    // so a remote pane reports none rather than
                                    // claiming the local machine's.
                                    ports: Vec::new(),
                                    // Hide installs no hook on another
                                    // machine, so a remote agent pane is
                                    // permanently uninstrumented and says so
                                    // rather than showing an empty chip row
                                    // (PRD B33, D-28, D-49).
                                    children: agent
                                        .map(|_| crate::model::PaneChildrenSnapshot::remote()),
                                    lineage_path: Vec::new(),
                                }
                            })
                            .collect::<Vec<_>>();
                        TabSnapshot {
                            id: Some(remote_tab_id(target_id, &tab.tab_id)),
                            workspace_id: Some(workspace_id.clone()),
                            checkout_id: Some(checkout_id.clone()),
                            label: Some(crate::model::display_tab_label(&tab.label, &tab.tab_id)),
                            empty: panes.is_empty(),
                            delegated: false,
                            panes,
                        }
                    })
                    .collect::<Vec<_>>();
                // The Herdr half of the strip in Herdr's order; the runtime
                // appends the device's file tabs (`join_device_editor_tabs`).
                let strip = StripTabSnapshot::from_herdr_tabs(&tabs);
                let active_tab_id = Some(remote_tab_id(target_id, &workspace.active_tab_id))
                    .filter(|active| tabs.iter().any(|tab| tab.id.as_ref() == Some(active)));
                WorkspaceSnapshot {
                    home_issues: Default::default(),
                    id: workspace_id.clone(),
                    label: workspace.label.clone(),
                    path: path.clone(),
                    remote_target_id: Some(target_id.to_owned()),
                    expanded: true,
                    device_id: target_id.to_owned(),
                    repo_name,
                    is_git: workspace.worktree.is_some(),
                    default_branch: None,
                    branches: Vec::new(),
                    registered: true,
                    temporary: false,
                    session_workspace_ids: vec![workspace.workspace_id.clone()],
                    last_activity_unix_ms: None,
                    checkouts: vec![CheckoutSnapshot {
                        id: checkout_id,
                        workspace_id,
                        label: workspace.label.clone(),
                        path: path.clone(),
                        branch: None,
                        is_worktree: workspace
                            .worktree
                            .as_ref()
                            .is_some_and(|worktree| worktree.is_linked_worktree),
                        exists: !path.is_empty(),
                        temporary: false,
                        // A remote checkout carries no worktree, pull-request,
                        // or disk facts: those readers describe this machine.
                        has_panes: !tabs.is_empty(),
                        purpose: workspace
                            .tokens
                            .get("purpose")
                            .and_then(Value::as_str)
                            .map(str::trim)
                            .filter(|purpose| !purpose.is_empty())
                            .map(|purpose| crate::model::CheckoutPurposeSnapshot {
                                text: purpose.chars().take(80).collect(),
                                origin: crate::model::CheckoutPurposeOrigin::Token,
                            }),
                        tabs,
                        active_tab_id,
                        strip,
                        next_tab_label,
                        ..CheckoutSnapshot::default()
                    }],
                    pinned: false,
                    inactive_checkouts: Default::default(),
                    removal: Default::default(),
                }
            })
            .collect::<Vec<_>>();
        for agent in &mut agents {
            let source_pane_id = agent.pane_id.clone();
            agent.id = format!("remote:{target_id}:agent:{source_pane_id}");
            agent.pane_id = remote_pane_id(target_id, &source_pane_id);
        }

        // Remote rows use the same representative-agent and purpose fallback
        // ladder as local rows. Run this after target-scoping pane ids so the
        // summary can join each agent to its projected checkout.
        crate::sidebar::sync_checkout_agent_summaries(&mut workspaces, &agents);

        // A remote project list follows the same activity order as a local
        // one, so a user reading two devices reads one rule. The agent pane
        // ids are rewritten first because the ordering matches agents to the
        // projected panes by id, and the projection carries the remote form.
        crate::project_context::sort_projects(&mut workspaces, &agents);

        let active_tab_ids = state
            .workspaces
            .iter()
            .map(|workspace| {
                (
                    remote_workspace_id(target_id, &workspace.workspace_id),
                    remote_tab_id(target_id, &workspace.active_tab_id),
                )
            })
            .collect();

        let focused = state
            .focused_pane_id
            .as_deref()
            .and_then(|pane_id| state.panes.iter().find(|pane| pane.pane_id == pane_id));
        let focused_workspace_id =
            focused.map(|pane| remote_workspace_id(target_id, &pane.workspace_id));
        let focused_checkout_id =
            focused.map(|pane| remote_checkout_id(target_id, &pane.workspace_id));
        let focused_tab_id = focused.map(|pane| remote_tab_id(target_id, &pane.tab_id));

        Ok((
            RemoteSessionSnapshot {
                workspaces,
                agents,
                active_tab_ids,
                focused_workspace_id,
                focused_checkout_id,
                focused_tab_id,
                focused_pane_id: state
                    .focused_pane_id
                    .as_deref()
                    .map(|pane_id| remote_pane_id(target_id, pane_id)),
                pane_layouts,
            },
            agent_projection.excluded,
        ))
    }

    /// Workspaces whose active tab a close removed and whose replacement Herdr
    /// has not named. Herdr emits `tab_focused` for the replacement only when
    /// that workspace holds its keyboard focus; in every other workspace the
    /// new active tab is that workspace's memory and reaches Hide only through
    /// a `workspace.get` read, which the coordinator issues for each of these.
    pub(crate) fn workspaces_awaiting_active_tab(&self) -> Vec<String> {
        self.pending_active_tab_focuses.iter().cloned().collect()
    }

    /// Applies a `workspace.get` answer to a workspace waiting for its
    /// replacement active tab, and reports whether that wait ended. A
    /// workspace a focus event already settled changes nothing. A tab the
    /// event stream has not delivered yet means the read ran ahead of the
    /// cursor, so the wait stays and the coordinator reads again on its
    /// bounded tick once the stream catches up.
    pub(crate) fn settle_active_tab(&mut self, workspace_id: &str, active_tab_id: &str) -> bool {
        if !self.pending_active_tab_focuses.contains(workspace_id) {
            return false;
        }
        let tab_known = self
            .state
            .tabs
            .iter()
            .any(|tab| tab.tab_id == active_tab_id && tab.workspace_id == workspace_id);
        if !tab_known {
            return false;
        }
        let Some(workspace) = self
            .state
            .workspaces
            .iter_mut()
            .find(|workspace| workspace.workspace_id == workspace_id)
        else {
            return false;
        };
        workspace.active_tab_id = active_tab_id.to_owned();
        self.pending_active_tab_focuses.remove(workspace_id);
        true
    }

    pub(crate) fn ready_to_publish(&self) -> bool {
        self.pending_layouts.is_empty()
            && self.pending_workspace_closures.is_empty()
            && self.pending_active_tab_focuses.is_empty()
    }

    /// Replaces the agent list and reports where agents stopped working.
    ///
    /// An agent leaving `working` is the moment its checkout's pull request is
    /// most likely to have just changed - it is what a run that ends in a push
    /// looks like from here - so it is the one event that re-reads `gh`
    /// without waiting out the five-minute window. Detecting it here rather
    /// than in the runtime keeps the comparison on the thread that already
    /// holds both the old and the new list.
    ///
    /// Returns the working directory of each agent that stopped, taken from
    /// the list it was working in: a pane that has since closed is gone from
    /// the new list and its old record is the one that still says where.
    pub(crate) fn replace_agents(&mut self, agents: Vec<ProjectedAgent>) -> Vec<String> {
        let stopped_in: Vec<String> = self
            .state
            .agents
            .iter()
            .filter(|agent| agent.agent_status.as_deref() == Some("working"))
            .filter(|agent| {
                !agents.iter().any(|next| {
                    next.pane_id == agent.pane_id && next.agent_status.as_deref() == Some("working")
                })
            })
            .filter_map(|agent| agent.cwd.clone())
            .collect();
        self.state.agents = agents;
        stopped_in
    }

    pub(crate) fn apply(
        &mut self,
        event: ReplicaEvent,
        mode: ApplyMode,
    ) -> Result<ApplyOutcome, SessionFetchError> {
        let mut candidate = self.clone();
        let refresh_worktrees = matches!(
            event,
            ReplicaEvent::WorktreeCreated { .. }
                | ReplicaEvent::WorktreeOpened { .. }
                | ReplicaEvent::WorktreeRemoved { .. }
        );
        let refresh_agents = match (candidate.apply_new_event(event), mode) {
            (Ok(refresh_agents), _) => refresh_agents,
            (Err(SessionFetchError::Malformed(detail)), ApplyMode::Reconcile) => {
                crate::diagnostic!(json!({
                    "component": "session_sync",
                    "kind": "event.reconciled",
                    "applied_events": self.applied_events,
                    "message": detail,
                }));
                return Ok(ApplyOutcome {
                    publish: false,
                    refresh_agents: false,
                    refresh_worktrees: false,
                });
            }
            (Err(error), _) => return Err(error),
        };
        candidate.applied_events = candidate.applied_events.saturating_add(1);
        let publish = candidate.refresh_published_state()?;
        *self = candidate;
        Ok(ApplyOutcome {
            publish,
            refresh_agents,
            refresh_worktrees,
        })
    }

    fn apply_new_event(&mut self, data: ReplicaEvent) -> Result<bool, SessionFetchError> {
        match data {
            ReplicaEvent::WorkspaceCreated {
                workspace: input_workspace,
            } => {
                let event = "workspace_created";
                validate_workspace_wire(event, &input_workspace)?;
                if self
                    .state
                    .workspaces
                    .iter()
                    .any(|workspace| workspace.workspace_id == input_workspace.workspace_id)
                {
                    return Err(malformed_event(event, "created workspace already exists"));
                }
                self.pending_layouts
                    .insert(input_workspace.active_tab_id.clone());
                self.state.workspaces.push(input_workspace);
            }
            ReplicaEvent::WorkspaceUpdated {
                workspace: input_workspace,
            } => {
                let event = "workspace_updated";
                validate_workspace_wire(event, &input_workspace)?;
                let workspace = self
                    .state
                    .workspaces
                    .iter_mut()
                    .find(|workspace| workspace.workspace_id == input_workspace.workspace_id)
                    .ok_or_else(|| malformed_event(event, "updated workspace does not exist"))?;
                *workspace = input_workspace;
            }
            ReplicaEvent::WorkspaceRenamed {
                workspace_id: input_workspace_id,
                label: input_label,
            } => {
                let event = "workspace_renamed";
                let workspace = self
                    .state
                    .workspaces
                    .iter_mut()
                    .find(|workspace| workspace.workspace_id == input_workspace_id)
                    .ok_or_else(|| malformed_event(event, "renamed workspace does not exist"))?;
                workspace.label = input_label;
            }
            ReplicaEvent::WorkspaceMoved {
                workspace_id: input_workspace_id,
                insert_index: input_insert_index,
                workspaces: input_workspaces,
            } => {
                let event = "workspace_moved";
                ensure_non_empty(event, "workspace_id", &input_workspace_id)?;
                for workspace in &input_workspaces {
                    validate_workspace_wire(event, workspace)?;
                }
                if input_insert_index > input_workspaces.len()
                    || !input_workspaces
                        .iter()
                        .any(|workspace| workspace.workspace_id == input_workspace_id)
                {
                    return Err(malformed_event(
                        event,
                        "resulting workspace order does not contain the moved workspace at a valid index",
                    ));
                }
                self.state.workspaces = input_workspaces;
            }
            ReplicaEvent::WorkspaceReordered {
                workspace_ids: input_workspace_ids,
                workspaces: input_workspaces,
            } => {
                let event = "workspace_reordered";
                for workspace in &input_workspaces {
                    validate_workspace_wire(event, workspace)?;
                }
                if input_workspace_ids.is_empty()
                    || input_workspace_ids.iter().any(|workspace_id| {
                        workspace_id.trim().is_empty()
                            || !input_workspaces
                                .iter()
                                .any(|workspace| &workspace.workspace_id == workspace_id)
                    })
                {
                    return Err(malformed_event(
                        event,
                        "resulting workspace order does not contain every reordered workspace",
                    ));
                }
                self.state.workspaces = input_workspaces;
            }
            ReplicaEvent::WorkspaceClosed {
                workspace_id: input_workspace_id,
            } => {
                let event = "workspace_closed";
                ensure_non_empty(event, "workspace_id", &input_workspace_id)?;
                if !self
                    .state
                    .workspaces
                    .iter()
                    .any(|workspace| workspace.workspace_id == input_workspace_id)
                {
                    return Err(malformed_event(event, "closed workspace does not exist"));
                }
                self.remove_workspace(&input_workspace_id);
            }
            ReplicaEvent::WorkspaceFocused {
                workspace_id: input_workspace_id,
            } => {
                let event = "workspace_focused";
                ensure_non_empty(event, "workspace_id", &input_workspace_id)?;
                if !self
                    .state
                    .workspaces
                    .iter()
                    .any(|workspace| workspace.workspace_id == input_workspace_id)
                {
                    return Err(malformed_event(event, "focused workspace does not exist"));
                }
                self.state.focused_workspace_id = Some(input_workspace_id);
            }
            ReplicaEvent::WorktreeCreated {
                workspace: input_workspace,
            } => {
                let event = "worktree_created";
                validate_workspace_wire(event, &input_workspace)?;
                upsert_workspace(&mut self.state.workspaces, input_workspace);
            }
            ReplicaEvent::WorktreeOpened {
                workspace: input_workspace,
            } => {
                let event = "worktree_opened";
                validate_workspace_wire(event, &input_workspace)?;
                upsert_workspace(&mut self.state.workspaces, input_workspace);
            }
            ReplicaEvent::WorktreeRemoved {
                workspace_id: input_workspace_id,
                workspace: input_workspace,
            } => {
                let event = "worktree_removed";
                ensure_non_empty(event, "workspace_id", &input_workspace_id)?;
                if let Some(workspace) = input_workspace {
                    validate_workspace_wire(event, &workspace)?;
                    if workspace.workspace_id != input_workspace_id {
                        return Err(malformed_event(
                            event,
                            "workspace does not match workspace_id",
                        ));
                    }
                    upsert_workspace(&mut self.state.workspaces, workspace);
                }
            }
            ReplicaEvent::TabCreated { tab: input_tab } => {
                let event = "tab_created";
                validate_tab_wire(event, &input_tab)?;
                if !self
                    .state
                    .workspaces
                    .iter()
                    .any(|workspace| workspace.workspace_id == input_tab.workspace_id)
                {
                    return Err(malformed_event(
                        event,
                        "created tab references a missing workspace",
                    ));
                }
                if self
                    .state
                    .tabs
                    .iter()
                    .any(|tab| tab.tab_id == input_tab.tab_id)
                {
                    return Err(malformed_event(event, "created tab already exists"));
                }
                self.pending_layouts.insert(input_tab.tab_id.clone());
                self.state.tabs.push(input_tab);
            }
            ReplicaEvent::TabClosed {
                workspace_id: input_workspace_id,
                tab_id: input_tab_id,
            } => {
                let event = "tab_closed";
                ensure_non_empty(event, "workspace_id", &input_workspace_id)?;
                let workspace_tab_count = self
                    .state
                    .tabs
                    .iter()
                    .filter(|tab| tab.workspace_id == input_workspace_id)
                    .count();
                let tab = self
                    .state
                    .tabs
                    .iter()
                    .find(|tab| tab.tab_id == input_tab_id)
                    .ok_or_else(|| malformed_event(event, "closed tab does not exist"))?;
                if tab.workspace_id != input_workspace_id {
                    return Err(malformed_event(
                        event,
                        "closed tab belongs to another workspace",
                    ));
                }
                let active_tab_closed = self
                    .state
                    .workspaces
                    .iter()
                    .find(|workspace| workspace.workspace_id == input_workspace_id)
                    .is_some_and(|workspace| workspace.active_tab_id == input_tab_id);
                if workspace_tab_count == 1 {
                    self.pending_workspace_closures
                        .insert(input_workspace_id.clone());
                } else if active_tab_closed {
                    self.pending_active_tab_focuses
                        .insert(input_workspace_id.clone());
                }
                self.remove_tab(&input_tab_id);
            }
            ReplicaEvent::TabRenamed {
                workspace_id: input_workspace_id,
                tab_id: input_tab_id,
                label: input_label,
            } => {
                let event = "tab_renamed";
                let tab = self
                    .state
                    .tabs
                    .iter_mut()
                    .find(|tab| tab.tab_id == input_tab_id)
                    .ok_or_else(|| malformed_event(event, "renamed tab does not exist"))?;
                if tab.workspace_id != input_workspace_id {
                    return Err(malformed_event(
                        event,
                        "renamed tab belongs to another workspace",
                    ));
                }
                tab.label = input_label;
            }
            ReplicaEvent::TabMoved {
                workspace_id: input_workspace_id,
                tab_id: input_tab_id,
                insert_index: input_insert_index,
                tabs: input_tabs,
            } => {
                let event = "tab_moved";
                ensure_non_empty(event, "workspace_id", &input_workspace_id)?;
                ensure_non_empty(event, "tab_id", &input_tab_id)?;
                for tab in &input_tabs {
                    validate_tab_wire(event, tab)?;
                }
                if input_insert_index > input_tabs.len()
                    || input_tabs
                        .iter()
                        .any(|tab| tab.workspace_id != input_workspace_id)
                    || !input_tabs.iter().any(|tab| tab.tab_id == input_tab_id)
                {
                    return Err(malformed_event(
                        event,
                        "resulting tab order is inconsistent with the moved tab",
                    ));
                }
                self.state
                    .tabs
                    .retain(|tab| tab.workspace_id != input_workspace_id);
                self.state.tabs.extend(input_tabs);
            }
            ReplicaEvent::TabFocused {
                workspace_id: input_workspace_id,
                tab_id: input_tab_id,
            } => {
                let event = "tab_focused";
                ensure_non_empty(event, "workspace_id", &input_workspace_id)?;
                ensure_non_empty(event, "tab_id", &input_tab_id)?;
                let workspace = self
                    .state
                    .workspaces
                    .iter_mut()
                    .find(|workspace| workspace.workspace_id == input_workspace_id)
                    .ok_or_else(|| malformed_event(event, "focused workspace does not exist"))?;
                let tab = self
                    .state
                    .tabs
                    .iter()
                    .find(|tab| tab.tab_id == input_tab_id)
                    .ok_or_else(|| malformed_event(event, "focused tab does not exist"))?;
                if tab.workspace_id != input_workspace_id {
                    return Err(malformed_event(
                        event,
                        "focused tab belongs to another workspace",
                    ));
                }
                workspace.active_tab_id = input_tab_id;
                self.state.focused_workspace_id = Some(input_workspace_id.clone());
                self.pending_active_tab_focuses.remove(&input_workspace_id);
            }
            ReplicaEvent::PaneCreated { pane: input_pane } => {
                let event = "pane_created";
                validate_pane_wire(event, &input_pane)?;
                if !self.state.tabs.iter().any(|tab| {
                    tab.tab_id == input_pane.tab_id && tab.workspace_id == input_pane.workspace_id
                }) {
                    return Err(malformed_event(
                        event,
                        "created pane references a missing tab",
                    ));
                }
                if self
                    .state
                    .panes
                    .iter()
                    .any(|pane| pane.pane_id == input_pane.pane_id)
                {
                    return Err(malformed_event(event, "created pane already exists"));
                }
                self.pending_layouts.insert(input_pane.tab_id.clone());
                self.state.panes.push(input_pane);
            }
            ReplicaEvent::PaneClosed {
                workspace_id: input_workspace_id,
                pane_id: input_pane_id,
            } => {
                let event = "pane_closed";
                ensure_non_empty(event, "workspace_id", &input_workspace_id)?;
                let pane = self
                    .state
                    .panes
                    .iter()
                    .find(|pane| pane.pane_id == input_pane_id)
                    .ok_or_else(|| malformed_event(event, "closed pane does not exist"))?;
                if pane.workspace_id != input_workspace_id {
                    return Err(malformed_event(
                        event,
                        "closed pane belongs to another workspace",
                    ));
                }
                let tab_id = pane.tab_id.clone();
                let last_pane_in_tab = self
                    .state
                    .panes
                    .iter()
                    .filter(|candidate| candidate.tab_id == tab_id)
                    .count()
                    == 1;
                let workspace_tab_count = self
                    .state
                    .tabs
                    .iter()
                    .filter(|tab| tab.workspace_id == input_workspace_id)
                    .count();
                if last_pane_in_tab && workspace_tab_count > 1 {
                    let active_tab_closed = self
                        .state
                        .workspaces
                        .iter()
                        .find(|workspace| workspace.workspace_id == input_workspace_id)
                        .is_some_and(|workspace| workspace.active_tab_id == tab_id);
                    if active_tab_closed {
                        self.pending_active_tab_focuses
                            .insert(input_workspace_id.clone());
                    }
                    // Herdr 0.8.2 removes an emptied tab as part of pane.close
                    // without emitting a separate tab.closed event. Mirror that
                    // authoritative cascade so the replica cannot wait forever
                    // for a layout.updated event for a tab that no longer exists.
                    self.remove_tab(&tab_id);
                } else {
                    self.pending_layouts.insert(tab_id);
                    self.remove_pane(&input_pane_id);
                }
            }
            ReplicaEvent::PaneUpdated { pane: input_pane } => {
                let event = "pane_updated";
                validate_pane_wire(event, &input_pane)?;
                let previous = self
                    .state
                    .panes
                    .iter()
                    .find(|pane| pane.pane_id == input_pane.pane_id)
                    .cloned()
                    .ok_or_else(|| malformed_event(event, "updated pane does not exist"))?;
                if previous.tab_id != input_pane.tab_id
                    || previous.workspace_id != input_pane.workspace_id
                {
                    self.pending_layouts.insert(previous.tab_id);
                    self.pending_layouts.insert(input_pane.tab_id.clone());
                }
                upsert_pane(&mut self.state.panes, input_pane);
            }
            ReplicaEvent::PaneFocused {
                workspace_id: input_workspace_id,
                pane_id: input_pane_id,
            } => {
                let event = "pane_focused";
                ensure_non_empty(event, "workspace_id", &input_workspace_id)?;
                if !self.state.panes.iter().any(|pane| {
                    pane.pane_id == input_pane_id && pane.workspace_id == input_workspace_id
                }) {
                    return Err(malformed_event(
                        event,
                        "focused pane does not exist in the stated workspace",
                    ));
                }
                self.state.focused_pane_id = Some(input_pane_id.clone());
                self.state.focused_workspace_id = Some(input_workspace_id.clone());
                if let Some(layout) = self.state.layouts.iter_mut().find(|layout| {
                    layout
                        .panes
                        .iter()
                        .any(|pane| pane.pane_id == input_pane_id)
                }) {
                    layout.focused_pane_id = input_pane_id;
                }
            }
            ReplicaEvent::PaneMoved(payload) => {
                self.apply_pane_moved(payload)?;
                return Ok(true);
            }
            ReplicaEvent::PaneExited {
                workspace_id: input_workspace_id,
                pane_id: input_pane_id,
            } => {
                let event = "pane_exited";
                ensure_non_empty(event, "workspace_id", &input_workspace_id)?;
                ensure_non_empty(event, "pane_id", &input_pane_id)?;
            }
            ReplicaEvent::PaneAgentDetected {
                workspace_id: input_workspace_id,
                pane_id: input_pane_id,
            } => {
                let event = "pane_agent_detected";
                ensure_non_empty(event, "workspace_id", &input_workspace_id)?;
                ensure_non_empty(event, "pane_id", &input_pane_id)?;
                return Ok(true);
            }
            ReplicaEvent::LayoutUpdated {
                layout: input_layout,
            } => {
                let event = "layout_updated";
                ensure_non_empty(event, "workspace_id", &input_layout.workspace_id)?;
                ensure_non_empty(event, "tab_id", &input_layout.tab_id)?;
                ensure_non_empty(event, "focused_pane_id", &input_layout.focused_pane_id)?;
                if !self.state.tabs.iter().any(|tab| {
                    tab.tab_id == input_layout.tab_id
                        && tab.workspace_id == input_layout.workspace_id
                }) {
                    return Err(malformed_event(event, "layout references a missing tab"));
                }
                let tab_id = input_layout.tab_id.clone();
                let focused_was_missing =
                    self.state.focused_pane_id.as_ref().is_none_or(|focused| {
                        !self.state.panes.iter().any(|pane| &pane.pane_id == focused)
                    });
                upsert_layout(&mut self.state.layouts, input_layout);
                self.pending_layouts.remove(&tab_id);
                if focused_was_missing
                    && let Some(layout) = self
                        .state
                        .layouts
                        .iter()
                        .find(|layout| layout.tab_id == tab_id)
                {
                    self.state.focused_pane_id = Some(layout.focused_pane_id.clone());
                }
            }
            ReplicaEvent::Unrequested(unknown) => {
                return Err(SessionFetchError::Malformed(format!(
                    "Herdr subscription emitted unrequested event {unknown:?}"
                )));
            }
        }
        Ok(false)
    }

    fn apply_pane_moved(&mut self, payload: PaneMove) -> Result<(), SessionFetchError> {
        ensure_non_empty("pane_moved", "previous_pane_id", &payload.previous_pane_id)?;
        ensure_non_empty(
            "pane_moved",
            "previous_workspace_id",
            &payload.previous_workspace_id,
        )?;
        ensure_non_empty("pane_moved", "previous_tab_id", &payload.previous_tab_id)?;
        validate_pane_wire("pane_moved", &payload.pane)?;
        if let Some(previous) = self
            .state
            .panes
            .iter()
            .find(|pane| pane.pane_id == payload.previous_pane_id)
            && (previous.workspace_id != payload.previous_workspace_id
                || previous.tab_id != payload.previous_tab_id)
        {
            return Err(malformed_event(
                "pane_moved",
                "previous pane scope does not match the replica",
            ));
        }
        let previous_agent = self
            .state
            .agents
            .iter()
            .find(|agent| agent.pane_id == payload.previous_pane_id)
            .cloned();
        let previous_tab_id = payload.previous_tab_id.clone();
        self.remove_pane(&payload.previous_pane_id);

        if let Some(workspace) = payload.created_workspace {
            validate_workspace_wire("pane_moved", &workspace)?;
            upsert_workspace(&mut self.state.workspaces, workspace);
        }
        if let Some(tab) = payload.created_tab {
            validate_tab_wire("pane_moved", &tab)?;
            self.pending_layouts.insert(tab.tab_id.clone());
            upsert_tab(&mut self.state.tabs, tab);
        }
        if let Some(tab_id) = payload.closed_tab_id.as_deref() {
            self.remove_tab(tab_id);
        } else {
            self.pending_layouts.insert(previous_tab_id);
        }
        if let Some(workspace_id) = payload.closed_workspace_id.as_deref() {
            self.remove_workspace(workspace_id);
        }

        self.pending_layouts.insert(payload.pane.tab_id.clone());
        let pane = payload.pane;
        if let Some(mut agent) = previous_agent {
            agent.pane_id = pane.pane_id.clone();
            agent.workspace_id = pane.workspace_id.clone();
            agent.tab_id = pane.tab_id.clone();
            agent.cwd = pane.cwd.clone();
            self.state
                .agents
                .retain(|candidate| candidate.pane_id != agent.pane_id);
            self.state.agents.push(agent);
        }
        upsert_pane(&mut self.state.panes, pane);
        Ok(())
    }

    fn remove_workspace(&mut self, workspace_id: &str) {
        let tab_ids = self
            .state
            .tabs
            .iter()
            .filter(|tab| tab.workspace_id == workspace_id)
            .map(|tab| tab.tab_id.clone())
            .collect::<HashSet<_>>();
        self.state
            .workspaces
            .retain(|workspace| workspace.workspace_id != workspace_id);
        if self.state.focused_workspace_id.as_deref() == Some(workspace_id) {
            self.state.focused_workspace_id = None;
        }
        self.state
            .tabs
            .retain(|tab| tab.workspace_id != workspace_id);
        self.state
            .panes
            .retain(|pane| pane.workspace_id != workspace_id);
        self.state
            .layouts
            .retain(|layout| layout.workspace_id != workspace_id);
        self.state
            .agents
            .retain(|agent| agent.workspace_id != workspace_id);
        self.pending_layouts
            .retain(|tab_id| !tab_ids.contains(tab_id));
        self.pending_workspace_closures.remove(workspace_id);
        self.pending_active_tab_focuses.remove(workspace_id);
        self.clear_missing_focus();
    }

    fn remove_tab(&mut self, tab_id: &str) {
        self.state.tabs.retain(|tab| tab.tab_id != tab_id);
        self.state.panes.retain(|pane| pane.tab_id != tab_id);
        self.state.layouts.retain(|layout| layout.tab_id != tab_id);
        self.state.agents.retain(|agent| agent.tab_id != tab_id);
        self.pending_layouts.remove(tab_id);
        self.clear_missing_focus();
    }

    fn remove_pane(&mut self, pane_id: &str) {
        self.state.panes.retain(|pane| pane.pane_id != pane_id);
        self.state.agents.retain(|agent| agent.pane_id != pane_id);
        self.clear_missing_focus();
    }

    fn clear_missing_focus(&mut self) {
        if self
            .state
            .focused_pane_id
            .as_ref()
            .is_some_and(|focused| !self.state.panes.iter().any(|pane| &pane.pane_id == focused))
        {
            self.state.focused_pane_id = None;
        }
    }

    fn validate_active_tabs(&self) -> Result<(), SessionFetchError> {
        let tabs_by_id = self
            .state
            .tabs
            .iter()
            .map(|tab| (tab.tab_id.as_str(), tab))
            .collect::<HashMap<_, _>>();
        for workspace in &self.state.workspaces {
            let Some(active_tab) = tabs_by_id.get(workspace.active_tab_id.as_str()) else {
                return Err(SessionFetchError::Malformed(format!(
                    "Herdr workspace {} references missing active tab {}",
                    workspace.workspace_id, workspace.active_tab_id
                )));
            };
            if active_tab.workspace_id != workspace.workspace_id {
                return Err(SessionFetchError::Malformed(format!(
                    "Herdr workspace {} references active tab {} from another workspace",
                    workspace.workspace_id, workspace.active_tab_id
                )));
            }
        }
        Ok(())
    }

    fn validate(&self) -> Result<(), SessionFetchError> {
        for workspace in &self.state.workspaces {
            if let Some(worktree) = &workspace.worktree {
                for (field, value) in [
                    ("repo_key", worktree.repo_key.as_str()),
                    ("repo_name", worktree.repo_name.as_str()),
                    ("repo_root", worktree.repo_root.as_str()),
                    ("checkout_path", worktree.checkout_path.as_str()),
                ] {
                    if value.trim().is_empty() {
                        return Err(SessionFetchError::Malformed(format!(
                            "Herdr workspace {} has an empty worktree {field}",
                            workspace.workspace_id
                        )));
                    }
                }
            }
        }
        let workspace_ids = unique_ids(
            "workspace",
            self.state
                .workspaces
                .iter()
                .map(|workspace| workspace.workspace_id.as_str()),
        )?;
        let tab_ids = unique_ids("tab", self.state.tabs.iter().map(|tab| tab.tab_id.as_str()))?;
        let pane_ids = unique_ids(
            "pane",
            self.state.panes.iter().map(|pane| pane.pane_id.as_str()),
        )?;
        let tabs_by_id = self
            .state
            .tabs
            .iter()
            .map(|tab| (tab.tab_id.as_str(), tab))
            .collect::<HashMap<_, _>>();
        for tab in &self.state.tabs {
            if !workspace_ids.contains(tab.workspace_id.as_str()) {
                return Err(SessionFetchError::Malformed(format!(
                    "Herdr tab {} references missing workspace {}",
                    tab.tab_id, tab.workspace_id
                )));
            }
        }
        let panes_by_id = self
            .state
            .panes
            .iter()
            .map(|pane| (pane.pane_id.as_str(), pane))
            .collect::<HashMap<_, _>>();
        for pane in &self.state.panes {
            if !workspace_ids.contains(pane.workspace_id.as_str())
                || !tab_ids.contains(pane.tab_id.as_str())
            {
                return Err(SessionFetchError::Malformed(format!(
                    "Herdr pane {} references missing workspace or tab",
                    pane.pane_id
                )));
            }
            if tabs_by_id
                .get(pane.tab_id.as_str())
                .is_some_and(|tab| tab.workspace_id != pane.workspace_id)
            {
                return Err(SessionFetchError::Malformed(format!(
                    "Herdr pane {} references a tab from another workspace",
                    pane.pane_id
                )));
            }
        }
        let mut layout_tabs = HashSet::new();
        let mut laid_out_panes = HashSet::new();
        for layout in &self.state.layouts {
            if !layout_tabs.insert(layout.tab_id.as_str()) {
                return Err(SessionFetchError::Malformed(format!(
                    "Herdr session contains duplicate layout for tab {}",
                    layout.tab_id
                )));
            }
            if !workspace_ids.contains(layout.workspace_id.as_str())
                || !tab_ids.contains(layout.tab_id.as_str())
            {
                return Err(SessionFetchError::Malformed(format!(
                    "Herdr layout {} references missing workspace or tab",
                    layout.tab_id
                )));
            }
            if tabs_by_id
                .get(layout.tab_id.as_str())
                .is_some_and(|tab| tab.workspace_id != layout.workspace_id)
            {
                return Err(SessionFetchError::Malformed(format!(
                    "Herdr layout {} belongs to the wrong workspace",
                    layout.tab_id
                )));
            }
            if !layout
                .panes
                .iter()
                .any(|pane| pane.pane_id == layout.focused_pane_id)
            {
                return Err(SessionFetchError::Malformed(format!(
                    "Herdr layout {} focuses a pane outside that layout",
                    layout.tab_id
                )));
            }
            for layout_pane in &layout.panes {
                let Some(pane) = panes_by_id.get(layout_pane.pane_id.as_str()) else {
                    return Err(SessionFetchError::Malformed(format!(
                        "Herdr layout {} references missing pane {}",
                        layout.tab_id, layout_pane.pane_id
                    )));
                };
                if pane.tab_id != layout.tab_id || pane.workspace_id != layout.workspace_id {
                    return Err(SessionFetchError::Malformed(format!(
                        "Herdr layout {} contains pane {} from another tab",
                        layout.tab_id, layout_pane.pane_id
                    )));
                }
                if !laid_out_panes.insert(layout_pane.pane_id.as_str()) {
                    return Err(SessionFetchError::Malformed(format!(
                        "Herdr pane {} appears in multiple layouts",
                        layout_pane.pane_id
                    )));
                }
            }
        }
        if let Some(missing_tab_id) = tab_ids
            .iter()
            .find(|tab_id| !layout_tabs.contains(**tab_id))
        {
            return Err(SessionFetchError::Malformed(format!(
                "Herdr tab {missing_tab_id} has no layout"
            )));
        }
        if let Some(missing_pane_id) = pane_ids
            .iter()
            .find(|pane_id| !laid_out_panes.contains(**pane_id))
        {
            return Err(SessionFetchError::Malformed(format!(
                "Herdr pane {missing_pane_id} is absent from every layout"
            )));
        }
        if let Some(focused) = self.state.focused_pane_id.as_deref()
            && !pane_ids.contains(focused)
        {
            return Err(SessionFetchError::Malformed(format!(
                "Herdr session focuses missing pane {focused}"
            )));
        }
        Ok(())
    }
}

fn remote_workspace_id(target_id: &str, workspace_id: &str) -> String {
    format!("remote:{target_id}:workspace:{workspace_id}")
}

fn remote_checkout_id(target_id: &str, workspace_id: &str) -> String {
    format!("remote:{target_id}:checkout:{workspace_id}")
}

pub(crate) fn remote_tab_id(target_id: &str, tab_id: &str) -> String {
    format!("remote:{target_id}:tab:{tab_id}")
}

fn remote_pane_id(target_id: &str, pane_id: &str) -> String {
    format!("remote:{target_id}:pane:{pane_id}")
}

fn unique_ids<'a>(
    kind: &str,
    ids: impl Iterator<Item = &'a str>,
) -> Result<HashSet<&'a str>, SessionFetchError> {
    let mut unique = HashSet::new();
    for id in ids {
        if id.trim().is_empty() {
            return Err(SessionFetchError::Malformed(format!(
                "Herdr {kind} id is empty"
            )));
        }
        if !unique.insert(id) {
            return Err(SessionFetchError::Malformed(format!(
                "Herdr session contains duplicate {kind} id {id}"
            )));
        }
    }
    Ok(unique)
}

fn ensure_non_empty(event: &str, field: &str, value: &str) -> Result<(), SessionFetchError> {
    if value.trim().is_empty() {
        Err(malformed_event(event, &format!("has empty {field}")))
    } else {
        Ok(())
    }
}

fn validate_workspace_wire(
    event: &str,
    workspace: &ProjectedWorkspace,
) -> Result<(), SessionFetchError> {
    ensure_non_empty(event, "workspace.workspace_id", &workspace.workspace_id)?;
    ensure_non_empty(event, "workspace.active_tab_id", &workspace.active_tab_id)?;
    if let Some(worktree) = &workspace.worktree {
        ensure_non_empty(event, "workspace.worktree.repo_key", &worktree.repo_key)?;
        ensure_non_empty(event, "workspace.worktree.repo_name", &worktree.repo_name)?;
        ensure_non_empty(event, "workspace.worktree.repo_root", &worktree.repo_root)?;
        ensure_non_empty(
            event,
            "workspace.worktree.checkout_path",
            &worktree.checkout_path,
        )?;
    }
    Ok(())
}

fn validate_tab_wire(event: &str, tab: &ProjectedTab) -> Result<(), SessionFetchError> {
    ensure_non_empty(event, "tab.tab_id", &tab.tab_id)?;
    ensure_non_empty(event, "tab.workspace_id", &tab.workspace_id)
}

fn validate_pane_wire(event: &str, pane: &ProjectedPane) -> Result<(), SessionFetchError> {
    ensure_non_empty(event, "pane.pane_id", &pane.pane_id)?;
    ensure_non_empty(event, "pane.workspace_id", &pane.workspace_id)?;
    ensure_non_empty(event, "pane.tab_id", &pane.tab_id)
}

fn upsert_workspace(workspaces: &mut Vec<ProjectedWorkspace>, workspace: ProjectedWorkspace) {
    if let Some(existing) = workspaces
        .iter_mut()
        .find(|existing| existing.workspace_id == workspace.workspace_id)
    {
        *existing = workspace;
    } else {
        workspaces.push(workspace);
    }
}

fn upsert_tab(tabs: &mut Vec<ProjectedTab>, tab: ProjectedTab) {
    if let Some(existing) = tabs
        .iter_mut()
        .find(|existing| existing.tab_id == tab.tab_id)
    {
        *existing = tab;
    } else {
        tabs.push(tab);
    }
}

fn upsert_pane(panes: &mut Vec<ProjectedPane>, pane: ProjectedPane) {
    if let Some(existing) = panes
        .iter_mut()
        .find(|existing| existing.pane_id == pane.pane_id)
    {
        *existing = pane;
    } else {
        panes.push(pane);
    }
}

fn upsert_layout(layouts: &mut Vec<SessionLayoutPayload>, layout: SessionLayoutPayload) {
    if let Some(existing) = layouts
        .iter_mut()
        .find(|existing| existing.tab_id == layout.tab_id)
    {
        *existing = layout;
    } else {
        layouts.push(layout);
    }
}

/// Inputs to replica transitions, with transport details removed by the boundary.
// The generated event boundary projects owned values directly into this
// transition enum; changing its representation belongs with measured sync work.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug)]
pub(crate) enum ReplicaEvent {
    WorkspaceCreated {
        workspace: ProjectedWorkspace,
    },
    WorkspaceUpdated {
        workspace: ProjectedWorkspace,
    },
    WorkspaceRenamed {
        workspace_id: String,
        label: String,
    },
    WorkspaceMoved {
        workspace_id: String,
        insert_index: usize,
        workspaces: Vec<ProjectedWorkspace>,
    },
    WorkspaceReordered {
        workspace_ids: Vec<String>,
        workspaces: Vec<ProjectedWorkspace>,
    },
    WorkspaceClosed {
        workspace_id: String,
    },
    WorkspaceFocused {
        workspace_id: String,
    },
    WorktreeCreated {
        workspace: ProjectedWorkspace,
    },
    WorktreeOpened {
        workspace: ProjectedWorkspace,
    },
    WorktreeRemoved {
        workspace_id: String,
        workspace: Option<ProjectedWorkspace>,
    },
    TabCreated {
        tab: ProjectedTab,
    },
    TabClosed {
        workspace_id: String,
        tab_id: String,
    },
    TabRenamed {
        workspace_id: String,
        tab_id: String,
        label: String,
    },
    TabMoved {
        workspace_id: String,
        tab_id: String,
        insert_index: usize,
        tabs: Vec<ProjectedTab>,
    },
    TabFocused {
        workspace_id: String,
        tab_id: String,
    },
    PaneCreated {
        pane: ProjectedPane,
    },
    PaneClosed {
        workspace_id: String,
        pane_id: String,
    },
    PaneUpdated {
        pane: ProjectedPane,
    },
    PaneFocused {
        workspace_id: String,
        pane_id: String,
    },
    PaneMoved(PaneMove),
    PaneExited {
        workspace_id: String,
        pane_id: String,
    },
    PaneAgentDetected {
        workspace_id: String,
        pane_id: String,
    },
    LayoutUpdated {
        layout: SessionLayoutPayload,
    },
    Unrequested(String),
}
#[derive(Clone, Debug)]
pub(crate) struct PaneMove {
    pub(crate) previous_pane_id: String,
    pub(crate) previous_workspace_id: String,
    pub(crate) previous_tab_id: String,
    pub(crate) pane: ProjectedPane,
    pub(crate) created_workspace: Option<ProjectedWorkspace>,
    pub(crate) created_tab: Option<ProjectedTab>,
    pub(crate) closed_workspace_id: Option<String>,
    pub(crate) closed_tab_id: Option<String>,
}

// Boxing Event would add an allocation to every subscription line merely to
// shrink the uncommon error variant's stack footprint.
#[allow(clippy::large_enum_variant)]
pub(crate) enum SubscriptionLine {
    Event(ReplicaEvent),
    Error { code: String, message: String },
}

fn malformed_event(event: &str, detail: &str) -> SessionFetchError {
    SessionFetchError::Malformed(format!("Herdr {event} event {detail}"))
}
