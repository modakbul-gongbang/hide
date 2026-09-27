//! Agent adapter: workspace-scoped intents and projection of Hide-owned tab groups.
use super::view_areas::ViewWorkspace;
use super::workspace_view::WorkspaceKey;
use super::*;
use crate::agent_layout::{Layout, Tab};
use crate::split_tree::{AreaItem, Edge, LayoutError};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub(super) struct AgentLayoutPayload {
    workspace: ViewWorkspace,
    #[serde(flatten)]
    action: AgentLayoutAction,
}
#[derive(Debug, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
enum AgentLayoutAction {
    Focus {
        tab_id: String,
    },
    FocusArea {
        area_id: String,
    },
    Move {
        tab_id: String,
        area_id: String,
        index: usize,
    },
    Split {
        tab_id: String,
        area_id: String,
        edge: Edge,
        request_id: String,
    },
    Resize {
        split_id: String,
        ratio: f32,
    },
}

impl Runtime {
    /// Common stale-frame boundary for both independently owned columns.
    pub(super) fn area_workspace_is_current(&mut self, key: &WorkspaceKey, column: &str) -> bool {
        if self.front_workspace_key().as_ref() == Some(key) {
            return true;
        }
        self.push_diagnostic(
            format!("{column}_layout.stale_workspace"),
            format!(
                "Ignored area intent for {} on {}; that Workspace is no longer in front",
                key.1, key.0
            ),
        );
        false
    }

    pub(super) fn agent_layout_of(&self, key: &WorkspaceKey) -> Option<&Layout> {
        Some(
            &self
                .workspace_views
                .as_ref()?
                .views
                .get(&key.0, &key.1)?
                .agent_layout,
        )
    }

    pub(super) fn apply_agent_layout(&mut self, payload: AgentLayoutPayload) -> bool {
        let key = (payload.workspace.device_id, payload.workspace.path);
        if !self.area_workspace_is_current(&key, "agent") {
            return false;
        }
        if key.0 != workspace::LOCAL_DEVICE_ID || self.workspace_views.is_none() {
            self.set_error(
                "agent_layout.unsupported",
                "Agent groups are available in local Workspaces",
                false,
            );
            return true;
        }
        let request = match &payload.action {
            AgentLayoutAction::Split { request_id, .. } => Some(format!("agent:{request_id}")),
            _ => None,
        };
        if request
            .as_ref()
            .is_some_and(|id| self.split_request_seen(&key, id))
        {
            return false;
        }
        let layout = &mut self
            .workspace_views
            .as_mut()
            .expect("checked")
            .views
            .entry(&key.0, &key.1)
            .agent_layout;
        let before = layout.clone();
        let stamp = layout.tree.next_stamp(unix_milliseconds());
        let resize = matches!(payload.action, AgentLayoutAction::Resize { .. });
        let replaces_canvas = matches!(
            payload.action,
            AgentLayoutAction::Focus { .. }
                | AgentLayoutAction::Move { .. }
                | AgentLayoutAction::Split { .. }
        );
        let outcome = match payload.action {
            AgentLayoutAction::Focus { tab_id } => layout.select(&tab_id, false, stamp),
            AgentLayoutAction::FocusArea { area_id } => layout.tree.focus_area(&area_id, stamp),
            AgentLayoutAction::Move {
                tab_id,
                area_id,
                index,
            } => layout.tree.move_display(&tab_id, &area_id, index, stamp),
            AgentLayoutAction::Split {
                tab_id,
                area_id,
                edge,
                ..
            } => layout
                .tree
                .split(&tab_id, &area_id, edge, stamp)
                .map(|_| true),
            AgentLayoutAction::Resize { split_id, ratio } => layout.tree.resize(&split_id, ratio),
        };
        if let Err(error) = outcome {
            self.push_diagnostic(
                "agent_layout.refused",
                format!("Agent layout intent refused: {error:?}"),
            );
            let message = match error {
                LayoutError::AreaLimit => "At most six Agent areas can be open",
                LayoutError::DepthLimit => "Agent areas can be split at most three levels deep",
                LayoutError::DisplayLimit => "At most 64 Agent tabs can be arranged",
                LayoutError::NothingToSplit => "The area's only tab cannot split its own area",
                LayoutError::InvalidRatio => "The divider position is invalid",
                _ => "The Agent tab or area is no longer available",
            };
            self.set_error("agent_layout.refused", message, false);
            return true;
        }
        // Moving a normal tab into an area replaces its delegated canvas.
        if replaces_canvas {
            layout.canvases.remove(&layout.tree.active_area);
        }
        layout
            .canvases
            .retain(|area, _| layout.tree.area(area).is_some());
        let changed = before != *layout;
        let tab = layout.active().map(str::to_owned);
        if let Some(request) = request {
            self.remember_split_request(&key, request);
        }
        if changed {
            self.persist_workspace_views();
        }
        if !resize
            && let Some(tab_id) = tab
            && let Some((workspace_id, checkout_id)) = self
                .front_checkout()
                .map(|(w, c)| (w.to_owned(), c.to_owned()))
        {
            let prior = self.focused_visible_tab_id();
            self.apply(Event::FocusTab(FocusTabPayload {
                workspace_id,
                checkout_id,
                tab_id,
                in_place: true,
                focus_device: false,
            }));
            self.agent_sleep_visit(prior.as_deref());
        }
        changed
    }

    /// A committed pane/tab selection finds its owning area; merely drawing
    /// another live canvas never enters this path or marks its agents read.
    pub(super) fn sync_agent_selection(&mut self) {
        let Some(key) = self
            .front_workspace_key()
            .filter(|key| key.0 == workspace::LOCAL_DEVICE_ID)
        else {
            return;
        };
        let Some(tab_id) = self.focused_visible_tab_id() else {
            return;
        };
        let Some(layout) = self.agent_layout_of(&key) else {
            return;
        };
        if layout.active() == Some(&tab_id) {
            return;
        }
        let delegated = self
            .snapshot
            .navigator
            .workspaces
            .iter()
            .flat_map(|w| &w.checkouts)
            .filter(|c| c.path == key.1)
            .flat_map(|c| &c.tabs)
            .find(|tab| tab.id.as_deref() == Some(&tab_id))
            .map(|tab| tab.delegated);
        let Some(delegated) = delegated else {
            return;
        };
        let layout = &mut self
            .workspace_views
            .as_mut()
            .expect("layout exists")
            .views
            .entry(&key.0, &key.1)
            .agent_layout;
        if let Err(error) = layout.select(
            &tab_id,
            delegated,
            layout.tree.next_stamp(unix_milliseconds()),
        ) {
            self.push_diagnostic(
                "agent_layout.selection_unavailable",
                format!("Could not place selected tab {tab_id}: {error:?}"),
            );
        } else {
            self.persist_workspace_views();
        }
    }

    pub(super) fn shown_agent_tabs(&self) -> Vec<String> {
        if let Some(key) = self
            .front_workspace_key()
            .filter(|key| key.0 == workspace::LOCAL_DEVICE_ID)
            && let Some(layout) = self.agent_layout_of(&key)
        {
            let shown = layout.shown();
            if !shown.is_empty() {
                return shown;
            }
        }
        self.focused_visible_tab_id().into_iter().collect()
    }

    pub(super) fn agent_layout_snapshot(
        &self,
        layout: &Layout,
    ) -> crate::model::AgentLayoutSnapshot {
        crate::model::AgentLayoutSnapshot {
            root: layout.tree.root.clone(),
            active_area: layout.tree.active_area.clone(),
            canvases: layout.canvases.clone(),
            limits: crate::model::ViewLimitsSnapshot {
                areas: Tab::LIMITS.areas,
                depth: Tab::LIMITS.depth,
                displays: Tab::LIMITS.items,
            },
            display_count: layout.tree.display_count(),
        }
    }
}
