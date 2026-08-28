use serde::Serialize;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[cfg_attr(not(test), allow(dead_code))]
pub enum PaneId {
    TerminalA,
    TerminalB,
    Editor,
    Browser,
}

pub const TAB_HEIGHT: f64 = 46.0;
pub const STATUS_HEIGHT: f64 = 30.0;
pub const NAVIGATOR_RATIO: f64 = 0.22;
pub const SPLIT_RATIO: f64 = 0.55;
pub const DIVIDER_WIDTH: f64 = 1.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PaneFrame {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// Logical-point geometry shared by AppKit hit-testing, AX frames, native
/// browser placement, and the WGPU renderer.  The renderer projects these
/// values to backing pixels exactly once at its device scale.
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
    /// Bottom-origin y coordinate used by the unflipped browser child view.
    pub appkit_canvas_bottom: f64,
    pub split_x: f64,
}

impl CanvasGeometry {
    pub fn for_size(width: f64, height: f64) -> Self {
        let width = width.max(0.0);
        let height = height.max(0.0);
        let navigator_width = (width * NAVIGATOR_RATIO).clamp(190.0, 260.0);
        let canvas_left = navigator_width;
        let canvas_width = (width - canvas_left).max(0.0);
        let canvas_top = TAB_HEIGHT;
        let canvas_bottom = (height - STATUS_HEIGHT).max(canvas_top);
        let canvas_height = canvas_bottom - canvas_top;
        let split_x = canvas_left + canvas_width * SPLIT_RATIO;
        Self {
            width,
            height,
            navigator_width,
            canvas_left,
            canvas_width,
            canvas_top,
            canvas_bottom,
            canvas_height,
            tab_height: TAB_HEIGHT,
            status_height: STATUS_HEIGHT,
            appkit_canvas_bottom: STATUS_HEIGHT,
            split_x,
        }
    }
}

pub fn navigator_width(window_width: f64) -> f64 {
    CanvasGeometry::for_size(window_width, 0.0).navigator_width
}

pub fn browser_frame(
    window_width: f64,
    window_height: f64,
    browser_enabled: bool,
    zoom: &ZoomState,
) -> Option<PaneFrame> {
    if !browser_enabled {
        return None;
    }
    let geometry = CanvasGeometry::for_size(window_width, window_height);
    let navigator = geometry.navigator_width;
    let canvas_width = geometry.canvas_width.max(1.0);
    let height = geometry.canvas_height.max(1.0);
    match zoom {
        ZoomState::Normal => {
            let width = (canvas_width * 0.34).max(260.0).min(canvas_width * 0.45);
            Some(PaneFrame {
                x: window_width - width,
                y: geometry.appkit_canvas_bottom,
                width,
                height,
            })
        }
        ZoomState::Zoomed {
            target: PaneId::Browser,
            ..
        } => Some(PaneFrame {
            x: navigator,
            y: geometry.appkit_canvas_bottom,
            width: canvas_width,
            height,
        }),
        ZoomState::Zoomed { .. } => None,
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SplitSnapshot {
    pub ratios: Vec<f32>,
    pub focused: PaneId,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ZoomState {
    Normal,
    Zoomed {
        target: PaneId,
        before: SplitSnapshot,
    },
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

#[derive(Clone, Debug)]
pub struct ZoomController {
    state: ZoomState,
    current: SplitSnapshot,
}

impl ZoomController {
    pub fn new(ratios: Vec<f32>, focused: PaneId) -> Self {
        Self {
            state: ZoomState::Normal,
            current: SplitSnapshot { ratios, focused },
        }
    }

    pub fn state(&self) -> &ZoomState {
        &self.state
    }

    pub fn current(&self) -> &SplitSnapshot {
        &self.current
    }

    pub fn toggle(&mut self, focused: Option<PaneId>) -> ZoomOutcome {
        match self.state.clone() {
            ZoomState::Normal => {
                let Some(target) = focused else {
                    return ZoomOutcome::Unavailable("no zoomable pane is focused");
                };
                let before = self.current.clone();
                self.state = ZoomState::Zoomed { target, before };
                ZoomOutcome::Entered(target)
            }
            ZoomState::Zoomed { before, .. } => {
                self.current = before.clone();
                self.state = ZoomState::Normal;
                ZoomOutcome::Restored(before.focused)
            }
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn before_topology_mutation(&mut self) -> Option<ZoomOutcome> {
        let ZoomState::Zoomed { target, before } = self.state.clone() else {
            return None;
        };
        self.current = before;
        self.state = ZoomState::Normal;
        Some(ZoomOutcome::ClearedForTopology(target))
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn target_removed(
        &mut self,
        removed: PaneId,
        replacement_focus: PaneId,
    ) -> Option<ZoomOutcome> {
        let ZoomState::Zoomed { target, mut before } = self.state.clone() else {
            return None;
        };
        if target != removed {
            return None;
        }
        before.focused = replacement_focus;
        self.current = before;
        self.state = ZoomState::Normal;
        Some(ZoomOutcome::ClearedRemovedTarget(removed))
    }

    pub fn set_focus(&mut self, pane: PaneId) {
        self.current.focused = pane;
    }

    pub fn indicator(&self) -> &'static str {
        match self.state {
            ZoomState::Normal => "Split layout",
            ZoomState::Zoomed { .. } => "Pane zoomed",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zoom_toggle_restores_exact_ratios_and_focus() {
        let mut zoom = ZoomController::new(vec![0.37, 0.63], PaneId::TerminalA);
        let expected = zoom.current().clone();

        assert_eq!(
            zoom.toggle(Some(PaneId::TerminalA)),
            ZoomOutcome::Entered(PaneId::TerminalA)
        );
        assert_eq!(
            zoom.toggle(Some(PaneId::TerminalA)),
            ZoomOutcome::Restored(PaneId::TerminalA)
        );
        assert_eq!(zoom.current(), &expected);
        assert_eq!(zoom.state(), &ZoomState::Normal);
    }

    #[test]
    fn missing_zoomable_focus_does_not_change_layout() {
        let mut zoom = ZoomController::new(vec![0.5, 0.5], PaneId::Editor);
        let expected = zoom.current().clone();

        assert_eq!(
            zoom.toggle(None),
            ZoomOutcome::Unavailable("no zoomable pane is focused")
        );
        assert_eq!(zoom.current(), &expected);
        assert_eq!(zoom.state(), &ZoomState::Normal);
    }

    #[test]
    fn removing_zoom_target_clears_stale_id_and_selects_replacement() {
        let mut zoom = ZoomController::new(vec![0.5, 0.5], PaneId::Browser);
        zoom.toggle(Some(PaneId::Browser));

        assert_eq!(
            zoom.target_removed(PaneId::Browser, PaneId::TerminalA),
            Some(ZoomOutcome::ClearedRemovedTarget(PaneId::Browser))
        );
        assert_eq!(zoom.state(), &ZoomState::Normal);
        assert_eq!(zoom.current().focused, PaneId::TerminalA);
    }

    #[test]
    fn topology_mutation_first_restores_the_pre_zoom_snapshot() {
        let mut zoom = ZoomController::new(vec![0.29, 0.71], PaneId::TerminalB);
        let expected = zoom.current().clone();
        zoom.toggle(Some(PaneId::TerminalB));

        assert_eq!(
            zoom.before_topology_mutation(),
            Some(ZoomOutcome::ClearedForTopology(PaneId::TerminalB))
        );
        assert_eq!(zoom.current(), &expected);
    }

    #[test]
    fn browser_zoom_preserves_navigator_tab_and_status_chrome() {
        let before = SplitSnapshot {
            ratios: vec![0.55, 0.45],
            focused: PaneId::Browser,
        };
        let frame = browser_frame(
            1180.0,
            720.0,
            true,
            &ZoomState::Zoomed {
                target: PaneId::Browser,
                before,
            },
        )
        .unwrap();
        assert_eq!(frame.x, navigator_width(1180.0));
        assert_eq!(frame.y, STATUS_HEIGHT);
        assert_eq!(frame.x + frame.width, 1180.0);
        assert_eq!(frame.y + frame.height, 720.0 - TAB_HEIGHT);
    }

    #[test]
    fn canvas_geometry_keeps_flipped_and_unflipped_origins_explicit() {
        let geometry = CanvasGeometry::for_size(1180.0, 720.0);
        assert_eq!(geometry.canvas_top, TAB_HEIGHT);
        assert_eq!(geometry.appkit_canvas_bottom, STATUS_HEIGHT);
        assert_eq!(geometry.canvas_bottom, 720.0 - STATUS_HEIGHT);
    }

    #[test]
    fn terminal_and_editor_zoom_remove_the_browser_native_child_surface() {
        for target in [PaneId::TerminalA, PaneId::TerminalB, PaneId::Editor] {
            let zoom = ZoomState::Zoomed {
                target,
                before: SplitSnapshot {
                    ratios: vec![0.55, 0.45],
                    focused: target,
                },
            };
            assert_eq!(browser_frame(1180.0, 720.0, true, &zoom), None);
        }
    }
}
