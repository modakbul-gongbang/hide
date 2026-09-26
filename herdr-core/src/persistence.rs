use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::model::{
    DeviceRegistration, PaneReadRecord, PetOriginSnapshot, RightPanelSection, SessionsMode,
    ThemePreference, UiStateSnapshot, WorkspaceRegistration, default_accent_hex, default_font_size,
    default_pane_text_scale, default_panel_visible,
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
    #[serde(default)]
    sessions_mode_by_project: BTreeMap<String, SessionsMode>,
    expanded_paths: Vec<String>,
    #[serde(default)]
    device_expanded_paths: BTreeMap<String, Vec<String>>,
    #[serde(default)]
    collapsed_workspace_ids: Vec<String>,
    #[serde(default)]
    collapsed_checkout_ids: Vec<String>,
    /// The web Projects list's opened checkouts; absent in an older store,
    /// which loads with every checkout closed.
    #[serde(default)]
    expanded_checkout_ids: Vec<String>,
    /// Project paths whose Inactive checkout row is open. The project path is
    /// stable across navigator rebuilds and matches the existing path-keyed
    /// expansion contract.
    #[serde(default)]
    expanded_inactive_checkout_project_paths: Vec<String>,
    /// Device ids whose Inactive projects row is open.
    #[serde(default)]
    expanded_inactive_project_device_ids: Vec<String>,
    #[serde(default)]
    project_base_branches: BTreeMap<String, String>,
    /// The folded-by-default agent tree's open rows. The former
    /// `collapsed_agent_pane_ids` key is ignored on load and dropped on the
    /// next write: the ids it held name panes of some earlier Herdr server,
    /// and a first launch after the upgrade starts every parent folded
    /// (PRD D-11).
    #[serde(default)]
    expanded_agent_pane_ids: Vec<String>,
    selected_path: Option<String>,
    selected_pane_id: Option<String>,
    #[serde(default)]
    shortcut_bindings: BTreeMap<String, String>,
    /// Absent in a store written before the Swift import existed, which loads
    /// as not imported and so imports once.
    #[serde(default)]
    shortcut_bindings_imported: bool,
    #[serde(default)]
    browser_shortcut_bindings: BTreeMap<String, String>,
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
    /// Kept as text so a value this build does not know loads as Dark with a
    /// diagnostic instead of making the whole store unreadable. Absent in a
    /// store written before the web theme existed, which loads as Dark.
    #[serde(default)]
    theme: Option<String>,
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
    /// Loaded, but the stored theme was not one this build knows; Dark was used.
    UnknownTheme,
    Missing,
    Corrupt,
}

/// What the Swift app's state file holds for `shortcut_bindings`.
#[derive(Debug, PartialEq)]
pub enum SwiftShortcuts {
    /// No file: the Swift app never saved state under this account.
    Missing,
    Found(BTreeMap<String, String>),
    /// The file is there but its bindings could not be read.
    Unreadable,
}

/// A Swift state file larger than this is not read: the import is one field
/// of a file the Swift shell owns, never a reason to load an unbounded one.
const SWIFT_STATE_READ_CAP: u64 = 8 * 1024 * 1024;

#[derive(Deserialize)]
struct SwiftShortcutFields {
    #[serde(default)]
    shortcut_bindings: BTreeMap<String, String>,
}

/// Reads only `shortcut_bindings` from the Swift app's state file; the rest of
/// that file is the Swift shell's, and the core takes nothing else from it.
pub fn read_swift_shortcuts(path: &Path) -> SwiftShortcuts {
    let bytes = match fs::metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return SwiftShortcuts::Missing;
        }
        Err(_) => return SwiftShortcuts::Unreadable,
        Ok(metadata) if metadata.len() > SWIFT_STATE_READ_CAP => return SwiftShortcuts::Unreadable,
        Ok(_) => match fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return SwiftShortcuts::Missing;
            }
            Err(_) => return SwiftShortcuts::Unreadable,
        },
    };
    match serde_json::from_slice::<SwiftShortcutFields>(&bytes) {
        Ok(fields) => SwiftShortcuts::Found(fields.shortcut_bindings),
        Err(_) => SwiftShortcuts::Unreadable,
    }
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
    let (theme, disposition) = match stored.theme.as_deref() {
        None => (ThemePreference::Dark, LoadDisposition::Loaded),
        Some(value) => match ThemePreference::parse(value) {
            Some(theme) => (theme, LoadDisposition::Loaded),
            None => (ThemePreference::Dark, LoadDisposition::UnknownTheme),
        },
    };
    (
        UiStateSnapshot {
            left_sidebar_visible: stored.left_sidebar_visible,
            right_panel_visible: stored.right_panel_visible,
            right_panel_section: stored.right_panel_section,
            sessions_mode_by_project: stored.sessions_mode_by_project,
            expanded_paths: stored.expanded_paths,
            device_expanded_paths: stored.device_expanded_paths,
            collapsed_workspace_ids: stored.collapsed_workspace_ids,
            collapsed_checkout_ids: stored.collapsed_checkout_ids,
            expanded_checkout_ids: stored.expanded_checkout_ids,
            expanded_inactive_checkout_project_paths: stored
                .expanded_inactive_checkout_project_paths,
            expanded_inactive_project_device_ids: stored.expanded_inactive_project_device_ids,
            project_base_branches: stored.project_base_branches,
            expanded_agent_pane_ids: stored.expanded_agent_pane_ids,
            selected_path: stored.selected_path,
            selected_pane_id: stored.selected_pane_id,
            shortcut_bindings: stored.shortcut_bindings,
            shortcut_bindings_imported: stored.shortcut_bindings_imported,
            browser_shortcut_bindings: stored.browser_shortcut_bindings,
            pet_visible: stored.pet_visible,
            pet_origin: stored.pet_origin,
            pet_shortcut: stored.pet_shortcut,
            focused_device_id: stored.focused_device_id,
            focused_checkout_id: stored.focused_checkout_id,
            workspace_registrations: stored.workspace_registrations,
            device_registrations: stored.device_registrations,
            accent_hex: stored.accent_hex,
            theme,
            font_size: stored.font_size,
            pane_text_scales: stored.pane_text_scales,
            editor_text_scale: stored.editor_text_scale,
            conversation_pane_ids: Default::default(),
            pane_read_records: stored.pane_read_records,
        },
        stored.pane_terminal_sizes,
        disposition,
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
        sessions_mode_by_project: state.sessions_mode_by_project.clone(),
        expanded_paths: state.expanded_paths.clone(),
        device_expanded_paths: state.device_expanded_paths.clone(),
        collapsed_workspace_ids: state.collapsed_workspace_ids.clone(),
        collapsed_checkout_ids: state.collapsed_checkout_ids.clone(),
        expanded_checkout_ids: state.expanded_checkout_ids.clone(),
        expanded_inactive_checkout_project_paths: state
            .expanded_inactive_checkout_project_paths
            .clone(),
        expanded_inactive_project_device_ids: state.expanded_inactive_project_device_ids.clone(),
        project_base_branches: state.project_base_branches.clone(),
        expanded_agent_pane_ids: state.expanded_agent_pane_ids.clone(),
        selected_path: state.selected_path.clone(),
        selected_pane_id: state.selected_pane_id.clone(),
        shortcut_bindings: state.shortcut_bindings.clone(),
        shortcut_bindings_imported: state.shortcut_bindings_imported,
        browser_shortcut_bindings: state.browser_shortcut_bindings.clone(),
        pet_visible: state.pet_visible,
        pet_origin: state.pet_origin,
        pet_shortcut: state.pet_shortcut.clone(),
        focused_device_id: state.focused_device_id.clone(),
        focused_checkout_id: state.focused_checkout_id.clone(),
        workspace_registrations: state.workspace_registrations.clone(),
        device_registrations: state.device_registrations.clone(),
        accent_hex: state.accent_hex.clone(),
        theme: Some(state.theme.as_str().to_owned()),
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

    #[test]
    fn retired_git_panel_restores_overview_without_losing_other_ui_state() {
        let mut persisted = serde_json::to_value(UiStateSnapshot::default()).unwrap();
        persisted["right_panel_section"] = serde_json::json!("git");
        persisted["expanded_agent_pane_ids"] = serde_json::json!(["parent"]);
        let state: UiStateSnapshot = serde_json::from_value(persisted).unwrap();
        assert_eq!(state.right_panel_section, RightPanelSection::Overview);
        assert_eq!(state.expanded_agent_pane_ids, ["parent"]);
        assert_eq!(
            serde_json::to_value(&state).unwrap()["right_panel_section"],
            "overview"
        );
        assert_eq!(
            RightPanelSection::parse("git"),
            Some(RightPanelSection::Overview)
        );
    }

    // PRD D-11: the folded set is not migrated. An old store's collapsed
    // ids name panes of a server that has since been restarted, so they are
    // dropped and every parent starts folded.
    #[test]
    fn a_stored_collapsed_set_is_ignored_and_every_parent_starts_folded() {
        let root =
            std::env::temp_dir().join(format!("herdr-core-collapsed-{}", std::process::id()));
        let path = root.join("state.json");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        save(
            &path,
            &UiStateSnapshot::default(),
            &PaneTerminalSizes::new(),
        )
        .unwrap();
        let mut persisted: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        persisted["collapsed_agent_pane_ids"] = serde_json::json!(["w1:p1", "w1:p2"]);
        fs::write(&path, serde_json::to_vec(&persisted).unwrap()).unwrap();
        let (state, _, disposition) = load(&path);
        assert_eq!(disposition, LoadDisposition::Loaded);
        assert!(state.expanded_agent_pane_ids.is_empty());
        save(&path, &state, &PaneTerminalSizes::new()).unwrap();
        let rewritten: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert!(rewritten.get("collapsed_agent_pane_ids").is_none());
        assert_eq!(rewritten["expanded_agent_pane_ids"], serde_json::json!([]));
        let _ = fs::remove_dir_all(&root);
    }

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
                session_id: Some("session-7".to_owned()),
                demand: "question".to_owned(),
                activity: "stopped".to_owned(),
                completed: false,
                descendant_signals: [crate::model::DescendantSignal {
                    pane_id: "w1:p2".to_owned(),
                    kind: "question".to_owned(),
                }]
                .into_iter()
                .collect(),
            },
        );
        save(&path, &state, &PaneTerminalSizes::new()).expect("state saves");

        let (reloaded, _sizes, disposition) = load(&path);
        assert_eq!(disposition, LoadDisposition::Loaded);
        assert_eq!(reloaded.pane_read_records, state.pane_read_records);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn read_records_written_before_session_identity_still_load() {
        let mut persisted = serde_json::to_value(UiStateSnapshot::default()).unwrap();
        persisted["pane_read_records"] = serde_json::json!({
            "w1:p1": {
                "state_change_seq": 41,
                "demand": "none",
                "activity": "stopped"
            }
        });

        let state: UiStateSnapshot = serde_json::from_value(persisted).unwrap();
        assert_eq!(state.pane_read_records["w1:p1"].session_id, None);
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

    #[test]
    fn sessions_mode_defaults_for_an_older_store_and_survives_a_restart() {
        let older = br#"{"schema_version":1,"expanded_paths":[],"selected_path":null,"selected_pane_id":null}"#;
        let (older_state, _sizes, disposition) = decode(older);
        assert_eq!(disposition, LoadDisposition::Loaded);
        assert!(older_state.sessions_mode_by_project.is_empty());

        let root =
            std::env::temp_dir().join(format!("herdr-core-sessions-mode-{}", std::process::id()));
        let path = root.join("state.json");
        let _ = fs::remove_dir_all(&root);
        let mut state = older_state;
        state
            .sessions_mode_by_project
            .insert("project-1".to_owned(), SessionsMode::Memory);
        save(&path, &state, &PaneTerminalSizes::new()).expect("state saves");

        let (reloaded, _sizes, disposition) = load(&path);
        assert_eq!(disposition, LoadDisposition::Loaded);
        assert_eq!(
            reloaded.sessions_mode_by_project.get("project-1"),
            Some(&SessionsMode::Memory)
        );
        let _ = fs::remove_dir_all(root);
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
    fn swift_shortcuts_read_only_the_bindings_field() {
        let root =
            std::env::temp_dir().join(format!("herdr-core-swift-shortcuts-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("state.json");
        assert_eq!(read_swift_shortcuts(&path), SwiftShortcuts::Missing);

        // A Swift store carries fields the core's own schema would refuse.
        fs::write(
            &path,
            br#"{"schema_version":7,"pet_visible":"yes","shortcut_bindings":{"toggle_zoom":"command+shift+return"}}"#,
        )
        .unwrap();
        assert_eq!(
            read_swift_shortcuts(&path),
            SwiftShortcuts::Found(BTreeMap::from([(
                "toggle_zoom".to_owned(),
                "command+shift+return".to_owned()
            )]))
        );

        fs::write(&path, br#"{"shortcut_bindings":["not","a","map"]}"#).unwrap();
        assert_eq!(read_swift_shortcuts(&path), SwiftShortcuts::Unreadable);
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
        state
            .browser_shortcut_bindings
            .insert("split_right".to_owned(), "alt+KeyR".to_owned());
        state.shortcut_bindings_imported = true;

        save(&path, &state, &PaneTerminalSizes::new()).expect("persist shortcut binding");
        let (restored, _sizes, disposition) = load(&path);

        assert_eq!(disposition, LoadDisposition::Loaded);
        assert!(restored.shortcut_bindings_imported);
        assert_eq!(
            restored
                .shortcut_bindings
                .get("split_right")
                .map(String::as_str),
            Some("command+option+r")
        );
        assert_eq!(
            restored
                .browser_shortcut_bindings
                .get("split_right")
                .map(String::as_str),
            Some("alt+KeyR")
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
            collapsed_checkout_ids: vec!["checkout:main".to_owned()],
            expanded_checkout_ids: vec!["checkout:feature".to_owned()],
            ..UiStateSnapshot::default()
        };

        save(&path, &state, &PaneTerminalSizes::new())
            .expect("persist independent expansion state");
        let (restored, _sizes, disposition) = load(&path);

        assert_eq!(disposition, LoadDisposition::Loaded);
        assert_eq!(restored.expanded_paths, ["/repo/src"]);
        assert_eq!(restored.collapsed_workspace_ids, ["workspace:alpha"]);
        assert_eq!(restored.collapsed_checkout_ids, ["checkout:main"]);
        assert_eq!(restored.expanded_checkout_ids, ["checkout:feature"]);

        state.expanded_paths.push("/repo/tests".to_owned());
        save(&path, &state, &PaneTerminalSizes::new())
            .expect("persist file tree expansion independently");
        let restored_again = load(&path).0;
        assert_eq!(restored_again.expanded_paths, ["/repo/src", "/repo/tests"]);
        assert_eq!(restored_again.collapsed_workspace_ids, ["workspace:alpha"]);
        assert_eq!(restored_again.collapsed_checkout_ids, ["checkout:main"]);

        let _ = fs::remove_file(path);
        let _ = fs::remove_dir(root);
    }

    /// B5, B12. Fold disclosure is independent at the project and device
    /// levels, survives a relaunch, and an older store defaults both closed.
    #[test]
    fn inactive_fold_expansion_survives_relaunch_and_defaults_closed() {
        let root =
            std::env::temp_dir().join(format!("herdr-core-inactive-folds-{}", std::process::id()));
        let path = root.join("state.json");
        let state = UiStateSnapshot {
            expanded_inactive_checkout_project_paths: vec!["/repo/alpha".to_owned()],
            expanded_inactive_project_device_ids: vec!["local".to_owned()],
            ..UiStateSnapshot::default()
        };

        save(&path, &state, &PaneTerminalSizes::new()).expect("persist inactive fold expansion");
        let (restored, _sizes, disposition) = load(&path);

        assert_eq!(disposition, LoadDisposition::Loaded);
        assert_eq!(
            restored.expanded_inactive_checkout_project_paths,
            ["/repo/alpha"]
        );
        assert_eq!(restored.expanded_inactive_project_device_ids, ["local"]);

        let legacy = br#"{"schema_version":1,"expanded_paths":[],"selected_path":null,"selected_pane_id":null}"#;
        let (legacy_state, _sizes, legacy_disposition) = decode(legacy);
        assert_eq!(legacy_disposition, LoadDisposition::Loaded);
        assert!(
            legacy_state
                .expanded_inactive_checkout_project_paths
                .is_empty()
        );
        assert!(legacy_state.expanded_inactive_project_device_ids.is_empty());

        let _ = fs::remove_file(path);
        let _ = fs::remove_dir(root);
    }

    /// B5. A pinned registration comes back pinned, and a store written before
    /// registrations carried the flag loads every project unpinned with no
    /// warning.
    #[test]
    fn registration_pin_survives_relaunch_and_an_older_store_loads_unpinned() {
        let root = std::env::temp_dir().join(format!(
            "herdr-core-registration-pin-{}",
            std::process::id()
        ));
        let path = root.join("state.json");
        let state = UiStateSnapshot {
            workspace_registrations: vec![WorkspaceRegistration {
                id: "workspace:alpha".to_owned(),
                label: "Alpha".to_owned(),
                path: "/repo/alpha".to_owned(),
                device_id: "local".to_owned(),
                pinned: true,
            }],
            ..UiStateSnapshot::default()
        };

        save(&path, &state, &PaneTerminalSizes::new()).expect("persist the pin");
        let (restored, _sizes, disposition) = load(&path);

        assert_eq!(disposition, LoadDisposition::Loaded);
        assert_eq!(
            restored.workspace_registrations,
            state.workspace_registrations
        );

        let older = br#"{"schema_version":1,"expanded_paths":[],"selected_path":null,"selected_pane_id":null,
            "workspace_registrations":[{"id":"workspace:alpha","label":"Alpha","path":"/repo/alpha"}]}"#;
        let (older_state, _sizes, older_disposition) = decode(older);
        assert_eq!(older_disposition, LoadDisposition::Loaded);
        assert_eq!(older_state.workspace_registrations.len(), 1);
        assert!(!older_state.workspace_registrations[0].pinned);

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
