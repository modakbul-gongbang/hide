use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::domain::AgentPhase;
use crate::presentation::AgentPresentationStore;

pub const PET_SIZE: i32 = 124;
pub const DRAG_THRESHOLD: i32 = 3;
pub const CLICK_SUPPRESSION_MS: u64 = 500;
const PET_SETTINGS_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PetPose {
    Error,
    Attention,
    Working,
    Idle,
    Ended,
    Disconnected,
}

impl PetPose {
    pub fn asset_path(self) -> &'static str {
        match self {
            Self::Error => "themes/default/assets/error-fire.png",
            Self::Attention => "themes/default/assets/attention-fire.png",
            Self::Working => "themes/default/assets/working-fire-v5.png",
            Self::Idle | Self::Ended => "themes/default/assets/idle-fire.png",
            Self::Disconnected => "themes/default/assets/disconnected-fire.png",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PetPresentation {
    pub pose: PetPose,
    pub asset_path: String,
    pub agent_id: Option<String>,
    pub summary: String,
    pub host: Option<String>,
    pub workspace_id: Option<String>,
    pub tab_id: Option<String>,
    pub pane_id: Option<String>,
}

pub fn present_agents(agents: &AgentPresentationStore) -> PetPresentation {
    let selected = agents.prioritized().into_iter().next();
    let Some(agent) = selected else {
        return PetPresentation {
            pose: PetPose::Idle,
            asset_path: PetPose::Idle.asset_path().to_owned(),
            agent_id: None,
            summary: "No active agents".to_owned(),
            host: None,
            workspace_id: None,
            tab_id: None,
            pane_id: None,
        };
    };
    let pose = match agent.phase {
        AgentPhase::Error => PetPose::Error,
        AgentPhase::Attention => PetPose::Attention,
        AgentPhase::Working => PetPose::Working,
        AgentPhase::Idle => PetPose::Idle,
        AgentPhase::Ended => PetPose::Ended,
    };
    PetPresentation {
        pose,
        asset_path: pose.asset_path().to_owned(),
        agent_id: Some(agent.stable_id.clone()),
        summary: agent.summary.clone(),
        host: Some(agent.host.clone()),
        workspace_id: Some(agent.workspace_id.clone()),
        tab_id: Some(agent.tab_id.clone()),
        pane_id: Some(agent.pane_id.clone()),
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct NativePetWindowContract {
    pub width: i32,
    pub height: i32,
    pub borderless: bool,
    pub always_on_top: bool,
    pub render_click_through: bool,
    pub hit_window_captures_pointer: bool,
}

impl Default for NativePetWindowContract {
    fn default() -> Self {
        Self {
            width: PET_SIZE,
            height: PET_SIZE,
            borderless: true,
            always_on_top: true,
            render_click_through: true,
            hit_window_captures_pointer: true,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MonitorRect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl MonitorRect {
    pub fn right(self) -> i32 {
        self.x + self.width
    }

    pub fn bottom(self) -> i32 {
        self.y + self.height
    }
}

pub fn clamp_position(position: Point, monitor: MonitorRect, size: i32) -> Point {
    let max_x = (monitor.right() - size).max(monitor.x);
    let max_y = (monitor.bottom() - size).max(monitor.y);
    Point {
        x: position.x.clamp(monitor.x, max_x),
        y: position.y.clamp(monitor.y, max_y),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DragSnapshot {
    pub pointer_down: Point,
    pub window_origin: Point,
    pub moved: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DragSession {
    snapshot: DragSnapshot,
    last_position: Point,
}

impl DragSession {
    pub fn begin(pointer_down: Point, window_origin: Point) -> Self {
        Self {
            snapshot: DragSnapshot {
                pointer_down,
                window_origin,
                moved: false,
            },
            last_position: window_origin,
        }
    }

    pub fn update(&mut self, cursor: Point, monitor: MonitorRect, size: i32) -> Point {
        let dx = cursor.x - self.snapshot.pointer_down.x;
        let dy = cursor.y - self.snapshot.pointer_down.y;
        if dx.abs() >= DRAG_THRESHOLD || dy.abs() >= DRAG_THRESHOLD {
            self.snapshot.moved = true;
        }
        self.last_position = clamp_position(
            Point {
                x: self.snapshot.window_origin.x + dx,
                y: self.snapshot.window_origin.y + dy,
            },
            monitor,
            size,
        );
        self.last_position
    }

    pub fn finish(self, now_ms: u64) -> DragOutcome {
        DragOutcome {
            position: self.last_position,
            moved: self.snapshot.moved,
            suppress_click_until_ms: self
                .snapshot
                .moved
                .then_some(now_ms.saturating_add(CLICK_SUPPRESSION_MS)),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DragOutcome {
    pub position: Point,
    pub moved: bool,
    pub suppress_click_until_ms: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PetSettingsState {
    pub schema_version: u32,
    pub enabled: bool,
    pub always_on_top: bool,
    pub position: Point,
    pub visible: bool,
}

impl Default for PetSettingsState {
    fn default() -> Self {
        Self {
            schema_version: PET_SETTINGS_SCHEMA_VERSION,
            enabled: true,
            always_on_top: true,
            position: Point { x: 32, y: 32 },
            visible: true,
        }
    }
}

impl PetSettingsState {
    pub fn load_or_default(path: &Path) -> Result<Self, PetSettingsError> {
        let bytes = match fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(error) => return Err(PetSettingsError::Io(error.to_string())),
        };
        let settings: Self = serde_json::from_slice(&bytes)
            .map_err(|error| PetSettingsError::InvalidJson(error.to_string()))?;
        if settings.schema_version != PET_SETTINGS_SCHEMA_VERSION {
            return Err(PetSettingsError::UnsupportedSchema(settings.schema_version));
        }
        Ok(settings)
    }

    pub fn save_atomic(&self, path: &Path) -> Result<(), PetSettingsError> {
        let parent = path
            .parent()
            .ok_or_else(|| PetSettingsError::Io("pet settings path has no parent".to_owned()))?;
        fs::create_dir_all(parent).map_err(|error| PetSettingsError::Io(error.to_string()))?;
        let temporary = path.with_extension(format!("tmp-{}", std::process::id()));
        let bytes = serde_json::to_vec_pretty(self)
            .map_err(|error| PetSettingsError::InvalidJson(error.to_string()))?;
        fs::write(&temporary, bytes).map_err(|error| PetSettingsError::Io(error.to_string()))?;
        if let Err(error) = fs::rename(&temporary, path) {
            let _ = fs::remove_file(&temporary);
            return Err(PetSettingsError::Io(error.to_string()));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PetSettingsError {
    Io(String),
    InvalidJson(String),
    UnsupportedSchema(u32),
}

impl std::fmt::Display for PetSettingsError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(message) => write!(formatter, "pet settings I/O failed: {message}"),
            Self::InvalidJson(message) => write!(formatter, "pet settings JSON invalid: {message}"),
            Self::UnsupportedSchema(version) => {
                write!(formatter, "pet settings schema unsupported: {version}")
            }
        }
    }
}

impl std::error::Error for PetSettingsError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OverlayAgent {
    pub stable_id: String,
    pub host: String,
    pub workspace_id: String,
    pub tab_id: String,
    pub pane_id: String,
    pub name: String,
    pub phase: AgentPhase,
    pub summary: String,
    pub idle: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OverlayGroup {
    pub key: String,
    pub host: String,
    pub workspace_id: String,
    pub collapsed: bool,
    pub agents: Vec<OverlayAgent>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OverlayKey {
    ArrowUp,
    ArrowDown,
    Tab,
    ShiftTab,
    Enter,
    Escape,
    Toggle,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OverlayAction {
    None,
    Focus {
        host: String,
        workspace_id: String,
        tab_id: String,
        pane_id: String,
        agent_id: String,
    },
    CloseRestoreFocus {
        previous_focus: Option<String>,
    },
}

#[derive(Clone, Debug, Default)]
pub struct OptionTabOverlay {
    open: bool,
    groups: Vec<OverlayGroup>,
    selected: Option<(usize, usize)>,
    previous_focus: Option<String>,
}

impl OptionTabOverlay {
    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn groups(&self) -> &[OverlayGroup] {
        &self.groups
    }

    pub fn open(&mut self, agents: &AgentPresentationStore, previous_focus: Option<String>) {
        let mut grouped = BTreeMap::<(String, String), Vec<OverlayAgent>>::new();
        for agent in agents.iter() {
            grouped
                .entry((agent.host.clone(), agent.workspace_id.clone()))
                .or_default()
                .push(OverlayAgent {
                    stable_id: agent.stable_id.clone(),
                    host: agent.host.clone(),
                    workspace_id: agent.workspace_id.clone(),
                    tab_id: agent.tab_id.clone(),
                    pane_id: agent.pane_id.clone(),
                    name: agent.name.clone(),
                    phase: agent.phase,
                    summary: agent.summary.clone(),
                    idle: matches!(agent.phase, AgentPhase::Idle | AgentPhase::Ended),
                });
        }
        let mut groups = grouped
            .into_iter()
            .map(|((host, workspace_id), mut agents)| {
                agents.sort_by_key(|agent| (phase_priority(agent.phase), agent.stable_id.clone()));
                let all_idle = agents.iter().all(|agent| agent.idle);
                OverlayGroup {
                    key: format!("{host}:{workspace_id}"),
                    host,
                    workspace_id,
                    collapsed: all_idle,
                    agents,
                }
            })
            .collect::<Vec<_>>();
        groups.sort_by(|left, right| left.key.cmp(&right.key));
        self.groups = groups;
        self.selected = self.first_visible_agent();
        self.previous_focus = previous_focus;
        self.open = true;
    }

    pub fn close(&mut self) -> OverlayAction {
        if !self.open {
            return OverlayAction::None;
        }
        self.open = false;
        self.selected = None;
        OverlayAction::CloseRestoreFocus {
            previous_focus: self.previous_focus.take(),
        }
    }

    pub fn toggle(
        &mut self,
        agents: &AgentPresentationStore,
        previous_focus: Option<String>,
    ) -> OverlayAction {
        if self.open {
            self.close()
        } else {
            self.open(agents, previous_focus);
            OverlayAction::None
        }
    }

    pub fn handle_key(&mut self, key: OverlayKey) -> OverlayAction {
        if !self.open {
            return OverlayAction::None;
        }
        match key {
            OverlayKey::Escape | OverlayKey::Toggle => self.close(),
            OverlayKey::ArrowUp | OverlayKey::ShiftTab => {
                self.move_selection(-1);
                OverlayAction::None
            }
            OverlayKey::ArrowDown | OverlayKey::Tab => {
                self.move_selection(1);
                OverlayAction::None
            }
            OverlayKey::Enter => self.selected_focus().unwrap_or(OverlayAction::None),
        }
    }

    pub fn render_text(&self) -> String {
        if !self.open {
            return String::new();
        }
        let mut lines = vec!["AGENT SWITCHER  (Option+Tab)".to_owned()];
        for (group_index, group) in self.groups.iter().enumerate() {
            let marker = if group.collapsed { "▸" } else { "▾" };
            lines.push(format!("{marker} {} / {}", group.host, group.workspace_id));
            if group.collapsed {
                continue;
            }
            for (agent_index, agent) in group.agents.iter().enumerate() {
                let selected = self.selected == Some((group_index, agent_index));
                let marker = if selected { "▸" } else { " " };
                lines.push(format!(
                    "  {marker} {} {} - {:?} - {} - {}",
                    agent.name, agent.stable_id, agent.phase, agent.pane_id, agent.summary
                ));
            }
        }
        lines.join("\n")
    }

    fn first_visible_agent(&self) -> Option<(usize, usize)> {
        self.groups
            .iter()
            .enumerate()
            .find_map(|(group_index, group)| {
                (!group.collapsed && !group.agents.is_empty()).then_some((group_index, 0))
            })
    }

    fn visible_agents(&self) -> Vec<(usize, usize)> {
        self.groups
            .iter()
            .enumerate()
            .filter(|(_, group)| !group.collapsed)
            .flat_map(|(group_index, group)| {
                group
                    .agents
                    .iter()
                    .enumerate()
                    .map(move |(agent_index, _)| (group_index, agent_index))
            })
            .collect()
    }

    fn move_selection(&mut self, delta: isize) {
        let visible = self.visible_agents();
        if visible.is_empty() {
            self.selected = None;
            return;
        }
        let index = self
            .selected
            .and_then(|selected| visible.iter().position(|candidate| *candidate == selected))
            .unwrap_or(0);
        let next = (index as isize + delta).rem_euclid(visible.len() as isize) as usize;
        self.selected = Some(visible[next]);
    }

    fn selected_focus(&self) -> Option<OverlayAction> {
        let (group_index, agent_index) = self.selected?;
        let agent = self.groups.get(group_index)?.agents.get(agent_index)?;
        Some(OverlayAction::Focus {
            host: agent.host.clone(),
            workspace_id: agent.workspace_id.clone(),
            tab_id: agent.tab_id.clone(),
            pane_id: agent.pane_id.clone(),
            agent_id: agent.stable_id.clone(),
        })
    }
}

fn phase_priority(phase: AgentPhase) -> u8 {
    match phase {
        AgentPhase::Error => 0,
        AgentPhase::Attention => 1,
        AgentPhase::Working => 2,
        AgentPhase::Idle => 3,
        AgentPhase::Ended => 4,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CLICK_SUPPRESSION_MS, DragSession, MonitorRect, NativePetWindowContract, OptionTabOverlay,
        OverlayAction, OverlayKey, PetPose, PetSettingsState, Point, clamp_position,
        present_agents,
    };
    use crate::domain::{AgentPhase, AgentProjection, DomainProjection, HostScope};
    use crate::presentation::AgentPresentationStore;

    fn store() -> AgentPresentationStore {
        let mut projection = DomainProjection::default();
        let mut snapshot = crate::domain::fixture_snapshot(1);
        snapshot.agents = vec![
            AgentProjection {
                agent_instance_id: "idle".to_owned(),
                parent_agent_instance_id: None,
                host: HostScope {
                    host_id: "local".to_owned(),
                    session_id: "fixture".to_owned(),
                },
                workspace_id: "w".to_owned(),
                tab_id: "t".to_owned(),
                pane_id: "p-idle".to_owned(),
                name: "Idle".to_owned(),
                kind: "codex".to_owned(),
                phase: AgentPhase::Idle,
                summary: Some("idle summary".to_owned()),
                elapsed_seconds: 2,
            },
            AgentProjection {
                agent_instance_id: "urgent".to_owned(),
                parent_agent_instance_id: None,
                host: HostScope {
                    host_id: "mini".to_owned(),
                    session_id: "fixture".to_owned(),
                },
                workspace_id: "remote".to_owned(),
                tab_id: "rt".to_owned(),
                pane_id: "p-urgent".to_owned(),
                name: "Urgent".to_owned(),
                kind: "claude".to_owned(),
                phase: AgentPhase::Error,
                summary: Some("needs attention".to_owned()),
                elapsed_seconds: 4,
            },
        ];
        projection.apply_snapshot(snapshot).unwrap();
        let mut agents = AgentPresentationStore::default();
        agents.rebuild(&projection);
        agents
    }

    #[test]
    fn pet_uses_shared_priority_and_existing_campfire_asset_names() {
        let presentation = present_agents(&store());
        assert_eq!(presentation.pose, PetPose::Error);
        assert_eq!(presentation.agent_id.as_deref(), Some("urgent"));
        assert!(presentation.asset_path.ends_with("error-fire.png"));
    }

    #[test]
    fn option_tab_groups_remote_agents_and_focuses_exact_target_without_hijacking_meta() {
        let agents = store();
        let mut overlay = OptionTabOverlay::default();
        assert_eq!(
            overlay.toggle(&agents, Some("pane-before".to_owned())),
            OverlayAction::None
        );
        assert!(overlay.is_open());
        assert_eq!(overlay.groups().len(), 2);
        let action = overlay.handle_key(OverlayKey::Enter);
        assert_eq!(
            action,
            OverlayAction::Focus {
                host: "mini".to_owned(),
                workspace_id: "remote".to_owned(),
                tab_id: "rt".to_owned(),
                pane_id: "p-urgent".to_owned(),
                agent_id: "urgent".to_owned(),
            }
        );
        assert!(overlay.is_open());
        let restored = overlay.handle_key(OverlayKey::Escape);
        assert_eq!(
            restored,
            OverlayAction::CloseRestoreFocus {
                previous_focus: Some("pane-before".to_owned())
            }
        );
    }

    #[test]
    fn idle_groups_are_collapsed_and_tab_navigation_wraps_visible_entries() {
        let agents = store();
        let mut overlay = OptionTabOverlay::default();
        overlay.open(&agents, None);
        // The local group contains only idle work and is folded by default.
        assert!(
            overlay
                .groups()
                .iter()
                .find(|group| group.host == "local")
                .is_some_and(|group| group.collapsed)
        );
        overlay.handle_key(OverlayKey::Tab);
        assert!(
            matches!(overlay.handle_key(OverlayKey::Enter), OverlayAction::Focus { agent_id, .. } if agent_id == "urgent")
        );
        overlay.handle_key(OverlayKey::ArrowDown);
        assert!(
            matches!(overlay.handle_key(OverlayKey::Enter), OverlayAction::Focus { agent_id, .. } if agent_id == "urgent")
        );
    }

    #[test]
    fn pet_drag_clamps_and_suppresses_release_click_after_real_move() {
        let monitor = MonitorRect {
            x: 0,
            y: 0,
            width: 1_000,
            height: 800,
        };
        assert_eq!(
            clamp_position(Point { x: 980, y: 790 }, monitor, 124),
            Point { x: 876, y: 676 }
        );
        let mut drag = DragSession::begin(Point { x: 100, y: 100 }, Point { x: 200, y: 200 });
        drag.update(Point { x: 400, y: 400 }, monitor, 124);
        let outcome = drag.finish(1_000);
        assert!(outcome.moved);
        assert_eq!(
            outcome.suppress_click_until_ms,
            Some(1_000 + CLICK_SUPPRESSION_MS)
        );
    }

    #[test]
    fn pet_window_contract_and_position_settings_are_stable_and_atomic() {
        let contract = NativePetWindowContract::default();
        assert_eq!(contract.width, 124);
        assert!(contract.borderless && contract.always_on_top);
        assert!(contract.render_click_through && contract.hit_window_captures_pointer);
        let directory =
            std::env::temp_dir().join(format!("herdr-pet-settings-{}", std::process::id()));
        let path = directory.join("pet.json");
        let _ = std::fs::remove_dir_all(&directory);
        let settings = PetSettingsState {
            position: Point { x: 42, y: 84 },
            ..PetSettingsState::default()
        };
        settings.save_atomic(&path).unwrap();
        let loaded = PetSettingsState::load_or_default(&path).unwrap();
        assert_eq!(loaded, settings);
        let _ = std::fs::remove_dir_all(&directory);
    }
}
