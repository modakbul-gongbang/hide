//! Each Workspace's presentation in a shell that draws Agent and View areas
//! side by side (PRD S6 D-10, B8, B19, B20).
//!
//! A Workspace is one checkout on one device, keyed by the device id and the
//! checkout path, because a Herdr workspace id is not stable across a Herdr
//! restart and one checkout can hold tabs from several Herdr workspaces. The
//! state is Hide's own presentation - which areas show, which tools are open,
//! where the boundary sits, which View tabs are open and which one is active -
//! and never a terminal layout: Herdr keeps panes, splits and zoom.
//!
//! It lives in its own versioned file, apart from `core-state.json` and the
//! Swift shell's state, so a shell that does not know it never reads it and an
//! older build's settings are never rewritten by it. A file this build cannot
//! read is moved aside, never overwritten, and the defaults load.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const SCHEMA_VERSION: u32 = 1;

/// The Workspaces remembered at once. The least recently used is forgotten
/// first, so the file stays bounded however many checkouts come and go.
pub const MAX_WORKSPACES: usize = 256;

/// The View tabs remembered per Workspace; a longer strip keeps its newest.
pub const MAX_VIEW_TABS: usize = 64;

/// The Agent area's share of the width while both areas show. The bounds
/// keep each area at a readable width on the narrowest supported window; the
/// shell also enforces a pixel minimum while it draws.
pub const MIN_AGENT_SHARE: f32 = 0.2;
pub const MAX_AGENT_SHARE: f32 = 0.8;
pub const DEFAULT_AGENT_SHARE: f32 = 0.5;

/// Which working areas a Workspace shows. Changing it only changes space:
/// no tab, document or pane is closed and no split is made (D-03).
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ViewMode {
    Agents,
    #[default]
    Together,
    Views,
}

impl ViewMode {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "agents" => Some(Self::Agents),
            "together" => Some(Self::Together),
            "views" => Some(Self::Views),
            _ => None,
        }
    }

    pub fn shows_views(self) -> bool {
        self != Self::Agents
    }

    pub fn shows_agents(self) -> bool {
        self != Self::Views
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ViewTabKind {
    File,
    Diff,
}

/// One open View tab, by what it shows rather than by a tab id, so it
/// survives a restart that mints new ids.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ViewTabRecord {
    pub path: String,
    pub kind: ViewTabKind,
    /// The Changes group a diff tab compares against; absent for a file.
    #[serde(default)]
    pub committed: Option<bool>,
    #[serde(default)]
    pub preview: bool,
}

impl ViewTabRecord {
    /// Whether two records name the same tab, preview aside.
    pub fn same_target(&self, other: &Self) -> bool {
        self.path == other.path && self.kind == other.kind && self.committed == other.committed
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct WorkspaceView {
    pub device_id: String,
    pub path: String,
    #[serde(default)]
    pub mode: ViewMode,
    #[serde(default = "default_explorer")]
    pub explorer: bool,
    #[serde(default)]
    pub changes: bool,
    #[serde(default = "default_agent_share")]
    pub agent_share: f32,
    #[serde(default)]
    pub tabs: Vec<ViewTabRecord>,
    #[serde(default)]
    pub active: Option<ViewTabRecord>,
    #[serde(default)]
    pub last_used_unix_ms: u64,
}

fn default_explorer() -> bool {
    true
}

fn default_agent_share() -> f32 {
    DEFAULT_AGENT_SHARE
}

impl WorkspaceView {
    pub fn new(device_id: &str, path: &str) -> Self {
        Self {
            device_id: device_id.to_owned(),
            path: path.to_owned(),
            mode: ViewMode::default(),
            explorer: default_explorer(),
            changes: false,
            agent_share: DEFAULT_AGENT_SHARE,
            tabs: Vec::new(),
            active: None,
            last_used_unix_ms: 0,
        }
    }

    pub fn is(&self, device_id: &str, path: &str) -> bool {
        self.device_id == device_id && self.path == path
    }
}

pub fn clamp_agent_share(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(MIN_AGENT_SHARE, MAX_AGENT_SHARE)
    } else {
        DEFAULT_AGENT_SHARE
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct WorkspaceViews {
    pub workspaces: Vec<WorkspaceView>,
}

impl WorkspaceViews {
    pub fn get(&self, device_id: &str, path: &str) -> Option<&WorkspaceView> {
        self.workspaces.iter().find(|view| view.is(device_id, path))
    }

    /// The Workspace's entry, made with the defaults on first use. Making one
    /// past the cap forgets the least recently used entry.
    pub fn entry(&mut self, device_id: &str, path: &str) -> &mut WorkspaceView {
        if let Some(index) = self
            .workspaces
            .iter()
            .position(|view| view.is(device_id, path))
        {
            return &mut self.workspaces[index];
        }
        if self.workspaces.len() >= MAX_WORKSPACES
            && let Some(oldest) = self
                .workspaces
                .iter()
                .enumerate()
                .min_by_key(|(_, view)| view.last_used_unix_ms)
                .map(|(index, _)| index)
        {
            self.workspaces.remove(oldest);
        }
        self.workspaces.push(WorkspaceView::new(device_id, path));
        self.workspaces.last_mut().expect("just pushed")
    }
}

#[derive(Deserialize, Serialize)]
struct StoredWorkspaceViews {
    schema_version: u32,
    #[serde(default)]
    workspaces: Vec<WorkspaceView>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LoadOutcome {
    Loaded,
    Missing,
    /// The file could not be read as this version. `preserved_as` is where
    /// the original now sits; `None` means it could not be moved and is left
    /// where it was, in which case nothing will be written over it either.
    Unreadable {
        reason: String,
        preserved_as: Option<PathBuf>,
    },
}

/// Reads the file. An unreadable or unknown-version file is renamed beside
/// itself (`<name>.unreadable-<ms>`) before the defaults load, so the next
/// save cannot destroy what an operator or another build wrote there.
pub fn load(path: &Path, now_unix_ms: u64) -> (WorkspaceViews, LoadOutcome) {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return (WorkspaceViews::default(), LoadOutcome::Missing);
        }
        Err(error) => {
            return (
                WorkspaceViews::default(),
                LoadOutcome::Unreadable {
                    reason: format!("the file could not be read: {error}"),
                    preserved_as: None,
                },
            );
        }
    };
    let reason = match serde_json::from_slice::<StoredWorkspaceViews>(&bytes) {
        Ok(stored) if stored.schema_version == SCHEMA_VERSION => {
            let mut workspaces = stored.workspaces;
            for view in &mut workspaces {
                view.agent_share = clamp_agent_share(view.agent_share);
                view.tabs.truncate(MAX_VIEW_TABS);
            }
            workspaces.truncate(MAX_WORKSPACES);
            return (WorkspaceViews { workspaces }, LoadOutcome::Loaded);
        }
        Ok(stored) => format!(
            "schema version {} is not {SCHEMA_VERSION}",
            stored.schema_version
        ),
        Err(error) => format!("the file is not valid: {error}"),
    };
    let preserved = preserved_path(path, now_unix_ms);
    let preserved_as = fs::rename(path, &preserved).ok().map(|()| preserved);
    (
        WorkspaceViews::default(),
        LoadOutcome::Unreadable {
            reason,
            preserved_as,
        },
    )
}

fn preserved_path(path: &Path, now_unix_ms: u64) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".unreadable-{now_unix_ms}"));
    path.with_file_name(name)
}

/// Writes the file atomically: a sibling temporary file, fsync, rename.
pub fn save(path: &Path, views: &WorkspaceViews) -> Result<(), String> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .ok_or_else(|| "Workspace view state path has no parent directory".to_owned())?;
    fs::create_dir_all(parent)
        .map_err(|_| "Workspace view state directory could not be prepared".to_owned())?;
    let stored = StoredWorkspaceViews {
        schema_version: SCHEMA_VERSION,
        workspaces: views.workspaces.clone(),
    };
    let bytes = serde_json::to_vec_pretty(&stored)
        .map_err(|_| "Workspace view state could not be encoded".to_owned())?;
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".next");
    let temporary = path.with_file_name(name);
    let mut output = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&temporary)
        .map_err(|_| "Workspace view state temporary file could not be opened".to_owned())?;
    output
        .write_all(&bytes)
        .and_then(|_| output.sync_all())
        .map_err(|_| "Workspace view state temporary file could not be written".to_owned())?;
    fs::rename(&temporary, path)
        .map_err(|_| "Workspace view state file could not be replaced".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "herdr-core-workspace-views-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn a_saved_workspace_loads_back_with_its_tabs_and_tools() {
        let root = scratch("roundtrip");
        let path = root.join("workspace-views.json");
        let mut views = WorkspaceViews::default();
        let entry = views.entry("local", "/repo");
        entry.mode = ViewMode::Views;
        entry.changes = true;
        entry.agent_share = 0.3;
        entry.tabs.push(ViewTabRecord {
            path: "/repo/a.md".into(),
            kind: ViewTabKind::File,
            committed: None,
            preview: false,
        });
        entry.active = entry.tabs.first().cloned();
        save(&path, &views).unwrap();
        let (loaded, outcome) = load(&path, 1);
        assert_eq!(outcome, LoadOutcome::Loaded);
        assert_eq!(loaded, views);
        let _ = fs::remove_dir_all(&root);
    }

    // B20: a file this build cannot read is kept, byte for byte, beside the
    // path, and the defaults load.
    #[test]
    fn an_unreadable_or_future_file_is_preserved_and_defaults_load() {
        let root = scratch("unreadable");
        let path = root.join("workspace-views.json");
        for (index, body) in [
            b"{not json".as_slice(),
            br#"{"schema_version":9,"workspaces":[]}"#.as_slice(),
        ]
        .into_iter()
        .enumerate()
        {
            fs::write(&path, body).unwrap();
            let (loaded, outcome) = load(&path, index as u64);
            assert!(loaded.workspaces.is_empty());
            let LoadOutcome::Unreadable {
                preserved_as: Some(preserved),
                ..
            } = outcome
            else {
                panic!("expected a preserved unreadable file, got {outcome:?}");
            };
            assert!(
                !path.exists(),
                "the original path is free for the next save"
            );
            assert_eq!(fs::read(&preserved).unwrap(), body);
        }
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn the_least_recently_used_workspace_is_forgotten_past_the_cap() {
        let mut views = WorkspaceViews::default();
        for index in 0..MAX_WORKSPACES {
            views
                .entry("local", &format!("/w{index}"))
                .last_used_unix_ms = index as u64 + 10;
        }
        views.entry("local", "/w0").last_used_unix_ms = 1_000;
        views.entry("local", "/new");
        assert_eq!(views.workspaces.len(), MAX_WORKSPACES);
        assert!(views.get("local", "/w1").is_none());
        assert!(views.get("local", "/w0").is_some());
    }

    #[test]
    fn a_stored_share_outside_the_bounds_is_clamped() {
        assert_eq!(clamp_agent_share(0.01), MIN_AGENT_SHARE);
        assert_eq!(clamp_agent_share(f32::NAN), DEFAULT_AGENT_SHARE);
        assert_eq!(clamp_agent_share(0.95), MAX_AGENT_SHARE);
    }
}
