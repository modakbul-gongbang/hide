//! Tab-owned File Views, projected through the existing Workspace snapshot.
//! The shared editor owns documents; each presentation owns its displays.

use super::workspace_view::WorkspaceKey;
use super::*;
use crate::workspace_control::{Action, Caller};

#[derive(Default)]
pub(super) struct Seen {
    shown: Vec<String>,
    active: Option<String>,
}

pub(super) struct Parked {
    before: crate::workspace_views::WorkspaceView,
    tab: String,
    reveal: bool,
    editor_active: Option<String>,
    called: Option<u64>,
    calls: u64,
    generation: u64,
    identities: HashMap<String, String>,
    documents: HashSet<String>,
}

impl Parked {
    pub(super) fn identity(&self, id: &str) -> String {
        self.identities
            .get(id)
            .cloned()
            .unwrap_or_else(|| id.to_owned())
    }

    pub(super) fn action(&self, mut action: Action) -> Action {
        let remap = |id: &mut String| {
            if let Some(next) = self.identities.get(id) {
                *id = next.clone();
            }
        };
        match &mut action {
            Action::Select {
                view_id,
                expected_browser_area,
                ..
            }
            | Action::Close {
                view_id,
                expected_browser_area,
            } => {
                remap(view_id);
                if let Some(area) = expected_browser_area {
                    remap(area);
                }
            }
            Action::Split {
                view_id, area_id, ..
            }
            | Action::Move {
                view_id, area_id, ..
            } => {
                remap(view_id);
                remap(area_id);
            }
            Action::OpenBrowser {
                area_id: Some(area),
                ..
            } => remap(area),
            _ => {}
        }
        action
    }
}

impl Runtime {
    fn agent_view_of(&self, key: &WorkspaceKey) -> (Vec<&str>, Option<&str>) {
        if self.arranges_agent_areas(&key.0)
            && let Some(layout) = self.agent_layout_of(key)
        {
            let shown: Vec<&str> = layout
                .tree
                .areas()
                .into_iter()
                .filter_map(|area| layout.shown_in(&area.id))
                .collect();
            if !shown.is_empty() {
                return (shown, layout.active());
            }
        }
        let active = self
            .catalog_workspaces()
            .filter(|workspace| workspace.device_id == key.0)
            .flat_map(|workspace| &workspace.checkouts)
            .find(|checkout| checkout.path == key.1)
            .and_then(|checkout| checkout.active_tab_id.as_deref());
        (active.into_iter().collect(), active)
    }

    /// The Workspace's active Agent tab: the one whose panel it is.
    pub(super) fn workspace_active_tab(&self, key: &WorkspaceKey) -> Option<String> {
        self.agent_view_of(key).1.map(str::to_owned)
    }

    pub(super) fn track_view_bookmarks(&mut self) -> bool {
        let Some(store) = self.workspace_views.as_ref() else {
            return false;
        };
        let Some(key) = store.front.clone() else {
            return false;
        };
        let (shown, active) = self.agent_view_of(&key);
        let newly_shown = active.is_some_and(|tab| {
            store
                .bookmark_seen
                .get(&key)
                .is_none_or(|seen| !seen.shown.iter().any(|held| held == tab))
        });
        let explicit = store
            .presentation_choice
            .as_ref()
            .is_some_and(|(chosen_key, tab)| chosen_key == &key && Some(tab.as_str()) == active);
        let seen_changed = store.bookmark_seen.get(&key).is_none_or(|seen| {
            seen.active.as_deref() != active
                || !seen
                    .shown
                    .iter()
                    .map(String::as_str)
                    .eq(shown.iter().copied())
        });
        let seen = seen_changed.then(|| Seen {
            shown: shown.into_iter().map(str::to_owned).collect(),
            active: active.map(str::to_owned),
        });
        let pending_result = active.is_some_and(|tab| {
            store
                .views
                .get(&key.0, &key.1)
                .and_then(|entry| entry.view_presentations.get(tab))
                .is_some_and(|presentation| presentation.pending_result)
        });
        let should_project = active.is_some_and(|tab| {
            store.views.get(&key.0, &key.1).is_some_and(|entry| {
                entry.view_owner.is_none()
                    || newly_shown
                    || (explicit
                        && entry
                            .view_presentations
                            .get(tab)
                            .is_some_and(|view| view.pending_result))
            })
        });
        let project = should_project.then(|| active.expect("checked active").to_owned());
        let store = self.workspace_views.as_mut().expect("checked store");
        let mut changed = false;
        let mut refusal = None;
        if let Some(entry) = store.views.get_mut(&key.0, &key.1)
            && let Some(active) = project.as_deref()
        {
            match entry.project_tab(active) {
                Ok(swapped) => changed |= swapped,
                Err(error) => refusal = Some(error),
            }
        }
        if explicit {
            store.presentation_choice = None;
        }
        if let Some(seen) = seen {
            store.bookmark_seen.insert(key.clone(), seen);
        }
        if changed {
            store.generation += 1;
            if should_project && pending_result {
                self.apply_area_intent_to(&key, AreaIntent::Files);
            }
            self.persist_workspace_views();
        }
        if let Some(error) = refusal {
            self.push_diagnostic("view_presentation.inheritance_refused", error.message());
        }
        changed
    }

    pub(super) fn choose_view_presentation(&mut self, key: WorkspaceKey, tab: String) {
        if let Some(store) = self.workspace_views.as_mut() {
            store.presentation_choice = Some((key, tab));
        }
    }

    pub(super) fn choose_remote_view_presentation(&mut self, device: &str, tab: &str) {
        let key = self
            .catalog_workspaces()
            .filter(|workspace| workspace.device_id == device)
            .flat_map(|workspace| &workspace.checkouts)
            .find(|checkout| {
                checkout
                    .tabs
                    .iter()
                    .any(|held| held.id.as_deref() == Some(tab))
            })
            .map(|checkout| (device.to_owned(), checkout.path.clone()));
        if let Some(key) = key {
            self.choose_view_presentation(key, tab.to_owned());
        }
    }

    /// The Herdr tab of a pane caller in the Workspace `key`; a checkout
    /// caller has none.
    pub(super) fn control_caller_tab(&self, key: &WorkspaceKey, caller_id: &str) -> Option<String> {
        let Caller::Pane(pane_id) = Caller::parse(caller_id) else {
            return None;
        };
        self.catalog_workspaces()
            .filter(|workspace| workspace.device_id == key.0)
            .flat_map(|workspace| &workspace.checkouts)
            .find(|checkout| checkout.path == key.1)?
            .tabs
            .iter()
            .find(|tab| tab.panes.iter().any(|pane| pane.id == pane_id))
            .and_then(|tab| tab.id.clone())
    }

    /// Temporarily project the caller under the same owner lock. No frame is
    /// published until commit puts the operator's presentation back.
    pub(super) fn parked_caller(
        &mut self,
        key: &WorkspaceKey,
        caller_id: &str,
        action: &Action,
    ) -> Result<Option<Parked>, crate::workspace_control::Refusal> {
        let bound_area = match action {
            Action::Select {
                expected_browser_area: Some(area),
                ..
            }
            | Action::Close {
                expected_browser_area: Some(area),
                ..
            } => Some(area),
            Action::OpenBrowser {
                area_id: Some(area),
                ..
            } if matches!(Caller::parse(caller_id), Caller::Checkout { .. }) => Some(area),
            _ => None,
        };
        let bound_owner = bound_area.and_then(|area| {
            let entry = self.workspace_views.as_ref()?.views.get(&key.0, &key.1)?;
            entry
                .view_presentations
                .iter()
                .find(|(_, view)| view.layout.area(area).is_some())
                .map(|(tab, _)| tab.clone())
                .or_else(|| entry.layout.area(area).and(entry.view_owner.clone()))
        });
        let Some(tab) = bound_owner.or_else(|| self.control_caller_tab(key, caller_id)) else {
            return Ok(None);
        };
        self.parked_tab(key, tab, action)
    }

    /// A worker's GUI read has the same presentation transaction as a
    /// caller, but its owner was captured before the worker started.
    pub(super) fn parked_tab(
        &mut self,
        key: &WorkspaceKey,
        tab: String,
        action: &Action,
    ) -> Result<Option<Parked>, crate::workspace_control::Refusal> {
        let active = self.workspace_active_tab(key);
        let topology_ready = key.0 != self.node.as_str() || self.live.is_some();
        if topology_ready
            && self.arranges_agent_areas(&key.0)
            && self.agent_tab_waiting_in(key, &tab)
        {
            return Err(crate::workspace_control::Refusal {
                reason: "view_limit",
                next_action: "Close an unused Agent tab to admit this tab and retry",
            });
        }
        let editor_active = self.snapshot.editor.active_tab_id.clone();
        let documents = self
            .snapshot
            .editor
            .tabs
            .iter()
            .filter(|document| {
                self.workspace_key(&document.workspace_id, &document.checkout_id)
                    .as_ref()
                    == Some(key)
            })
            .map(|document| document.id.clone())
            .collect();
        let Some(store) = self.workspace_views.as_mut() else {
            return Ok(None);
        };
        let called = store.lane_called.get(key).copied();
        let calls = store.lane_calls;
        let generation = store.generation;
        let entry = store.views.entry(&key.0, &key.1);
        if entry.view_owner.is_none() && active.as_deref().is_none_or(|active| active == tab) {
            return Ok(None);
        }
        let before = entry.clone();
        if entry.view_owner.is_none() {
            entry.view_owner = active.clone();
        }
        if entry.view_owner.as_deref() == Some(&tab)
            && active.as_deref() == Some(&tab)
            && !entry.view_presentations.contains_key(&tab)
        {
            return Ok(None);
        }
        let reserved = if entry.view_owner.as_deref() == Some(&tab) {
            let next = entry
                .view_presentations
                .remove(&tab)
                .unwrap_or_else(|| entry.owner_copy(&tab));
            entry.layout = next.layout;
            Some(before.layout.clone())
        } else {
            if entry.project_tab(&tab).is_err() {
                *entry = before;
                return Err(crate::workspace_control::Refusal {
                    reason: "view_limit",
                    next_action: "Close an unused View or Agent tab and retry",
                });
            }
            None
        };
        let mut identities = HashMap::new();
        if !before.view_presentations.contains_key(&tab) {
            for (prior, next) in before.layout.areas().into_iter().zip(entry.layout.areas()) {
                identities.insert(prior.id.clone(), next.id.clone());
                for (prior, next) in prior.displays.iter().zip(&next.displays) {
                    identities.insert(prior.id.clone(), next.id.clone());
                }
            }
        }
        store.view_transaction = Some((key.clone(), reserved));
        Ok(Some(Parked {
            before,
            tab,
            reveal: action_reveals(action),
            editor_active,
            called,
            calls,
            generation,
            identities,
            documents,
        }))
    }

    pub(super) fn note_control_view(
        &mut self,
        key: &WorkspaceKey,
        _caller_id: &str,
        action: &Action,
        _view_id: &str,
        parked: Option<Parked>,
    ) -> Result<(), crate::workspace_control::Refusal> {
        let Some(store) = self.workspace_views.as_mut() else {
            return Ok(());
        };
        let entry = store.views.entry(&key.0, &key.1);
        let opened = opens_or_selects(action);
        // An open into a tab or Workspace the operator is not looking at
        // leaves every lane as it was; the files icon counts the new view
        // (PRD right-lane B17).
        if let Some(parked) = parked {
            if !parked.reveal {
                let result = crate::workspace_views::Presentation {
                    layout: entry.layout.clone(),
                    pending_result: opened
                        || parked
                            .before
                            .view_presentations
                            .get(&parked.tab)
                            .is_some_and(|view| view.pending_result),
                };
                let next_id = entry.next_view_identity();
                *entry = parked.before.clone();
                entry.view_presentations.insert(parked.tab, result);
                entry.view_next_id = next_id.max(entry.next_view_identity());
                if !entry.within_view_budget() {
                    *entry = parked.before;
                    self.snapshot.editor.active_tab_id = parked.editor_active;
                    store.view_transaction = None;
                    store.generation = parked.generation;
                    store.lane_calls = parked.calls;
                    match parked.called {
                        Some(called) => {
                            store.lane_called.insert(key.clone(), called);
                        }
                        None => {
                            store.lane_called.remove(key);
                        }
                    }
                    self.retire_refused_view_documents(key, &parked.documents);
                    return Err(crate::workspace_control::Refusal {
                        reason: "view_limit",
                        next_action: "Close an unused View or Agent tab and retry",
                    });
                }
                self.snapshot.editor.active_tab_id = parked.editor_active;
                store.lane_calls = parked.calls;
                match parked.called {
                    Some(called) => {
                        store.lane_called.insert(key.clone(), called);
                    }
                    None => {
                        store.lane_called.remove(key);
                    }
                }
            }
            let changed = entry != &parked.before;
            store.generation = parked.generation + u64::from(changed);
            store.view_transaction = None;
            if changed {
                self.persist_workspace_views();
            }
        }
        Ok(())
    }

    pub(super) fn put_back_parked(&mut self, key: &WorkspaceKey, parked: Option<Parked>) {
        if let Some(parked) = parked
            && let Some(store) = self.workspace_views.as_mut()
        {
            *store.views.entry(&key.0, &key.1) = parked.before;
            self.snapshot.editor.active_tab_id = parked.editor_active;
            store.lane_calls = parked.calls;
            match parked.called {
                Some(called) => {
                    store.lane_called.insert(key.clone(), called);
                }
                None => {
                    store.lane_called.remove(key);
                }
            }
            store.view_transaction = None;
            store.generation = parked.generation;
            self.retire_refused_view_documents(key, &parked.documents);
        }
    }

    fn retire_refused_view_documents(&mut self, key: &WorkspaceKey, before: &HashSet<String>) {
        self.retire_unreferenced_view_documents(key, |id| !before.contains(id));
    }

    fn retire_unreferenced_view_documents(
        &mut self,
        key: &WorkspaceKey,
        eligible: impl Fn(&str) -> bool,
    ) {
        let created: Vec<_> = self
            .snapshot
            .editor
            .tabs
            .iter()
            .filter(|tab| {
                eligible(&tab.id)
                    && self
                        .workspace_key(&tab.workspace_id, &tab.checkout_id)
                        .as_ref()
                        == Some(key)
                    && !self.document_kept(&tab.id)
                    && !self
                        .all_view_layouts(key)
                        .flat_map(|layout| layout.displays())
                        .any(|display| display.tab_id.as_deref() == Some(&tab.id))
            })
            .map(|tab| tab.id.clone())
            .collect();
        for id in created {
            if let Some(index) = self
                .snapshot
                .editor
                .tabs
                .iter()
                .position(|tab| tab.id == id)
            {
                self.retire_editor_tab(index);
            }
        }
    }

    pub(super) fn prune_view_bookmarks(&mut self, key: &WorkspaceKey, tabs: &[&str]) -> bool {
        let reads_dropped = self.prune_gui_reads(key, tabs);
        let Some(store) = self.workspace_views.as_mut() else {
            return reads_dropped;
        };
        let Some(entry) = store.views.get_mut(&key.0, &key.1) else {
            return reads_dropped;
        };
        let documents: HashSet<String> = entry
            .layouts()
            .flat_map(|layout| layout.displays())
            .filter_map(|display| display.tab_id.clone())
            .collect();
        entry.view_next_id = entry.next_view_identity();
        let before = entry.view_presentations.len();
        entry
            .view_presentations
            .retain(|tab, _| tabs.contains(&tab.as_str()));
        let owner_gone = entry
            .view_owner
            .as_deref()
            .is_some_and(|tab| !tabs.contains(&tab));
        if owner_gone {
            entry.view_owner = None;
            if let Some(tab) = tabs.first()
                && let Some(next) = entry.view_presentations.remove(*tab)
            {
                entry.layout = next.layout;
                entry.view_owner = Some((*tab).to_owned());
            } else {
                entry.layout = crate::view_layout::Layout::default();
                entry.layout.active_area = format!("a{}", entry.view_next_id);
                if let crate::view_layout::Node::Area(area) = &mut entry.layout.root {
                    area.id = entry.layout.active_area.clone();
                }
                entry.view_next_id += 1;
                entry.layout.next_id = entry.view_next_id;
            }
        }
        let changed = entry.view_bookmarks.retain_tabs(|tab| tabs.contains(&tab))
            || before != entry.view_presentations.len()
            || owner_gone;
        if changed {
            store.generation += 1;
            self.retire_unreferenced_view_documents(key, |id| documents.contains(id));
        }
        changed | reads_dropped
    }
}

fn opens_or_selects(action: &Action) -> bool {
    matches!(
        action,
        Action::OpenFile { .. }
            | Action::OpenDiff { .. }
            | Action::OpenBrowser { .. }
            | Action::Select { .. }
    )
}

fn action_reveals(action: &Action) -> bool {
    matches!(
        action,
        Action::OpenFile { reveal: true, .. }
            | Action::OpenDiff { reveal: true, .. }
            | Action::OpenBrowser { reveal: true, .. }
            | Action::Select { reveal: true, .. }
    )
}
