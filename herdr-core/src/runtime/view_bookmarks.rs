//! Each Agent tab's bookmark on the checkout's one View list (PRD
//! lane-files-checkout-views D-03, D-09, D-11, D-16, D-19).
//!
//! The checkout owns the Views, their split and their selection, whichever
//! Agent tab is in front. A tab remembers one View, the one it last put in
//! front, and gets it back when the tab appears on screen after not being on
//! it. The bookmark never moves the area in use or the keyboard.

use super::workspace_view::WorkspaceKey;
use super::*;
use crate::workspace_control::{Action, Caller};

/// What the pass saw of a Workspace it has had in front: the Agent tabs that
/// were on screen and the View in front of the area in use.
#[derive(Default)]
pub(super) struct Seen {
    shown: Vec<String>,
    front: Option<String>,
}

/// The selection a background caller must leave as it found it (PRD
/// lane-files-checkout-views B7): the area in use, the View in front of each
/// area and the editor's active tab.
pub(super) struct Kept {
    key: WorkspaceKey,
    editor_active: Option<String>,
    active_area: String,
    fronts: Vec<String>,
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

    /// The one comparison point for Agent tab bookmarks, after every event.
    /// A tab that was on no Agent area and now is the active one gets its
    /// View back in front of its area (D-11); otherwise a change of the View
    /// in front of the area in use is the active tab's new bookmark (D-16).
    pub(super) fn track_view_bookmarks(&mut self) -> bool {
        let Some(store) = self.workspace_views.as_ref() else {
            return false;
        };
        let Some(key) = store.front.as_ref() else {
            return false;
        };
        let (shown, active) = self.agent_view_of(key);
        // Most syncs see what the last one saw; those skip the owned copies and the write.
        let front_now = store
            .views
            .get(&key.0, &key.1)
            .and_then(|entry| entry.layout.active_area().active.as_deref());
        if store.bookmark_seen.get(key).is_some_and(|seen| {
            seen.front.as_deref() == front_now
                && seen
                    .shown
                    .iter()
                    .map(String::as_str)
                    .eq(shown.iter().copied())
        }) {
            return false;
        }
        let key = key.clone();
        let shown: Vec<String> = shown.into_iter().map(str::to_owned).collect();
        let active = active.map(str::to_owned);
        let now = unix_milliseconds();
        let store = self.workspace_views.as_mut().expect("the front is set");
        let seen = store.bookmark_seen.get(&key);
        let mut changed = false;
        let mut front = None;
        if let Some(entry) = store.views.get_mut(&key.0, &key.1) {
            front = entry.layout.active_area().active.clone();
            if let Some(tab) = active.as_deref() {
                if seen.is_none_or(|seen| !seen.shown.iter().any(|held| held == tab)) {
                    let bookmark = entry.view_bookmarks.of(tab).map(str::to_owned);
                    if let Some(display) = bookmark {
                        let stamp = entry.layout.next_stamp(now);
                        // A View that closed since is ignored (D-16).
                        changed = entry.layout.show(&display, stamp).unwrap_or(false);
                        front = entry.layout.active_area().active.clone();
                    }
                } else if let Some(front) = front.as_deref()
                    && seen.is_some_and(|seen| seen.front.as_deref() != Some(front))
                {
                    changed = entry.view_bookmarks.record(tab, front);
                }
            }
        }
        store.bookmark_seen.insert(key, Seen { shown, front });
        if changed {
            store.generation += 1;
            self.persist_workspace_views();
        }
        changed
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

    /// The tab of a caller who is not looking at the Workspace's front: a
    /// pane of an Agent tab other than the active one. A checkout caller and
    /// the active tab's panes are in front (D-04, D-10).
    pub(super) fn background_caller_tab(
        &self,
        key: &WorkspaceKey,
        caller_id: &str,
    ) -> Option<String> {
        let tab = self.control_caller_tab(key, caller_id)?;
        (self.workspace_active_tab(key).as_deref() != Some(tab.as_str())).then_some(tab)
    }

    /// What a background caller's open leaves as it was (B7).
    pub(super) fn keep_front(&self, key: &WorkspaceKey) -> Kept {
        let empty = crate::view_layout::Layout::default();
        let layout = self.view_layout_of(key).unwrap_or(&empty);
        Kept {
            key: key.clone(),
            editor_active: self.snapshot.editor.active_tab_id.clone(),
            active_area: layout.active_area.clone(),
            fronts: layout
                .areas()
                .into_iter()
                .filter_map(|area| area.active.clone())
                .collect(),
        }
    }

    /// Puts the selection back after a background caller's open: the new View
    /// stays where it landed, in no area's front and never in the keyboard's.
    pub(super) fn restore_front(&mut self, kept: Kept) {
        self.snapshot.editor.active_tab_id = kept.editor_active;
        let outcome = self.change_view_layout(&kept.key, |layout, stamp| {
            let mut changed = false;
            for display in &kept.fronts {
                changed |= layout.show(display, stamp).unwrap_or(false);
            }
            if layout.area(&kept.active_area).is_some() && layout.active_area != kept.active_area {
                layout.active_area = kept.active_area.clone();
                changed = true;
            }
            Ok(((), changed))
        });
        if let Err(error) = outcome {
            self.push_diagnostic("view_layout.front_not_restored", error.message());
        }
        // What is in front now is not the operator's change to record as the
        // active tab's bookmark: a background open into an empty list leaves
        // its new View there.
        let front = self
            .view_layout_of(&kept.key)
            .and_then(|layout| layout.active_area().active.clone());
        if let Some(seen) = self
            .workspace_views
            .as_mut()
            .and_then(|store| store.bookmark_seen.get_mut(&kept.key))
        {
            seen.front = front;
        }
    }

    /// The View a tab's agent opened or selected is the tab's bookmark, in
    /// front or not (D-16).
    pub(super) fn note_caller_view(
        &mut self,
        key: &WorkspaceKey,
        caller_id: &str,
        action: &Action,
        view_id: &str,
    ) {
        if !opens_or_selects(action) {
            return;
        }
        let Some(tab) = self.control_caller_tab(key, caller_id) else {
            return;
        };
        self.record_tab_view(key, &tab, view_id);
    }

    pub(super) fn record_tab_view(&mut self, key: &WorkspaceKey, tab: &str, view_id: &str) {
        let Some(store) = self.workspace_views.as_mut() else {
            return;
        };
        let entry = store.views.entry(&key.0, &key.1);
        if entry.view_bookmarks.record(tab, view_id) {
            store.generation += 1;
            self.persist_workspace_views();
        }
    }

    /// A tab that left Herdr takes its bookmark with it, and so do the opens
    /// still being read for it (D-16).
    pub(super) fn prune_view_bookmarks(&mut self, key: &WorkspaceKey, tabs: &[&str]) -> bool {
        let reads_dropped = self.prune_gui_reads(key, tabs);
        let Some(store) = self.workspace_views.as_mut() else {
            return reads_dropped;
        };
        let Some(entry) = store.views.get_mut(&key.0, &key.1) else {
            return reads_dropped;
        };
        let changed = entry.view_bookmarks.retain_tabs(|tab| tabs.contains(&tab));
        if changed {
            store.generation += 1;
        }
        changed | reads_dropped
    }
}

pub(super) fn opens_or_selects(action: &Action) -> bool {
    matches!(
        action,
        Action::OpenFile { .. }
            | Action::OpenDiff { .. }
            | Action::OpenBrowser { .. }
            | Action::Select { .. }
    )
}

pub(super) fn action_reveals(action: &Action) -> bool {
    matches!(
        action,
        Action::OpenFile { reveal: true, .. }
            | Action::OpenDiff { reveal: true, .. }
            | Action::OpenBrowser { reveal: true, .. }
            | Action::Select { reveal: true, .. }
    )
}
