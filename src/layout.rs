use std::collections::BTreeMap;

use serde::Serialize;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[cfg_attr(not(test), allow(dead_code))]
pub enum PaneId {
    TerminalA,
    TerminalB,
    Editor,
    Browser,
}

impl PaneId {
    pub const fn stable_id(self) -> &'static str {
        match self {
            Self::TerminalA => "terminal-a",
            Self::TerminalB => "terminal-b",
            Self::Editor => "editor",
            Self::Browser => "browser",
        }
    }

    pub fn from_stable_id(value: &str) -> Option<Self> {
        match value {
            "terminal-a" => Some(Self::TerminalA),
            "terminal-b" => Some(Self::TerminalB),
            "editor" => Some(Self::Editor),
            "browser" => Some(Self::Browser),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(test), allow(dead_code))]
pub enum ZoomOutcome {
    Entered(PaneId),
    Restored(PaneId),
    ClearedRemovedTarget(PaneId),
    ClearedForTopology(PaneId),
    Unavailable(&'static str),
}

#[derive(Clone, Debug, PartialEq)]
pub struct StableSplitSnapshot {
    pub ratios: Vec<f32>,
    pub focused_pane_id: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TabZoomState {
    current: StableSplitSnapshot,
    zoomed: Option<(String, StableSplitSnapshot)>,
    last_reason: Option<String>,
}

impl TabZoomState {
    pub fn new(ratios: Vec<f32>, focused_pane_id: impl Into<String>) -> Self {
        Self {
            current: StableSplitSnapshot {
                ratios,
                focused_pane_id: focused_pane_id.into(),
            },
            zoomed: None,
            last_reason: None,
        }
    }

    pub fn current(&self) -> &StableSplitSnapshot {
        &self.current
    }

    pub fn zoomed_pane_id(&self) -> Option<&str> {
        self.zoomed.as_ref().map(|(pane_id, _)| pane_id.as_str())
    }

    pub fn last_reason(&self) -> Option<&str> {
        self.last_reason.as_deref()
    }

    pub fn set_focus(&mut self, pane_id: impl Into<String>) {
        self.current.focused_pane_id = pane_id.into();
    }

    pub fn toggle(
        &mut self,
        focused_pane_id: Option<&str>,
    ) -> Result<Option<String>, &'static str> {
        if let Some((target, before)) = self.zoomed.take() {
            self.current = before;
            self.last_reason = Some(format!("restored split after zooming {target}"));
            return Ok(None);
        }
        let Some(target) = focused_pane_id else {
            self.last_reason = Some("zoom unavailable: no zoomable pane is focused".to_owned());
            return Err("no zoomable pane is focused");
        };
        let before = self.current.clone();
        self.current.focused_pane_id = target.to_owned();
        self.zoomed = Some((target.to_owned(), before));
        self.last_reason = Some(format!("zoomed pane {target}"));
        Ok(Some(target.to_owned()))
    }

    pub fn before_topology_mutation(&mut self) -> Option<String> {
        let (target, before) = self.zoomed.take()?;
        self.current = before;
        self.last_reason = Some(format!("zoom cleared before topology mutation: {target}"));
        Some(target)
    }

    pub fn target_removed(&mut self, pane_id: &str, replacement_focus: &str) -> bool {
        let Some((target, mut before)) = self.zoomed.take() else {
            return false;
        };
        if target != pane_id {
            self.zoomed = Some((target, before));
            return false;
        }
        before.focused_pane_id = replacement_focus.to_owned();
        self.current = before;
        self.last_reason = Some(format!(
            "zoom target {pane_id} was removed; focused {replacement_focus}"
        ));
        true
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct WindowLayoutState {
    pub window_id: String,
    active_tab_id: String,
    tabs: BTreeMap<String, TabZoomState>,
}

impl WindowLayoutState {
    pub fn new(window_id: impl Into<String>, active_tab_id: impl Into<String>) -> Self {
        Self {
            window_id: window_id.into(),
            active_tab_id: active_tab_id.into(),
            tabs: BTreeMap::new(),
        }
    }

    pub fn register_tab(&mut self, tab_id: impl Into<String>, state: TabZoomState) {
        self.tabs.insert(tab_id.into(), state);
    }

    pub fn select_tab(&mut self, tab_id: &str) -> Result<(), String> {
        if !self.tabs.contains_key(tab_id) {
            return Err(format!("unknown tab {tab_id:?}"));
        }
        self.active_tab_id = tab_id.to_owned();
        Ok(())
    }

    pub fn active_tab_id(&self) -> &str {
        &self.active_tab_id
    }

    pub fn active(&self) -> Result<&TabZoomState, String> {
        self.tabs
            .get(&self.active_tab_id)
            .ok_or_else(|| format!("active tab {:?} is not registered", self.active_tab_id))
    }

    pub fn active_mut(&mut self) -> Result<&mut TabZoomState, String> {
        self.tabs
            .get_mut(&self.active_tab_id)
            .ok_or_else(|| format!("active tab {:?} is not registered", self.active_tab_id))
    }

    pub fn default_for_app() -> Self {
        let mut state = Self::new("main-window", "main-tab");
        state.register_tab(
            "main-tab",
            TabZoomState::new(vec![0.55, 0.45], PaneId::TerminalA.stable_id()),
        );
        state
    }

    pub fn focused_pane(&self) -> PaneId {
        self.active()
            .ok()
            .and_then(|tab| PaneId::from_stable_id(&tab.current().focused_pane_id))
            .unwrap_or(PaneId::TerminalA)
    }

    pub fn zoomed_pane(&self) -> Option<PaneId> {
        self.active()
            .ok()
            .and_then(|tab| tab.zoomed_pane_id())
            .and_then(PaneId::from_stable_id)
    }

    pub fn set_focus(&mut self, pane: PaneId) {
        if let Ok(active) = self.active_mut() {
            active.set_focus(pane.stable_id());
        }
    }

    pub fn toggle_zoom(&mut self, focused: PaneId) -> ZoomOutcome {
        self.toggle_zoom_focused(Some(focused))
    }

    pub fn toggle_zoom_focused(&mut self, focused: Option<PaneId>) -> ZoomOutcome {
        let Ok(active) = self.active_mut() else {
            return ZoomOutcome::Unavailable("active tab is not registered");
        };
        let Some(focused) = focused else {
            return ZoomOutcome::Unavailable("no zoomable pane is focused");
        };
        let previous_zoomed = active.zoomed_pane_id().and_then(PaneId::from_stable_id);
        match active.toggle(Some(focused.stable_id())) {
            Ok(Some(target)) => PaneId::from_stable_id(&target)
                .map(ZoomOutcome::Entered)
                .unwrap_or(ZoomOutcome::Unavailable("zoom target is not a known pane")),
            Ok(None) => previous_zoomed
                .map(ZoomOutcome::Restored)
                .unwrap_or(ZoomOutcome::Unavailable("zoom state was not consistent")),
            Err(_) => ZoomOutcome::Unavailable("no zoomable pane is focused"),
        }
    }

    pub fn before_topology_mutation(&mut self) -> Option<ZoomOutcome> {
        let active = self.active_mut().ok()?;
        let target = active.zoomed_pane_id().and_then(PaneId::from_stable_id)?;
        active.before_topology_mutation();
        Some(ZoomOutcome::ClearedForTopology(target))
    }

    pub fn target_removed(
        &mut self,
        removed: PaneId,
        replacement_focus: PaneId,
    ) -> Option<ZoomOutcome> {
        let active = self.active_mut().ok()?;
        if active.target_removed(removed.stable_id(), replacement_focus.stable_id()) {
            Some(ZoomOutcome::ClearedRemovedTarget(removed))
        } else {
            None
        }
    }

    pub fn indicator(&self) -> &'static str {
        if self.zoomed_pane().is_some() {
            "Pane zoomed"
        } else {
            "Split layout"
        }
    }
}

/// Pixel geometry shared by hit-testing, pane sizing, rendering, and AX.
///
/// Keeping the derived widths and split boundary together prevents one surface
/// from drifting away from another when the window is resized or zoom changes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CanvasRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LayoutNode {
    pub pane_id: PaneId,
    pub rect: CanvasRect,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CanvasGeometry {
    pub width: f64,
    pub height: f64,
    pub navigator_width: f64,
    pub canvas_left: f64,
    pub canvas_width: f64,
    pub canvas_top: f64,
    pub canvas_bottom: f64,
    pub canvas_height: f64,
    pub tab_height: f64,
    pub status_height: f64,
    pub split_x: f64,
}

impl CanvasGeometry {
    pub fn for_size(width: f64, height: f64) -> Self {
        let width = width.max(0.0);
        let height = height.max(0.0);
        let navigator_width = (width * 0.22).clamp(190.0, 260.0);
        let canvas_left = navigator_width;
        let canvas_width = (width - canvas_left).max(0.0);
        let tab_height = 46.0;
        let status_height = 30.0;
        let canvas_top = tab_height;
        let canvas_bottom = (height - status_height).max(canvas_top);
        let canvas_height = canvas_bottom - canvas_top;
        let split_x = canvas_left + canvas_width * 0.55;
        Self {
            width,
            height,
            navigator_width,
            canvas_left,
            canvas_width,
            canvas_top,
            canvas_bottom,
            canvas_height,
            tab_height,
            status_height,
            split_x,
        }
    }

    pub fn terminal_rect(&self, zoomed: Option<PaneId>) -> Option<CanvasRect> {
        let surface = self.pane_rect(PaneId::TerminalA, zoomed)?;
        let right_inset = if zoomed.is_some() { 22.0 } else { 14.0 };
        Some(CanvasRect {
            x: surface.x + 22.0,
            y: surface.y + 22.0,
            width: (surface.width - 22.0 - right_inset).max(0.0),
            height: (surface.height - 22.0 - 12.0).max(0.0),
        })
    }

    pub fn canvas_rect(&self) -> CanvasRect {
        CanvasRect {
            x: self.canvas_left,
            y: self.canvas_top,
            width: self.canvas_width,
            height: self.canvas_height,
        }
    }

    pub fn editor_rect(&self) -> CanvasRect {
        self.pane_rect(PaneId::Editor, None).unwrap_or(CanvasRect {
            x: self.split_x,
            y: self.canvas_top,
            width: 0.0,
            height: self.canvas_height,
        })
    }

    /// Returns the one canonical surface rectangle for every pane consumer.
    /// The optional zoom target replaces the split with that pane's canvas rect.
    pub fn pane_rect(&self, pane: PaneId, zoomed: Option<PaneId>) -> Option<CanvasRect> {
        if let Some(target) = zoomed {
            return (target == pane).then_some(self.canvas_rect());
        }
        match pane {
            PaneId::TerminalA | PaneId::TerminalB => Some(CanvasRect {
                x: self.canvas_left,
                y: self.canvas_top,
                width: (self.split_x - self.canvas_left).max(0.0),
                height: self.canvas_height,
            }),
            PaneId::Editor => Some(CanvasRect {
                x: self.split_x,
                y: self.canvas_top,
                width: (self.width - self.split_x).max(0.0),
                height: self.canvas_height,
            }),
            PaneId::Browser => None,
        }
    }

    pub fn layout_nodes(&self, zoomed: Option<PaneId>) -> Vec<LayoutNode> {
        [PaneId::TerminalA, PaneId::Editor, PaneId::Browser]
            .into_iter()
            .filter_map(|pane_id| {
                self.pane_rect(pane_id, zoomed)
                    .map(|rect| LayoutNode { pane_id, rect })
            })
            .collect()
    }

    pub fn pane_at(&self, x: f64, y: f64, zoomed: Option<PaneId>) -> Option<PaneId> {
        self.layout_nodes(zoomed).into_iter().find_map(|node| {
            let inside = x >= node.rect.x
                && y >= node.rect.y
                && x < node.rect.x + node.rect.width
                && y < node.rect.y + node.rect.height;
            inside.then_some(node.pane_id)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canvas_geometry_is_deterministic_and_shared_across_surface_boundaries() {
        let geometry = CanvasGeometry::for_size(1180.0, 720.0);
        assert!((geometry.navigator_width - 259.6).abs() < f64::EPSILON * 8.0);
        assert_eq!(geometry.canvas_left, geometry.navigator_width);
        assert_eq!(
            geometry.split_x,
            geometry.canvas_left + geometry.canvas_width * 0.55
        );
        assert_eq!(
            geometry.terminal_rect(None),
            Some(CanvasRect {
                x: geometry.canvas_left + 22.0,
                y: geometry.canvas_top + 22.0,
                width: geometry.split_x - geometry.canvas_left - 36.0,
                height: geometry.canvas_height - 34.0,
            })
        );
        assert_eq!(geometry.terminal_rect(Some(PaneId::Editor)), None);
    }

    #[test]
    fn canvas_geometry_rejects_non_renderable_terminal_rects() {
        let geometry = CanvasGeometry::for_size(0.0, 0.0);
        let rect = geometry
            .terminal_rect(None)
            .expect("terminal rect remains typed");
        assert_eq!(rect.width, 0.0);
        assert_eq!(rect.height, 0.0);
    }

    #[test]
    fn pane_ids_round_trip_through_the_stable_layout_contract() {
        for pane in [
            PaneId::TerminalA,
            PaneId::TerminalB,
            PaneId::Editor,
            PaneId::Browser,
        ] {
            assert_eq!(PaneId::from_stable_id(pane.stable_id()), Some(pane));
        }
        assert_eq!(PaneId::from_stable_id("unknown"), None);
    }

    #[test]
    fn pane_layout_is_one_source_for_hit_testing_render_and_accessibility() {
        let geometry = CanvasGeometry::for_size(1180.0, 720.0);
        let nodes = geometry.layout_nodes(None);
        assert_eq!(nodes.len(), 2);
        assert_eq!(
            nodes[0].rect,
            geometry.pane_rect(PaneId::TerminalA, None).unwrap()
        );
        assert_eq!(nodes[1].rect, geometry.editor_rect());
        assert_eq!(
            geometry.pane_at(geometry.split_x - 1.0, geometry.canvas_top + 1.0, None),
            Some(PaneId::TerminalA)
        );
        assert_eq!(
            geometry.pane_at(geometry.split_x + 1.0, geometry.canvas_top + 1.0, None),
            Some(PaneId::Editor)
        );
        assert_eq!(
            geometry.pane_at(
                geometry.canvas_left + 1.0,
                geometry.canvas_top + 1.0,
                Some(PaneId::Editor)
            ),
            Some(PaneId::Editor)
        );
        assert_eq!(
            geometry.pane_rect(PaneId::TerminalA, Some(PaneId::Editor)),
            None
        );
    }

    #[test]
    fn zoom_toggle_restores_exact_ratios_and_focus() {
        let mut window = WindowLayoutState::new("window-a", "tab-a");
        window.register_tab(
            "tab-a",
            TabZoomState::new(vec![0.37, 0.63], PaneId::TerminalA.stable_id()),
        );
        let expected = window.active().unwrap().current().clone();

        assert_eq!(
            window.toggle_zoom(PaneId::TerminalA),
            ZoomOutcome::Entered(PaneId::TerminalA)
        );
        assert_eq!(
            window.toggle_zoom(PaneId::TerminalA),
            ZoomOutcome::Restored(PaneId::TerminalA)
        );
        assert_eq!(window.active().unwrap().current(), &expected);
        assert_eq!(window.zoomed_pane(), None);
    }

    #[test]
    fn missing_zoomable_focus_does_not_change_layout() {
        let mut window = WindowLayoutState::new("window-a", "tab-a");
        window.register_tab(
            "tab-a",
            TabZoomState::new(vec![0.5, 0.5], PaneId::Editor.stable_id()),
        );
        let expected = window.active().unwrap().current().clone();

        assert_eq!(
            window.toggle_zoom_focused(None),
            ZoomOutcome::Unavailable("no zoomable pane is focused")
        );
        assert_eq!(window.active().unwrap().current(), &expected);
        assert_eq!(window.zoomed_pane(), None);
    }

    #[test]
    fn removing_zoom_target_clears_stale_id_and_selects_replacement() {
        let mut window = WindowLayoutState::new("window-a", "tab-a");
        window.register_tab(
            "tab-a",
            TabZoomState::new(vec![0.5, 0.5], PaneId::Browser.stable_id()),
        );
        window.toggle_zoom(PaneId::Browser);

        assert_eq!(
            window.target_removed(PaneId::Browser, PaneId::TerminalA),
            Some(ZoomOutcome::ClearedRemovedTarget(PaneId::Browser))
        );
        assert_eq!(window.zoomed_pane(), None);
        assert_eq!(window.focused_pane(), PaneId::TerminalA);
    }

    #[test]
    fn topology_mutation_first_restores_the_pre_zoom_snapshot() {
        let mut window = WindowLayoutState::new("window-a", "tab-a");
        window.register_tab(
            "tab-a",
            TabZoomState::new(vec![0.29, 0.71], PaneId::TerminalB.stable_id()),
        );
        let expected = window.active().unwrap().current().clone();
        window.toggle_zoom(PaneId::TerminalB);

        assert_eq!(
            window.before_topology_mutation(),
            Some(ZoomOutcome::ClearedForTopology(PaneId::TerminalB))
        );
        assert_eq!(window.active().unwrap().current(), &expected);
    }

    #[test]
    fn zoom_is_window_local_and_preserved_per_tab_during_tab_switch() {
        let mut window = WindowLayoutState::new("window-a", "tab-a");
        window.register_tab("tab-a", TabZoomState::new(vec![0.4, 0.6], "pane-a"));
        window.register_tab("tab-b", TabZoomState::new(vec![0.7, 0.3], "pane-b"));

        window.active_mut().unwrap().toggle(Some("pane-a")).unwrap();
        window.select_tab("tab-b").unwrap();
        assert_eq!(window.active().unwrap().zoomed_pane_id(), None);
        window.select_tab("tab-a").unwrap();
        assert_eq!(window.active().unwrap().zoomed_pane_id(), Some("pane-a"));
        assert_eq!(window.active().unwrap().current().ratios, vec![0.4, 0.6]);
    }

    #[test]
    fn stable_zoom_target_removal_clears_id_and_selects_replacement() {
        let mut state = TabZoomState::new(vec![0.31, 0.69], "pane-browser");
        state.toggle(Some("pane-browser")).unwrap();
        assert!(state.target_removed("pane-browser", "pane-terminal"));
        assert_eq!(state.zoomed_pane_id(), None);
        assert_eq!(state.current().focused_pane_id, "pane-terminal");
        assert!(state.last_reason().unwrap().contains("was removed"));
    }

    #[test]
    fn stable_topology_mutation_restores_exact_pre_zoom_ratios() {
        let mut state = TabZoomState::new(vec![0.23, 0.77], "pane-editor");
        let expected = state.current().clone();
        state.toggle(Some("pane-editor")).unwrap();
        assert_eq!(
            state.before_topology_mutation(),
            Some("pane-editor".to_owned())
        );
        assert_eq!(state.current(), &expected);
        assert_eq!(state.zoomed_pane_id(), None);
    }
}
