//! Agent tab View bookmarks at runtime (PRD tab-view-bookmark).
//!
//! A View area is shared by every Agent tab of a Workspace; each tab only
//! remembers, per area, the display that was in front while it was the active
//! tab (`WorkspaceView::view_bookmarks`). One pass, `track_view_bookmarks`,
//! keeps that memory: it runs inside `sync_workspace_view`, which follows
//! every event that can move the screen and precedes every snapshot read, so
//! a tab that becomes visible by any path (an event, Herdr's own focus
//! followed by the catalog, a device's confirmed switch) is bookmarked in the
//! same frame it appears in, and when nothing moved the pass is a comparison
//! that writes nothing and does no I/O.
//!
//! - Restore: the active Agent tab was not shown in any Agent area of the
//!   Workspace at the previous pass. Each area that still holds the bookmarked
//!   display shows it; nothing opens, closes or splits, and neither the area
//!   in use, the panel nor the keyboard moves (D-01, D-03, D-12).
//! - Record: the active tab is the one of the previous pass and an area's
//!   front changed, so the new front is that tab's bookmark (D-04).
//!
//! A pane that asks through `hide file/diff/browser open` or `hide view
//! select` from a tab that is not the active one, without `--reveal`, writes
//! its own tab's bookmark and leaves the operator's fronts as they were
//! (D-05, D-06).

use super::workspace_view::WorkspaceKey;
use super::*;
use crate::view_layout::Layout as ViewLayout;
use crate::workspace_control::{Action, Caller};

/// What the last pass saw of one Workspace.
#[derive(Default)]
pub(super) struct Seen {
    shown: Vec<String>,
    active: Option<String>,
    fronts: Vec<(String, String)>,
}

impl Seen {
    fn is(&self, shown: &[&str], active: Option<&str>, fronts: &[(&str, &str)]) -> bool {
        self.active.as_deref() == active
            && self
                .shown
                .iter()
                .map(String::as_str)
                .eq(shown.iter().copied())
            && self
                .fronts
                .iter()
                .map(|(area, display)| (area.as_str(), display.as_str()))
                .eq(fronts.iter().copied())
    }
}

/// The fronts a Workspace's View areas had, and the area in use.
pub(super) struct Fronts {
    active_area: String,
    areas: Vec<(String, String)>,
}

fn fronts_of(layout: &ViewLayout) -> Vec<(&str, &str)> {
    layout
        .areas()
        .into_iter()
        .filter_map(|area| Some((area.id.as_str(), area.active.as_deref()?)))
        .collect()
}

/// What `track_view_bookmarks` decided to do about one Workspace.
struct Plan {
    key: WorkspaceKey,
    shown: Vec<String>,
    active: Option<String>,
    restore: Vec<String>,
    record: Vec<(String, String)>,
}

impl Runtime {
    /// The Agent tabs shown in the Workspace's Agent areas, and the active
    /// one. A device Workspace has one area, so its shown tab is the
    /// checkout's active tab.
    fn agent_view_of(&self, key: &WorkspaceKey) -> (Vec<&str>, Option<&str>) {
        if key.0 == self.node.as_str()
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
    fn workspace_active_tab(&self, key: &WorkspaceKey) -> Option<String> {
        self.agent_view_of(key).1.map(str::to_owned)
    }

    /// The pass. Returns whether it changed the front Workspace's View
    /// layout, so the caller reconciles the editor with it before the
    /// snapshot is published.
    pub(super) fn track_view_bookmarks(&mut self) -> bool {
        let Some(plan) = self.plan_view_bookmarks() else {
            return false;
        };
        let Some(store) = self.workspace_views.as_mut() else {
            return false;
        };
        let stamp_now = unix_milliseconds();
        let mut layout_changed = false;
        let mut bookmarks_changed = false;
        let mut fronts = Vec::new();
        // A Workspace with no stored entry has nothing to write and nothing
        // to remember, and observing it must not make one.
        if let Some(entry) = store.views.get_mut(&plan.key.0, &plan.key.1) {
            if let Some(active) = plan.active.as_deref() {
                for (area, display) in &plan.record {
                    bookmarks_changed |= entry.view_bookmarks.record(active, area, display);
                }
            }
            for display in &plan.restore {
                let stamp = entry.layout.next_stamp(stamp_now);
                layout_changed |= entry.layout.show(display, stamp).unwrap_or(false);
            }
            // An area that collapsed takes its entries with it (D-11); the
            // fronts differ from what was seen whenever the areas changed.
            let layout = &entry.layout;
            bookmarks_changed |= entry
                .view_bookmarks
                .retain_areas(|area| layout.area(area).is_some());
            fronts = fronts_of(&entry.layout)
                .into_iter()
                .map(|(area, display)| (area.to_owned(), display.to_owned()))
                .collect();
        }
        store.bookmark_seen.insert(
            plan.key,
            Seen {
                shown: plan.shown,
                active: plan.active,
                fronts,
            },
        );
        if layout_changed {
            store.generation += 1;
        }
        if layout_changed || bookmarks_changed {
            self.persist_workspace_views();
        }
        layout_changed
    }

    /// Compares the front Workspace with what the last pass saw. Nothing
    /// moved is `None` and costs borrowed comparisons only.
    fn plan_view_bookmarks(&self) -> Option<Plan> {
        let store = self.workspace_views.as_ref()?;
        let key = store.front.as_ref()?;
        let (shown, active) = self.agent_view_of(key);
        let view = store.views.get(&key.0, &key.1);
        let fronts = view.map(|view| fronts_of(&view.layout)).unwrap_or_default();
        let seen = store.bookmark_seen.get(key);
        if seen.is_some_and(|seen| seen.is(&shown, active, &fronts)) {
            return None;
        }
        // D-03: only a tab that no Agent area showed before is restored.
        let newly_shown = active.is_some_and(|active| {
            seen.is_none_or(|seen| !seen.shown.iter().any(|tab| tab == active))
        });
        let restore = match (active, view) {
            (Some(active), Some(view)) if newly_shown => view
                .view_bookmarks
                .of(active)
                .into_iter()
                .flatten()
                .filter(|(area, display)| {
                    view.layout.area(area).is_some_and(|area| {
                        area.active.as_deref() != Some(display.as_str())
                            && area.displays.iter().any(|held| held.id == **display)
                    })
                })
                .map(|(_, display)| display.clone())
                .collect(),
            _ => Vec::new(),
        };
        // D-04: the same active tab as before, so a front that changed is the
        // operator's (or an agent's) doing for this tab.
        let record = match (active, seen) {
            (Some(active), Some(seen)) if seen.active.as_deref() == Some(active) => fronts
                .iter()
                .filter(|front| {
                    !seen
                        .fronts
                        .iter()
                        .any(|(a, d)| (a.as_str(), d.as_str()) == **front)
                })
                .map(|(area, display)| ((*area).to_owned(), (*display).to_owned()))
                .collect(),
            _ => Vec::new(),
        };
        Some(Plan {
            key: key.clone(),
            shown: shown.into_iter().map(str::to_owned).collect(),
            active: active.map(str::to_owned),
            restore,
            record,
        })
    }

    /// The Herdr tab of a pane caller in the Workspace `key`; a checkout
    /// caller has none.
    fn control_caller_tab(&self, key: &WorkspaceKey, caller_id: &str) -> Option<String> {
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

    /// The caller of a `hide ... open` or `hide view select` that must not
    /// take the operator's screen: a pane of a tab that is not the active one,
    /// asking without `--reveal` (D-05, D-06). Returns the caller's tab and
    /// the fronts to put back.
    pub(super) fn parked_caller(
        &self,
        key: &WorkspaceKey,
        caller_id: &str,
        action: &Action,
    ) -> Option<(String, Fronts)> {
        let tab = self.control_caller_tab(key, caller_id)?;
        if !opens_or_selects(action) || action_reveals(action) {
            return None;
        }
        if self.workspace_active_tab(key).as_deref() == Some(tab.as_str()) {
            return None;
        }
        let layout = self.view_layout_of(key)?;
        Some((
            tab,
            Fronts {
                active_area: layout.active_area.clone(),
                areas: fronts_of(layout)
                    .into_iter()
                    .map(|(area, display)| (area.to_owned(), display.to_owned()))
                    .collect(),
            },
        ))
    }

    /// Writes the bookmarks an open or a select of `view_id` is owed, after
    /// it ran. A parked caller's own tab gets it and the fronts go back; any
    /// other caller's action stands as it ran, and the active tab, and a pane
    /// caller's tab, remember the front it made.
    pub(super) fn note_control_view(
        &mut self,
        key: &WorkspaceKey,
        caller_id: &str,
        action: &Action,
        view_id: &str,
        parked: Option<(String, Fronts)>,
    ) {
        if !opens_or_selects(action) {
            return;
        }
        match parked {
            Some((tab, before)) => {
                self.restore_fronts(key, &before);
                self.record_view_bookmark(key, &tab, view_id);
            }
            None => {
                if let Some(active) = self.workspace_active_tab(key) {
                    self.record_view_bookmark(key, &active, view_id);
                }
                if let Some(tab) = self.control_caller_tab(key, caller_id) {
                    self.record_view_bookmark(key, &tab, view_id);
                }
            }
        }
    }

    /// A parked action that was refused may still have placed a display
    /// before it failed, so the operator's fronts go back either way.
    pub(super) fn put_back_parked(&mut self, key: &WorkspaceKey, parked: Option<(String, Fronts)>) {
        if let Some((_, before)) = parked {
            self.restore_fronts(key, &before);
        }
    }

    fn restore_fronts(&mut self, key: &WorkspaceKey, before: &Fronts) {
        let restored = self.change_view_layout(key, |layout, _| {
            let mut changed = false;
            for (area, display) in &before.areas {
                let stale = layout.area(area).is_some_and(|held| {
                    held.active.as_deref() != Some(display.as_str())
                        && held.displays.iter().any(|shown| shown.id == *display)
                });
                if stale && let Some(held) = layout.area_mut(area) {
                    held.active = Some(display.clone());
                    changed = true;
                }
            }
            if layout.active_area != before.active_area
                && layout.area(&before.active_area).is_some()
            {
                layout.active_area = before.active_area.clone();
                changed = true;
            }
            Ok(((), changed))
        });
        if let Err(error) = restored {
            self.push_diagnostic(
                "view_bookmarks.park_failed",
                format!("The operator's View fronts could not be kept: {error:?}"),
            );
        }
    }

    fn record_view_bookmark(&mut self, key: &WorkspaceKey, tab: &str, view_id: &str) {
        let Some(store) = self.workspace_views.as_mut() else {
            return;
        };
        let entry = store.views.entry(&key.0, &key.1);
        let Some(area) = entry.layout.area_of(view_id).map(|area| area.id.clone()) else {
            return;
        };
        if entry.view_bookmarks.record(tab, &area, view_id) {
            self.persist_workspace_views();
        }
    }

    /// D-11: a tab Herdr no longer lists loses its bookmark. Called with
    /// authoritative topology only, like the Agent layout's own reconcile.
    pub(super) fn prune_view_bookmarks(&mut self, key: &WorkspaceKey, tabs: &[&str]) -> bool {
        let Some(store) = self.workspace_views.as_mut() else {
            return false;
        };
        store
            .views
            .get_mut(&key.0, &key.1)
            .is_some_and(|entry| entry.view_bookmarks.retain_tabs(|tab| tabs.contains(&tab)))
    }
}

/// The view an open or a select acts on is named by its result; every other
/// action shares the layout and applies at once (D-06).
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
