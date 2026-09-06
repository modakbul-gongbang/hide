use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::model::{
    DeviceRegistration, PaneReadRecord, PetOriginSnapshot, RightPanelSection, UiStateSnapshot,
    WorkspaceRegistration, default_accent_hex, default_font_size, default_pane_text_scale,
    default_panel_visible,
};

const UI_STATE_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, Deserialize, Serialize)]
struct StoredUiState {
    schema_version: u32,
    #[serde(default = "default_panel_visible")]
    left_sidebar_visible: bool,
    #[serde(default = "default_panel_visible")]
    right_panel_visible: bool,
    #[serde(default)]
    right_panel_section: RightPanelSection,
    expanded_paths: Vec<String>,
    #[serde(default)]
    collapsed_workspace_ids: Vec<String>,
    selected_path: Option<String>,
    selected_pane_id: Option<String>,
    #[serde(default)]
    shortcut_bindings: BTreeMap<String, String>,
    #[serde(default = "default_pet_visible")]
    pet_visible: bool,
    #[serde(default)]
    pet_origin: Option<PetOriginSnapshot>,
    #[serde(default)]
    pet_shortcut: Option<String>,
    #[serde(default)]
    focused_device_id: Option<String>,
    #[serde(default)]
    focused_checkout_id: Option<String>,
    #[serde(default)]
    workspace_registrations: Vec<WorkspaceRegistration>,
    #[serde(default)]
    device_registrations: Vec<DeviceRegistration>,
    #[serde(default = "default_accent_hex")]
    accent_hex: String,
    #[serde(default = "default_font_size")]
    font_size: f32,
    #[serde(default)]
    pane_text_scales: BTreeMap<String, f32>,
    /// Absent in a store written while the editor's zoom still lived in
    /// `pane_text_scales`, where the pane prune kept deleting it. It loads at
    /// the default scale, so the operator's editor opens unzoomed once.
    #[serde(default = "default_pane_text_scale")]
    editor_text_scale: f32,
    /// Absent in a store written before Hide owned the read axis. It loads as
    /// an empty map, which reads as everything unread, rather than bumping the
    /// schema version and discarding the rest of the operator's state.
    #[serde(default)]
    pane_read_records: BTreeMap<String, PaneReadRecord>,
    /// Each pane's last reported terminal size, as (rows, cols). Herdr sizes
    /// a pane's PTY from the attach, so a launch that already knows a pane's
    /// size attaches at it and takes one full frame instead of one at a guess
    /// and a second after the resize. Absent in a store written before that,
    /// which loads empty and makes the first launch after it wait one canvas
    /// build for the size, as a first launch always does.
    #[serde(default)]
    pane_terminal_sizes: BTreeMap<String, (u16, u16)>,
}

/// A store written before the pet existed carries no visibility, and the pet
/// shows itself by default (D-09).
fn default_pet_visible() -> bool {
    true
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LoadDisposition {
    Loaded,
    Missing,
    Corrupt,
}

/// Terminal sizes are kept beside the UI state rather than inside it: they
/// are PTY geometry the core attaches with, and the shell never draws them,
/// so they do not belong on the snapshot wire.
pub type PaneTerminalSizes = BTreeMap<String, (u16, u16)>;

pub fn load(path: &Path) -> (UiStateSnapshot, PaneTerminalSizes, LoadDisposition) {
    match fs::read(path) {
        Ok(bytes) => decode(&bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => (
            UiStateSnapshot::default(),
            PaneTerminalSizes::new(),
            LoadDisposition::Missing,
        ),
        Err(_) => (
            UiStateSnapshot::default(),
            PaneTerminalSizes::new(),
            LoadDisposition::Corrupt,
        ),
    }
}

fn decode(bytes: &[u8]) -> (UiStateSnapshot, PaneTerminalSizes, LoadDisposition) {
    let Ok(stored) = serde_json::from_slice::<StoredUiState>(bytes) else {
        return (
            UiStateSnapshot::default(),
            PaneTerminalSizes::new(),
            LoadDisposition::Corrupt,
        );
    };
    if stored.schema_version != UI_STATE_SCHEMA_VERSION {
        return (
            UiStateSnapshot::default(),
            PaneTerminalSizes::new(),
            LoadDisposition::Corrupt,
        );
    }
    (
        UiStateSnapshot {
            left_sidebar_visible: stored.left_sidebar_visible,
            right_panel_visible: stored.right_panel_visible,
            right_panel_section: stored.right_panel_section,
            expanded_paths: stored.expanded_paths,
            collapsed_workspace_ids: stored.collapsed_workspace_ids,
            selected_path: stored.selected_path,
            selected_pane_id: stored.selected_pane_id,
            shortcut_bindings: stored.shortcut_bindings,
            pet_visible: stored.pet_visible,
            pet_origin: stored.pet_origin,
            pet_shortcut: stored.pet_shortcut,
            focused_device_id: stored.focused_device_id,
            focused_checkout_id: stored.focused_checkout_id,
            workspace_registrations: stored.workspace_registrations,
            device_registrations: stored.device_registrations,
            accent_hex: stored.accent_hex,
            font_size: stored.font_size,
            pane_text_scales: stored.pane_text_scales,
            editor_text_scale: stored.editor_text_scale,
            pane_read_records: stored.pane_read_records,
        },
        stored.pane_terminal_sizes,
        LoadDisposition::Loaded,
    )
}

pub fn save(
    path: &Path,
    state: &UiStateSnapshot,
    pane_terminal_sizes: &PaneTerminalSizes,
) -> Result<(), String> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .ok_or_else(|| "UI state path has no parent directory".to_owned())?;
    fs::create_dir_all(parent)
        .map_err(|_| "UI state directory could not be prepared".to_owned())?;
    let stored = StoredUiState {
        schema_version: UI_STATE_SCHEMA_VERSION,
        left_sidebar_visible: state.left_sidebar_visible,
        right_panel_visible: state.right_panel_visible,
        right_panel_section: state.right_panel_section,
        expanded_paths: state.expanded_paths.clone(),
        collapsed_workspace_ids: state.collapsed_workspace_ids.clone(),
        selected_path: state.selected_path.clone(),
        selected_pane_id: state.selected_pane_id.clone(),
        shortcut_bindings: state.shortcut_bindings.clone(),
        pet_visible: state.pet_visible,
        pet_origin: state.pet_origin,
        pet_shortcut: state.pet_shortcut.clone(),
        focused_device_id: state.focused_device_id.clone(),
        focused_checkout_id: state.focused_checkout_id.clone(),
        workspace_registrations: state.workspace_registrations.clone(),
        device_registrations: state.device_registrations.clone(),
        accent_hex: state.accent_hex.clone(),
        font_size: state.font_size,
        pane_text_scales: state.pane_text_scales.clone(),
        editor_text_scale: state.editor_text_scale,
        pane_read_records: state.pane_read_records.clone(),
        pane_terminal_sizes: pane_terminal_sizes.clone(),
    };
    let bytes = serde_json::to_vec_pretty(&stored)
        .map_err(|_| "UI state could not be encoded".to_owned())?;
    let temporary = temporary_path(path);
    let mut output = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&temporary)
        .map_err(|_| "UI state temporary file could not be opened".to_owned())?;
    output
        .write_all(&bytes)
        .and_then(|_| output.sync_all())
        .map_err(|_| "UI state temporary file could not be written".to_owned())?;
    fs::rename(&temporary, path).map_err(|_| "UI state file could not be replaced".to_owned())
}

fn temporary_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".next");
    path.with_file_name(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// AC4, SC3. A read record written before a restart is the same after it,
    /// so an item the operator read stays read and one they did not stays in
    /// Needs You or Done.
    #[test]
    fn read_records_survive_a_restart() {
        let root = std::env::temp_dir().join(format!("herdr-core-read-{}", std::process::id()));
        let path = root.join("state.json");
        let _ = fs::remove_dir_all(&root);
        let mut state = UiStateSnapshot::default();
        state.pane_read_records.insert(
            "w1:p1".to_owned(),
            PaneReadRecord {
                state_change_seq: Some(7),
                demand: "question".to_owned(),
                activity: "stopped".to_owned(),
            },
        );
        save(&path, &state, &PaneTerminalSizes::new()).expect("state saves");

        let (reloaded, _sizes, disposition) = load(&path);
        assert_eq!(disposition, LoadDisposition::Loaded);
        assert_eq!(reloaded.pane_read_records, state.pane_read_records);
        let _ = fs::remove_dir_all(&root);
    }

    /// AC4. A store written before Hide owned the read axis still loads. Its
    /// missing record reads as everything unread, which is the honest answer,
    /// rather than discarding the operator's other settings.
    #[test]
    fn read_records_default_to_empty_on_an_older_store() {
        let source = br#"{"schema_version":1,"expanded_paths":[],"selected_path":null,"selected_pane_id":null}"#;
        let (state, _sizes, disposition) = decode(source);
        assert_eq!(disposition, LoadDisposition::Loaded);
        assert!(state.pane_read_records.is_empty());
    }

    /// AC4, SC3. A damaged store loads empty and says so through the
    /// disposition the runtime turns into a diagnostic. Nothing is silently
    /// treated as read.
    #[test]
    fn read_records_load_empty_from_a_corrupt_store() {
        let (state, _sizes, disposition) =
            decode(b"{\"schema_version\":1,\"pane_read_records\":\"not-a-map\"}");
        assert_eq!(disposition, LoadDisposition::Corrupt);
        assert!(
            state.pane_read_records.is_empty(),
            "a damaged record is emptied, never read"
        );
    }

    #[test]
    fn valid_state_round_trips_through_the_stable_schema() {
        let source = br#"{"schema_version":1,"expanded_paths":["/repo/src"],"selected_path":"/repo/src/lib.rs","selected_pane_id":"p1"}"#;
        let (state, _sizes, disposition) = decode(source);
        assert_eq!(disposition, LoadDisposition::Loaded);
        assert_eq!(state.expanded_paths, ["/repo/src"]);
        assert_eq!(state.selected_pane_id.as_deref(), Some("p1"));
        assert!(state.left_sidebar_visible);
        assert!(state.right_panel_visible);
        assert!(state.shortcut_bindings.is_empty());
        assert!(state.pet_visible, "a pre-pet store still shows the pet");
        assert_eq!(state.pet_origin, None);
        assert_eq!(state.pet_shortcut, None);
    }

    #[test]
    fn panel_visibility_survives_save_and_relaunch_load() {
        let root = std::env::temp_dir().join(format!("herdr-core-panels-{}", std::process::id()));
        let path = root.join("state.json");
        let state = UiStateSnapshot {
            left_sidebar_visible: false,
            right_panel_visible: false,
            ..UiStateSnapshot::default()
        };

        save(&path, &state, &PaneTerminalSizes::new()).expect("persist panel visibility");
        let (restored, _sizes, disposition) = load(&path);

        assert_eq!(disposition, LoadDisposition::Loaded);
        assert!(!restored.left_sidebar_visible);
        assert!(!restored.right_panel_visible);
        let _ = fs::remove_file(path);
        let _ = fs::remove_dir(root);
    }

    #[test]
    fn pet_position_visibility_and_shortcut_survive_a_relaunch() {
        let root = std::env::temp_dir().join(format!("herdr-core-pet-{}", std::process::id()));
        let path = root.join("state.json");
        let mut state = UiStateSnapshot {
            pet_visible: false,
            pet_origin: Some(PetOriginSnapshot { x: 120.0, y: 640.0 }),
            pet_shortcut: Some("command+option+p".to_owned()),
            ..UiStateSnapshot::default()
        };

        save(&path, &state, &PaneTerminalSizes::new()).expect("persist pet state");
        let (restored, _sizes, disposition) = load(&path);

        assert_eq!(disposition, LoadDisposition::Loaded);
        assert!(
            !restored.pet_visible,
            "hidden at exit means hidden at start"
        );
        assert_eq!(
            restored.pet_origin,
            Some(PetOriginSnapshot { x: 120.0, y: 640.0 })
        );
        assert_eq!(restored.pet_shortcut.as_deref(), Some("command+option+p"));

        // Saving the same state twice is a no-op the next load cannot tell apart.
        state.pet_visible = false;
        save(&path, &state, &PaneTerminalSizes::new()).expect("persist pet state again");
        assert_eq!(load(&path).0.pet_origin, restored.pet_origin);

        let _ = fs::remove_file(path);
        let _ = fs::remove_dir(root);
    }

    #[test]
    fn shortcut_bindings_survive_save_and_relaunch_load() {
        let root =
            std::env::temp_dir().join(format!("herdr-core-shortcuts-{}", std::process::id()));
        let path = root.join("state.json");
        let mut state = UiStateSnapshot::default();
        state
            .shortcut_bindings
            .insert("split_right".to_owned(), "command+option+r".to_owned());

        save(&path, &state, &PaneTerminalSizes::new()).expect("persist shortcut binding");
        let (restored, _sizes, disposition) = load(&path);

        assert_eq!(disposition, LoadDisposition::Loaded);
        assert_eq!(
            restored
                .shortcut_bindings
                .get("split_right")
                .map(String::as_str),
            Some("command+option+r")
        );
        let _ = fs::remove_file(path);
        let _ = fs::remove_dir(root);
    }

    #[test]
    fn workspace_collapse_survives_relaunch_without_changing_file_tree_expansion() {
        let root = std::env::temp_dir().join(format!(
            "herdr-core-workspace-collapse-{}",
            std::process::id()
        ));
        let path = root.join("state.json");
        let mut state = UiStateSnapshot {
            expanded_paths: vec!["/repo/src".to_owned()],
            collapsed_workspace_ids: vec!["workspace:alpha".to_owned()],
            ..UiStateSnapshot::default()
        };

        save(&path, &state, &PaneTerminalSizes::new())
            .expect("persist independent expansion state");
        let (restored, _sizes, disposition) = load(&path);

        assert_eq!(disposition, LoadDisposition::Loaded);
        assert_eq!(restored.expanded_paths, ["/repo/src"]);
        assert_eq!(restored.collapsed_workspace_ids, ["workspace:alpha"]);

        state.expanded_paths.push("/repo/tests".to_owned());
        save(&path, &state, &PaneTerminalSizes::new())
            .expect("persist file tree expansion independently");
        let restored_again = load(&path).0;
        assert_eq!(restored_again.expanded_paths, ["/repo/src", "/repo/tests"]);
        assert_eq!(restored_again.collapsed_workspace_ids, ["workspace:alpha"]);

        let _ = fs::remove_file(path);
        let _ = fs::remove_dir(root);
    }

    #[test]
    fn corrupt_and_unknown_schema_states_fall_back_without_blocking() {
        for source in [
            b"not json".as_slice(),
            br#"{"schema_version":99,"expanded_paths":[],"selected_path":null,"selected_pane_id":null}"#,
        ] {
            let (state, _sizes, disposition) = decode(source);
            assert_eq!(disposition, LoadDisposition::Corrupt);
            assert!(state.expanded_paths.is_empty());
        }
    }
}
