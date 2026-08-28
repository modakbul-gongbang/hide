use serde::Serialize;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[cfg_attr(not(test), allow(dead_code))]
pub enum PaneId {
    TerminalA,
    TerminalB,
    Editor,
    Browser,
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
}
