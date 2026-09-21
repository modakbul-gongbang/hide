use super::*;

fn suppress_unconfirmed_created_purposes(
    pending: &HashMap<String, String>,
    workspaces: &mut [crate::model::WorkspaceSnapshot],
    agents: &[crate::model::SidebarAgentSnapshot],
) -> bool {
    use crate::model::CheckoutPurposeOrigin;

    let mut changed = false;
    for checkout in workspaces
        .iter_mut()
        .flat_map(|workspace| workspace.checkouts.iter_mut())
    {
        let path = workspace::normalized_for_comparison(Path::new(&checkout.path));
        let Some(unconfirmed) = pending.get(&path) else {
            continue;
        };
        if checkout.purpose.as_ref().is_some_and(|purpose| {
            purpose.origin == CheckoutPurposeOrigin::Token && purpose.text == *unconfirmed
        }) {
            checkout.purpose = None;
            changed = true;
        }
    }
    if changed {
        changed |= crate::sidebar::sync_checkout_purposes(workspaces, agents);
    }
    changed
}

impl Runtime {
    pub(crate) fn unconfirmed_created_purpose_values(&self) -> HashMap<String, String> {
        let mut suppressions = self.unconfirmed_created_purposes.clone();
        suppressions.extend(self.created_purpose_writes_in_flight.clone());
        suppressions
    }

    /// Registers the exact value a creation worker is about to write before
    /// the Herdr token request can publish an event to the purpose mirror.
    pub(crate) fn begin_created_purpose_write(&mut self, path: &str, purpose: &str) {
        let path = workspace::normalized_for_comparison(Path::new(path));
        self.created_purpose_writes_in_flight
            .insert(path, purpose.to_owned());
    }

    /// Groups the session's working directories under the Herdr workspace that
    /// owns them. Shared with the session-sync coordinator so a catalog
    /// precomputed outside the runtime lock is built from the same inputs.
    ///
    /// Layouts carry the workspace a pane belongs to and `panes` carries its
    /// directory, so the two together give each workspace the set of
    /// repositories it actually occupies without asking git anything.
    /// The Herdr workspaces and the directories their panes occupy, as the
    /// project catalog sees them.
    ///
    pub fn session_spaces(payload: &SessionSnapshotPayload) -> Vec<workspace::SessionSpace> {
        let labels: HashMap<&str, &str> = payload
            .workspaces
            .iter()
            .map(|workspace| (workspace.workspace_id.as_str(), workspace.label.trim()))
            .collect();
        let mut spaces: Vec<workspace::SessionSpace> = Vec::new();
        for layout in &payload.layouts {
            let index = match spaces
                .iter()
                .position(|space| space.id == layout.workspace_id)
            {
                Some(index) => index,
                None => {
                    let label = labels
                        .get(layout.workspace_id.as_str())
                        .copied()
                        .filter(|label| !label.is_empty())
                        .unwrap_or(layout.workspace_id.as_str())
                        .to_owned();
                    spaces.push(workspace::SessionSpace {
                        id: layout.workspace_id.clone(),
                        label,
                        cwds: Vec::new(),
                        purpose: payload
                            .workspaces
                            .iter()
                            .find(|workspace| workspace.workspace_id == layout.workspace_id)
                            .and_then(|workspace| workspace.tokens.get("purpose"))
                            .and_then(serde_json::Value::as_str)
                            .map(str::trim)
                            .filter(|purpose| !purpose.is_empty())
                            .map(str::to_owned),
                    });
                    spaces.len() - 1
                }
            };
            for pane in &layout.panes {
                let Some(cwd) = Self::pane_cwd(payload, &pane.pane_id) else {
                    continue;
                };
                if !spaces[index].cwds.contains(&cwd) {
                    spaces[index].cwds.push(cwd);
                }
            }
        }
        spaces
    }
    pub(super) fn pane_cwd(payload: &SessionSnapshotPayload, pane_id: &str) -> Option<String> {
        payload
            .panes
            .iter()
            .find(|pane| pane.pane_id == pane_id)
            .and_then(|pane| pane.cwd.clone())
            .or_else(|| {
                payload
                    .agents
                    .iter()
                    .find(|agent| agent.pane_id.as_deref().or(agent.id.as_deref()) == Some(pane_id))
                    .and_then(|agent| agent.cwd.clone())
            })
            .map(|cwd| cwd.trim().to_owned())
            // Herdr reports `/` for a pane whose process has exited, which
            // says where the pane is not rather than where it is. Treating it
            // as a directory produced an unnamed checkout row with no panes
            // under it.
            .filter(|cwd| !cwd.is_empty() && cwd != "/")
    }
    /// Rebuilds the navigator from the Herdr workspaces the session reports
    /// and any registration Herdr has no workspace for. Registrations are
    /// durable metadata only: removing one never removes a checkout or ends a
    /// remote process.
    pub(super) fn reconcile_session_catalog(
        &mut self,
        payload: &SessionSnapshotPayload,
        precomputed: Option<session_sync::PrecomputedCatalog>,
    ) -> bool {
        self.last_session_spaces = Self::session_spaces(payload);
        self.issue_tokens = crate::wire::issue_tokens(payload);
        // The catalog and the root index shell out to git, so the sync
        // coordinator builds them before taking the runtime lock. A
        // precomputation whose registrations no longer match current state is
        // stale; the last accepted catalog stands in for it and the next
        // publish, a second away, brings a fresh one. It is not rebuilt here:
        // one `git rev-parse` per tab under this lock stalled the main thread
        // and every attach reader for a third of their time (2026-09-06,
        // 18 agents, load 7 to 11). Only a runtime that has never accepted a
        // catalog builds one inline, which is the fixture and test path.
        let (mut workspaces, roots) = match precomputed {
            Some(catalog)
                if catalog.registrations == self.snapshot.ui_state.workspace_registrations =>
            {
                self.last_accepted_catalog = Some(catalog.workspaces.clone());
                self.catalog_roots = catalog.roots.clone();
                (catalog.workspaces, catalog.roots)
            }
            Some(_) if self.last_accepted_catalog.is_some() => {
                self.push_diagnostic(
                    "catalog.precomputed_stale",
                    "The precomputed workspace catalog no longer matches the registrations; the last accepted catalog stands until the next publish".to_owned(),
                );
                (
                    self.last_accepted_catalog
                        .clone()
                        .expect("checked by the match guard"),
                    self.catalog_roots.clone(),
                )
            }
            _ => {
                let workspaces = workspace::build_catalog(
                    &self.snapshot.ui_state.workspace_registrations,
                    &self.last_session_spaces,
                    &self.worktree_catalog,
                );
                let roots = workspace::root_index(&self.last_session_spaces);
                self.last_accepted_catalog = Some(workspaces.clone());
                self.catalog_roots = roots.clone();
                (workspaces, roots)
            }
        };
        let mut unresolved_roots: Vec<String> = Vec::new();
        Self::apply_workspace_expansion(
            &mut workspaces,
            &self.snapshot.ui_state.collapsed_workspace_ids,
        );
        let projected_agents = project_agents(payload.clone()).agents;
        let listening_ports = self.listening_ports.entries.clone();

        // Every workspace's whole tab list, in Herdr's order, before any of it
        // is split across checkouts. A tab whose layout has not arrived yet is
        // in it, because Herdr counts it when it indexes a move. Rebuilt whole
        // each reconcile so a closed workspace leaves no stale order behind.
        self.herdr_workspace_tab_order = payload.tabs.iter().fold(
            BTreeMap::<String, Vec<String>>::new(),
            |mut order, session_tab| {
                order
                    .entry(session_tab.workspace_id.clone())
                    .or_default()
                    .push(session_tab.tab_id.clone());
                order
            },
        );

        // Herdr's tab order is the navigator's tab order. A layout is the
        // per-tab detail looked up by tab id, never what decides where a tab
        // sits: layouts arrive in the order each tab was first drawn, so a tab
        // Herdr moved kept its original place forever and a tab that redrew
        // never moved back.
        let mut placed_tabs: BTreeMap<&str, usize> = BTreeMap::new();
        // Herdr's own labels, kept per checkout while they are still raw. The
        // snapshot's tabs carry the formatted form, so the free number has to
        // be taken here or read back out of display text later.
        let mut raw_tab_labels: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for session_tab in &payload.tabs {
            let Some(layout) = payload
                .layouts
                .iter()
                .find(|layout| layout.tab_id == session_tab.tab_id)
            else {
                continue;
            };
            // A plain terminal pane is not necessarily represented in the
            // agent list. Its cwd is still authoritative for attaching the
            // live layout to the registered checkout. Falling back to the
            // agent record keeps agent-specific cwd handling intact.
            let context_path = layout.panes.iter().find_map(|pane| {
                payload
                    .panes
                    .iter()
                    .find(|source| source.pane_id == pane.pane_id)
                    .and_then(|source| source.cwd.clone())
                    .filter(|path| !path.trim().is_empty())
                    .or_else(|| {
                        payload
                            .agents
                            .iter()
                            .find(|agent| {
                                agent.pane_id.as_deref().or(agent.id.as_deref())
                                    == Some(pane.pane_id.as_str())
                            })
                            .and_then(|agent| agent.cwd.clone())
                    })
            });
            let Some(workspace_snapshot) = find_workspace_for_context(
                &mut workspaces,
                context_path.as_deref(),
                &layout.workspace_id,
                &roots,
                &mut unresolved_roots,
            ) else {
                continue;
            };
            let checkout_index = context_path.as_deref().and_then(|path| {
                workspace_snapshot
                    .checkouts
                    .iter()
                    .position(|checkout| path_is_within_checkout(path, &checkout.path))
            });
            let Some(checkout) = workspace_snapshot
                .checkouts
                .get_mut(checkout_index.unwrap_or(0))
            else {
                continue;
            };
            let panes = project_layout_panes(
                layout,
                payload,
                &projected_agents,
                &listening_ports,
                &checkout.path,
            );
            let tab = TabSnapshot {
                id: Some(session_tab.tab_id.clone()),
                workspace_id: Some(workspace_snapshot.id.clone()),
                checkout_id: Some(checkout.id.clone()),
                label: Some(crate::model::display_tab_label(
                    &session_tab.label,
                    &session_tab.tab_id,
                )),
                empty: panes.is_empty(),
                delegated: false,
                panes,
            };
            if let Some(existing) = checkout
                .tabs
                .iter_mut()
                .find(|existing| existing.id == tab.id)
            {
                *existing = tab;
            } else {
                checkout.tabs.push(tab);
            }
            *placed_tabs.entry(layout.workspace_id.as_str()).or_default() += 1;
            raw_tab_labels
                .entry(checkout.id.clone())
                .or_default()
                .push(session_tab.label.clone());
        }

        // A worktree earns its row from git, not from a pane, so which rows
        // have a terminal is only known once the tabs are attached.
        for workspace in &mut workspaces {
            for checkout in &mut workspace.checkouts {
                checkout.has_panes = checkout.tabs.iter().any(|tab| !tab.panes.is_empty());
            }
        }

        for workspace in &mut workspaces {
            for checkout in &mut workspace.checkouts {
                checkout.next_tab_label = crate::model::next_tab_label(
                    raw_tab_labels
                        .get(&checkout.id)
                        .map_or(&[][..], Vec::as_slice)
                        .iter()
                        .map(String::as_str),
                );
            }
        }

        // Herdr names one active tab per workspace. Hide owns which tab is
        // visible, so that name is what Hide reconciles against rather than
        // what it obeys: it confirms a switch Hide made, or it is an operator
        // focusing a tab outside Hide and Hide follows it and says so.
        // A checkout is keyed by path, so two Herdr workspaces at one path
        // land in one checkout with one active tab each. The view is kept
        // per Herdr workspace for that reason: folding it to one tab per
        // checkout let the last workspace in payload order overwrite the
        // others, and a tab focus on any other workspace was then never
        // confirmed and always followed back.
        if !unresolved_roots.is_empty() {
            unresolved_roots.sort();
            unresolved_roots.dedup();
            self.push_diagnostic(
                "catalog.root_unresolved",
                format!(
                    "{} pane director{} placed by path alone because the root index did not carry {}: {}",
                    unresolved_roots.len(),
                    if unresolved_roots.len() == 1 { "y was" } else { "ies were" },
                    if unresolved_roots.len() == 1 { "it" } else { "them" },
                    unresolved_roots.iter().take(3).cloned().collect::<Vec<_>>().join(", ")
                ),
            );
        }
        let mut unresolved_active_tabs = BTreeSet::new();
        let herdr_tabs = HerdrTabView::from_payload(payload);
        for session_workspace in &payload.workspaces {
            let Some(active_tab_id) = session_workspace
                .active_tab_id
                .as_deref()
                .map(str::trim)
                .filter(|active_tab_id| !active_tab_id.is_empty())
            else {
                continue;
            };
            let resolved = workspaces.iter().any(|workspace| {
                workspace.checkouts.iter().any(|checkout| {
                    checkout
                        .tabs
                        .iter()
                        .any(|tab| tab.id.as_deref() == Some(active_tab_id))
                })
            });
            // A workspace none of whose tabs reached the navigator is not a
            // contradiction, only a workspace outside every registered
            // checkout. An active tab missing from a workspace that did place
            // tabs is the state worth reporting.
            if !resolved
                && placed_tabs
                    .get(session_workspace.workspace_id.as_str())
                    .is_some_and(|placed| *placed > 0)
            {
                unresolved_active_tabs.insert(format!(
                    "{}/{active_tab_id}",
                    session_workspace.workspace_id
                ));
            }
        }
        self.report_unresolved_active_tabs(unresolved_active_tabs);
        self.reconcile_visible_tabs(&mut workspaces, &herdr_tabs);

        let previous = self.snapshot.navigator.clone();
        let previous_card = self.snapshot.card.clone();
        crate::sidebar::sync_checkout_agent_summaries(
            &mut workspaces,
            &self.snapshot.navigator.agents,
        );
        let created_purpose_suppressions = self.unconfirmed_created_purpose_values();
        suppress_unconfirmed_created_purposes(
            &created_purpose_suppressions,
            &mut workspaces,
            &self.snapshot.navigator.agents,
        );
        crate::project_context::sort_projects(&mut workspaces, &projected_agents);
        self.snapshot.navigator.workspaces = workspaces;
        self.snapshot.navigator.devices =
            workspace::devices(&self.snapshot.ui_state.device_registrations);
        self.refresh_device_snapshots();
        // An agent belongs to the device whose project holds its pane. The
        // project label is no longer Herdr's workspace label once a
        // registration covers the repository, so labels cannot be the key.
        for device in &mut self.snapshot.navigator.devices {
            device.agent_count = projected_agents
                .iter()
                .filter(|agent| {
                    self.snapshot
                        .navigator
                        .workspaces
                        .iter()
                        .filter(|workspace| workspace.device_id == device.id)
                        .flat_map(|workspace| workspace.checkouts.iter())
                        .flat_map(|checkout| checkout.tabs.iter())
                        .flat_map(|tab| tab.panes.iter())
                        .any(|pane| pane.id == agent.pane_id)
                })
                .count() as u32;
        }
        if self.snapshot.navigator.focused_device_id.is_none() {
            self.snapshot.navigator.focused_device_id = Some(workspace::LOCAL_DEVICE_ID.to_owned());
        }
        self.resync_navigator_focus();
        self.rebuild_tab_strips();
        previous != self.snapshot.navigator || previous_card != self.snapshot.card
    }
    /// Puts one strip entry at a new place in its checkout's strip.
    ///
    /// The two kinds of entry have different owners. A file tab's slot is
    /// Hide's, so a move that only rearranges file slots is committed here and
    /// nothing is sent to Herdr. The relative order of Herdr's tabs is Herdr's,
    /// so a move that changes it is a request: the arrangement is held until
    /// Herdr reports the order it actually has, and a refusal leaves the strip
    /// on the order Herdr last reported.
    pub(super) fn reorder_tab(&mut self, payload: ReorderTabPayload) -> bool {
        let Some(workspace) = self
            .snapshot
            .navigator
            .workspaces
            .iter()
            .find(|workspace| workspace.id == payload.workspace_id)
        else {
            self.set_error(
                "tab.unknown_workspace",
                format!("Workspace {} is not registered", payload.workspace_id),
                false,
            );
            return true;
        };
        if workspace.remote_target_id.is_some() {
            self.set_error(
                "tab.reorder_remote",
                "A remote target's tab order is Herdr's alone and cannot be rearranged here",
                false,
            );
            return true;
        }
        let Some(checkout) = workspace
            .checkouts
            .iter()
            .find(|checkout| checkout.id == payload.checkout_id)
        else {
            self.set_error(
                "tab.unknown_checkout",
                format!("Checkout {} is not available", payload.checkout_id),
                false,
            );
            return true;
        };
        let strip = checkout.strip.clone();
        if self
            .pending_tab_move
            .get(&payload.checkout_id)
            .is_some_and(|pending| {
                matches!(
                    pending.phase.as_str(),
                    "transmitting" | "awaiting_topology" | "unknown"
                )
            })
        {
            self.set_error(
                "tab.move_in_progress",
                format!(
                    "Tab order for checkout {} is still being confirmed; the new move was not sent",
                    payload.checkout_id
                ),
                true,
            );
            self.sync_async_operations();
            return false;
        }
        let Some(from) = strip.iter().position(|entry| entry.id == payload.tab_id) else {
            self.set_error(
                "tab.reorder_unknown",
                format!("Tab {} is not in this checkout's strip", payload.tab_id),
                false,
            );
            return true;
        };
        if payload.to_index >= strip.len() {
            self.set_error(
                "tab.reorder_out_of_range",
                format!(
                    "Position {} is past the end of a strip of {}",
                    payload.to_index,
                    strip.len()
                ),
                false,
            );
            return true;
        }

        let mut desired = strip.clone();
        let moved = desired.remove(from);
        desired.insert(payload.to_index, moved.clone());
        let desired_ids = desired
            .iter()
            .map(|entry| entry.id.clone())
            .collect::<Vec<_>>();
        let herdr_ids = |entries: &[StripTabSnapshot]| {
            entries
                .iter()
                .filter(|entry| entry.kind == StripTabKind::Herdr)
                .map(|entry| entry.source_id.clone())
                .collect::<Vec<_>>()
        };
        let current_herdr = herdr_ids(&strip);
        let desired_herdr = herdr_ids(&desired);

        if current_herdr == desired_herdr {
            // Only slots Hide owns changed, so Herdr has nothing to do and the
            // arrangement is the operator's the moment they drop it.
            self.pending_tab_move.remove(&payload.checkout_id);
            self.checkout_tab_order
                .insert(payload.checkout_id.clone(), desired_ids);
            // A preview tab the operator placed by hand is one they mean to
            // keep (B7); the rebuild below draws the promoted slot.
            if moved.preview {
                self.promote_editor_tab(&moved.source_id);
            }
            self.rebuild_tab_strips();
            return true;
        }

        // Herdr indexes a move inside the workspace that owns the tab, so the
        // drag - not the checkout - decides whether Herdr hears about it. What
        // matters is the moved tab's own workspace subsequence: a drag that
        // only steps over tabs belonging to another Herdr workspace changes
        // nothing Herdr can see, and the strip settles locally.
        //
        // Deciding this per checkout is what refused every drag in a checkout
        // whose tabs come from two Herdr workspaces, which is the ordinary
        // arrangement for a repository opened twice.
        let Some((moved_workspace_id, workspace_order)) = self
            .herdr_workspace_tab_order
            .iter()
            .find(|(_, order)| order.contains(&moved.source_id))
            .map(|(workspace_id, order)| (workspace_id.clone(), order.clone()))
        else {
            self.set_error(
                "tab.reorder_inconsistent",
                format!(
                    "Tab {} cannot be placed there: Herdr does not list it in any workspace",
                    moved.source_id
                ),
                false,
            );
            return true;
        };
        let owned = |entries: &[String]| {
            entries
                .iter()
                .filter(|tab_id| workspace_order.contains(tab_id))
                .cloned()
                .collect::<Vec<_>>()
        };
        let current_owned = owned(&current_herdr);
        let desired_owned = owned(&desired_herdr);
        if current_owned == desired_owned {
            // The moved tab kept its place among its own workspace's tabs, so
            // only slots Hide arranges changed and the drop is final now.
            self.pending_tab_move.remove(&payload.checkout_id);
            self.checkout_tab_order
                .insert(payload.checkout_id.clone(), desired_ids);
            self.rebuild_tab_strips();
            return true;
        }

        let Some(insert_index) =
            herdr_insert_index(&workspace_order, &desired_owned, &moved.source_id)
        else {
            self.set_error(
                "tab.reorder_inconsistent",
                format!(
                    "Tab {} cannot be placed there: it is not one of Herdr's tabs in this checkout",
                    moved.source_id
                ),
                false,
            );
            return true;
        };
        let Some(context) = self.live.as_ref().cloned() else {
            self.set_error(
                "tab.control_unavailable",
                "Moving a Herdr tab requires a live Herdr connection",
                true,
            );
            return true;
        };
        let generation = self.next_tab_move_generation;
        self.next_tab_move_generation += 1;
        let now = unix_milliseconds();
        self.pending_tab_move.insert(
            payload.checkout_id.clone(),
            PendingTabMove {
                desired: desired_ids,
                workspace_id: moved_workspace_id,
                // The order asked for is the moved tab's own workspace's, the
                // same subsequence the request was indexed in. Holding the
                // checkout's mixed order here would wait for an interleaving
                // Herdr never reports once a checkout draws tabs from two
                // workspaces, and the drag would snap back and stay back.
                herdr_order: desired_owned.clone(),
                target_id: moved.source_id.clone(),
                generation,
                connection_generation: self.live_generation,
                phase: "transmitting".to_owned(),
                stage: "request".to_owned(),
                started_at_unix_ms: now,
                deadline_at_unix_ms: Some(now.saturating_add(CLOSE_STAGE_TIMEOUT_MS)),
                message: Some("Waiting for Herdr to confirm the tab order".to_owned()),
                retryable: false,
            },
        );
        self.sync_async_operations();
        self.push_diagnostic(
            "tab.move.requested",
            format!(
                "Asking Herdr to insert tab {} at {insert_index} in its own workspace",
                moved.source_id
            ),
        );
        if let Err(message) = live::spawn_local_control(
            context,
            RemoteControlAction::MoveTab {
                checkout_id: payload.checkout_id.clone(),
                tab_id: moved.source_id,
                insert_index,
                // Herdr answers with its own workspace's tabs, so the order to
                // check the answer against is the moved tab's workspace
                // subsequence, never the checkout's mixed order.
                expected_order: desired_owned,
                generation,
                connection_generation: self.live_generation,
            },
        ) {
            self.pending_tab_move.remove(&payload.checkout_id);
            self.sync_async_operations();
            self.set_error("tab.move_worker_failed", message, true);
        }
        true
    }
    /// Drops a held reorder and says why, so a refused move is never a strip
    /// that silently stayed where it was.
    pub(super) fn abandon_tab_move(
        &mut self,
        checkout_id: &str,
        generation: u64,
        connection_generation: u64,
        reason: String,
    ) -> bool {
        // A result from a drag a later drag has replaced must not cancel the
        // newer one.
        if self
            .pending_tab_move
            .get(checkout_id)
            .is_none_or(|pending| {
                pending.generation != generation
                    || pending.connection_generation != connection_generation
            })
        {
            return false;
        }
        self.pending_tab_move.remove(checkout_id);
        self.set_error("tab.move_refused", reason, true);
        self.rebuild_tab_strips();
        true
    }
    /// Rewrites every local checkout's tab strip from the Herdr and editor
    /// tabs it currently holds.
    ///
    /// A remote checkout keeps the strip its own projection built: the remote
    /// context browses Herdr's tabs and has no file tabs to mix in.
    pub(super) fn rebuild_tab_strips(&mut self) {
        let editor_tabs = &self.snapshot.editor.tabs;
        let order = &mut self.checkout_tab_order;
        let pending = &mut self.pending_tab_move;
        // Which Herdr workspace each tab belongs to, so a strip slot is
        // refilled from that workspace's order rather than from a flat one.
        let owners = &self
            .herdr_workspace_tab_order
            .iter()
            .flat_map(|(workspace_id, tab_ids)| {
                tab_ids
                    .iter()
                    .map(move |tab_id| (tab_id.clone(), workspace_id.clone()))
            })
            .collect::<BTreeMap<String, String>>();
        let mut live_checkouts = BTreeSet::new();
        // Checkouts whose held arrangement became unreachable. The diagnostic
        // is pushed after the loop, which is where the snapshot is free again.
        let mut dropped_moves = Vec::new();
        for workspace in &mut self.snapshot.navigator.workspaces {
            if workspace.remote_target_id.is_some() {
                continue;
            }
            for checkout in &mut workspace.checkouts {
                live_checkouts.insert(checkout.id.clone());
                let herdr = StripTabSnapshot::from_herdr_tabs(&checkout.tabs);
                let editor = editor_tabs
                    .iter()
                    .filter(|tab| {
                        tab.workspace_id == checkout.workspace_id && tab.checkout_id == checkout.id
                    })
                    .map(StripTabSnapshot::editor)
                    .collect::<Vec<_>>();
                let stored = order.entry(checkout.id.clone()).or_default();
                // A held reorder lands the moment Herdr reports the order it
                // asked for, whichever path carried it: the `tab_moved` event,
                // a move Herdr had already made, or a move made from the TUI.
                // A held reorder whose tabs are no longer the checkout's tabs
                // can never be reported, so it is dropped rather than kept
                // waiting for an order that cannot arrive.
                if let Some(held) = pending.get(&checkout.id) {
                    // Only the tabs the move was asked about: the workspace
                    // it named, as this checkout currently holds them.
                    let live_owned = herdr
                        .iter()
                        .map(|entry| &entry.source_id)
                        .filter(|tab_id| owners.get(*tab_id) == Some(&held.workspace_id))
                        .cloned()
                        .collect::<Vec<_>>();
                    if live_owned.iter().cloned().collect::<BTreeSet<_>>()
                        != held.herdr_order.iter().cloned().collect::<BTreeSet<_>>()
                    {
                        pending.remove(&checkout.id);
                        dropped_moves.push(checkout.id.clone());
                    } else if live_owned == held.herdr_order {
                        *stored = held.desired.clone();
                        pending.remove(&checkout.id);
                    }
                }
                checkout.strip = ordered_strip(stored, &herdr, &editor, owners);
                *stored = checkout
                    .strip
                    .iter()
                    .map(|entry| entry.id.clone())
                    .collect();
            }
        }
        order.retain(|checkout_id, _| live_checkouts.contains(checkout_id));
        pending.retain(|checkout_id, _| live_checkouts.contains(checkout_id));
        // A rebuilt entry starts without its agent; name it before the strip
        // is published so a fresh tab never draws as a bare number first.
        sync_strip_agent_identity(
            &mut self.snapshot.navigator.workspaces,
            &self.snapshot.navigator.agents,
        );
        for checkout_id in dropped_moves {
            self.push_diagnostic(
                "tab.move.dropped",
                format!(
                    "The tab arrangement held for {checkout_id} was abandoned: its tabs changed before Herdr reported the order"
                ),
            );
        }
        self.sync_async_operations();
    }
    /// Reconciles the focused checkout, its owning workspace, root path, and
    /// active tab projection after a catalog replacement. Catalog rebuilds
    /// happen from both background sync and event handlers, so this policy
    /// must have one implementation to keep those paths convergent.
    pub(super) fn resync_navigator_focus(&mut self) {
        let focused_checkout_exists = self.snapshot.navigator.workspaces.iter().any(|workspace| {
            workspace.checkouts.iter().any(|checkout| {
                Some(checkout.id.as_str()) == self.snapshot.navigator.focused_checkout_id.as_deref()
            })
        });
        if !focused_checkout_exists && self.snapshot.ui_state.focused_checkout_id.is_none() {
            self.snapshot.navigator.focused_checkout_id = self
                .snapshot
                .navigator
                .workspaces
                .iter()
                .flat_map(|workspace| workspace.checkouts.iter())
                .find(|checkout| checkout.exists)
                .map(|checkout| checkout.id.clone());
        }
        let focused = self
            .snapshot
            .navigator
            .workspaces
            .iter()
            .find_map(|workspace| {
                workspace
                    .checkouts
                    .iter()
                    .find(|checkout| {
                        Some(checkout.id.as_str())
                            == self.snapshot.navigator.focused_checkout_id.as_deref()
                    })
                    .map(|checkout| (workspace.id.clone(), checkout.path.clone()))
            });
        self.snapshot.navigator.focused_workspace_id = focused
            .as_ref()
            .map(|(workspace_id, _)| workspace_id.clone());
        self.snapshot.navigator.root_path = focused.map(|(_, path)| path);
        self.sync_active_tab_projection();
        // Every catalog rebuild and every focus change lands here, so this is
        // the one place the pull-request badges and the summary card have to
        // be re-derived from. Doing it at each call site is how the row and
        // the card would come to disagree.
        self.apply_pull_requests();
        self.refresh_worktree_projection();
    }
    /// Decides which tab each checkout shows, given what Herdr says is active
    /// and what Hide has already chosen.
    ///
    /// Hide owns the visible tab, so Herdr's name is read four ways:
    /// it agrees with Hide and confirms a switch in flight; it disagrees while
    /// Hide's notification is still unconfirmed, and Hide keeps its own value
    /// until Herdr answers; it disagrees with nothing in flight, which is an
    /// operator focusing that tab outside Hide, so Hide follows it and reports
    /// the tab and where the change came from; or Herdr names no tab in this
    /// checkout, and Hide keeps showing what it was showing.
    ///
    /// A checkout that has tabs always ends with one of them visible. Leaving
    /// a sibling checkout of a split workspace without an active tab is what
    /// made its canvas draw the empty-checkout state over real panes.
    pub(super) fn reconcile_visible_tabs(
        &mut self,
        workspaces: &mut [WorkspaceSnapshot],
        herdr: &HerdrTabView,
    ) {
        let mut followed: Vec<(String, String, String)> = Vec::new();
        let mut follow_pane: Option<String> = None;
        let mut confirmed_pending = false;
        // Herdr's focus moved since the last update. Only then is its focused
        // tab an action to follow; an unchanged focus that differs from
        // Hide's tab is the state a timed-out notification leaves behind, and
        // Hide keeps its value through that.
        let herdr_focus_moved = herdr.focused_tab_id != self.herdr_focused_tab_seen;
        self.herdr_focused_tab_seen = herdr.focused_tab_id.clone();
        let selected_pane_id = self.snapshot.terminal.pane_id.clone();
        // The catalog is rebuilt whole on every pass, so a checkout absent
        // from it is gone rather than momentarily missing. Keeping its tab
        // would grow this map for the life of the process.
        let live_checkout_ids = workspaces
            .iter()
            .flat_map(|workspace| workspace.checkouts.iter())
            .map(|checkout| checkout.id.clone())
            .collect::<HashSet<_>>();
        self.visible_tab_ids
            .retain(|checkout_id, _| live_checkout_ids.contains(checkout_id));
        for workspace in workspaces.iter_mut() {
            for checkout in workspace.checkouts.iter_mut() {
                let has_tab = |tab_id: &str| {
                    checkout
                        .tabs
                        .iter()
                        .any(|tab| tab.id.as_deref() == Some(tab_id))
                };
                let hide_tab = self
                    .visible_tab_ids
                    .get(&checkout.id)
                    .filter(|tab_id| has_tab(tab_id))
                    .cloned();
                let herdr_tab = herdr
                    .focused_tab_id
                    .as_deref()
                    .filter(|tab_id| herdr_focus_moved && has_tab(tab_id))
                    .map(str::to_owned);
                let pending_tab = self
                    .pending_tab_focus
                    .as_ref()
                    .filter(|pending| pending.scope_id == checkout.id)
                    .map(|pending| pending.target_id.clone());
                // A tab focus is confirmed by the workspace that owns the
                // tab showing it, whichever workspace Herdr's keyboard is in.
                if pending_tab
                    .as_deref()
                    .is_some_and(|tab_id| herdr.is_active_in_its_workspace(tab_id))
                {
                    confirmed_pending = true;
                }
                // The tab holding the selected pane, when it is in this
                // checkout. With no tab of its own yet, Hide shows the tab the
                // keyboard is in rather than one Herdr remembers, so a restore
                // draws the layout it attaches.
                let selected_tab = selected_pane_id.as_deref().and_then(|pane_id| {
                    checkout
                        .tabs
                        .iter()
                        .find(|tab| tab.panes.iter().any(|pane| pane.id == pane_id))
                        .and_then(|tab| tab.id.clone())
                });
                // A pending tab Herdr has not listed yet (one just created)
                // keeps its claim on the checkout instead of being replaced
                // by whichever tab is drawn while it arrives.
                let pending_tab_unlisted = pending_tab
                    .as_deref()
                    .is_some_and(|tab_id| !has_tab(tab_id));
                let visible = match (hide_tab, herdr_tab) {
                    (Some(hide_tab), Some(herdr_tab)) if hide_tab == herdr_tab => Some(hide_tab),
                    (Some(hide_tab), Some(herdr_tab)) => {
                        if pending_tab.as_deref() == Some(hide_tab.as_str()) {
                            Some(hide_tab)
                        } else {
                            // Following the tab has to bring the keyboard with
                            // it. Leaving the projection on the tab that just
                            // stopped being visible parks the focus ring and
                            // the first responder on a pane nobody can see.
                            if self.snapshot.navigator.focused_checkout_id.as_deref()
                                == Some(checkout.id.as_str())
                            {
                                let first_pane_id = checkout
                                    .tabs
                                    .iter()
                                    .find(|tab| tab.id.as_deref() == Some(herdr_tab.as_str()))
                                    .and_then(|tab| tab.panes.first())
                                    .map(|pane| pane.id.clone());
                                follow_pane = self.tab_focus_pane_id(&herdr_tab, first_pane_id);
                            }
                            followed.push((checkout.id.clone(), hide_tab, herdr_tab.clone()));
                            Some(herdr_tab)
                        }
                    }
                    (Some(hide_tab), None) => Some(hide_tab),
                    // With no value of its own yet, Hide takes the first of:
                    // the tab it asked for, the tab holding the keyboard,
                    // the tab Herdr has focused, a tab Herdr shows in any of
                    // the checkout's workspaces, the first tab.
                    (None, herdr_tab) => pending_tab
                        .clone()
                        .filter(|tab_id| has_tab(tab_id))
                        .or(selected_tab)
                        .or(herdr_tab)
                        .or_else(|| {
                            checkout
                                .tabs
                                .iter()
                                .filter_map(|tab| tab.id.as_deref())
                                .find(|tab_id| herdr.is_active_in_its_workspace(tab_id))
                                .map(str::to_owned)
                        })
                        .or_else(|| checkout.tabs.first().and_then(|tab| tab.id.clone())),
                };
                match visible {
                    Some(tab_id) => {
                        if !pending_tab_unlisted {
                            self.visible_tab_ids
                                .insert(checkout.id.clone(), tab_id.clone());
                        }
                        checkout.active_tab_id = Some(tab_id);
                    }
                    None => {
                        if !pending_tab_unlisted {
                            self.visible_tab_ids.remove(&checkout.id);
                        }
                        checkout.active_tab_id = None;
                    }
                }
            }
        }
        if confirmed_pending {
            self.pending_tab_focus = None;
        }
        if let Some(pane_id) = follow_pane {
            self.select_terminal_pane(Some(pane_id));
            // Herdr moved the keyboard, not the operator. The read record
            // follows only a focus the operator made in Hide.
            self.operator_focused_pane_id = None;
        }
        for (checkout_id, hide_tab, herdr_tab) in followed {
            crate::diagnostic!(serde_json::json!({
                "component": "view_state",
                "kind": "tab.focus.followed",
                "checkout_id": checkout_id,
                "from_tab_id": hide_tab,
                "to_tab_id": herdr_tab,
                "origin": "herdr",
            }));
            self.push_diagnostic(
                "tab.focus.followed",
                format!(
                    "Herdr focused tab {herdr_tab} in {checkout_id}; Hide was showing {hide_tab}"
                ),
            );
        }
    }
    /// The pane that becoming visible should put the keyboard on: the one the
    /// operator last had in that tab, and its first pane before it has ever
    /// been visited.
    pub(super) fn tab_focus_pane_id(
        &self,
        tab_id: &str,
        first_pane_id: Option<String>,
    ) -> Option<String> {
        self.snapshot
            .pane_layouts
            .iter()
            .find(|layout| layout.tab_id == tab_id)
            .map(|layout| layout.focused_pane_id.clone())
            .or(first_pane_id)
    }
    /// Keeps the focused checkout drawing the tab that holds the keyboard.
    ///
    /// The visible tab and the selected pane are two core-owned values with
    /// one invariant between them: the selected pane lies in the visible tab
    /// of the focused checkout. A tab action moves the pane into the tab; a
    /// pane action, a restore or a retirement moves the tab to the pane,
    /// here. Without it the canvas drew one tab while the pane attached was
    /// in another, and the operator saw five terminals with nothing in them.
    pub(super) fn align_visible_tab_with_selected_pane(&mut self) -> bool {
        let Some(pane_id) = self.snapshot.terminal.pane_id.clone() else {
            return false;
        };
        let Some(checkout_id) = self.snapshot.navigator.focused_checkout_id.clone() else {
            return false;
        };
        let Some(checkout) = self
            .snapshot
            .navigator
            .workspaces
            .iter_mut()
            .flat_map(|workspace| workspace.checkouts.iter_mut())
            .find(|checkout| checkout.id == checkout_id)
        else {
            return false;
        };
        let Some(tab_id) = checkout
            .tabs
            .iter()
            .find(|tab| tab.panes.iter().any(|pane| pane.id == pane_id))
            .and_then(|tab| tab.id.clone())
        else {
            return false;
        };
        if checkout.active_tab_id.as_deref() == Some(tab_id.as_str())
            && self.visible_tab_ids.get(&checkout_id) == Some(&tab_id)
        {
            return false;
        }
        let from_tab_id = checkout.active_tab_id.replace(tab_id.clone());
        self.visible_tab_ids
            .insert(checkout_id.clone(), tab_id.clone());
        self.sync_active_tab_projection();
        crate::diagnostic!(serde_json::json!({
            "component": "view_state",
            "kind": "tab.visible_aligned",
            "checkout_id": checkout_id,
            "from_tab_id": from_tab_id,
            "to_tab_id": tab_id,
            "pane_id": pane_id,
        }));
        self.push_diagnostic(
            "tab.visible_aligned",
            format!("Tab {tab_id} is visible because it holds the selected pane {pane_id}"),
        );
        true
    }
    /// Stops waiting on a view-state notification Herdr never answered.
    ///
    /// The value Hide chose is kept: the operator's tab and pane are Hide's,
    /// and a silent Herdr is a reason to report, not a reason to move the
    /// screen out from under them. Dropping the wait is what lets the next
    /// Herdr event be read as an external focus rather than as a late answer.
    pub(super) fn expire_pending_view_focus(&mut self, now_unix_ms: u64) -> bool {
        let mut expired = Vec::new();
        for slot in ViewFocusSlot::ALL {
            let pending = self.pending_view_focus(slot);
            if let Some(pending) = pending.as_ref()
                && pending.expired_at(now_unix_ms)
            {
                expired.push((slot, pending.clone()));
                *self.pending_view_focus_mut(slot) = None;
            }
        }
        let changed = !expired.is_empty();
        for (slot, pending) in expired {
            let what = slot.what();
            let target_id = pending.target_id;
            if slot == ViewFocusSlot::Pane {
                self.finish_pane_focus_request(
                    pending.request_id.as_deref(),
                    &target_id,
                    "failed",
                    Some(format!(
                        "Herdr did not confirm pane focus within {VIEW_FOCUS_NOTIFICATION_TIMEOUT_MS} ms."
                    )),
                    true,
                );
            }
            crate::diagnostic!(serde_json::json!({
                "component": "view_state",
                "kind": "view_focus.timed_out",
                "what": what,
                "target_id": target_id,
                "timeout_ms": VIEW_FOCUS_NOTIFICATION_TIMEOUT_MS,
            }));
            self.push_diagnostic(
                "view_focus.timed_out",
                format!(
                    "Herdr did not confirm {what} focus {target_id} within {VIEW_FOCUS_NOTIFICATION_TIMEOUT_MS} ms; Hide keeps it"
                ),
            );
        }
        changed
    }
    pub(super) fn sync_active_tab_projection(&mut self) {
        let Some(focused_checkout_id) = self.snapshot.navigator.focused_checkout_id.as_deref()
        else {
            self.snapshot.tab = TabSnapshot {
                id: None,
                workspace_id: None,
                checkout_id: None,
                label: None,
                empty: true,
                delegated: false,
                panes: Vec::new(),
            };
            return;
        };
        let Some((workspace_id, checkout)) =
            self.snapshot
                .navigator
                .workspaces
                .iter()
                .find_map(|workspace| {
                    workspace
                        .checkouts
                        .iter()
                        .find(|checkout| checkout.id == focused_checkout_id)
                        .map(|checkout| (workspace.id.clone(), checkout))
                })
        else {
            self.snapshot.tab = TabSnapshot {
                id: None,
                workspace_id: None,
                checkout_id: None,
                label: None,
                empty: true,
                delegated: false,
                panes: Vec::new(),
            };
            return;
        };
        // The visible tab is looked up by the id Hide holds for this checkout.
        // The first tab is not a stand-in for a missing one: reading position
        // as focus is what made a tab move look like a focus change. A
        // checkout that has tabs always names one, so the only tabless case
        // left is a checkout with no tabs at all.
        let active = checkout.active_tab_id.as_deref().and_then(|active_tab_id| {
            checkout
                .tabs
                .iter()
                .find(|tab| tab.id.as_deref() == Some(active_tab_id))
        });
        if let Some(tab) = active {
            self.snapshot.tab = tab.clone();
        } else {
            self.snapshot.tab = TabSnapshot {
                id: None,
                workspace_id: Some(workspace_id),
                checkout_id: Some(checkout.id.clone()),
                label: Some("No tabs".to_owned()),
                empty: true,
                delegated: false,
                panes: Vec::new(),
            };
        }
    }
    /// Applies a live session-sync result: projected agents on success, an
    /// explicit Herdr status on failure. Returns whether the snapshot changed.
    pub fn ingest_session(
        &mut self,
        fetched: Result<SessionSnapshotPayload, SessionFetchError>,
    ) -> bool {
        self.ingest_session_with_catalog(fetched, None)
    }
    /// Applies the authoritative session projection for one configured remote
    /// Herdr target. The last valid session remains visible through a stale or
    /// disconnected interval, while status always names the current failure.
    pub fn ingest_remote_session(
        &mut self,
        target_id: &str,
        mut fetched: Result<RemoteSessionSnapshot, SessionFetchError>,
    ) -> bool {
        // A remote projection is built off the runtime, so it cannot see the
        // read ledger; without this every stopped remote pane published `Done`
        // and demanded a close confirmation. Hide never focuses a remote pane,
        // so the remote server's own focus is the read signal.
        let mut read_changed = false;
        if let Ok(session) = fetched.as_mut() {
            read_changed = self.apply_remote_read_state(target_id, session);
        }
        let pane_sets = fetched.as_ref().ok().map(|session| {
            remote_terminal_pane_sets(
                session,
                self.snapshot.navigator.focused_device_id.as_deref() == Some(target_id),
            )
        });
        let Some(status_index) = self
            .snapshot
            .status
            .remote
            .iter()
            .position(|status| status.target_id == target_id)
        else {
            self.set_error(
                "remote.target_unknown",
                format!("Remote Herdr sync returned an unconfigured target {target_id}"),
                false,
            );
            return true;
        };

        let was_connected = self.snapshot.status.remote[status_index].state == "connected";
        if (fetched.is_ok() && !was_connected) || (fetched.is_err() && was_connected) {
            let generation = self
                .remote_connection_generations
                .entry(target_id.to_owned())
                .or_insert(0);
            *generation = generation.saturating_add(1);
            self.push_diagnostic(
                "remote.connection_generation_advanced",
                format!(
                    "Advanced remote connection generation for {target_id} after a session state transition"
                ),
            );
        }
        let herdr_version = self
            .remote_connections
            .get(target_id)
            .and_then(|connection| connection.client.cached_herdr_version());
        let status = &mut self.snapshot.status.remote[status_index];

        let mut changed = read_changed;
        if status.herdr_version != herdr_version {
            status.herdr_version = herdr_version;
            changed = true;
        }
        let mut observed_session = None;
        match fetched {
            Ok(session) => {
                if status.state != "connected" || status.message.is_some() {
                    status.state = "connected".to_owned();
                    status.message = None;
                    changed = true;
                }
                if status.files.root_path.as_deref().is_some_and(|root_path| {
                    !session
                        .workspaces
                        .iter()
                        .flat_map(|workspace| workspace.checkouts.iter())
                        .any(|checkout| checkout.path == root_path)
                }) {
                    status.files = RemoteFileListSnapshot::idle();
                    changed = true;
                }
                if status.session.as_ref() != Some(&session) {
                    Self::log_unknown_descendants(
                        status
                            .session
                            .as_ref()
                            .map(|previous| previous.agents.as_slice())
                            .unwrap_or(&[]),
                        &session.agents,
                    );
                    status.session = Some(session);
                    changed = true;
                }
                observed_session = status.session.clone();
            }
            Err(error) => {
                if status.state != error.state()
                    || status.message.as_deref() != Some(error.message())
                {
                    status.state = error.state().to_owned();
                    status.message = Some(error.message().to_owned());
                    changed = true;
                }
            }
        }

        let agent_count = status
            .session
            .as_ref()
            .map(|session| session.agents.len())
            .unwrap_or(0)
            .min(u32::MAX as usize) as u32;
        if let Some(device) = self
            .snapshot
            .navigator
            .devices
            .iter_mut()
            .find(|device| device.id == target_id)
            && device.agent_count != agent_count
        {
            device.agent_count = agent_count;
            changed = true;
        }
        changed |= self.refresh_device_snapshots();
        if let Some((live_pane_ids, active_pane_ids)) = pane_sets {
            changed |=
                self.reconcile_remote_terminal_panes(target_id, &live_pane_ids, &active_pane_ids);
        }
        changed |= self.expire_remote_operations(unix_milliseconds());
        if let Some(session) = observed_session.as_ref() {
            changed |= self.observe_remote_operations(session);
        }
        if changed {
            self.sync_async_operations();
        }
        changed
    }
    /// Reconciles the pane and checkout ids loaded from disk against the first
    /// live session, then retires the hint.
    ///
    /// Persisted ids name a session that has already ended, so a pane Herdr no
    /// longer has is the expected case on launch rather than a fault. Dropping
    /// the unusable parts here lets the ordinary resolution below pick the
    /// session's own focus, and keeps `pane.projection_unavailable` meaning
    /// what it says: a pane the user chose against a live session went away.
    pub(super) fn consume_restore_hint(&mut self, payload: &SessionSnapshotPayload) {
        if !self.restore_hint_pending {
            return;
        }
        self.restore_hint_pending = false;

        let restored_pane_exists = self
            .snapshot
            .ui_state
            .selected_pane_id
            .as_deref()
            .is_some_and(|pane_id| {
                payload
                    .layouts
                    .iter()
                    .any(|layout| layout.panes.iter().any(|pane| pane.pane_id == pane_id))
            });
        if !restored_pane_exists {
            self.snapshot.ui_state.selected_pane_id = None;
            self.snapshot.terminal.pane_id = None;
            self.snapshot.focused.pane_id = None;
        }

        let restored_checkout_exists = self
            .snapshot
            .ui_state
            .focused_checkout_id
            .as_deref()
            .is_some_and(|checkout_id| {
                self.snapshot
                    .navigator
                    .workspaces
                    .iter()
                    .flat_map(|workspace| workspace.checkouts.iter())
                    .any(|checkout| checkout.id == checkout_id)
            });
        if !restored_checkout_exists {
            self.snapshot.ui_state.focused_checkout_id = None;
            self.snapshot.navigator.focused_checkout_id = None;
            self.resync_navigator_focus();
        }
    }
    /// Like [`Self::ingest_session`], with a workspace catalog the caller
    /// built outside the runtime lock.
    pub fn ingest_session_with_catalog(
        &mut self,
        fetched: Result<SessionSnapshotPayload, SessionFetchError>,
        precomputed: Option<session_sync::PrecomputedCatalog>,
    ) -> bool {
        // The session update is this runtime's only regular tick, so it is
        // also where a notification Herdr never answered stops being pending.
        // Doing it first lets this same update be read as an external focus
        // rather than as a late answer to a request that has gone quiet.
        let now_unix_ms = unix_milliseconds();
        let fresh_layout_signatures = fetched.as_ref().ok().map(|payload| {
            payload
                .layouts
                .iter()
                .map(|layout| {
                    (
                        layout.tab_id.clone(),
                        Self::session_layout_signature(layout),
                    )
                })
                .collect::<HashMap<_, _>>()
        });
        let timed_out = self.expire_pending_view_focus(now_unix_ms);
        let tab_move_timed_out = self.expire_tab_moves(now_unix_ms);
        let close_timed_out = self.expire_close_operations(now_unix_ms);
        let pane_operation_timed_out = self.expire_pane_operations(now_unix_ms);
        let close_topology_changed = fetched
            .as_ref()
            .is_ok_and(|payload| self.observe_close_topology(payload));
        let pane_topology_changed = fetched
            .as_ref()
            .is_ok_and(|payload| self.observe_pane_operations(payload));
        let session_confirms_pending_pane = fetched.as_ref().ok().is_some_and(|payload| {
            let Some(pending) = self.pending_pane_focus.as_ref() else {
                return false;
            };
            let herdr_tabs = HerdrTabView::from_payload(payload);
            payload.layouts.iter().any(|layout| {
                layout.focused_pane_id == pending.target_id
                    && layout
                        .panes
                        .iter()
                        .any(|pane| pane.pane_id == pending.target_id)
                    && herdr_tabs.is_active_in_its_workspace(&layout.tab_id)
            })
        });
        if let Ok(payload) = &fetched {
            self.herdr_active_tab_ids = HerdrTabView::from_payload(payload).active_tab_ids();
        }
        let previously_projected_pane = self
            .snapshot
            .terminal
            .pane_id
            .as_deref()
            .filter(|pane_id| self.layout_holding_pane(pane_id).is_some())
            .map(str::to_owned);
        let previously_projected_tab = self
            .active_pane_layout()
            .map(|layout| layout.tab_id.clone());
        let previously_projected_in_focused_checkout =
            previously_projected_pane.as_deref().is_some_and(|pane_id| {
                self.snapshot
                    .navigator
                    .focused_checkout_id
                    .as_deref()
                    .and_then(|checkout_id| {
                        self.snapshot
                            .navigator
                            .workspaces
                            .iter()
                            .flat_map(|workspace| workspace.checkouts.iter())
                            .find(|checkout| checkout.id == checkout_id)
                    })
                    .is_some_and(|checkout| {
                        checkout
                            .tabs
                            .iter()
                            .flat_map(|tab| tab.panes.iter())
                            .any(|pane| pane.id == pane_id)
                    })
            });
        let live_pane_ids = fetched.as_ref().ok().map(|payload| {
            payload
                .layouts
                .iter()
                .flat_map(|layout| layout.panes.iter())
                .map(|pane| pane.pane_id.clone())
                .collect::<HashSet<_>>()
        });
        if let Some(live_pane_ids) = live_pane_ids.as_ref() {
            let keep =
                |pane_id: &str| pane_id.starts_with("remote:") || live_pane_ids.contains(pane_id);
            self.retain_terminal_pane_state(keep);
        }
        let mut excluded = Vec::new();
        let mut rejected_layouts: Vec<(String, String)> = Vec::new();
        let catalog_changed = fetched
            .as_ref()
            .map(|payload| self.reconcile_session_catalog(payload, precomputed))
            .unwrap_or(false);
        let protocol_details = fetched.as_ref().err().and_then(|error| {
            error
                .protocol_details()
                .map(|(expected, received, version)| {
                    (expected, received, version.map(str::to_owned))
                })
        });
        let (state, message, agents, layouts, layout, selection_changed) = match fetched {
            Ok(payload) => {
                self.consume_restore_hint(&payload);
                let focused_checkout = self
                    .snapshot
                    .navigator
                    .focused_checkout_id
                    .as_deref()
                    .and_then(|focused_checkout_id| {
                        self.snapshot
                            .navigator
                            .workspaces
                            .iter()
                            .flat_map(|workspace| workspace.checkouts.iter())
                            .find(|checkout| checkout.id == focused_checkout_id)
                    });
                let focused_checkout_pane_ids = focused_checkout
                    .map(|checkout| {
                        checkout
                            .tabs
                            .iter()
                            .flat_map(|tab| tab.panes.iter())
                            .map(|pane| pane.id.clone())
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                let focused_checkout_pane_set = focused_checkout_pane_ids
                    .iter()
                    .map(String::as_str)
                    .collect::<HashSet<_>>();
                // A `remote:` id names a pane on another machine, which this
                // local session can never hold, so it is not a local selection
                // that has gone missing. Reading it as one is what left the
                // shell stuck on "Selected pane remote:...:pane:w59:p2 is not
                // available for the selected checkout" after a trip to a
                // remote device and back: every local sync tick compared the
                // leftover remote id against local layouts, never matched, and
                // re-raised the same error. Dropping it here means no path
                // that leaves a remote id in the selection can poison local
                // projection, rather than fixing the one navigation that did.
                let selected_pane_id = self
                    .snapshot
                    .terminal
                    .pane_id
                    .clone()
                    .or_else(|| self.snapshot.ui_state.selected_pane_id.clone())
                    .filter(|pane_id| !pane_id.starts_with("remote:"));
                let selected_still_exists = selected_pane_id.as_deref().is_some_and(|pane_id| {
                    payload
                        .layouts
                        .iter()
                        .any(|layout| layout.panes.iter().any(|pane| pane.pane_id == pane_id))
                });
                let selected_pane_missing = selected_pane_id.is_some() && !selected_still_exists;
                let selected_was_projected = previously_projected_in_focused_checkout
                    && selected_pane_id.as_deref() == previously_projected_pane.as_deref();
                let selected_left_focused_checkout =
                    selected_pane_id.as_deref().is_some_and(|pane_id| {
                        focused_checkout.is_some() && !focused_checkout_pane_set.contains(pane_id)
                    });
                // A rendered pane that leaves the selected checkout has
                // completed an expected lifecycle transition. Retarget only
                // inside that checkout, or leave it empty. A pane that was
                // never rendered is still a pending or invalid selection and
                // keeps the explicit projection error below.
                let selected_pane_retired = selected_was_projected
                    && (selected_pane_missing || selected_left_focused_checkout);
                let replacement_pane_id = {
                    selected_pane_retired
                        .then(|| {
                            previously_projected_tab
                                .as_deref()
                                .and_then(|tab_id| {
                                    payload
                                        .layouts
                                        .iter()
                                        .find(|layout| layout.tab_id == tab_id)
                                        .map(|layout| layout.focused_pane_id.as_str())
                                })
                                .filter(|pane_id| focused_checkout_pane_set.contains(*pane_id))
                                .or_else(|| {
                                    payload.focused_pane_id.as_deref().filter(|pane_id| {
                                        focused_checkout_pane_set.contains(*pane_id)
                                    })
                                })
                                .or_else(|| {
                                    // A close moves Herdr's keyboard to another
                                    // tab, and the pane focus for it can arrive
                                    // after this snapshot; the tab Herdr names now
                                    // is where the operator is looking, not the
                                    // checkout's first pane.
                                    HerdrTabView::from_payload(&payload)
                                        .focused_tab_id
                                        .and_then(|tab_id| {
                                            payload
                                                .layouts
                                                .iter()
                                                .find(|layout| layout.tab_id == tab_id)
                                        })
                                        .map(|layout| layout.focused_pane_id.as_str())
                                        .filter(|pane_id| {
                                            focused_checkout_pane_set.contains(*pane_id)
                                        })
                                })
                                .or_else(|| focused_checkout_pane_ids.first().map(String::as_str))
                                .map(str::to_owned)
                        })
                        .flatten()
                };
                let explicit_checkout_missing =
                    self.snapshot.ui_state.focused_checkout_id.is_some()
                        && focused_checkout.is_none();
                // Once the user or persisted state chooses a pane, a session
                // snapshot that omits that workspace must not silently retarget
                // commands to Herdr's unrelated globally focused workspace.
                let target_pane_id = if selected_pane_retired {
                    replacement_pane_id.as_deref()
                } else if selected_pane_missing || explicit_checkout_missing {
                    None
                } else if focused_checkout.is_some() {
                    // With no selection, the keyboard lands in the tab the
                    // checkout is showing, on that tab's remembered pane, so
                    // the tab drawn is the tab attached. The first pane of the
                    // checkout is for a checkout that shows no tab yet.
                    let visible_tab_pane_id = self
                        .snapshot
                        .ui_state
                        .focused_checkout_id
                        .as_deref()
                        .and_then(|checkout_id| self.visible_tab_ids.get(checkout_id))
                        .and_then(|tab_id| {
                            payload
                                .layouts
                                .iter()
                                .find(|layout| &layout.tab_id == tab_id)
                        })
                        .map(|layout| layout.focused_pane_id.as_str())
                        .filter(|pane_id| focused_checkout_pane_set.contains(*pane_id));
                    selected_pane_id
                        .as_deref()
                        .filter(|pane_id| {
                            selected_still_exists && focused_checkout_pane_set.contains(*pane_id)
                        })
                        .or(visible_tab_pane_id)
                        .or_else(|| {
                            focused_checkout_pane_ids
                                .iter()
                                .find(|pane_id| {
                                    payload.layouts.iter().any(|layout| {
                                        layout
                                            .panes
                                            .iter()
                                            .any(|pane| pane.pane_id == pane_id.as_str())
                                    })
                                })
                                .map(|pane_id| pane_id.as_str())
                        })
                } else if selected_pane_id.is_some() && !selected_still_exists {
                    None
                } else {
                    selected_pane_id
                        .as_deref()
                        .or(payload.focused_pane_id.as_deref())
                        .or_else(|| {
                            payload
                                .layouts
                                .first()
                                .map(|layout| layout.focused_pane_id.as_str())
                        })
                };
                let selected_pane_invalid_for_context = selected_pane_id.is_some()
                    && ((focused_checkout.is_some() && target_pane_id.is_none())
                        || (focused_checkout.is_none()
                            && self.snapshot.ui_state.focused_checkout_id.is_some()));
                let mut selection_changed = false;
                if selected_pane_retired {
                    let retired_pane_id = selected_pane_id.as_deref().unwrap_or("<missing>");
                    self.fail_pending_pane_focus_for_target(
                        retired_pane_id,
                        format!("Pane {retired_pane_id} retired before Herdr confirmed focus."),
                    );
                    self.clear_terminal_projection();
                    self.snapshot.terminal.pane_id = replacement_pane_id.clone();
                    self.snapshot.focused.pane_id = replacement_pane_id.clone();
                    self.snapshot.ui_state.selected_pane_id = replacement_pane_id.clone();
                    if self
                        .snapshot
                        .status
                        .last_error
                        .as_ref()
                        .is_some_and(|error| error.kind == "pane.projection_unavailable")
                    {
                        self.snapshot.status.last_error = None;
                    }
                    let message = replacement_pane_id.as_deref().map_or_else(
                        || {
                            format!(
                                "Retired pane {retired_pane_id}; the selected checkout is now empty"
                            )
                        },
                        |replacement| {
                            format!("Retired pane {retired_pane_id}; selected {replacement}")
                        },
                    );
                    self.push_diagnostic("pane.selection_retired", message);
                    selection_changed = true;
                } else if selected_pane_missing || selected_pane_invalid_for_context {
                    let pane_id = selected_pane_id.as_deref().unwrap_or("<missing>");
                    self.fail_pending_pane_focus_for_target(
                        pane_id,
                        format!("Pane {pane_id} is no longer available in the selected checkout."),
                    );
                    self.clear_terminal_projection();
                    self.set_error(
                        "pane.projection_unavailable",
                        format!(
                            "Selected pane {pane_id} is not available for the selected checkout; terminal projection is waiting"
                        ),
                        true,
                    );
                }
                let (layouts, rejected) = live::project_layouts(&payload);
                rejected_layouts = rejected;
                let layout = target_pane_id
                    .map(|pane_id| live::project_layout_for_pane(&payload, pane_id))
                    .transpose();
                self.pane_hook_tokens = payload
                    .panes
                    .iter()
                    .map(|pane| {
                        (
                            pane.pane_id.clone(),
                            crate::agent_hooks::PaneHookTokens::read(&pane.tokens),
                        )
                    })
                    .collect();
                let projection = project_agents(payload);
                excluded = projection.excluded;
                match layout {
                    Ok(layout) => (
                        "connected",
                        None,
                        Some(projection.agents),
                        layouts,
                        layout,
                        selection_changed,
                    ),
                    Err(projection_error) => (
                        "malformed",
                        Some(format!(
                            "Herdr pane layout could not be projected: {projection_error}"
                        )),
                        None,
                        layouts,
                        None,
                        selection_changed,
                    ),
                }
            }
            Err(error) => (
                error.state(),
                Some(error.message().to_owned()),
                None,
                Vec::new(),
                None,
                false,
            ),
        };

        // A single unreadable agent record excludes only itself; the
        // exclusion is stated rather than silently folded into the count.
        for exclusion in &excluded {
            let pane_id = exclusion.pane_id.as_deref().unwrap_or("<missing pane id>");
            crate::diagnostic!(serde_json::json!({
                "component": "session",
                "kind": "agent.excluded",
                "pane_id": pane_id,
                "source_index": exclusion.source_index,
                "message": exclusion.reason,
            }));
            self.push_diagnostic(
                "agent.excluded",
                format!("Agent {pane_id} was excluded: {}", exclusion.reason),
            );
        }

        let mut changed = catalog_changed
            || selection_changed
            || timed_out
            || tab_move_timed_out
            || close_timed_out
            || pane_operation_timed_out
            || close_topology_changed
            || pane_topology_changed
            || !excluded.is_empty();
        let (expected_protocol, received_protocol, received_version) = protocol_details
            .map(|(expected, received, version)| (Some(expected), Some(received), version))
            .unwrap_or((None, None, None));
        if self.snapshot.status.herdr.state != state
            || self.snapshot.status.herdr.message.as_deref() != message.as_deref()
            || self.snapshot.status.herdr.expected_protocol != expected_protocol
            || self.snapshot.status.herdr.received_protocol != received_protocol
            || self.snapshot.status.herdr.received_version != received_version
        {
            self.snapshot.status.herdr.state = state.to_owned();
            self.snapshot.status.herdr.message = message;
            self.snapshot.status.herdr.expected_protocol = expected_protocol;
            self.snapshot.status.herdr.received_protocol = received_protocol;
            self.snapshot.status.herdr.received_version = received_version;
            changed = true;
        }
        if let Some(mut agents) = agents {
            self.place_agents_in_navigator(&mut agents);
            // The lineage is built before the read axis is applied, because
            // the read fingerprint carries what each row's descendants are
            // asking for and that is only known once the tree exists.
            if crate::sidebar::prune_lineage_expansion(
                &mut self.snapshot.ui_state.expanded_agent_pane_ids,
                &agents,
                ReadRecordScope::Local,
            ) {
                self.persist_ui_state();
                changed = true;
            }
            crate::sidebar::apply_lineage(
                &mut agents,
                &self.snapshot.navigator.workspaces,
                &self.snapshot.ui_state.expanded_agent_pane_ids,
            );
            changed |= self.apply_pane_read_state(
                &mut agents,
                ReadRecordScope::Local,
                live_pane_ids.as_ref(),
            );
            changed |= self.sync_conversation_modes(&agents);
            if self.snapshot.navigator.agents != agents {
                Self::log_unknown_descendants(&self.snapshot.navigator.agents, &agents);
                self.snapshot.navigator.agents = agents;
                changed = true;
            }
            changed |= self.sync_pane_lineage();
            changed |= self.relocate_delegated_child_panes();
        }
        for (tab_id, reason) in &rejected_layouts {
            crate::diagnostic!(serde_json::json!({
                "component": "session",
                "kind": "layout.excluded",
                "tab_id": tab_id,
                "message": reason,
            }));
            self.push_diagnostic(
                "layout.excluded",
                format!("Tab {tab_id} has no drawable layout: {reason}"),
            );
        }
        changed |= self.store_pane_layouts(layouts);
        if let Some(signatures) = fresh_layout_signatures {
            self.confirmed_pane_layout_signatures = signatures;
        }
        if let Some(layout) = layout {
            if self.snapshot.terminal.pane_id.is_none() {
                let pane_id = layout.focused_pane_id.clone();
                self.snapshot.terminal.pane_id = Some(pane_id.clone());
                self.snapshot.focused.pane_id = Some(pane_id);
            }
            changed |= self.apply_pane_layout(layout, session_confirms_pending_pane);
        }
        changed |= self.refresh_worktree_projection();
        changed |= self.align_visible_tab_with_selected_pane();
        changed |= self.track_visible_tab_attachments();
        changed | self.refresh_pet()
    }
    /// Moves the navigator to the checkout that owns a pane without emitting
    /// a second Herdr command.
    ///
    /// A relationship action is one intent. If its child belongs to another
    /// checkout, changing only the pane leaves reconciliation scoped to the
    /// old checkout and the next session event rejects Herdr's confirmation.
    /// Moving the local context here lets the following `pane.focus` remain
    /// the single external effect.
    fn focus_context_for_pane(&mut self, pane_id: &str) -> bool {
        let Some((workspace_id, checkout_id, checkout_path)) = self
            .snapshot
            .navigator
            .workspaces
            .iter()
            .find_map(|workspace| {
                workspace
                    .checkouts
                    .iter()
                    .find(|checkout| {
                        checkout
                            .tabs
                            .iter()
                            .any(|tab| tab.panes.iter().any(|pane| pane.id == pane_id))
                    })
                    .map(|checkout| {
                        (
                            workspace.id.clone(),
                            checkout.id.clone(),
                            checkout.path.clone(),
                        )
                    })
            })
        else {
            return false;
        };
        if self.snapshot.navigator.focused_workspace_id.as_deref() == Some(workspace_id.as_str())
            && self.snapshot.navigator.focused_checkout_id.as_deref() == Some(checkout_id.as_str())
        {
            return false;
        }
        let from_workspace_id = self.snapshot.navigator.focused_workspace_id.clone();
        let from_checkout_id = self.snapshot.navigator.focused_checkout_id.clone();
        self.snapshot.navigator.focused_workspace_id = Some(workspace_id.clone());
        self.snapshot.navigator.focused_checkout_id = Some(checkout_id.clone());
        self.snapshot.navigator.root_path = Some(checkout_path);
        self.refresh_inactive_groups();
        self.remeasure_disk();
        self.deactivate_editor_tab();
        crate::diagnostic!(serde_json::json!({
            "component": "view_state",
            "kind": "pane.focus_context_changed",
            "pane_id": pane_id,
            "from_workspace_id": from_workspace_id,
            "from_checkout_id": from_checkout_id,
            "to_workspace_id": workspace_id,
            "to_checkout_id": checkout_id,
        }));
        self.push_diagnostic(
            "pane.focus_context_changed",
            format!("Pane {pane_id} moved focus to checkout {checkout_id}"),
        );
        true
    }

    /// Moves the keyboard focus to a pane and tells Herdr afterwards.
    ///
    /// Hide owns the focused pane, so the focus ring and the first responder
    /// move on this frame rather than on Herdr's confirming event. What Herdr
    /// still owns is which panes exist and how they are split; this only says
    /// which of them has the keyboard.
    ///
    /// For an operator focus the pane also becomes the one the read record
    /// follows. The record is raised here rather than when the resulting
    /// layout lands, so one click on a Done row clears that row inside the
    /// same dispatch. A focus that never reaches Herdr arms nothing.
    pub(super) fn focus_pane(
        &mut self,
        pane_id: String,
        origin: PaneFocusOrigin,
        request_id: Option<String>,
    ) {
        if self.close_operation_holds_pane(&pane_id) {
            self.set_error(
                "pane.close_pending",
                format!("Pane {pane_id} is closing; focus was not moved back to it"),
                true,
            );
            return;
        }
        let request_id = request_id.filter(|value| !value.trim().is_empty());
        if let Some(request_id) = request_id.as_deref() {
            if self
                .snapshot
                .status
                .pane_focus_request
                .as_ref()
                .is_some_and(|request| request.request_id == request_id)
            {
                self.push_diagnostic(
                    "pane.focus.duplicate_ignored",
                    format!("Ignored duplicate pane focus request {request_id} for {pane_id}"),
                );
                return;
            }
            self.snapshot.status.pane_focus_request = Some(PaneFocusRequestSnapshot {
                request_id: request_id.to_owned(),
                target_pane_id: pane_id.clone(),
                phase: "pending".to_owned(),
                message: None,
                retryable: false,
            });
            if !self.pane_exists_for_focus(&pane_id) {
                let message = format!("Pane {pane_id} is no longer available.");
                self.finish_pane_focus_request(
                    Some(request_id),
                    &pane_id,
                    "failed",
                    Some(message.clone()),
                    true,
                );
                self.set_error("pane.focus_target_unavailable", message, true);
                return;
            }
        }
        let context_changed = self.focus_context_for_pane(&pane_id);
        let already_focused =
            !context_changed && self.snapshot.focused.pane_id.as_deref() == Some(pane_id.as_str());
        // The persisted selection follows the ring. The shell echoes this
        // field back on every UI-state save, and a stale value there put the
        // keyboard back on the previous pane when the sidebar was toggled.
        self.select_terminal_pane(Some(pane_id.clone()));
        if context_changed {
            self.sync_active_tab_projection();
        }
        // A pane in a tab the checkout is not showing brings its tab forward.
        self.align_visible_tab_with_selected_pane();
        if context_changed {
            self.persist_current_ui_state();
        }
        let Some(context) = self.live.as_ref().cloned() else {
            let message = "Pane focus requires a live Herdr connection".to_owned();
            self.finish_pane_focus_request(
                request_id.as_deref(),
                &pane_id,
                "failed",
                Some(message.clone()),
                true,
            );
            self.set_error("pane.control_unavailable", message, true);
            return;
        };
        // Rule 11: focusing the pane that already has the keyboard, with
        // nothing in flight, converges without a second notification. The
        // look itself still counts, so only the notification is skipped.
        // Settled means Herdr's own layout agrees too. A checkout coming
        // forward selects a pane locally without telling Herdr, and skipping
        // the notification then let Herdr's next layout take the focus back.
        let herdr_agrees = self.layout_holding_pane(&pane_id).is_none_or(|layout| {
            layout.focused_pane_id == pane_id && self.herdr_active_tab_ids.contains(&layout.tab_id)
        });
        let notify = !already_focused
            || !herdr_agrees
            || !self.view_focus_settled_on(ViewFocusSlot::Pane, &pane_id);
        if notify {
            if let Some(pending) = self.pending_pane_focus.take() {
                self.finish_pane_focus_request(
                    pending.request_id.as_deref(),
                    &pending.target_id,
                    "failed",
                    Some("A newer pane focus replaced this request.".to_owned()),
                    true,
                );
            }
            self.push_diagnostic("pane.focus.requested", format!("Focusing pane {pane_id}"));
            if let Err(message) = live::spawn_pane_control(
                context,
                PaneControlAction::Focus {
                    pane_id: pane_id.clone(),
                },
            ) {
                self.finish_pane_focus_request(
                    request_id.as_deref(),
                    &pane_id,
                    "failed",
                    Some(message.clone()),
                    true,
                );
                self.set_error("pane.focus_worker_failed", message, true);
                return;
            }
            // Latest request wins, so a second click while the first is
            // unconfirmed cannot be pulled back by Herdr's answer to the
            // first.
            self.pending_pane_focus = Some(match request_id {
                Some(request_id) => PendingViewFocus::pane_request(pane_id.clone(), request_id),
                None => PendingViewFocus::new(String::new(), pane_id.clone()),
            });
        } else {
            self.finish_pane_focus_request(
                request_id.as_deref(),
                &pane_id,
                "succeeded",
                None,
                false,
            );
        }
        if origin == PaneFocusOrigin::Restore {
            return;
        }
        self.operator_focused_pane_id = Some(pane_id);
        self.refresh_pane_read_state();
    }
    pub(super) fn pane_exists_for_focus(&self, pane_id: &str) -> bool {
        self.snapshot
            .pane_layouts
            .iter()
            .any(|layout| layout.pane_ids().contains(&pane_id))
            || self
                .snapshot
                .terminal
                .panes
                .iter()
                .any(|pane| pane.pane_id == pane_id)
    }
    pub(super) fn finish_pane_focus_request(
        &mut self,
        request_id: Option<&str>,
        target_pane_id: &str,
        phase: &str,
        message: Option<String>,
        retryable: bool,
    ) {
        let Some(request_id) = request_id else { return };
        let Some(request) = self.snapshot.status.pane_focus_request.as_mut() else {
            return;
        };
        if request.request_id != request_id || request.target_pane_id != target_pane_id {
            return;
        }
        request.phase = phase.to_owned();
        request.message = message;
        request.retryable = retryable;
    }
    pub(super) fn finish_pane_focus_request_by_id(
        &mut self,
        request_id: &str,
        phase: &str,
        message: Option<String>,
        retryable: bool,
    ) {
        let Some(request) = self.snapshot.status.pane_focus_request.as_mut() else {
            return;
        };
        if request.request_id != request_id {
            return;
        }
        request.phase = phase.to_owned();
        request.message = message;
        request.retryable = retryable;
    }
    pub(super) fn fail_pending_pane_focus_for_target(
        &mut self,
        target_pane_id: &str,
        message: String,
    ) {
        let Some(pending) = self
            .pending_pane_focus
            .as_ref()
            .filter(|pending| pending.target_id == target_pane_id)
            .cloned()
        else {
            return;
        };
        self.pending_pane_focus = None;
        self.finish_pane_focus_request(
            pending.request_id.as_deref(),
            target_pane_id,
            "failed",
            Some(message),
            true,
        );
    }
    /// The layout of the tab that holds this pane. Every tab in the session
    /// has one, so this answers for a pane in any tab, not only the visible
    /// one.
    pub(super) fn layout_holding_pane(&self, pane_id: &str) -> Option<&PaneLayoutSnapshot> {
        self.snapshot
            .pane_layouts
            .iter()
            .find(|layout| layout.pane_ids().contains(&pane_id))
    }
    /// The layout being drawn: the one holding the selected pane.
    pub(super) fn active_pane_layout(&self) -> Option<&PaneLayoutSnapshot> {
        self.snapshot.active_pane_layout()
    }
    /// Replaces the session's layouts wholesale from one session projection.
    ///
    /// Herdr sends the whole session on every topology update, so a
    /// `layout_updated` for one tab arrives as a payload in which only that
    /// tab's entry differs; comparing the projected vector is what keeps the
    /// other tabs' entries and the revision they ride untouched.
    pub(super) fn store_pane_layouts(&mut self, mut layouts: Vec<PaneLayoutSnapshot>) -> bool {
        // Sorted by tab id, because Herdr's own order for the layouts array
        // carries no meaning - the tab list is what orders tabs - and a
        // reshuffle of it would otherwise restamp the revisioned section and
        // resend the whole navigator with it.
        layouts.sort_by(|left, right| left.tab_id.cmp(&right.tab_id));
        if self.snapshot.pane_layouts == layouts {
            return false;
        }
        self.snapshot.pane_layouts = layouts;
        true
    }
    pub(super) fn apply_pane_layout(
        &mut self,
        layout: PaneLayoutSnapshot,
        session_confirms_pending_pane: bool,
    ) -> bool {
        let pane_ids = layout
            .pane_ids()
            .into_iter()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        let layout_changed = self
            .snapshot
            .pane_layouts
            .iter()
            .find(|stored| stored.tab_id == layout.tab_id)
            != Some(&layout);
        // The rendered projection is compared on its own, because an
        // unchanged layout no longer implies an unchanged projection. Layouts
        // now survive a tab switch, so this runs for a tab whose geometry
        // Herdr never altered while the panes it puts on the canvas still
        // change. Returning the layout comparison alone would then withhold
        // the notification for a canvas that did change.
        let previous_pane_ids = self
            .snapshot
            .terminal
            .panes
            .iter()
            .map(|pane| pane.pane_id.clone())
            .collect::<Vec<_>>();
        let previous_selected = self.snapshot.terminal.pane_id.clone();
        let previous_zoomed = self.snapshot.zoomed.clone();

        // A pane a visited tab left behind keeps its projection entry. The
        // terminal view the shell holds open for that tab reads its transport
        // state from here, and dropping the entry on every switch is what
        // dropped the attach with it and made the tab come back empty. Only a
        // pane that has left the session goes, and the session is the union
        // of every tab's layout.
        let arriving_tab_id = layout.tab_id.clone();
        match self
            .snapshot
            .pane_layouts
            .iter_mut()
            .find(|stored| stored.tab_id == arriving_tab_id)
        {
            Some(stored) => *stored = layout.clone(),
            None => {
                self.snapshot.pane_layouts.push(layout.clone());
                self.snapshot
                    .pane_layouts
                    .sort_by(|left, right| left.tab_id.cmp(&right.tab_id));
            }
        }
        let session_pane_ids = self
            .snapshot
            .pane_layouts
            .iter()
            .flat_map(|stored| stored.pane_ids())
            .map(str::to_owned)
            .collect::<HashSet<_>>();
        self.snapshot.terminal.panes.retain(|pane| {
            pane.pane_id.starts_with("remote:") || session_pane_ids.contains(&pane.pane_id)
        });
        for pane_id in &pane_ids {
            self.ensure_terminal_pane(pane_id);
        }

        // Hide owns the focused pane. An arriving layout confirms the focus
        // Hide notified Herdr about, or - with nothing in flight - it is a
        // focus made outside Hide and Hide follows it and says so. While a
        // notification is unconfirmed the layout's geometry is taken and its
        // focus is not, so the operator's click is not undone by the frame
        // that was already on its way.
        let previous_focus = self.snapshot.focused.pane_id.clone();
        let pending_pane = self.pending_pane_focus.clone();
        let arriving_confirms_pending_tab = self
            .pending_tab_focus
            .as_ref()
            .is_some_and(|pending| pending.target_id == arriving_tab_id);
        let adopt_focus = match pending_pane.as_ref() {
            Some(pending)
                if pending.target_id == layout.focused_pane_id && session_confirms_pending_pane =>
            {
                self.pending_pane_focus = None;
                self.finish_pane_focus_request(
                    pending.request_id.as_deref(),
                    &pending.target_id,
                    "succeeded",
                    None,
                    false,
                );
                true
            }
            Some(_) => false,
            None => true,
        };
        if adopt_focus {
            if previous_focus.as_deref() != Some(layout.focused_pane_id.as_str())
                && pending_pane.is_none()
                && !arriving_confirms_pending_tab
            {
                self.report_followed_pane_focus(previous_focus.as_deref(), &layout.focused_pane_id);
            }
            self.snapshot.terminal.pane_id = Some(layout.focused_pane_id.clone());
            self.snapshot.focused.pane_id = Some(layout.focused_pane_id.clone());
            self.release_operator_focus_if_moved(&previous_focus, &layout.focused_pane_id);
            // Zoom is Herdr's, and its subject is the focused pane. While
            // Hide keeps a focus Herdr has not confirmed, taking the zoom
            // would hide the pane the operator just clicked behind another.
            self.snapshot.zoomed = layout.zoomed.then(|| layout.focused_pane_id.clone());
        }
        let mut notice_cleared = false;
        if self
            .snapshot
            .status
            .last_error
            .as_ref()
            .is_some_and(|error| error.kind == "pane.projection_unavailable")
        {
            self.snapshot.status.last_error = None;
            notice_cleared = true;
        }
        self.sync_focused_terminal_projection();

        if self.live.is_some() {
            for pane_id in pane_ids {
                self.request_terminal_control(&pane_id);
            }
        }
        let projection_changed = notice_cleared
            || previous_selected != self.snapshot.terminal.pane_id
            || previous_focus != self.snapshot.focused.pane_id
            || previous_zoomed != self.snapshot.zoomed
            || previous_pane_ids
                != self
                    .snapshot
                    .terminal
                    .panes
                    .iter()
                    .map(|pane| pane.pane_id.clone())
                    .collect::<Vec<_>>();
        layout_changed || projection_changed
    }
    /// Ends the wait on a view-state focus Herdr refused, keeping the value
    /// Hide chose and reporting the refusal.
    ///
    /// A refusal that names some other target is not this wait's answer and
    /// is left alone, so a late refusal for a tab the operator has already
    /// moved on from cannot end the wait on the current one.
    pub(super) fn clear_refused_view_focus(
        &mut self,
        slot: ViewFocusSlot,
        target_id: &str,
        message: &str,
    ) {
        let Some(pending) = self
            .pending_view_focus(slot)
            .as_ref()
            .filter(|pending| pending.target_id == target_id)
            .cloned()
        else {
            return;
        };
        *self.pending_view_focus_mut(slot) = None;
        if slot == ViewFocusSlot::Pane {
            self.finish_pane_focus_request(
                pending.request_id.as_deref(),
                target_id,
                "failed",
                Some(message.to_owned()),
                true,
            );
        }
        let what = slot.what();
        crate::diagnostic!(serde_json::json!({
            "component": "view_state",
            "kind": slot.refused_kind(),
            slot.id_key(): target_id,
            "message": message,
        }));
        self.push_diagnostic(
            slot.refused_kind(),
            format!(
                "Herdr refused {what} focus {target_id}: {message}; {}",
                slot.kept_phrase()
            ),
        );
    }
    /// Rule 11: whether repeating this view-state change would converge on
    /// what the core already holds, so Herdr needs no second notification.
    /// True when nothing is in flight for the slot, or what is in flight is
    /// this very target.
    pub(super) fn view_focus_settled_on(&self, slot: ViewFocusSlot, target_id: &str) -> bool {
        self.pending_view_focus(slot)
            .as_ref()
            .is_none_or(|pending| pending.target_id == target_id)
    }
    pub(super) fn pending_view_focus(&self, slot: ViewFocusSlot) -> &Option<PendingViewFocus> {
        match slot {
            ViewFocusSlot::Tab => &self.pending_tab_focus,
            ViewFocusSlot::Pane => &self.pending_pane_focus,
        }
    }
    pub(super) fn pending_view_focus_mut(
        &mut self,
        slot: ViewFocusSlot,
    ) -> &mut Option<PendingViewFocus> {
        match slot {
            ViewFocusSlot::Tab => &mut self.pending_tab_focus,
            ViewFocusSlot::Pane => &mut self.pending_pane_focus,
        }
    }
    /// Reports that Hide moved its keyboard focus to follow a pane focus made
    /// outside it.
    ///
    /// Rule 9: the record names the panes and where the change came from, and
    /// carries nothing about what is in them.
    pub(super) fn report_followed_pane_focus(&mut self, previous: Option<&str>, arriving: &str) {
        let from = previous.unwrap_or("<none>").to_owned();
        crate::diagnostic!(serde_json::json!({
            "component": "view_state",
            "kind": "pane.focus.followed",
            "from_pane_id": from,
            "to_pane_id": arriving,
            "origin": "herdr",
        }));
        self.push_diagnostic(
            "pane.focus.followed",
            format!("Herdr focused pane {arriving}; Hide was on {from}"),
        );
    }
    /// Drops the operator focus once Herdr moves focus off the pane the
    /// operator chose.
    ///
    /// The pane has to have been focused before it can be moved away from.
    /// Requiring that is what lets a requested focus survive the stale layout
    /// a tab brings forward on its way: bringing a checkout forward to reach
    /// its pane makes Herdr report that tab's remembered pane first, and
    /// clearing on that would leave the clicked row unread.
    pub(super) fn release_operator_focus_if_moved(
        &mut self,
        previous: &Option<String>,
        arriving: &str,
    ) {
        let Some(operator) = self.operator_focused_pane_id.as_deref() else {
            return;
        };
        if arriving == operator || previous.as_deref() != Some(operator) {
            return;
        }
        self.push_diagnostic(
            "pane.read_focus.released",
            format!("Herdr moved focus from {operator} to {arriving}"),
        );
        self.operator_focused_pane_id = None;
    }
    pub(super) fn begin_task_operation(
        &mut self,
        kind: &str,
        repository_root: Option<String>,
        branch: Option<String>,
        base_branch: Option<String>,
        agent_kind: Option<String>,
    ) -> Result<u64, String> {
        if self
            .snapshot
            .task_operation
            .as_ref()
            .is_some_and(|operation| operation.phase == "working")
        {
            return Err("Another task operation is still running".into());
        }
        self.next_task_operation_id = self.next_task_operation_id.wrapping_add(1).max(1);
        let id = self.next_task_operation_id;
        self.snapshot.task_operation = Some(crate::model::TaskOperationSnapshot {
            id,
            kind: kind.to_owned(),
            phase: "working".into(),
            repository_root,
            branch,
            base_branch,
            path: None,
            pane_id: None,
            agent_kind,
            message: None,
        });
        Ok(id)
    }
    pub fn ingest_task_operation_result(
        &mut self,
        id: u64,
        result: Result<live::WorktreeTaskOutcome, String>,
    ) -> bool {
        let Some(operation) = self.snapshot.task_operation.as_ref() else {
            return false;
        };
        if operation.id != id || operation.phase != "working" {
            return false;
        }
        let operation_kind = operation.kind.clone();
        let repository_root = operation.repository_root.clone();
        let should_focus = operation_kind != "branch_migrate";
        match result {
            Ok(outcome) => {
                let live::WorktreeTaskOutcome {
                    path,
                    pane_id,
                    purpose_error,
                    unconfirmed_purpose_token,
                } = outcome;
                let operation = self
                    .snapshot
                    .task_operation
                    .as_mut()
                    .expect("task operation was checked above");
                operation.phase = "ready".into();
                operation.path = Some(path.clone());
                operation.pane_id = Some(pane_id.clone());
                if should_focus {
                    self.snapshot.terminal.pane_id = Some(pane_id.clone());
                    self.snapshot.focused.surface = Surface::Terminal;
                    self.snapshot.focused.pane_id = Some(pane_id.clone());
                    self.snapshot.ui_state.selected_pane_id = Some(pane_id);
                }
                let normalized = workspace::normalized_for_comparison(Path::new(&path));
                self.created_purpose_writes_in_flight.remove(&normalized);
                if let Some(unconfirmed) = unconfirmed_purpose_token {
                    self.unconfirmed_created_purposes
                        .insert(normalized, unconfirmed);
                    suppress_unconfirmed_created_purposes(
                        &self.unconfirmed_created_purposes,
                        &mut self.snapshot.navigator.workspaces,
                        &self.snapshot.navigator.agents,
                    );
                } else {
                    self.unconfirmed_created_purposes.remove(&normalized);
                }
                if let Some(detail) = purpose_error {
                    self.push_diagnostic(
                        "checkout_purpose.create_failed",
                        format!(
                            "The worktree was created, but its purpose was not saved: {detail}"
                        ),
                    );
                }
                if operation_kind == "worktree_create"
                    && let Some(workspace_id) = repository_root.as_deref().and_then(|root| {
                        self.snapshot
                            .navigator
                            .workspaces
                            .iter()
                            .find(|workspace| workspace.path == root)
                            .map(|workspace| workspace.id.clone())
                    })
                {
                    let checkout_id =
                        workspace::checkout_id_for_path(&workspace_id, std::path::Path::new(&path));
                    if !self
                        .snapshot
                        .ui_state
                        .collapsed_checkout_ids
                        .contains(&checkout_id)
                    {
                        self.snapshot
                            .ui_state
                            .collapsed_checkout_ids
                            .push(checkout_id);
                        self.snapshot.ui_state.collapsed_checkout_ids.sort();
                        self.persist_ui_state();
                    }
                }
                self.refresh_worktrees();
            }
            Err(message) => {
                let operation = self
                    .snapshot
                    .task_operation
                    .as_mut()
                    .expect("task operation was checked above");
                operation.phase = "failed".into();
                operation.message = Some(message);
                self.refresh_worktrees();
            }
        }
        true
    }

    pub fn ingest_purpose_operation_result(
        &mut self,
        id: u64,
        result: Result<live::PurposeTaskOutcome, String>,
    ) -> bool {
        let Some(operation) = self.snapshot.task_operation.as_ref() else {
            return false;
        };
        if operation.id != id
            || operation.kind != "checkout_purpose"
            || operation.phase != "working"
        {
            return false;
        }
        let target = self
            .purpose_operation_target
            .as_ref()
            .filter(|target| target.id == id)
            .cloned();
        let mut visible_purpose = None;
        let (phase, message, diagnostic) = match result {
            Ok(live::PurposeTaskOutcome::Saved {
                purpose,
                token_written,
            }) => {
                visible_purpose = Some((purpose, token_written));
                ("ready", None, None)
            }
            Ok(live::PurposeTaskOutcome::GitFailed {
                purpose,
                token_written,
                detail,
            }) => {
                if token_written {
                    visible_purpose = Some((purpose, true));
                }
                (
                    "failed",
                    Some(if token_written {
                        "Saved to Herdr, but git did not record it. Save tries again.".to_owned()
                    } else {
                        "Git did not record it. Your text is kept; Save tries again.".to_owned()
                    }),
                    Some(("checkout_purpose.git_failed", detail)),
                )
            }
            Err(detail) => (
                "failed",
                Some("Herdr did not answer. Your text is kept; Save tries again.".to_owned()),
                Some(("checkout_purpose.token_failed", detail)),
            ),
        };
        if visible_purpose.is_some()
            && target
                .as_ref()
                .is_some_and(|target| target.remote_target_id.is_none())
            && let Some(path) = self
                .snapshot
                .navigator
                .workspaces
                .iter()
                .flat_map(|workspace| workspace.checkouts.iter())
                .find(|checkout| {
                    target
                        .as_ref()
                        .is_some_and(|target| checkout.id == target.checkout_id)
                })
                .map(|checkout| checkout.path.clone())
        {
            let normalized = workspace::normalized_for_comparison(Path::new(&path));
            self.unconfirmed_created_purposes.remove(&normalized);
        }
        if let Some((purpose, token_written)) = visible_purpose {
            let next = (!purpose.is_empty()).then_some(crate::model::CheckoutPurposeSnapshot {
                text: purpose,
                origin: if token_written {
                    crate::model::CheckoutPurposeOrigin::Token
                } else {
                    crate::model::CheckoutPurposeOrigin::BranchDescription
                },
            });
            match target
                .as_ref()
                .and_then(|target| target.remote_target_id.as_deref())
            {
                Some(remote_target_id) => {
                    if let Some(session) = self
                        .snapshot
                        .status
                        .remote
                        .iter_mut()
                        .find(|status| status.target_id == remote_target_id)
                        .and_then(|status| status.session.as_mut())
                    {
                        if let Some(checkout) = session
                            .workspaces
                            .iter_mut()
                            .flat_map(|workspace| workspace.checkouts.iter_mut())
                            .find(|checkout| {
                                target
                                    .as_ref()
                                    .is_some_and(|target| checkout.id == target.checkout_id)
                            })
                        {
                            checkout.purpose = next;
                        }
                        crate::sidebar::sync_checkout_agent_summaries(
                            &mut session.workspaces,
                            &session.agents,
                        );
                    }
                }
                None => {
                    if let Some(checkout) = self
                        .snapshot
                        .navigator
                        .workspaces
                        .iter_mut()
                        .flat_map(|workspace| workspace.checkouts.iter_mut())
                        .find(|checkout| {
                            target
                                .as_ref()
                                .is_some_and(|target| checkout.id == target.checkout_id)
                        })
                    {
                        checkout.purpose = next;
                    }
                    crate::sidebar::sync_checkout_purposes(
                        &mut self.snapshot.navigator.workspaces,
                        &self.snapshot.navigator.agents,
                    );
                }
            }
        }
        let operation = self
            .snapshot
            .task_operation
            .as_mut()
            .expect("purpose operation was checked above");
        operation.phase = phase.to_owned();
        operation.message = message;
        self.purpose_operation_target = None;
        if let Some((kind, detail)) = diagnostic {
            self.push_diagnostic(kind, detail);
        }
        if phase == "ready" {
            self.refresh_worktrees();
        }
        true
    }

    pub(super) fn fail_purpose_operation(
        &mut self,
        id: u64,
        message: impl Into<String>,
        diagnostic_kind: &'static str,
    ) -> bool {
        let message = message.into();
        let Some(operation) = self.snapshot.task_operation.as_mut() else {
            return false;
        };
        if operation.id != id
            || operation.kind != "checkout_purpose"
            || operation.phase != "working"
        {
            return false;
        }
        operation.phase = "failed".to_owned();
        operation.message = Some(message.clone());
        self.purpose_operation_target = None;
        self.push_diagnostic(diagnostic_kind, message);
        true
    }
    /// Decides an explorer change under the lock and runs it off the lock.
    ///
    /// The decision reads nothing from disk: `plan` refuses a path outside
    /// the focused checkout and a name that is not one component from the
    /// strings alone, and the refusal lands in the slot as a failed
    /// operation so the tree can say why under the row. The filesystem call
    /// then runs on a worker with the mutex released and reports back
    /// through `ingest_explorer_operation_result`; a runtime without a
    /// worker context has no shared mutex and runs it in place.
    pub(super) fn start_explorer_operation(
        &mut self,
        plan: impl FnOnce(&Path) -> Result<files::ExplorerOperation, String>,
        root: &str,
        started_from: &str,
    ) -> bool {
        if self
            .snapshot
            .explorer_operation
            .as_ref()
            .is_some_and(|operation| operation.phase == "working")
        {
            self.set_error(
                "explorer.busy",
                "Another file operation is still running",
                true,
            );
            return true;
        }
        self.next_explorer_operation_id = self.next_explorer_operation_id.wrapping_add(1).max(1);
        let id = self.next_explorer_operation_id;
        let planned = match self.snapshot.navigator.root_path.as_deref() {
            Some(focused_root) if focused_root == root => plan(Path::new(root)),
            Some(focused_root) => Err(format!("{root} is not the focused checkout {focused_root}")),
            None => Err("No local checkout is focused".to_owned()),
        };
        let operation = match planned {
            Ok(operation) => operation,
            Err(message) => {
                self.snapshot.explorer_operation = Some(ExplorerOperationSnapshot {
                    id,
                    kind: "refused".to_owned(),
                    phase: "failed".to_owned(),
                    path: started_from.to_owned(),
                    destination: started_from.to_owned(),
                    message: Some(message.clone()),
                });
                self.push_diagnostic("explorer.refused", format!("{started_from}: {message}"));
                return true;
            }
        };
        self.snapshot.explorer_operation = Some(ExplorerOperationSnapshot {
            id,
            kind: operation.kind.as_str().to_owned(),
            phase: "working".to_owned(),
            path: operation.source.to_string_lossy().into_owned(),
            destination: operation.destination.to_string_lossy().into_owned(),
            message: None,
        });
        let Some(context) = self.worker_context.clone() else {
            let result = files::apply_explorer_operation(&operation);
            return self.ingest_explorer_operation_result(id, &operation, result);
        };
        let worker_operation = operation.clone();
        match thread::Builder::new()
            .name(format!("herdr-core-explorer-{}", operation.kind.as_str()))
            .spawn(move || {
                let operation = worker_operation;
                let result = files::apply_explorer_operation(&operation);
                let Some(runtime) = context.runtime.upgrade() else {
                    return;
                };
                let changed = match runtime.lock() {
                    Ok(mut guard) => guard.ingest_explorer_operation_result(id, &operation, result),
                    Err(_) => return,
                };
                drop(runtime);
                if changed {
                    context.notifier.notify();
                }
            }) {
            Ok(_) => true,
            Err(error) => {
                let message = format!("The file operation worker could not start: {error}");
                self.ingest_explorer_operation_result(id, &operation, Err(message))
            }
        }
    }
    /// Settles the slot with what the filesystem said. Success moves the
    /// selection to the operation's `selection` and carries the paths the
    /// core owns - the tree's expanded folders and any open file tab - from
    /// the old path to the new one, so a renamed folder stays open and a
    /// renamed file's tab still saves to the file it shows. An item moved to
    /// the Trash drops its expanded folders and keeps its file tabs: the tab
    /// is the operator's draft, and saving it recreates the file (D-05).
    /// Failure changes no core state beyond the message.
    pub(crate) fn ingest_explorer_operation_result(
        &mut self,
        id: u64,
        operation: &files::ExplorerOperation,
        result: Result<(), String>,
    ) -> bool {
        let Some(slot) = self.snapshot.explorer_operation.as_mut() else {
            return false;
        };
        if slot.id != id || slot.phase != "working" {
            return false;
        }
        let source = operation.source.to_string_lossy().into_owned();
        let destination = operation.destination.to_string_lossy().into_owned();
        match result {
            Ok(()) => {
                slot.phase = "finished".to_owned();
                if source != destination {
                    for expanded in &mut self.snapshot.ui_state.expanded_paths {
                        if let Some(moved) = retarget_path(expanded, &source, &destination) {
                            *expanded = moved;
                        }
                    }
                    let mut retargeted = Vec::new();
                    for tab in &mut self.snapshot.editor.tabs {
                        if tab.kind != EditorTabKind::File {
                            continue;
                        }
                        if let Some(moved) = retarget_path(&tab.path, &source, &destination) {
                            tab.label = Path::new(&moved)
                                .file_name()
                                .and_then(|name| name.to_str())
                                .filter(|name| !name.is_empty())
                                .map(str::to_owned)
                                .unwrap_or_else(|| moved.clone());
                            tab.path = moved.clone();
                            retargeted.push((tab.id.clone(), moved));
                        }
                    }
                    for (tab_id, moved) in retargeted {
                        if let Some(document) = self.editor_documents.get_mut(&tab_id) {
                            document.path = moved;
                        }
                    }
                    self.sync_active_editor_document();
                }
                if operation.kind == files::ExplorerOperationKind::PathTrash {
                    self.snapshot.ui_state.expanded_paths.retain(|expanded| {
                        expanded != &source && !expanded.starts_with(&format!("{source}/"))
                    });
                }
                self.snapshot.ui_state.selected_path =
                    Some(operation.selection.to_string_lossy().into_owned());
                self.push_diagnostic(
                    format!("explorer.{}", operation.kind.as_str()),
                    format!("{source} -> {destination}"),
                );
                // A created file opens as an editor tab in this same result,
                // so the tree selection and the tab land in one frame with no
                // second dispatch from the shell. New Folder, Rename and move
                // open nothing. If the file cannot be read into a tab it still
                // exists on disk, so the reason rides the finished slot's
                // message and the tree keeps the created file (B10 pattern).
                if operation.kind == files::ExplorerOperationKind::FileCreate
                    && let Some((workspace_id, checkout_id)) = self
                        .focused_local_checkout()
                        .map(|(workspace, checkout)| (workspace.id.clone(), checkout.id.clone()))
                {
                    match self.prepare_file_tab(&workspace_id, &checkout_id, &destination) {
                        Ok(prepared) => self.show_file_tab(
                            prepared,
                            &workspace_id,
                            &checkout_id,
                            &destination,
                            false,
                        ),
                        Err(message) => {
                            if let Some(slot) = self.snapshot.explorer_operation.as_mut() {
                                slot.message = Some(message.clone());
                            }
                            self.push_diagnostic(
                                "explorer.file_create.open_failed",
                                format!("{destination}: {message}"),
                            );
                        }
                    }
                }
                self.persist_current_ui_state();
            }
            Err(message) => {
                slot.phase = "failed".to_owned();
                slot.message = Some(message.clone());
                self.push_diagnostic(
                    format!("explorer.{}_failed", operation.kind.as_str()),
                    format!("{source}: {message}"),
                );
            }
        }
        true
    }
    pub(super) fn acknowledge_task_operation(&mut self, id: u64) -> bool {
        if self
            .snapshot
            .task_operation
            .as_ref()
            .is_some_and(|operation| operation.id == id && operation.phase != "working")
        {
            self.snapshot.task_operation = None;
            true
        } else {
            false
        }
    }
    pub fn ingest_local_control_result(
        &mut self,
        action: RemoteControlAction,
        result: Result<RemoteControlOutcome, String>,
        elapsed_ms: u128,
    ) -> bool {
        self.ingest_local_control_result_with_failure(
            action,
            result.map_err(live::ControlFailure::Definite),
            elapsed_ms,
        )
    }

    pub(crate) fn ingest_local_control_failure(
        &mut self,
        action: RemoteControlAction,
        result: Result<RemoteControlOutcome, live::ControlFailure>,
        elapsed_ms: u128,
    ) -> bool {
        self.ingest_local_control_result_with_failure(action, result, elapsed_ms)
    }

    fn ingest_local_control_result_with_failure(
        &mut self,
        action: RemoteControlAction,
        result: Result<RemoteControlOutcome, live::ControlFailure>,
        elapsed_ms: u128,
    ) -> bool {
        let action_kind = action.kind();
        if let RemoteControlAction::MoveTab {
            checkout_id,
            tab_id,
            expected_order,
            generation,
            connection_generation,
            ..
        } = &action
        {
            return self.ingest_tab_move_result(
                TabMoveResultContext {
                    checkout_id,
                    tab_id,
                    expected_order,
                    generation: *generation,
                    connection_generation: *connection_generation,
                    elapsed_ms,
                },
                result,
            );
        }
        match result {
            Ok(RemoteControlOutcome::Acknowledged {
                created_tab_id,
                created_pane_id,
            }) => {
                if matches!(
                    action,
                    RemoteControlAction::CreateTab { .. }
                        | RemoteControlAction::CreateWorkspace { .. }
                ) && let Some(pane_id) = created_pane_id.as_ref()
                {
                    // tab.create returns the authoritative root pane before
                    // the ordered event projection catches up. Preserve that
                    // focus intent so the next snapshot cannot retain the old
                    // tab merely because its pane still exists.
                    self.snapshot.terminal.pane_id = Some(pane_id.clone());
                    self.snapshot.focused.surface = Surface::Terminal;
                    self.snapshot.focused.pane_id = Some(pane_id.clone());
                    self.snapshot.ui_state.selected_pane_id = Some(pane_id.clone());
                    // The tab was created with focus, so it is Hide's visible
                    // tab from this acknowledgment and Herdr's `tab_focused`
                    // is its confirmation. Without this the strip's active
                    // mark stayed where it was until the operator clicked the
                    // new tab a second time.
                    if let (Some(tab_id), Some(checkout_id)) = (
                        created_tab_id.as_ref(),
                        self.snapshot.navigator.focused_checkout_id.clone(),
                    ) {
                        self.visible_tab_ids
                            .insert(checkout_id.clone(), tab_id.clone());
                        self.pending_tab_focus =
                            Some(PendingViewFocus::new(checkout_id, tab_id.clone()));
                    }
                    self.deactivate_editor_tab();
                    self.persist_current_ui_state();
                }
                self.push_diagnostic(
                    "tab.control.ready",
                    format!(
                        "{action_kind} acknowledged in {elapsed_ms} ms; awaiting authoritative Herdr event"
                    ),
                );
                crate::diagnostic!(serde_json::json!({
                    "component": "tab_control",
                    "kind": "tab.control.ready",
                    "action": action_kind,
                    "created_tab_id": created_tab_id,
                    "created_pane_id": created_pane_id,
                    "duration_ms": elapsed_ms,
                }));
            }
            Err(error) => {
                let message = error.message().to_owned();
                // Hide keeps the tab it made visible. The refusal is reported
                // and the wait ends, so the next Herdr event naming another
                // tab is read as an external focus rather than a late answer.
                if let RemoteControlAction::FocusTab { tab_id } = &action {
                    self.clear_refused_view_focus(ViewFocusSlot::Tab, tab_id, &message);
                }
                self.set_error(
                    "tab.control.failed",
                    format!("{action_kind} failed: {message}"),
                    true,
                );
                crate::diagnostic!(serde_json::json!({
                    "component": "tab_control",
                    "kind": "tab.control.failed",
                    "action": action_kind,
                    "message": message,
                    "duration_ms": elapsed_ms,
                }));
            }
            // `tab.move` is the only action that reports a tab order and it
            // is handled above, before this match.
            Ok(RemoteControlOutcome::TabsOrdered { .. }) => {
                self.set_error(
                    "tab.control.failed",
                    format!("{action_kind} returned a tab order it was not asked for"),
                    false,
                );
            }
        }
        true
    }
    /// Reads what Herdr did with a requested tab move.
    ///
    /// Herdr answers with the workspace's tab list in its new order, so the
    /// answer says whether the move landed as asked without waiting for the
    /// event. An answer that matches leaves the arrangement held until the
    /// order reaches the navigator; anything else drops it and says so, so a
    /// refused or differently-placed move is never a strip that quietly
    /// stayed where it was.
    pub(super) fn ingest_tab_move_result(
        &mut self,
        context: TabMoveResultContext<'_>,
        result: Result<RemoteControlOutcome, live::ControlFailure>,
    ) -> bool {
        let TabMoveResultContext {
            checkout_id,
            tab_id,
            expected_order,
            generation,
            connection_generation,
            elapsed_ms,
        } = context;
        if self
            .pending_tab_move
            .get(checkout_id)
            .is_none_or(|pending| {
                pending.generation != generation
                    || pending.connection_generation != connection_generation
            })
        {
            // The result belongs to an older drag. It is consumed without
            // touching the newer operation or publishing a refusal for it.
            return true;
        }
        let Some(pending) = self.pending_tab_move.get(checkout_id).cloned() else {
            return true;
        };
        if pending.phase != "transmitting" {
            // Once the answer has been accepted, timed out, or settled, the
            // ordered session projection is the only authority left for this
            // request. A duplicate or late answer cannot move the held strip.
            return true;
        }
        if pending
            .deadline_at_unix_ms
            .is_some_and(|deadline| deadline <= unix_milliseconds())
        {
            return self.mark_tab_move_unknown(
                checkout_id,
                generation,
                connection_generation,
                "the response arrived after its deadline".to_owned(),
            );
        }
        let outcome = match result {
            Ok(RemoteControlOutcome::TabsOrdered { tab_ids }) => Ok(tab_ids),
            Ok(RemoteControlOutcome::Acknowledged { .. }) => Err(live::ControlFailure::Ambiguous(
                "tab.move did not report the resulting tab order".to_owned(),
            )),
            Err(error) => Err(error),
        };
        match outcome {
            Ok(tab_ids) => {
                // The response lists the whole workspace, which can hold tabs
                // from sibling checkouts. Only the order of this checkout's
                // tabs was asked for, so only that is checked.
                let placed = tab_ids
                    .iter()
                    .filter(|candidate| expected_order.contains(candidate))
                    .cloned()
                    .collect::<Vec<_>>();
                if placed == expected_order {
                    if let Some(pending) = self.pending_tab_move.get_mut(checkout_id)
                        && pending.generation == generation
                        && pending.connection_generation == connection_generation
                    {
                        pending.phase = "awaiting_topology".to_owned();
                        pending.stage = "topology".to_owned();
                        pending.message = Some(
                            "Request accepted; waiting for the ordered Herdr event".to_owned(),
                        );
                        pending.retryable = false;
                    }
                    self.sync_async_operations();
                    self.push_diagnostic(
                        "tab.move.ready",
                        format!(
                            "Herdr placed tab {tab_id} as asked in {elapsed_ms} ms; awaiting the ordered event"
                        ),
                    );
                    crate::diagnostic!(serde_json::json!({
                        "component": "tab_control",
                        "kind": "tab.move.ready",
                        "checkout_id": checkout_id,
                        "tab_id": tab_id,
                        "order": placed,
                        "duration_ms": elapsed_ms,
                    }));
                    return true;
                }
                crate::diagnostic!(serde_json::json!({
                    "component": "tab_control",
                    "kind": "tab.move.diverged",
                    "checkout_id": checkout_id,
                    "tab_id": tab_id,
                    "requested": expected_order,
                    "placed": placed,
                    "duration_ms": elapsed_ms,
                }));
                self.abandon_tab_move(
                    checkout_id,
                    generation,
                    connection_generation,
                    format!("Herdr put tab {tab_id} somewhere else; the strip follows Herdr"),
                );
                true
            }
            Err(error) if error.is_ambiguous() => {
                let message = error.message().to_owned();
                crate::diagnostic!(serde_json::json!({
                    "component": "tab_control",
                    "kind": "tab.move.unknown",
                    "checkout_id": checkout_id,
                    "tab_id": tab_id,
                    "requested": expected_order,
                    "message": message,
                    "duration_ms": elapsed_ms,
                }));
                self.mark_tab_move_unknown(checkout_id, generation, connection_generation, message);
                true
            }
            Err(error) => {
                let message = error.message().to_owned();
                crate::diagnostic!(serde_json::json!({
                    "component": "tab_control",
                    "kind": "tab.move.failed",
                    "checkout_id": checkout_id,
                    "tab_id": tab_id,
                    "requested": expected_order,
                    "message": message,
                    "duration_ms": elapsed_ms,
                }));
                self.abandon_tab_move(
                    checkout_id,
                    generation,
                    connection_generation,
                    format!("Herdr refused to move tab {tab_id}: {message}"),
                );
                true
            }
        }
    }
    pub(super) fn rebuild_catalog(&mut self) {
        let mut workspaces = workspace::build_catalog(
            &self.snapshot.ui_state.workspace_registrations,
            &self.last_session_spaces,
            &self.worktree_catalog,
        );
        self.last_accepted_catalog = Some(workspaces.clone());
        self.catalog_roots = workspace::root_index(&self.last_session_spaces);
        Self::apply_workspace_expansion(
            &mut workspaces,
            &self.snapshot.ui_state.collapsed_workspace_ids,
        );
        crate::sidebar::sync_checkout_agent_summaries(
            &mut workspaces,
            &self.snapshot.navigator.agents,
        );
        let created_purpose_suppressions = self.unconfirmed_created_purpose_values();
        suppress_unconfirmed_created_purposes(
            &created_purpose_suppressions,
            &mut workspaces,
            &self.snapshot.navigator.agents,
        );
        self.snapshot.navigator.workspaces = workspaces;
        self.snapshot.navigator.devices =
            workspace::devices(&self.snapshot.ui_state.device_registrations);
        self.refresh_device_snapshots();
        self.resync_navigator_focus();
    }
    pub(super) fn apply_workspace_expansion(
        workspaces: &mut [crate::model::WorkspaceSnapshot],
        collapsed_workspace_ids: &[String],
    ) {
        let collapsed = collapsed_workspace_ids
            .iter()
            .map(String::as_str)
            .collect::<HashSet<_>>();
        for workspace in workspaces {
            workspace.expanded = !collapsed.contains(workspace.id.as_str());
        }
    }
    pub(super) fn persist_current_ui_state(&mut self) {
        self.snapshot.ui_state.focused_device_id =
            self.snapshot.navigator.focused_device_id.clone();
        self.snapshot.ui_state.focused_checkout_id =
            self.snapshot.navigator.focused_checkout_id.clone();
        self.persist_ui_state();
    }
    /// Everything one clicked path changes on screen, decided in one event.
    ///
    /// The checkout comes forward, the right panel opens on Explorer, the tree
    /// expands every ancestor and selects the path, and a file also takes an
    /// editor tab. They are one event because the shell's dispatch is
    /// fire-and-forget: sent as separate events the operator would watch the
    /// checkout switch, then the panel appear, then the tree move, and a
    /// refusal partway would leave the screen in a state nobody asked for.
    pub(super) fn reveal_path(&mut self, payload: RevealPathPayload) -> bool {
        let Some(checkout_path) = self
            .snapshot
            .navigator
            .workspaces
            .iter()
            .find(|workspace| workspace.id == payload.workspace_id)
            .and_then(|workspace| {
                workspace
                    .checkouts
                    .iter()
                    .find(|checkout| checkout.id == payload.checkout_id)
                    .map(|checkout| checkout.path.clone())
            })
        else {
            self.set_error(
                "reveal.unknown_checkout",
                format!(
                    "Checkout {} is not registered, so {} was not revealed",
                    payload.checkout_id, payload.path
                ),
                false,
            );
            return true;
        };
        // The file is read before anything moves. Reading is the only part of
        // a reveal that can fail, and a reveal that settles the whole screen
        // at once must not leave the checkout focused and the tree expanded
        // around a document that never arrived.
        let prepared = if payload.is_directory {
            None
        } else {
            match self.prepare_file_tab(&payload.workspace_id, &payload.checkout_id, &payload.path)
            {
                Ok(prepared) => Some(prepared),
                Err(message) => {
                    self.set_error("file.open_failed", message, true);
                    return true;
                }
            }
        };
        if self.snapshot.navigator.focused_checkout_id.as_deref()
            != Some(payload.checkout_id.as_str())
        {
            self.focus_checkout(&payload.workspace_id, &payload.checkout_id);
        }
        self.snapshot.ui_state.right_panel_visible = true;
        self.snapshot.ui_state.right_panel_section = RightPanelSection::Explorer;
        for expanded in reveal_expansion_paths(&checkout_path, &payload.path, payload.is_directory)
        {
            if !self.snapshot.ui_state.expanded_paths.contains(&expanded) {
                self.snapshot.ui_state.expanded_paths.push(expanded);
            }
        }
        self.snapshot.ui_state.selected_path = Some(payload.path.clone());
        if let Some(prepared) = prepared {
            // A revealed path was named on purpose: a terminal link or a
            // Markdown link opens an ordinary tab, not the preview (D-09).
            self.show_file_tab(
                prepared,
                &payload.workspace_id,
                &payload.checkout_id,
                &payload.path,
                false,
            );
        }
        self.push_diagnostic(
            "path.revealed",
            format!(
                "Revealed {} {} in checkout {}",
                if payload.is_directory {
                    "folder"
                } else {
                    "file"
                },
                payload.path,
                payload.checkout_id
            ),
        );
        self.persist_current_ui_state();
        true
    }
    pub(super) fn focus_checkout(&mut self, workspace_id: &str, checkout_id: &str) -> bool {
        // The checkout comes forward on the tab it was showing, with the
        // keyboard on the pane the operator last had there. Its first pane
        // is only for a checkout Hide has never shown.
        let visible_tab_id = self.visible_tab_ids.get(checkout_id).cloned();
        let Some((checkout_path, first_pane_id, has_herdr_tab)) = self
            .snapshot
            .navigator
            .workspaces
            .iter()
            .find(|workspace| workspace.id == workspace_id)
            .and_then(|workspace| {
                workspace
                    .checkouts
                    .iter()
                    .find(|checkout| checkout.id == checkout_id)
                    .map(|checkout| {
                        let visible_tab = visible_tab_id.as_deref().and_then(|tab_id| {
                            checkout
                                .tabs
                                .iter()
                                .find(|tab| tab.id.as_deref() == Some(tab_id))
                        });
                        let first_pane_id = visible_tab
                            .and_then(|tab| tab.panes.first())
                            .or_else(|| {
                                checkout.tabs.iter().flat_map(|tab| tab.panes.iter()).next()
                            })
                            .map(|pane| pane.id.clone());
                        (
                            checkout.path.clone(),
                            first_pane_id,
                            !checkout.tabs.is_empty(),
                        )
                    })
            })
        else {
            let workspace_exists = self
                .snapshot
                .navigator
                .workspaces
                .iter()
                .any(|workspace| workspace.id == workspace_id);
            let (kind, message) = if workspace_exists {
                (
                    "checkout.unknown",
                    format!("Checkout {checkout_id} is not available in {workspace_id}"),
                )
            } else {
                (
                    "checkout.unknown_workspace",
                    format!("Workspace {workspace_id} is not registered"),
                )
            };
            self.set_error(kind, message, false);
            return true;
        };
        let next_pane_id = match visible_tab_id.as_deref() {
            Some(tab_id) => self.tab_focus_pane_id(tab_id, first_pane_id),
            None => first_pane_id,
        };
        self.snapshot.navigator.focused_workspace_id = Some(workspace_id.to_owned());
        self.snapshot.navigator.focused_checkout_id = Some(checkout_id.to_owned());
        self.snapshot.navigator.root_path = Some(checkout_path);
        self.refresh_inactive_groups();
        // Selecting a checkout measures it, and selecting the one already
        // selected measures it again - that is the card's cheapest refresh
        // for a number that moves whenever a build runs (R8).
        self.remeasure_disk();
        self.reset_terminal_projection(next_pane_id.clone());
        self.sync_active_tab_projection();
        self.align_visible_tab_with_selected_pane();
        // The operator chose a checkout, not a pane: the record stops
        // following the pane just left, and nothing is raised for the pane
        // the checkout came forward on. A sidebar row click dispatches this
        // and then a pane focus, and raising the remembered pane here for
        // that one frame cleared a question nobody had read.
        self.operator_focused_pane_id = None;
        self.refresh_pane_read_state();
        self.deactivate_editor_tab();
        if !has_herdr_tab
            && let Some(file_tab_id) = self
                .snapshot
                .editor
                .tabs
                .iter()
                .rev()
                .find(|tab| tab.workspace_id == workspace_id && tab.checkout_id == checkout_id)
                .map(|tab| tab.id.clone())
            && let Err(message) = self.activate_editor_tab(&file_tab_id)
        {
            self.set_error("file.focus_failed", message, false);
        }
        self.persist_current_ui_state();
        if let (Some(context), Some(pane_id)) = (self.live.as_ref().cloned(), next_pane_id)
            && let Err(message) =
                live::spawn_pane_control(context, PaneControlAction::Project { pane_id })
        {
            self.set_error("pane.projection_worker_failed", message, true);
        }
        true
    }
    pub fn ingest_workspace_creation(
        &mut self,
        request_path: &str,
        result: Result<live::WorkspaceCreationOutcome, String>,
        elapsed_ms: u128,
    ) -> bool {
        self.workspace_creations_in_flight.remove(request_path);
        let outcome = match result {
            Ok(outcome) => outcome,
            Err(message) => {
                self.set_error("workspace.create_failed", message, false);
                return true;
            }
        };
        // A path the front-door check could not match (a symlink, a
        // different spelling) can still land on an id whose removal is
        // closing panes. The worker has already opened this project's first
        // pane, so the removal is the request that gives way: dropping it
        // from the in-flight set makes the close's late answer authorize
        // nothing, and the banner says which request won.
        if self
            .workspace_removals_in_flight
            .remove(&outcome.registration.id)
        {
            self.set_error(
                "workspace.remove_cancelled",
                format!(
                    "{} was added again while its removal was closing panes; the project stays registered",
                    outcome.registration.label
                ),
                false,
            );
        }
        let catalog_inputs_match =
            self.snapshot.ui_state.workspace_registrations == outcome.base_registrations;
        if catalog_inputs_match {
            self.snapshot.ui_state.workspace_registrations = outcome.registrations;
        } else if !self
            .snapshot
            .ui_state
            .workspace_registrations
            .iter()
            .any(|registration| registration.id == outcome.registration.id)
        {
            self.snapshot
                .ui_state
                .workspace_registrations
                .push(outcome.registration.clone());
            self.push_diagnostic(
                "workspace.catalog.refresh_pending",
                "Workspace registrations changed during creation; session sync will refresh the catalog",
            );
        }
        self.snapshot.navigator.focused_device_id = Some(workspace::LOCAL_DEVICE_ID.to_owned());
        self.reconcile_remote_terminal_selection();
        let target_checkout = outcome
            .workspaces
            .iter()
            .flat_map(|workspace| workspace.checkouts.iter())
            .find(|checkout| checkout.path == outcome.registration.path);
        if let Some(checkout) = target_checkout {
            self.snapshot.navigator.focused_checkout_id = Some(checkout.id.clone());
            self.snapshot.ui_state.focused_checkout_id = Some(checkout.id.clone());
        }
        let target_pane_id = outcome.created_pane_id.clone().or_else(|| {
            target_checkout
                .and_then(|checkout| checkout.tabs.first())
                .and_then(|tab| tab.panes.first())
                .map(|pane| pane.id.clone())
        });
        if catalog_inputs_match {
            self.reset_terminal_projection(target_pane_id);
            self.ingest_session_with_catalog(
                Ok(outcome.session),
                Some(session_sync::PrecomputedCatalog {
                    registrations: self.snapshot.ui_state.workspace_registrations.clone(),
                    workspaces: outcome.workspaces,
                    roots: self.catalog_roots.clone(),
                }),
            );
        } else {
            self.push_diagnostic(
                "workspace.session.refresh_pending",
                "Workspace creation completed after registrations changed; session sync will publish the authoritative catalog",
            );
        }
        self.persist_current_ui_state();
        let git_init_failed = outcome.git_init_error.is_some();
        if let Some(message) = outcome.git_init_error.as_ref() {
            self.set_error(
                "workspace.git_init_failed",
                format!("Workspace was registered, but Git initialization failed: {message}"),
                true,
            );
        }
        self.push_diagnostic(
            "workspace.registered",
            format!(
                "Registered workspace {} in {elapsed_ms} ms",
                outcome.registration.path
            ),
        );
        crate::diagnostic!(serde_json::json!({
            "component": "workspace",
            "kind": "workspace.registered",
            "path": outcome.registration.path,
            "duration_ms": elapsed_ms,
                "git_init": if git_init_failed { "failed" } else { "complete_or_skipped" },
        }));
        true
    }
}
