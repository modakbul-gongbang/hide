use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::model::{
    DeviceRegistration, PetOriginSnapshot, UiStateSnapshot, WorkspaceRegistration,
    default_accent_hex, default_font_size, default_panel_visible,
};

const UI_STATE_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, Deserialize, Serialize)]
struct StoredUiState {
    schema_version: u32,
    #[serde(default = "default_panel_visible")]
    left_sidebar_visible: bool,
    #[serde(default = "default_panel_visible")]
    right_panel_visible: bool,
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

pub fn load(path: &Path) -> (UiStateSnapshot, LoadDisposition) {
    match fs::read(path) {
        Ok(bytes) => decode(&bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            (UiStateSnapshot::default(), LoadDisposition::Missing)
        }
        Err(_) => (UiStateSnapshot::default(), LoadDisposition::Corrupt),
    }
}

fn decode(bytes: &[u8]) -> (UiStateSnapshot, LoadDisposition) {
    let Ok(stored) = serde_json::from_slice::<StoredUiState>(bytes) else {
        return (UiStateSnapshot::default(), LoadDisposition::Corrupt);
    };
    if stored.schema_version != UI_STATE_SCHEMA_VERSION {
        return (UiStateSnapshot::default(), LoadDisposition::Corrupt);
    }
    (
        UiStateSnapshot {
            left_sidebar_visible: stored.left_sidebar_visible,
            right_panel_visible: stored.right_panel_visible,
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
        },
        LoadDisposition::Loaded,
    )
}

pub fn save(path: &Path, state: &UiStateSnapshot) -> Result<(), String> {
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

    #[test]
    fn valid_state_round_trips_through_the_stable_schema() {
        let source = br#"{"schema_version":1,"expanded_paths":["/repo/src"],"selected_path":"/repo/src/lib.rs","selected_pane_id":"p1"}"#;
        let (state, disposition) = decode(source);
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

        save(&path, &state).expect("persist panel visibility");
        let (restored, disposition) = load(&path);

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

        save(&path, &state).expect("persist pet state");
        let (restored, disposition) = load(&path);

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
        save(&path, &state).expect("persist pet state again");
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

        save(&path, &state).expect("persist shortcut binding");
        let (restored, disposition) = load(&path);

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

        save(&path, &state).expect("persist independent expansion state");
        let (restored, disposition) = load(&path);

        assert_eq!(disposition, LoadDisposition::Loaded);
        assert_eq!(restored.expanded_paths, ["/repo/src"]);
        assert_eq!(restored.collapsed_workspace_ids, ["workspace:alpha"]);

        state.expanded_paths.push("/repo/tests".to_owned());
        save(&path, &state).expect("persist file tree expansion independently");
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
            let (state, disposition) = decode(source);
            assert_eq!(disposition, LoadDisposition::Corrupt);
            assert!(state.expanded_paths.is_empty());
        }
    }
}
