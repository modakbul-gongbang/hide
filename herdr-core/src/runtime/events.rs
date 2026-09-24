use serde::Deserialize;
use serde_json::Value;

use super::*;

#[derive(Debug, Deserialize)]
pub(super) struct EventEnvelope {
    pub(super) schema_version: u32,
    pub(super) kind: String,
    pub(super) payload: Value,
}

#[derive(Debug, Deserialize)]
pub(super) struct KeyPayload {
    pub(super) input_trace: Option<crate::model::TerminalInputTrace>,
    pub(super) pane_id: String,
    pub(super) bytes_base64: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct AttachmentPayload {
    pub request_id: String,
    pub pane_id: String,
    pub bracketed_paste: bool,
    #[serde(default)]
    pub clipboard: bool,
    #[serde(default)]
    pub paths: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct AttachmentCompletionPayload {
    pub request_id: String,
    pub pane_id: String,
    pub error: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct AttachmentActionPayload {
    pub request_id: String,
    pub pane_id: String,
    pub action: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct TerminalOutputPayload {
    pub(super) pane_id: String,
    pub(super) bytes_base64: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct ClickPayload {
    pub(super) surface: Surface,
    pub(super) x: f64,
    pub(super) y: f64,
    pub(super) button: MouseButton,
    pub(super) click_count: u8,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum MouseButton {
    Left,
    Right,
}

#[derive(Debug, Deserialize)]
pub(super) struct FocusPanePayload {
    pub(super) pane_id: String,
}

/// Why the shell asked for a pane focus.
///
/// The read axis needs the two apart. An operator focus is the act the whole
/// read record rests on; a launch restore reinstates the selection the last
/// session ended on, which says nothing about whether the operator has looked
/// at what changed while the app was closed.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub(super) enum PaneFocusOrigin {
    Operator,
    Restore,
}

#[derive(Debug, Deserialize)]
pub(super) struct FocusPaneRequestPayload {
    pub(super) pane_id: String,
    pub(super) origin: PaneFocusOrigin,
    #[serde(default)]
    pub(super) request_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct OpenBrowserPayload {
    pub(super) profile: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct BrowserStatusPayload {
    pub(super) state: String,
    pub(super) profile: String,
    pub(super) current_url: Option<String>,
    pub(super) current_title: Option<String>,
    pub(super) message: Option<String>,
    pub(super) last_checked_at_unix_ms: u64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct GithubRequestPayload {
    pub(super) workspace_id: String,
    #[serde(default)]
    pub(super) refresh: bool,
}

#[derive(Debug, Deserialize)]
pub(super) struct CreateWorkspacePayload {
    /// The device that holds the folder; this machine when absent. A
    /// device's own helper judges the folder (`Call::Registrable`).
    #[serde(default)]
    pub(super) device_id: Option<String>,
    pub(super) path: String,
    pub(super) label: String,
    pub(super) initialize_git: bool,
}

#[derive(Debug, Deserialize)]
pub(super) struct CreateTabPayload {
    pub(super) workspace_id: String,
    #[serde(default)]
    pub(super) checkout_id: Option<String>,
    pub(super) label: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct FocusCheckoutPayload {
    pub(super) workspace_id: String,
    pub(super) checkout_id: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct FocusTabPayload {
    pub(super) workspace_id: String,
    pub(super) checkout_id: String,
    pub(super) tab_id: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct ReorderTabPayload {
    pub(super) workspace_id: String,
    pub(super) checkout_id: String,
    /// The strip entry being moved, by its strip id, not the Herdr tab id:
    /// the strip is what the operator dragged in and it holds both kinds.
    pub(super) tab_id: String,
    /// Where the entry ends up, as its index in the resulting strip.
    pub(super) to_index: usize,
}

#[derive(Debug, Deserialize)]
pub(super) struct FocusDevicePayload {
    pub(super) device_id: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct InactiveCheckoutsTogglePayload {
    pub(super) project_path: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct InactiveProjectsTogglePayload {
    pub(super) device_id: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct RemoveWorkspacePayload {
    pub(super) workspace_id: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct WorkspacePinSetPayload {
    pub(super) workspace_id: String,
    pub(super) pinned: bool,
}

#[derive(Debug, Deserialize)]
pub(super) struct RegisterDevicePayload {
    pub(super) id: String,
    pub(super) label: String,
    pub(super) ssh_alias: String,
    /// The device's Herdr socket when its server is not at the default path.
    #[serde(default)]
    pub(super) herdr_socket_path: Option<String>,
    /// The operator allowed Hide's helper on the device in the same form
    /// (PRD S5.5 B50); absent or false registers it without file access.
    #[serde(default)]
    pub(super) host_consent: bool,
}

#[derive(Debug, Deserialize)]
pub(super) struct DeviceHostConsentPayload {
    pub(super) device_id: String,
    pub(super) allow: bool,
}

#[derive(Debug, Deserialize)]
pub(super) struct DeviceHostRetryPayload {
    pub(super) device_id: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct RemoveDevicePayload {
    pub(super) device_id: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct TestDevicePayload {
    pub(super) device_id: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct CreatePanePayload {
    pub(super) tab_id: String,
    pub(super) cwd: String,
    pub(super) command: Option<String>,
    pub(super) direction: PaneSplitDirection,
}

#[derive(Debug, Deserialize)]
pub(super) struct ResizePanePayload {
    pub(super) pane_id: String,
    pub(super) direction: PaneResizeDirection,
    pub(super) amount: f32,
}

#[derive(Debug, Deserialize)]
pub(super) struct ToggleZoomPayload {
    pub(super) pane_id: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct ConfirmedWorkspacePayload {
    pub(super) workspace_id: String,
    pub(super) confirmed: bool,
}

#[derive(Debug, Deserialize)]
pub(super) struct ConfirmedTabPayload {
    pub(super) tab_id: String,
    pub(super) confirmed: bool,
}

#[derive(Debug, Deserialize)]
pub(super) struct ConfirmedPanePayload {
    pub(super) pane_id: String,
    pub(super) confirmed: bool,
}

#[derive(Debug, Deserialize)]
pub(super) struct CheckCloseStatusPayload {
    pub(super) key: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct PaneTargetPayload {
    pub(super) pane_id: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct RemoteControlPayload {
    pub(super) target_id: String,
    pub(super) request_id: String,
    /// The request id always belongs to the remote transport and its dedupe
    /// receipt. Only relationship Open/Return additionally projects that
    /// receipt into B24's visible pane-focus outcome.
    #[serde(default)]
    pub(super) report_pane_focus_outcome: bool,
    #[serde(flatten)]
    pub(super) request: RemoteControlRequest,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub(super) enum RemoteControlRequest {
    FocusPane {
        pane_id: String,
    },
    SplitPane {
        pane_id: String,
        direction: PaneSplitDirection,
        #[serde(default)]
        cwd: Option<String>,
    },
    TogglePaneZoom {
        pane_id: String,
    },
    ClosePane {
        pane_id: String,
        confirmed: bool,
    },
    /// `checkout_id` names the Herdr workspace on the host: a device's
    /// project can hold several (`device_catalog`). Without it,
    /// `workspace_id` must itself be a Herdr workspace's row.
    FocusWorkspace {
        workspace_id: String,
        #[serde(default)]
        checkout_id: Option<String>,
    },
    FocusTab {
        tab_id: String,
    },
    CreateTab {
        workspace_id: String,
        #[serde(default)]
        checkout_id: Option<String>,
        cwd: String,
        label: String,
    },
    CloseTab {
        tab_id: String,
        confirmed: bool,
    },
}

impl RemoteControlRequest {
    pub(super) fn pane_id(&self) -> Option<&str> {
        match self {
            Self::FocusPane { pane_id }
            | Self::SplitPane { pane_id, .. }
            | Self::TogglePaneZoom { pane_id }
            | Self::ClosePane { pane_id, .. } => Some(pane_id),
            Self::FocusWorkspace { .. }
            | Self::FocusTab { .. }
            | Self::CreateTab { .. }
            | Self::CloseTab { .. } => None,
        }
    }

    pub(super) fn confirmed(&self) -> bool {
        matches!(
            self,
            Self::ClosePane {
                confirmed: true,
                ..
            } | Self::CloseTab {
                confirmed: true,
                ..
            }
        )
    }

    pub(super) fn mutation_kind(&self) -> Option<&'static str> {
        match self {
            Self::SplitPane { .. } => Some("pane.split"),
            Self::TogglePaneZoom { .. } => Some("pane.zoom"),
            Self::ClosePane { .. } => Some("pane.close"),
            Self::CloseTab { .. } => Some("tab.close"),
            Self::FocusPane { .. }
            | Self::FocusWorkspace { .. }
            | Self::FocusTab { .. }
            | Self::CreateTab { .. } => None,
        }
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct FileOpenPayload {
    pub(super) path: String,
    pub(super) workspace_id: String,
    pub(super) checkout_id: String,
    /// Whether the open wants the checkout's preview slot: an Explorer single
    /// click does, a double-click, Cmd+P and every other entry point do not.
    #[serde(default)]
    pub(super) preview: bool,
}

#[derive(Debug, Deserialize)]
pub(super) struct FileTabPayload {
    pub(super) tab_id: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct FileClosePayload {
    pub(super) tab_id: String,
    #[serde(default)]
    pub(super) pending_save: Option<FileSavePayload>,
}

/// One clicked path, already resolved on the filesystem by the shell.
///
/// The shell owns filesystem resolution because reading directory entries
/// under the runtime mutex is exactly what the performance guide forbids. The
/// core owns every screen effect the click has, and it owns them together:
/// the checkout switch, the panel, the tree and the editor tab land in one
/// event or the dispatch's fire-and-forget ordering would let the operator
/// see a half-applied reveal.
#[derive(Debug, Deserialize)]
pub(super) struct RevealPathPayload {
    pub(super) path: String,
    pub(super) workspace_id: String,
    pub(super) checkout_id: String,
    pub(super) is_directory: bool,
}

#[derive(Debug, Deserialize)]
pub(super) struct RemoteFileListPayload {
    pub(super) target_id: String,
    pub(super) root_path: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct FileSavePayload {
    pub(super) tab_id: String,
    pub(super) path: String,
    pub(super) contents_utf8: String,
}

/// A new file or folder: the folder it goes in and the name it takes. The
/// root is the tree the shell drew it in, which the core checks against the
/// focused checkout before it trusts the parent to be inside it.
#[derive(Debug, Deserialize)]
pub(super) struct ExplorerCreatePayload {
    /// The device whose tree asked; absent for this machine.
    #[serde(default)]
    pub(super) device_id: Option<String>,
    pub(super) root: String,
    pub(super) parent: String,
    pub(super) name: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct PathRenamePayload {
    /// The device whose tree asked; absent for this machine.
    #[serde(default)]
    pub(super) device_id: Option<String>,
    pub(super) root: String,
    pub(super) path: String,
    pub(super) name: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct PathMovePayload {
    /// The device whose tree asked; absent for this machine.
    #[serde(default)]
    pub(super) device_id: Option<String>,
    pub(super) root: String,
    pub(super) path: String,
    /// The folder the item lands in; the item keeps its name.
    pub(super) destination: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct PathTrashPayload {
    /// The device whose tree asked; absent for this machine.
    #[serde(default)]
    pub(super) device_id: Option<String>,
    pub(super) root: String,
    pub(super) path: String,
    /// The row the tree selects once the item is gone: its next sibling,
    /// else its previous sibling, else its parent. The tree decides it
    /// because only the tree knows its own row order; the core still
    /// refuses one outside the checkout or inside the item.
    pub(super) select_after: String,
    /// The inode the tree read when it built the prompt, so the core moves
    /// the item the modal named and refuses one that replaced it while the
    /// modal was open. Absent when the tree could not read one.
    #[serde(default)]
    pub(super) inode: Option<u64>,
}

#[derive(Debug, Deserialize)]
pub(super) struct FileViewPayload {
    pub(super) tab_id: String,
    pub(super) markdown_live: bool,
    pub(super) wrap: bool,
}

#[derive(Debug, Deserialize)]
pub(super) struct FileDraftPayload {
    pub(super) contents_utf8: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct FileConflictPayload {
    pub(super) action: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct UiStateUpdatePayload {
    #[serde(default)]
    pub(super) left_sidebar_visible: Option<bool>,
    #[serde(default)]
    pub(super) right_panel_visible: Option<bool>,
    #[serde(default)]
    pub(super) right_panel_section: Option<String>,
    pub(super) expanded_paths: Vec<String>,
    /// Absent keeps the devices' expansion: the Swift shell does not carry it.
    #[serde(default)]
    pub(super) device_expanded_paths: Option<BTreeMap<String, Vec<String>>>,
    #[serde(default)]
    pub(super) collapsed_workspace_ids: Vec<String>,
    #[serde(default)]
    pub(super) collapsed_checkout_ids: Option<Vec<String>>,
    #[serde(default)]
    pub(super) expanded_agent_pane_ids: Option<Vec<String>>,
    pub(super) selected_path: Option<String>,
    pub(super) selected_pane_id: Option<String>,
    #[serde(default)]
    pub(super) shortcut_bindings: std::collections::BTreeMap<String, String>,
    /// Absent keeps the web shell's chords; only that shell sends them.
    #[serde(default)]
    pub(super) browser_shortcut_bindings: Option<std::collections::BTreeMap<String, String>>,
    #[serde(default)]
    pub(super) focused_device_id: Option<Option<String>>,
    #[serde(default)]
    pub(super) focused_checkout_id: Option<Option<String>>,
    #[serde(default)]
    pub(super) workspace_registrations: Option<Vec<crate::model::WorkspaceRegistration>>,
    #[serde(default)]
    pub(super) device_registrations: Option<Vec<crate::model::DeviceRegistration>>,
    #[serde(default)]
    pub(super) accent_hex: Option<String>,
    #[serde(default)]
    pub(super) font_size: Option<f32>,
    /// Ephemeral observation hints for the core-owned provider usage timer.
    /// They ride the existing UI-state event but are never persisted.
    #[serde(default)]
    pub(super) usage_window_visible: Option<bool>,
    #[serde(default)]
    pub(super) usage_popover_open: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub(super) struct SessionsModePayload {
    pub(super) mode: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct SessionsFilterPayload {
    pub(super) provider: String,
    #[serde(default)]
    pub(super) query: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct ArchiveOpenPayload {
    pub(super) kind: String,
    pub(super) id: String,
    #[serde(default = "default_true")]
    pub(super) preview: bool,
}

#[derive(Debug, Deserialize)]
pub(super) struct MemoryOpenForTurnPayload {
    pub(super) item_ids: Vec<String>,
}

fn default_true() -> bool {
    true
}

#[derive(Clone, Debug, Deserialize)]
pub(super) struct MemoryActionPayload {
    pub(super) action: String,
    #[serde(default)]
    pub(super) item_id: Option<String>,
    #[serde(default)]
    pub(super) candidate_id: Option<String>,
    #[serde(default)]
    pub(super) body: Option<String>,
    #[serde(default)]
    pub(super) batch_id: Option<String>,
    #[serde(default)]
    pub(super) conflict_choice: Option<String>,
}

/// Which changed file the changes view is showing the diff for. `None`
/// deselects, which is what closing the diff means.
#[derive(Debug, Deserialize)]
pub(super) struct ChangesSelectPayload {
    /// Whether the row is in the committed group, which decides what its diff
    /// is taken against.
    #[serde(default)]
    pub(super) committed: bool,
    #[serde(default)]
    pub(super) path: Option<String>,
    /// Whether the diff opens in the checkout's preview slot, which a single
    /// click on a Changes row asks for.
    #[serde(default)]
    pub(super) preview: bool,
}

#[derive(Debug, Deserialize)]
pub(super) struct CleanupConfirmPayload {
    pub(super) id: u64,
    pub(super) paths: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct GitWorktreeOpenPayload {
    pub(super) checkout_path: String,
}

/// `overview_open_section`: one checkout brought forward and the right
/// panel moved to `section` in the same event, so the screen never shows
/// the half of the move a refusal would leave (AGENTS.md, "a user action is
/// one event").
#[derive(Debug, Deserialize)]
pub(super) struct OverviewOpenSectionPayload {
    pub(super) checkout_path: String,
    pub(super) section: String,
}

/// `agent_start_in_checkout`: a new tab in the checkout's Herdr workspace
/// with the checkout as its cwd, and the provider the shell then starts in
/// it. `terminal` means the tab alone.
#[derive(Debug, Deserialize)]
pub(super) struct AgentStartInCheckoutPayload {
    pub(super) checkout_path: String,
    pub(super) provider: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct GitWorktreeSetBasePayload {
    pub(super) repository_root: String,
    pub(super) branch: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct CreateWorktreePayload {
    /// The device that holds the repository; this machine when absent.
    #[serde(default)]
    pub(super) device_id: Option<String>,
    pub(super) repository_root: String,
    pub(super) branch: String,
    #[serde(default)]
    pub(super) base_branch: Option<String>,
    #[serde(default)]
    pub(super) agent_kind: Option<String>,
    #[serde(default)]
    pub(super) purpose: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct SetCheckoutPurposePayload {
    pub(super) checkout_id: String,
    pub(super) text: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct MigrateMainBranchPayload {
    pub(super) repository_root: String,
    pub(super) base_branch: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct TaskOperationAckPayload {
    pub(super) id: u64,
}

#[derive(Debug, Deserialize)]
pub(super) struct TaskAgentRetryPayload {
    pub(super) id: u64,
}

#[derive(Debug, Deserialize)]
pub(super) struct RemoveWorktreePayload {
    /// The device that holds the worktree; this machine when absent.
    #[serde(default)]
    pub(super) device_id: Option<String>,
    pub(super) checkout_path: String,
    #[serde(default)]
    pub(super) delete_branch: bool,
}

#[derive(Debug, Deserialize)]
pub(super) struct RetryConnectPayload {
    pub(super) target_id: String,
}

/// One runtime the operator asked Hide to install its hook into.
///
/// It exists because Hide installs once on first run and then leaves the
/// operator's configuration alone; every later install is this event, sent
/// from the Settings diagnosis after they said yes (PRD B28, D-31).
#[derive(Debug, Deserialize)]
pub(super) struct InstallAgentHooksPayload {
    pub(super) runtime_id: String,
}

/// One Background AI settings event, carrying whatever it is about.
///
/// It folds three things a single screen does into one event, the way
/// `ui_state_update` already folds that screen's other state: the group
/// appearing or going away, a chosen agent, and a chosen model. Nothing here
/// is filled in on the core's side, so an event that names a model without
/// its provider is refused rather than guessed at.
#[derive(Deserialize)]
pub(super) struct AiSettingsPayload {
    /// True while the Background AI group is on screen. The provider probe
    /// starts child processes, so it runs only while somebody is looking.
    #[serde(default)]
    pub(super) observing: Option<bool>,
    /// The provider the operator chose.
    #[serde(default)]
    pub(super) provider: Option<String>,
    /// The model for `provider`; never for whichever provider happens to be
    /// selected.
    #[serde(default)]
    pub(super) model: Option<String>,
}

/// One pane search. An empty `term` clears the search rather than needing its
/// own event, and `step` folds "search this" and "go to the next one" into one
/// path: 0 searches and keeps the current match, +1 and -1 move.
#[derive(Debug, Deserialize)]
pub(super) struct PaneFindPayload {
    pub(super) pane_id: String,
    pub(super) term: String,
    #[serde(default)]
    pub(super) case_sensitive: bool,
    #[serde(default)]
    pub(super) whole_word: bool,
    #[serde(default)]
    pub(super) regex: bool,
    #[serde(default)]
    pub(super) step: i64,
}

#[derive(Debug, Deserialize)]
pub(super) struct PaneTextScalePayload {
    pub(super) pane_id: String,
    pub(super) direction: String,
}

/// The editor is one surface, not one per document, so its zoom carries a
/// direction and nothing to key it by.
#[derive(Debug, Deserialize)]
pub(super) struct EditorTextScalePayload {
    pub(super) direction: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct PetVisibilityPayload {
    pub(super) visible: bool,
}

#[derive(Debug, Deserialize)]
pub(super) struct PetMovePayload {
    /// Already clamped to a visible screen by the shell, which owns the
    /// display geometry; the core only records where the pet ended up.
    pub(super) x: f64,
    pub(super) y: f64,
}

#[derive(Debug, Deserialize)]
pub(super) struct PetDragPayload {
    pub(super) dragging: bool,
}

#[derive(Debug, Deserialize)]
pub(super) struct PetShortcutPayload {
    /// `null` or blank means the user cleared the binding: nothing is
    /// registered and no shortcut fires (D-14).
    pub(super) accelerator: Option<String>,
    #[serde(default)]
    pub(super) error: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct TerminalScrollPayload {
    pub(super) pane_id: String,
    pub(super) direction: String,
    pub(super) lines: u16,
    #[serde(default)]
    pub(super) column: Option<u16>,
    #[serde(default)]
    pub(super) row: Option<u16>,
    #[serde(default)]
    pub(super) modifiers: u8,
}

#[derive(Debug, Deserialize)]
pub(super) struct TerminalClickPayload {
    pub(super) pane_id: String,
    pub(super) column: u16,
    pub(super) row: u16,
    pub(super) modifiers: u8,
}

#[derive(Debug, Deserialize)]
pub(super) struct TerminalResizePayload {
    pub(super) pane_id: String,
    pub(super) cols: u16,
    pub(super) rows: u16,
    #[serde(default)]
    pub(super) new_view: bool,
}

pub(super) enum Event {
    WorkspaceView(WorkspaceViewPayload),
    Key(KeyPayload),
    Attachment(AttachmentPayload),
    AttachmentReady(AttachmentCompletionPayload),
    AttachmentAction(AttachmentActionPayload),
    TerminalOutput(TerminalOutputPayload),
    SessionSnapshot(SessionSnapshotPayload),
    RefreshStatus,
    Click(ClickPayload),
    FocusPane(FocusPaneRequestPayload),
    OpenBrowser(OpenBrowserPayload),
    BrowserStatus(BrowserStatusPayload),
    CreateWorkspace(CreateWorkspacePayload),
    CreateTab(CreateTabPayload),
    FocusCheckout(FocusCheckoutPayload),
    FocusTab(FocusTabPayload),
    ReorderTab(ReorderTabPayload),
    FocusDevice(FocusDevicePayload),
    InactiveCheckoutsToggle(InactiveCheckoutsTogglePayload),
    InactiveProjectsToggle(InactiveProjectsTogglePayload),
    WorkspacePinSet(WorkspacePinSetPayload),
    RemoveWorkspace(RemoveWorkspacePayload),
    RegisterDevice(RegisterDevicePayload),
    DeviceHostConsent(DeviceHostConsentPayload),
    DeviceHostRetry(DeviceHostRetryPayload),
    RemoveDevice(RemoveDevicePayload),
    TestDevice(TestDevicePayload),
    CreatePane(CreatePanePayload),
    ResizePane(ResizePanePayload),
    ToggleZoom(ToggleZoomPayload),
    ToggleConversation(PaneTargetPayload),
    CloseWorkspace(ConfirmedWorkspacePayload),
    CloseTab(ConfirmedTabPayload),
    ClosePane(ConfirmedPanePayload),
    CheckCloseStatus(CheckCloseStatusPayload),
    ReopenClosed,
    ForkPane(PaneTargetPayload),
    AgentTreeToggle(PaneTargetPayload),
    RemoteControl(RemoteControlPayload),
    RemoteFileList(RemoteFileListPayload),
    FileOpen(FileOpenPayload),
    RevealPath(RevealPathPayload),
    FileFocus(FileTabPayload),
    /// Keep Open: the preview tab becomes an ordinary tab in the same slot.
    FileKeepOpen(FileTabPayload),
    FileClose(FileClosePayload),
    FileDraft(FileDraftPayload),
    FileView(FileViewPayload),
    FileSave(FileSavePayload),
    FileConflict(FileConflictPayload),
    FileCreate(ExplorerCreatePayload),
    DirCreate(ExplorerCreatePayload),
    PathRename(PathRenamePayload),
    PathMove(PathMovePayload),
    PathTrash(PathTrashPayload),
    UiStateUpdate(Box<UiStateUpdatePayload>),
    SessionsRefresh,
    SessionsSetMode(SessionsModePayload),
    SessionsSetFilter(SessionsFilterPayload),
    ArchiveOpen(ArchiveOpenPayload),
    MemoryOpenForTurn(MemoryOpenForTurnPayload),
    MemoryAction(MemoryActionPayload),
    RetryConnect(RetryConnectPayload),
    InstallAgentHooks(InstallAgentHooksPayload),
    AiSettings(AiSettingsPayload),
    TerminalResize(TerminalResizePayload),
    TerminalViewport(TerminalResizePayload),
    TerminalScroll(TerminalScrollPayload),
    TerminalClick(TerminalClickPayload),
    PaneFind(PaneFindPayload),
    PaneTextScale(PaneTextScalePayload),
    EditorTextScale(EditorTextScalePayload),
    ChangesSelect(ChangesSelectPayload),
    GitWorktreeOpen(GitWorktreeOpenPayload),
    GitWorktreeSetBase(GitWorktreeSetBasePayload),
    CreateWorktree(CreateWorktreePayload),
    SetCheckoutPurpose(SetCheckoutPurposePayload),
    SetCheckoutIssue(SetCheckoutPurposePayload),
    MigrateMainBranch(MigrateMainBranchPayload),
    TaskOperationAck(TaskOperationAckPayload),
    TaskAgentRetry(TaskAgentRetryPayload),
    RemoveWorktree(RemoveWorktreePayload),
    GithubRequest(GithubRequestPayload),
    /// The card's refresh button, opening the delete confirmation, and a
    /// completed worktree removal. All three say "read again now" about a
    /// different set of readers, and none needs a target: the card is always
    /// the selected checkout, and a removal changes the whole worktree list.
    OverviewOpenSection(OverviewOpenSectionPayload),
    AgentStartInCheckout(AgentStartInCheckoutPayload),
    CleanupReview,
    CleanupConfirm(CleanupConfirmPayload),
    CleanupDismiss,
    CardRefresh,
    CardMeasureDisk,
    ReconnectPane(FocusPanePayload),
    PetSetVisible(PetVisibilityPayload),
    PetToggleVisible,
    PetMove(PetMovePayload),
    PetDrag(PetDragPayload),
    PetActivity,
    PetShortcutUpdate(PetShortcutPayload),
}

/// The web shell rebinds a handful of pane commands; the bound keeps a
/// malformed client from growing the persisted state without limit.
const BROWSER_BINDINGS_CAP: usize = 16;
const BROWSER_BINDING_TEXT_CAP: usize = 64;

fn browser_bindings_fit(bindings: &std::collections::BTreeMap<String, String>) -> bool {
    bindings.len() <= BROWSER_BINDINGS_CAP
        && bindings.iter().all(|(command, chord)| {
            command.len() <= BROWSER_BINDING_TEXT_CAP && chord.len() <= BROWSER_BINDING_TEXT_CAP
        })
}

pub(super) fn decode(bytes: &[u8]) -> Result<Event, EventValidationError> {
    let event =
        serde_json::from_slice::<EventEnvelope>(bytes).map_err(|_| EventValidationError {
            kind: "event.invalid_json",
            message: "Event JSON could not be decoded".to_owned(),
        })?;

    if event.schema_version != SCHEMA_VERSION {
        return Err(EventValidationError {
            kind: "schema_version.mismatch",
            message: format!(
                "Event schema version {} does not match {}",
                event.schema_version, SCHEMA_VERSION
            ),
        });
    }

    validate_event(event)
}

pub(super) struct EventValidationError {
    pub(super) kind: &'static str,
    pub(super) message: String,
}

pub(super) fn validate_event(event: EventEnvelope) -> Result<Event, EventValidationError> {
    let EventEnvelope { kind, payload, .. } = event;
    let invalid_payload = |kind: &str| EventValidationError {
        kind: "event.invalid_payload",
        message: format!("Event payload for {kind} does not match schema version {SCHEMA_VERSION}"),
    };

    macro_rules! decode {
        ($payload:ty, $variant:ident) => {
            serde_json::from_value::<$payload>(payload)
                .map(Event::$variant)
                .map_err(|_| invalid_payload(&kind))
        };
    }

    match kind.as_str() {
        "key" => decode!(KeyPayload, Key),
        "terminal_attachment" => decode!(AttachmentPayload, Attachment),
        "terminal_attachment_ready" => decode!(AttachmentCompletionPayload, AttachmentReady),
        "terminal_attachment_action" => decode!(AttachmentActionPayload, AttachmentAction),
        "terminal_output" => decode!(TerminalOutputPayload, TerminalOutput),
        "session_snapshot" => decode!(SessionSnapshotPayload, SessionSnapshot),
        "refresh_status" => Ok(Event::RefreshStatus),
        "click" => decode!(ClickPayload, Click),
        "focus_pane" => decode!(FocusPaneRequestPayload, FocusPane),
        "open_browser" => decode!(OpenBrowserPayload, OpenBrowser),
        "browser_status" => decode!(BrowserStatusPayload, BrowserStatus),
        "create_workspace" => decode!(CreateWorkspacePayload, CreateWorkspace),
        "create_tab" => decode!(CreateTabPayload, CreateTab),
        "focus_checkout" => decode!(FocusCheckoutPayload, FocusCheckout),
        "focus_tab" => decode!(FocusTabPayload, FocusTab),
        "reorder_tab" => decode!(ReorderTabPayload, ReorderTab),
        "focus_device" => decode!(FocusDevicePayload, FocusDevice),
        "inactive_checkouts_toggle" => {
            decode!(InactiveCheckoutsTogglePayload, InactiveCheckoutsToggle)
        }
        "inactive_projects_toggle" => {
            decode!(InactiveProjectsTogglePayload, InactiveProjectsToggle)
        }
        "workspace_pin_set" => decode!(WorkspacePinSetPayload, WorkspacePinSet),
        "remove_workspace" => decode!(RemoveWorkspacePayload, RemoveWorkspace),
        "register_device" => decode!(RegisterDevicePayload, RegisterDevice),
        "device_host_consent" => decode!(DeviceHostConsentPayload, DeviceHostConsent),
        "device_host_retry" => decode!(DeviceHostRetryPayload, DeviceHostRetry),
        "remove_device" => decode!(RemoveDevicePayload, RemoveDevice),
        "test_device" => decode!(TestDevicePayload, TestDevice),
        "create_pane" => decode!(CreatePanePayload, CreatePane),
        "resize_pane" => decode!(ResizePanePayload, ResizePane),
        "toggle_zoom" => decode!(ToggleZoomPayload, ToggleZoom),
        "toggle_conversation" => decode!(PaneTargetPayload, ToggleConversation),
        "close_workspace" => decode!(ConfirmedWorkspacePayload, CloseWorkspace),
        "close_tab" => decode!(ConfirmedTabPayload, CloseTab),
        "close_pane" => decode!(ConfirmedPanePayload, ClosePane),
        "check_close_status" => decode!(CheckCloseStatusPayload, CheckCloseStatus),
        "reopen_closed" => Ok(Event::ReopenClosed),
        "fork_pane" => decode!(PaneTargetPayload, ForkPane),
        "agent_tree_toggle" => decode!(PaneTargetPayload, AgentTreeToggle),
        "remote_control" => decode!(RemoteControlPayload, RemoteControl),
        "remote_file_list" => decode!(RemoteFileListPayload, RemoteFileList),
        "file_open" => decode!(FileOpenPayload, FileOpen),
        "reveal_path" => decode!(RevealPathPayload, RevealPath),
        "file_focus" => decode!(FileTabPayload, FileFocus),
        "file_keep_open" => decode!(FileTabPayload, FileKeepOpen),
        "file_close" => decode!(FileClosePayload, FileClose),
        "file_draft" => decode!(FileDraftPayload, FileDraft),
        "file_view" => decode!(FileViewPayload, FileView),
        "file_save" => decode!(FileSavePayload, FileSave),
        "file_conflict" => decode!(FileConflictPayload, FileConflict),
        "file_create" => decode!(ExplorerCreatePayload, FileCreate),
        "dir_create" => decode!(ExplorerCreatePayload, DirCreate),
        "path_rename" => decode!(PathRenamePayload, PathRename),
        "path_move" => decode!(PathMovePayload, PathMove),
        "path_trash" => decode!(PathTrashPayload, PathTrash),
        "ui_state_update" => serde_json::from_value::<Box<UiStateUpdatePayload>>(payload)
            .map(Event::UiStateUpdate)
            .map_err(|_| invalid_payload(&kind)),
        "sessions_refresh" => Ok(Event::SessionsRefresh),
        "sessions_set_mode" => decode!(SessionsModePayload, SessionsSetMode),
        "sessions_set_filter" => decode!(SessionsFilterPayload, SessionsSetFilter),
        "archive_open" => decode!(ArchiveOpenPayload, ArchiveOpen),
        "memory_open_for_turn" => decode!(MemoryOpenForTurnPayload, MemoryOpenForTurn),
        "memory_action" => decode!(MemoryActionPayload, MemoryAction),
        "retry_connect" => decode!(RetryConnectPayload, RetryConnect),
        "install_agent_hooks" => decode!(InstallAgentHooksPayload, InstallAgentHooks),
        "ai_settings" => decode!(AiSettingsPayload, AiSettings),
        "terminal_resize" => decode!(TerminalResizePayload, TerminalResize),
        "terminal_viewport" => decode!(TerminalResizePayload, TerminalViewport),
        "terminal_scroll" => decode!(TerminalScrollPayload, TerminalScroll),
        "terminal_click" => decode!(TerminalClickPayload, TerminalClick),
        "pane_find" => decode!(PaneFindPayload, PaneFind),
        "pane_text_scale" => decode!(PaneTextScalePayload, PaneTextScale),
        "editor_text_scale" => decode!(EditorTextScalePayload, EditorTextScale),
        "changes_select" => decode!(ChangesSelectPayload, ChangesSelect),
        "git_worktree_open" => decode!(GitWorktreeOpenPayload, GitWorktreeOpen),
        "git_worktree_set_base" => decode!(GitWorktreeSetBasePayload, GitWorktreeSetBase),
        "create_worktree" => decode!(CreateWorktreePayload, CreateWorktree),
        "set_checkout_issue" => decode!(SetCheckoutPurposePayload, SetCheckoutIssue),
        "set_checkout_purpose" => decode!(SetCheckoutPurposePayload, SetCheckoutPurpose),
        "migrate_main_branch" => decode!(MigrateMainBranchPayload, MigrateMainBranch),
        "task_operation_ack" => decode!(TaskOperationAckPayload, TaskOperationAck),
        "task_agent_retry" => decode!(TaskAgentRetryPayload, TaskAgentRetry),
        "remove_worktree" => decode!(RemoveWorktreePayload, RemoveWorktree),
        "github_request" => decode!(GithubRequestPayload, GithubRequest),
        "cleanup_review" => Ok(Event::CleanupReview),
        "cleanup_confirm" => decode!(CleanupConfirmPayload, CleanupConfirm),
        "cleanup_dismiss" => Ok(Event::CleanupDismiss),
        "overview_open_section" => decode!(OverviewOpenSectionPayload, OverviewOpenSection),
        "agent_start_in_checkout" => decode!(AgentStartInCheckoutPayload, AgentStartInCheckout),
        "card_refresh" => Ok(Event::CardRefresh),
        "card_measure_disk" => Ok(Event::CardMeasureDisk),
        "reconnect_pane" => decode!(FocusPanePayload, ReconnectPane),
        "pet_set_visible" => decode!(PetVisibilityPayload, PetSetVisible),
        "pet_toggle_visible" => Ok(Event::PetToggleVisible),
        "pet_move" => decode!(PetMovePayload, PetMove),
        "pet_drag" => decode!(PetDragPayload, PetDrag),
        "pet_activity" => Ok(Event::PetActivity),
        "pet_shortcut_update" => decode!(PetShortcutPayload, PetShortcutUpdate),
        "workspace_view" => decode!(WorkspaceViewPayload, WorkspaceView),
        _ => Err(EventValidationError {
            kind: "event.unknown_kind",
            message: format!("Unknown event kind: {kind}"),
        }),
    }
}

impl Runtime {
    pub(super) fn apply(&mut self, event: Event) -> bool {
        match event {
            Event::WorkspaceView(payload) => self.apply_workspace_view(payload),
            Event::SessionsRefresh => self.request_sessions_refresh(),
            Event::SessionsSetMode(payload) => self.set_sessions_mode(&payload.mode),
            Event::SessionsSetFilter(payload) => {
                self.set_sessions_filter(&payload.provider, payload.query)
            }
            Event::ArchiveOpen(payload) => {
                self.open_archive_detail(&payload.kind, &payload.id, payload.preview)
            }
            Event::MemoryOpenForTurn(payload) => self.open_memory_for_turn(payload.item_ids),
            Event::MemoryAction(payload) => self.apply_memory_action(payload),
            Event::Attachment(payload) => self.begin_attachment(payload),
            Event::AttachmentReady(payload) => self.attachment_ready(payload),
            Event::AttachmentAction(payload) => self.attachment_action(payload),
            Event::Key(payload) => {
                if let Some(changed) = self.hold_attachment_input(&payload) {
                    return changed;
                }
                self.snapshot.input_generation = self.snapshot.input_generation.saturating_add(1);
                self.snapshot.focused.surface = Surface::Terminal;
                self.snapshot.focused.pane_id = Some(payload.pane_id.clone());
                self.snapshot.terminal.pane_id = Some(payload.pane_id.clone());
                self.ensure_terminal_pane(&payload.pane_id);
                self.sync_focused_terminal_projection();
                if self.live.is_some()
                    || self.remote_terminals.keys().any(|target_id| {
                        remote_pane_source_id(target_id, &payload.pane_id).is_some()
                    })
                {
                    self.write_terminal_control(
                        &payload.pane_id,
                        &payload.bytes_base64,
                        payload.input_trace,
                    );
                } else {
                    // Fixture mode has no PTY behind the pane; the loopback
                    // echo is the whole byte bridge.
                    self.append_terminal_chunk(payload.pane_id, payload.bytes_base64);
                }
                true
            }
            Event::TerminalOutput(payload) => {
                if self.snapshot.terminal.pane_id.is_none() {
                    self.snapshot.terminal.pane_id = Some(payload.pane_id.clone());
                    self.snapshot.focused.pane_id = Some(payload.pane_id.clone());
                }
                self.ensure_terminal_pane(&payload.pane_id);
                self.append_terminal_chunk(payload.pane_id, payload.bytes_base64);
                true
            }
            Event::SessionSnapshot(payload) => self.ingest_session(Ok(payload)),
            Event::RefreshStatus => self.request_status_refresh(),
            Event::PetSetVisible(payload) => self.set_pet_visible(payload.visible),
            Event::PetToggleVisible => {
                let visible = !self.snapshot.ui_state.pet_visible;
                self.set_pet_visible(visible)
            }
            Event::PetMove(payload) => {
                let origin = PetOriginSnapshot {
                    x: payload.x,
                    y: payload.y,
                };
                if self.snapshot.ui_state.pet_origin == Some(origin) {
                    return false;
                }
                self.snapshot.ui_state.pet_origin = Some(origin);
                self.persist_ui_state();
                self.refresh_pet();
                true
            }
            Event::PetDrag(payload) => {
                if self.pet_dragging == payload.dragging {
                    return false;
                }
                self.pet_dragging = payload.dragging;
                self.note_pet_activity();
                self.refresh_pet()
            }
            Event::PetActivity => {
                self.note_pet_activity();
                self.refresh_pet()
            }
            Event::PetShortcutUpdate(payload) => {
                let accelerator = payload
                    .accelerator
                    .map(|value| value.trim().to_owned())
                    .filter(|value| !value.is_empty());
                let error = payload.error.filter(|value| !value.trim().is_empty());
                let unchanged = self.snapshot.ui_state.pet_shortcut == accelerator
                    && self.snapshot.pet.shortcut_error == error;
                if unchanged {
                    return false;
                }
                self.snapshot.ui_state.pet_shortcut = accelerator;
                self.snapshot.pet.shortcut_error = error;
                self.persist_ui_state();
                self.refresh_pet();
                true
            }
            Event::Click(payload) => {
                let _ = (payload.x, payload.y, payload.button, payload.click_count);
                self.snapshot.focused.surface = payload.surface;
                true
            }
            Event::FocusPane(payload) => {
                self.focus_pane(payload.pane_id, payload.origin, payload.request_id);
                true
            }
            Event::ReconnectPane(payload) => {
                self.terminal_recovery.remove(&payload.pane_id);
                let pane_id = payload.pane_id;
                let pane_exists = self
                    .snapshot
                    .navigator
                    .workspaces
                    .iter()
                    .flat_map(|workspace| workspace.checkouts.iter())
                    .flat_map(|checkout| checkout.tabs.iter())
                    .flat_map(|tab| tab.panes.iter())
                    .any(|pane| pane.id == pane_id);
                if !pane_exists {
                    self.set_error(
                        "pane.reconnect_missing",
                        format!("Pane {pane_id} no longer exists"),
                        false,
                    );
                    return true;
                }
                let previous_attempt = self
                    .terminal_session_lifecycles
                    .get(&pane_id)
                    .map_or(0, |lifecycle| lifecycle.attempt);
                let _retired_session = self.terminal_sessions.remove(&pane_id);
                self.terminal_session_lifecycles.insert(
                    pane_id.clone(),
                    TerminalSessionLifecycle {
                        attempt: previous_attempt,
                        retry_decision: "manual",
                        ..TerminalSessionLifecycle::default()
                    },
                );
                self.push_diagnostic(
                    "pane.reconnect.requested",
                    format!("Reconnect requested for pane {pane_id}"),
                );
                self.request_terminal_control(&pane_id);
                true
            }
            Event::OpenBrowser(payload) => {
                self.snapshot.status.chromux.profile = payload.profile;
                let action = chromux::plan_open(&self.snapshot.status.chromux.profile, None, None);
                self.snapshot.status.chromux.state = "parked".to_owned();
                self.snapshot.status.chromux.message = Some(match action {
                    chromux::BrowserAction::Parked(message) => message,
                    _ => "Runtime execution is parked for this approved batch".to_owned(),
                });
                true
            }
            Event::BrowserStatus(payload) => {
                self.snapshot.status.chromux.state = payload.state;
                self.snapshot.status.chromux.profile = payload.profile;
                self.snapshot.status.chromux.current_url = payload.current_url;
                self.snapshot.status.chromux.current_title = payload.current_title;
                self.snapshot.status.chromux.message = payload.message;
                self.snapshot.status.chromux.last_checked_at_unix_ms =
                    Some(payload.last_checked_at_unix_ms);
                true
            }
            Event::InstallAgentHooks(payload) => {
                self.request_agent_hook_install(&payload.runtime_id)
            }
            Event::AiSettings(payload) => self.apply_ai_settings(payload),
            Event::RetryConnect(payload) => self.retry_remote_device(&payload.target_id),
            Event::CreateWorkspace(payload) => {
                if let Some(device) = payload
                    .device_id
                    .clone()
                    .filter(|device| device != workspace::LOCAL_DEVICE_ID)
                {
                    return self.create_device_registration(&device, payload.path, payload.label);
                }
                if let Some(context) = self.live.as_ref().cloned() {
                    // A folder whose removal is still closing panes keeps the
                    // registration id it is about to lose; adding it now would
                    // open a pane in that workspace and then watch the retire
                    // delete the registration under it, with nothing to say
                    // the add did not stick.
                    if let Some(workspace_id) = self.workspace_removal_in_flight_for(&payload.path)
                    {
                        self.set_error(
                            "workspace.remove_in_flight",
                            format!(
                                "{} is still being removed; wait for its panes to close, then add it again",
                                payload.path
                            ),
                            false,
                        );
                        self.push_diagnostic(
                            "workspace.create.refused_during_removal",
                            format!("Workspace {workspace_id} removal is closing panes"),
                        );
                        return true;
                    }
                    if !self
                        .workspace_creations_in_flight
                        .insert(payload.path.clone())
                    {
                        self.push_diagnostic(
                            "workspace.create.duplicate",
                            format!("Workspace creation is already running for {}", payload.path),
                        );
                        return false;
                    }
                    let path = payload.path.clone();
                    let result = live::spawn_workspace_creation(
                        context,
                        payload.path,
                        payload.label,
                        payload.initialize_git,
                        self.snapshot.ui_state.workspace_registrations.clone(),
                    );
                    if let Err(message) = result {
                        self.workspace_creations_in_flight.remove(&path);
                        self.set_error("workspace.create_worker_failed", message, true);
                    } else {
                        self.push_diagnostic(
                            "workspace.create.requested",
                            format!("Creating workspace from {path}"),
                        );
                    }
                    return true;
                }
                self.set_error(
                    "workspace.control_unavailable",
                    "Workspace creation requires a live Herdr connection so its initial tab and pane can be created",
                    true,
                );
                true
            }
            Event::CreateTab(payload) => {
                let Some((workspace_id, workspace_label, checkout_id, cwd)) = self
                    .snapshot
                    .navigator
                    .workspaces
                    .iter()
                    .find(|workspace| workspace.id == payload.workspace_id)
                    .and_then(|workspace| {
                        payload
                            .checkout_id
                            .as_deref()
                            .and_then(|checkout_id| {
                                workspace
                                    .checkouts
                                    .iter()
                                    .find(|checkout| checkout.id == checkout_id)
                            })
                            .or_else(|| workspace.checkouts.first())
                            .map(|checkout| {
                                (
                                    workspace.id.clone(),
                                    workspace.label.clone(),
                                    checkout.id.clone(),
                                    checkout.path.clone(),
                                )
                            })
                    })
                else {
                    let workspace_exists = self
                        .snapshot
                        .navigator
                        .workspaces
                        .iter()
                        .any(|workspace| workspace.id == payload.workspace_id);
                    if workspace_exists {
                        self.set_error("tab.no_checkout", "Workspace has no checkout", false);
                        return true;
                    }
                    self.set_error(
                        "tab.unknown_workspace",
                        format!("Workspace {} is not registered", payload.workspace_id),
                        false,
                    );
                    return true;
                };
                let label = payload.label.trim();
                if label.is_empty() {
                    self.set_error("tab.invalid_label", "Tab label cannot be empty", false);
                    return true;
                }
                let session_workspace_id =
                    self.reusable_session_workspace_id(&workspace_id, &checkout_id);
                let Some(context) = self.live.as_ref().cloned() else {
                    self.set_error(
                        "tab.control_unavailable",
                        "Tab creation requires a live Herdr connection",
                        true,
                    );
                    return true;
                };
                self.snapshot.navigator.focused_workspace_id = Some(workspace_id.clone());
                self.snapshot.navigator.focused_checkout_id = Some(checkout_id);
                self.snapshot.navigator.root_path = Some(cwd.clone());
                self.sync_changes_root_path();
                self.yield_surface_to_terminal();
                self.persist_current_ui_state();
                let action = match session_workspace_id {
                    Some(session_workspace_id) => RemoteControlAction::CreateTab {
                        workspace_id: session_workspace_id,
                        cwd,
                        label: label.to_owned(),
                    },
                    None => RemoteControlAction::CreateWorkspace {
                        cwd,
                        label: workspace_label,
                    },
                };
                self.push_diagnostic(
                    "tab.create.requested",
                    format!("Creating {}", action.kind()),
                );
                if let Err(message) = live::spawn_local_control(context, action) {
                    self.set_error("tab.create_worker_failed", message, true);
                }
                true
            }
            Event::FocusCheckout(payload) => {
                self.focus_checkout(&payload.workspace_id, &payload.checkout_id)
            }
            Event::FocusTab(payload) => {
                let Some(workspace_snapshot) = self
                    .snapshot
                    .navigator
                    .workspaces
                    .iter_mut()
                    .find(|workspace| workspace.id == payload.workspace_id)
                else {
                    self.set_error(
                        "tab.unknown_workspace",
                        format!("Workspace {} is not registered", payload.workspace_id),
                        false,
                    );
                    return true;
                };
                let Some(checkout) = workspace_snapshot
                    .checkouts
                    .iter_mut()
                    .find(|checkout| checkout.id == payload.checkout_id)
                else {
                    self.set_error(
                        "tab.unknown_checkout",
                        format!("Checkout {} is not available", payload.checkout_id),
                        false,
                    );
                    return true;
                };
                let Some(index) = checkout
                    .tabs
                    .iter()
                    .position(|tab| tab.id.as_deref() == Some(payload.tab_id.as_str()))
                else {
                    self.set_error(
                        "tab.unknown",
                        format!("Tab {} is not available", payload.tab_id),
                        false,
                    );
                    return true;
                };
                let checkout_path = checkout.path.clone();
                // Hide owns the visible tab, so the active mark moves here,
                // on the frame the operator asked for it, and Herdr is told
                // afterwards. Focusing a tab still does not reorder the
                // strip: Herdr owns the order, and moving the tab here made
                // every switch look like a reorder until the next catalog
                // rebuild undid it.
                let already_visible = checkout.active_tab_id.as_deref() == Some(&payload.tab_id);
                checkout.active_tab_id = Some(payload.tab_id.clone());
                let first_pane_id = checkout.tabs[index]
                    .panes
                    .first()
                    .map(|pane| pane.id.clone());
                // Return to the pane the operator last had in that tab. Its
                // layout is already here, so the tab's own focused pane is
                // known without asking Herdr for it.
                let next_pane_id = self.tab_focus_pane_id(&payload.tab_id, first_pane_id);
                self.snapshot.navigator.focused_workspace_id = Some(payload.workspace_id);
                self.snapshot.navigator.focused_checkout_id = Some(payload.checkout_id.clone());
                self.snapshot.navigator.root_path = Some(checkout_path);
                self.sync_changes_root_path();
                self.refresh_inactive_groups();
                self.visible_tab_ids
                    .insert(payload.checkout_id.clone(), payload.tab_id.clone());
                // Nothing is cleared here. The tab being selected already has
                // its layout in the snapshot, so the canvas draws it on this
                // frame instead of showing an empty canvas until Herdr
                // answers.
                self.select_terminal_pane(next_pane_id.clone());
                self.sync_active_tab_projection();
                // The operator chose this tab, so its pane now holds Hide's
                // keyboard and the read record follows it. Leaving the record
                // on the pane of the tab just left kept marking that pane read
                // while nobody was looking at it.
                self.operator_focused_pane_id = next_pane_id;
                self.refresh_pane_read_state();
                self.yield_surface_to_terminal();
                self.persist_current_ui_state();
                // A first visit attaches the tab's panes now. Waiting for the
                // next session update to do it left the canvas empty until
                // Herdr happened to emit something, up to the catalog window.
                if self.live.is_some() {
                    let pane_ids = self
                        .snapshot
                        .pane_layouts
                        .iter()
                        .find(|layout| layout.tab_id == payload.tab_id)
                        .map(|layout| {
                            layout
                                .pane_ids()
                                .into_iter()
                                .map(str::to_owned)
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default();
                    for pane_id in pane_ids {
                        self.request_terminal_control(&pane_id);
                    }
                }
                // Rule 11: reaching for the tab already showing, with nothing
                // in flight, converges on the state it is already in and
                // sends Herdr no second notification.
                if already_visible
                    && self.view_focus_settled_on(ViewFocusSlot::Tab, &payload.tab_id)
                {
                    return true;
                }
                let Some(context) = self.live.as_ref().cloned() else {
                    self.set_error(
                        "tab.control_unavailable",
                        "Tab focus requires a live Herdr connection",
                        true,
                    );
                    return true;
                };
                if let Err(message) = live::spawn_local_control(
                    context,
                    RemoteControlAction::FocusTab {
                        tab_id: payload.tab_id.clone(),
                    },
                ) {
                    self.set_error("tab.focus_worker_failed", message, true);
                    return true;
                }
                // Latest request wins. A second switch while the first is
                // unconfirmed replaces it, so Herdr's answer to the first
                // cannot pull the canvas back off the tab the operator is
                // now on.
                self.pending_tab_focus =
                    Some(PendingViewFocus::new(payload.checkout_id, payload.tab_id));
                true
            }
            Event::ReorderTab(payload) => self.reorder_tab(payload),
            Event::FocusDevice(payload) => {
                if !self
                    .snapshot
                    .navigator
                    .devices
                    .iter()
                    .any(|device| device.id == payload.device_id)
                {
                    self.set_error(
                        "device.unknown",
                        format!("Device {} is not registered", payload.device_id),
                        false,
                    );
                    return true;
                }
                let local = payload.device_id == workspace::LOCAL_DEVICE_ID;
                self.snapshot.navigator.focused_device_id = Some(payload.device_id);
                self.yield_surface_to_terminal();
                if local {
                    self.return_keyboard_to_local_pane();
                }
                self.reconcile_remote_terminal_selection();
                self.sync_recent_closed_snapshot();
                self.persist_current_ui_state();
                true
            }
            Event::InactiveCheckoutsToggle(payload) => {
                let Some(group) = self
                    .snapshot
                    .navigator
                    .workspaces
                    .iter()
                    .find(|workspace| workspace.path == payload.project_path)
                    .map(|workspace| &workspace.inactive_checkouts)
                else {
                    self.set_error(
                        "inactive_checkouts.unknown_project",
                        format!("Project {} is not available", payload.project_path),
                        false,
                    );
                    return true;
                };
                if group.checkout_ids.is_empty() {
                    self.set_error(
                        "inactive_checkouts.unavailable",
                        format!("Project {} has no inactive checkouts", payload.project_path),
                        false,
                    );
                    return true;
                }
                let expanded = &mut self
                    .snapshot
                    .ui_state
                    .expanded_inactive_checkout_project_paths;
                if expanded.contains(&payload.project_path) {
                    expanded.retain(|path| path != &payload.project_path);
                } else {
                    expanded.push(payload.project_path);
                    expanded.sort();
                }
                self.refresh_inactive_groups();
                self.persist_ui_state();
                true
            }
            Event::InactiveProjectsToggle(payload) => {
                let available = self
                    .snapshot
                    .navigator
                    .inactive_projects
                    .iter()
                    .any(|group| {
                        group.device_id == payload.device_id && !group.project_ids.is_empty()
                    });
                if !available {
                    self.set_error(
                        "inactive_projects.unavailable",
                        format!("Device {} has no inactive projects", payload.device_id),
                        false,
                    );
                    return true;
                }
                let expanded = &mut self.snapshot.ui_state.expanded_inactive_project_device_ids;
                if expanded.contains(&payload.device_id) {
                    expanded.retain(|device_id| device_id != &payload.device_id);
                } else {
                    expanded.push(payload.device_id);
                    expanded.sort();
                }
                self.refresh_inactive_groups();
                self.persist_ui_state();
                true
            }
            Event::WorkspacePinSet(payload) => self.set_workspace_pinned(payload),
            Event::RemoveWorkspace(payload) => self.remove_workspace(payload),
            Event::DeviceHostConsent(payload) => {
                self.set_host_consent(payload.device_id.trim(), payload.allow)
            }
            Event::DeviceHostRetry(payload) => {
                let device_id = payload.device_id.trim().to_owned();
                if self.device_registration_exists(&device_id) {
                    self.close_device_host(&device_id, "retry requested");
                    self.start_device_host(&device_id);
                } else {
                    self.set_error(
                        "device.host.unknown_device",
                        format!("Device {device_id} is not registered"),
                        false,
                    );
                }
                true
            }
            Event::RegisterDevice(payload) => {
                let id = payload.id.trim().to_owned();
                let label = payload.label.trim().to_owned();
                let ssh_alias = payload.ssh_alias.trim().to_owned();
                if id.is_empty() || label.is_empty() || ssh_alias.is_empty() {
                    self.set_error(
                        "device.invalid",
                        "Device id, label, and SSH alias are required",
                        false,
                    );
                    return true;
                }
                let herdr_socket_path = payload
                    .herdr_socket_path
                    .as_deref()
                    .map(str::trim)
                    .filter(|path| !path.is_empty())
                    .map(str::to_owned);
                if herdr_socket_path
                    .as_deref()
                    .is_some_and(|path| !crate::remote::valid_remote_socket_path(path))
                {
                    self.set_error(
                        "device.invalid",
                        "The device's Herdr socket must be an absolute path on the device",
                        false,
                    );
                    return true;
                }
                if id == workspace::LOCAL_DEVICE_ID
                    || self
                        .snapshot
                        .ui_state
                        .device_registrations
                        .iter()
                        .any(|device| device.id == id)
                {
                    self.set_error(
                        "device.duplicate",
                        format!("Device {id} is already registered"),
                        false,
                    );
                    return true;
                }
                let registration = crate::model::DeviceRegistration {
                    id: id.clone(),
                    label,
                    ssh_alias: Some(ssh_alias.clone()),
                    herdr_socket_path,
                    host_consent: payload.host_consent.then(|| self.new_host_consent()),
                };
                self.snapshot
                    .ui_state
                    .device_registrations
                    .push(registration.clone());
                self.rebuild_device_rows();
                self.persist_current_ui_state();
                self.push_diagnostic(
                    "device.registered",
                    format!("Registered SSH device {id} ({ssh_alias})"),
                );
                self.connect_remote_device(&registration);
                true
            }
            Event::RemoveDevice(payload) => {
                if payload.device_id == workspace::LOCAL_DEVICE_ID {
                    self.set_error(
                        "device.local_remove_denied",
                        "This Mac cannot be removed",
                        false,
                    );
                    return true;
                }
                let before = self.snapshot.ui_state.device_registrations.len();
                self.snapshot
                    .ui_state
                    .device_registrations
                    .retain(|device| device.id != payload.device_id);
                if before == self.snapshot.ui_state.device_registrations.len() {
                    self.set_error(
                        "device.unknown",
                        format!("Device {} is not registered", payload.device_id),
                        false,
                    );
                    return true;
                }
                self.disconnect_remote_device(&payload.device_id);
                // Removing a device removes Hide's own record of it: its
                // project registrations, expanded folders and file tabs. Its
                // host, panes, agents and folders are not touched.
                let scope = format!("remote:{}:", payload.device_id);
                let registrations = self.snapshot.ui_state.workspace_registrations.len();
                self.snapshot
                    .ui_state
                    .workspace_registrations
                    .retain(|registration| registration.device_id != payload.device_id);
                let registrations =
                    registrations - self.snapshot.ui_state.workspace_registrations.len();
                self.snapshot
                    .ui_state
                    .device_expanded_paths
                    .remove(&payload.device_id);
                let tabs = self
                    .snapshot
                    .editor
                    .tabs
                    .iter()
                    .filter(|tab| tab.checkout_id.starts_with(&scope))
                    .count();
                self.retire_device_editor_tabs(&payload.device_id);
                self.rebuild_device_rows();
                self.rebuild_tab_strips();
                self.persist_current_ui_state();
                self.push_diagnostic(
                    "device.unregistered",
                    format!(
                        "Unregistered device {} without touching its host; forgot {registrations} project registrations and closed {tabs} file tabs",
                        payload.device_id
                    ),
                );
                true
            }
            Event::TestDevice(payload) => {
                let known = self
                    .snapshot
                    .navigator
                    .devices
                    .iter()
                    .any(|device| device.id == payload.device_id && device.kind == "remote");
                if !known {
                    self.set_error(
                        "device.unknown",
                        format!(
                            "Device {} is not a registered SSH device",
                            payload.device_id
                        ),
                        false,
                    );
                    return true;
                }
                self.start_device_test(&payload.device_id)
            }
            Event::CreatePane(payload) => {
                // A split names no pane; it acts on the current terminal pane.
                // With another device selected that pane is not the one the
                // operator is looking at, so the split is refused rather than
                // landing on this machine (PRD S5 B19). A shell that routes
                // remote splits sends `remote_control` instead.
                if let Some(device_id) = self
                    .snapshot
                    .navigator
                    .focused_device_id
                    .as_deref()
                    .filter(|device_id| *device_id != workspace::LOCAL_DEVICE_ID)
                {
                    self.set_error(
                        "pane.device_mismatch",
                        format!(
                            "Device {device_id} is selected; nothing was split on this machine"
                        ),
                        false,
                    );
                    return true;
                }
                if payload.command.is_some() {
                    self.set_error(
                        "pane.command_unsupported",
                        "Pane split starts the configured shell; a command cannot be supplied",
                        false,
                    );
                    return true;
                }
                let Some(pane_id) = self.snapshot.terminal.pane_id.clone() else {
                    self.set_error(
                        "pane.no_current_pane",
                        "Select a terminal pane before splitting",
                        false,
                    );
                    return true;
                };
                let _ = payload.tab_id;
                let context = self.live.as_ref().cloned();
                let Some(context) = context else {
                    self.set_error(
                        "pane.control_unavailable",
                        "Pane split requires a live Herdr connection",
                        true,
                    );
                    return true;
                };
                let direction = payload.direction;
                let action = PaneControlAction::Split {
                    pane_id: pane_id.clone(),
                    direction,
                    cwd: Some(payload.cwd),
                };
                self.push_diagnostic(
                    format!("pane.split.{}.requested", direction.as_str()),
                    format!("Splitting pane {pane_id} {}", direction.as_str()),
                );
                self.begin_pane_operation(context, action);
                true
            }
            Event::ResizePane(payload) => {
                if !payload.amount.is_finite() || !(0.001..=0.5).contains(&payload.amount) {
                    self.set_error(
                        "pane.resize_invalid_amount",
                        "Pane resize amount must be between 0.001 and 0.5",
                        false,
                    );
                    return true;
                }
                let Some(context) = self.live.as_ref().cloned() else {
                    self.set_error(
                        "pane.control_unavailable",
                        "Pane resize requires a live Herdr connection",
                        true,
                    );
                    return true;
                };
                let pane_id = payload.pane_id;
                let direction = payload.direction;
                self.push_diagnostic(
                    "pane.resize.requested",
                    format!("Resizing pane {pane_id} {}", direction.as_str()),
                );
                self.begin_pane_operation(
                    context,
                    PaneControlAction::Resize {
                        pane_id,
                        direction,
                        amount: payload.amount,
                    },
                );
                true
            }
            Event::ToggleZoom(payload) => {
                let context = self.live.as_ref().cloned();
                let Some(context) = context else {
                    self.set_error(
                        "pane.control_unavailable",
                        "Pane zoom requires a live Herdr connection",
                        true,
                    );
                    return true;
                };
                let pane_id = payload.pane_id;
                self.push_diagnostic(
                    "pane.zoom.requested",
                    format!("Toggling zoom for pane {pane_id}"),
                );
                self.begin_pane_operation(context, PaneControlAction::ToggleZoom { pane_id });
                true
            }
            Event::ToggleConversation(payload) => {
                let is_agent = self.snapshot.navigator.agents.iter().any(|agent| {
                    agent.pane_id == payload.pane_id && conversation_agent_kind(&agent.agent_kind)
                });
                if !is_agent {
                    self.set_error(
                        "conversation.unsupported_pane",
                        format!(
                            "Pane {} does not expose an agent conversation",
                            payload.pane_id
                        ),
                        false,
                    );
                    return true;
                }
                let conversation = &mut self.snapshot.ui_state.conversation_pane_ids;
                if !conversation.remove(&payload.pane_id) {
                    conversation.insert(payload.pane_id.clone());
                }
                true
            }
            Event::CloseWorkspace(payload) => {
                let _ = (payload.workspace_id, payload.confirmed);
                false
            }
            Event::CloseTab(payload) => {
                let tab = self
                    .snapshot
                    .navigator
                    .workspaces
                    .iter()
                    .flat_map(|workspace| workspace.checkouts.iter())
                    .flat_map(|checkout| checkout.tabs.iter())
                    .find(|tab| tab.id.as_deref() == Some(payload.tab_id.as_str()))
                    .cloned();
                let Some(tab) = tab else {
                    self.set_error(
                        "tab.unknown",
                        format!("Tab {} is not available", payload.tab_id),
                        false,
                    );
                    return true;
                };
                let pane_ids = tab
                    .panes
                    .iter()
                    .map(|pane| pane.id.as_str())
                    .collect::<HashSet<_>>();
                let status_unknown = self.snapshot.navigator.agents.iter().any(|agent| {
                    pane_ids.contains(agent.pane_id.as_str()) && agent.requires_close_status_check
                });
                if status_unknown {
                    self.set_error(
                        "tab.close_status_unknown",
                        format!(
                            "Tab {} has a pane whose activity status is unknown; refresh status before closing",
                            payload.tab_id
                        ),
                        true,
                    );
                    return true;
                }
                let requires_confirmation = self.snapshot.navigator.agents.iter().any(|agent| {
                    pane_ids.contains(agent.pane_id.as_str()) && agent.requires_close_confirmation
                });
                if requires_confirmation && !payload.confirmed {
                    self.set_error(
                        "tab.close_confirmation_required",
                        format!(
                            "Tab {} contains an agent that is working or needs attention; close_tab requires confirmed=true",
                            payload.tab_id
                        ),
                        false,
                    );
                    return true;
                }
                let tab_id = payload.tab_id;
                self.push_diagnostic("tab.close.requested", format!("Closing tab {tab_id}"));
                self.start_close_capture(live::CloseCaptureTarget::Tab { tab_id }, tab)
            }
            Event::ClosePane(payload) => {
                let status_unknown = self.snapshot.navigator.agents.iter().any(|agent| {
                    agent.pane_id == payload.pane_id && agent.requires_close_status_check
                });
                if status_unknown {
                    self.set_error(
                        "pane.close_status_unknown",
                        format!(
                            "Pane {} has an unknown activity status; refresh status before closing",
                            payload.pane_id
                        ),
                        true,
                    );
                    return true;
                }
                let requires_confirmation = self.snapshot.navigator.agents.iter().any(|agent| {
                    agent.pane_id == payload.pane_id && agent.requires_close_confirmation
                });
                if requires_confirmation && !payload.confirmed {
                    self.set_error(
                        "pane.close_confirmation_required",
                        format!(
                            "Pane {} is working or needs attention; close_pane requires confirmed=true",
                            payload.pane_id
                        ),
                        false,
                    );
                    return true;
                }
                let pane_id = payload.pane_id;
                if self.live.is_none() {
                    self.set_error(
                        "pane.control_unavailable",
                        "Pane close requires a live Herdr connection",
                        true,
                    );
                    return true;
                }
                let tab = self
                    .snapshot
                    .navigator
                    .workspaces
                    .iter()
                    .flat_map(|workspace| workspace.checkouts.iter())
                    .flat_map(|checkout| checkout.tabs.iter())
                    .find(|tab| tab.panes.iter().any(|pane| pane.id == pane_id))
                    .cloned();
                let Some(tab) = tab else {
                    self.set_error(
                        "pane.unknown",
                        format!("Pane {pane_id} is not available"),
                        false,
                    );
                    return true;
                };
                self.retain_project_before_last_pane_closes(&pane_id);
                self.push_diagnostic("pane.close.requested", format!("Closing pane {pane_id}"));
                self.start_close_capture(live::CloseCaptureTarget::Pane { pane_id }, tab)
            }
            Event::CheckCloseStatus(payload) => self.check_close_status(&payload.key),
            Event::ReopenClosed => self.reopen_closed(),
            Event::ForkPane(payload) => {
                let pane_id = payload.pane_id;
                let Some(agent) = self
                    .snapshot
                    .navigator
                    .agents
                    .iter()
                    .find(|agent| agent.pane_id == pane_id)
                else {
                    self.set_error(
                        "pane.fork_no_agent",
                        format!("Pane {pane_id} is not running an agent that can be forked"),
                        false,
                    );
                    return true;
                };
                let (Some(agent_kind), Some(session_id)) = (
                    ForkableAgent::parse(&agent.agent_kind),
                    agent.session_id.clone(),
                ) else {
                    self.set_error(
                        "pane.fork_unsupported_agent",
                        format!(
                            "Pane {pane_id} runs {} with no forkable session id",
                            agent.agent_kind
                        ),
                        false,
                    );
                    return true;
                };
                // A fork blocks until the agent has started, which takes long
                // enough for a second click to land. Refusing the second one by
                // name is what keeps one activation from becoming two sessions.
                if !self.forks_in_flight.insert(pane_id.clone()) {
                    self.set_error(
                        "pane.fork_already_running",
                        format!("Pane {pane_id} is already being forked"),
                        false,
                    );
                    return true;
                }
                let Some(context) = self.live.as_ref().cloned() else {
                    self.forks_in_flight.remove(&pane_id);
                    self.set_error(
                        "pane.control_unavailable",
                        "Forking a pane requires a live Herdr connection",
                        true,
                    );
                    return true;
                };
                let cwd = self
                    .snapshot
                    .navigator
                    .workspaces
                    .iter()
                    .flat_map(|workspace| workspace.checkouts.iter())
                    .flat_map(|checkout| checkout.tabs.iter())
                    .flat_map(|tab| tab.panes.iter())
                    .find(|pane| pane.id == pane_id)
                    .map(|pane| pane.cwd.clone());
                self.fork_sequence += 1;
                let name = fork_name(
                    &pane_id,
                    &format!("{}-{}", self.fork_sequence, unix_milliseconds()),
                );
                let request = ForkRequest {
                    parent_pane_id: pane_id.clone(),
                    agent: agent_kind,
                    session_id,
                    cwd,
                    name,
                };
                self.push_diagnostic("pane.fork.requested", format!("Forking pane {pane_id}"));
                if let Err(message) = live::spawn_agent_fork(context, request) {
                    // A worker that never started is a fork that failed, and
                    // the operator is owed the same notice and the same
                    // stderr diagnostic either way.
                    self.ingest_fork_result(&pane_id, Err(message), 0);
                }
                true
            }
            Event::RemoteControl(payload) => self.request_remote_control(payload),
            Event::RemoteFileList(payload) => self.request_remote_file_list(payload),
            Event::FileOpen(payload) => {
                let context_exists = self
                    .catalog_checkout(&payload.workspace_id, &payload.checkout_id)
                    .is_some();
                let context_is_focused = self.front_checkout()
                    == Some((payload.workspace_id.as_str(), payload.checkout_id.as_str()));
                if !context_exists || !context_is_focused {
                    self.set_error(
                        "file.invalid_context",
                        "A file tab requires the selected workspace and checkout",
                        false,
                    );
                    return true;
                }
                self.open_file_tab(
                    &payload.workspace_id,
                    &payload.checkout_id,
                    &payload.path,
                    payload.preview,
                );
                self.persist_current_ui_state();
                true
            }
            Event::FileKeepOpen(payload) => self.promote_editor_tab(&payload.tab_id),
            Event::RevealPath(payload) => self.reveal_path(payload),
            Event::FileFocus(payload) => {
                let Some(tab) = self
                    .snapshot
                    .editor
                    .tabs
                    .iter()
                    .find(|tab| tab.id == payload.tab_id)
                    .cloned()
                else {
                    self.set_error(
                        "file.focus_failed",
                        format!("File tab {} is not open", payload.tab_id),
                        false,
                    );
                    return true;
                };
                let Some((device_id, checkout)) = self
                    .catalog_checkout(&tab.workspace_id, &tab.checkout_id)
                    .map(|(workspace, checkout)| (workspace.device_id.clone(), checkout.clone()))
                else {
                    self.set_error("editor.invalid_context", "The editor tab's project or checkout is no longer available. Selection was kept.", false);
                    return true;
                };
                // Validate the document/diff before moving any selection. One
                // event restores the surface and its context, including MRU
                // visits across checkouts; no intermediate terminal frame.
                if let Err(message) = self.activate_editor_tab(&payload.tab_id) {
                    self.set_error("editor.focus_failed", message, false);
                    return true;
                }
                // A device's focus is its Herdr session's, which the core
                // follows; showing one of its files moves nothing here.
                if device_id != workspace::LOCAL_DEVICE_ID {
                    return true;
                }
                let pane_id = checkout.active_tab_id.as_deref().and_then(|id| {
                    let first = checkout
                        .tabs
                        .iter()
                        .find(|tab| tab.id.as_deref() == Some(id))
                        .and_then(|tab| tab.panes.first())
                        .map(|pane| pane.id.clone());
                    self.tab_focus_pane_id(id, first)
                });
                self.snapshot.navigator.focused_workspace_id = Some(tab.workspace_id);
                self.snapshot.navigator.focused_checkout_id = Some(tab.checkout_id);
                self.snapshot.navigator.root_path = Some(checkout.path);
                self.sync_changes_root_path();
                self.select_terminal_pane(pane_id);
                self.operator_focused_pane_id = None;
                self.refresh_pane_read_state();
                self.persist_current_ui_state();
                true
            }
            Event::FileClose(payload) => {
                if let Some(pending_save) = payload.pending_save {
                    self.start_file_save_then_close(payload.tab_id, pending_save)
                } else {
                    self.close_file_tab_now(&payload.tab_id)
                }
            }
            Event::FileView(payload) => {
                let Some(tab) = self
                    .snapshot
                    .editor
                    .tabs
                    .iter_mut()
                    .find(|tab| tab.id == payload.tab_id && tab.kind == EditorTabKind::File)
                else {
                    self.set_error(
                        "file.view_rejected",
                        "The file tab is no longer open",
                        false,
                    );
                    return true;
                };
                if tab.markdown_live == payload.markdown_live && tab.wrap == payload.wrap {
                    return false;
                }
                tab.markdown_live = payload.markdown_live;
                tab.wrap = payload.wrap;
                true
            }
            Event::FileDraft(payload) => {
                let Some(tab_id) = self.snapshot.editor.active_tab_id.clone() else {
                    self.set_error("file.draft_rejected", "No file tab is active", false);
                    return true;
                };
                let Some(document) = self.editor_documents.get_mut(&tab_id) else {
                    self.set_error(
                        "file.draft_rejected",
                        "The active file tab has no document state",
                        false,
                    );
                    return true;
                };
                match files::update_draft(document, payload.contents_utf8) {
                    Ok(()) => {
                        let edited = document.dirty;
                        self.sync_file_tab_dirty(&tab_id);
                        self.sync_active_editor_document();
                        // The first edit keeps a preview tab (B5); an echo of
                        // the same contents is not an edit.
                        if edited {
                            self.promote_editor_tab(&tab_id);
                        }
                    }
                    Err(message) => self.set_error("file.draft_rejected", message, false),
                }
                true
            }
            Event::FileSave(payload) => self.request_file_save(payload, false),
            Event::FileConflict(payload) => {
                let Some(tab_id) = self.snapshot.editor.active_tab_id.clone() else {
                    self.set_error("file.conflict_without_tab", "No file tab is active", false);
                    return true;
                };
                match payload.action.as_str() {
                    "reload" => self.reload_document(&tab_id),
                    "keep_editing" => self.keep_editing_document(&tab_id),
                    _ => {
                        self.set_error(
                            "file.invalid_conflict_action",
                            "Conflict action must be reload or keep_editing",
                            false,
                        );
                        true
                    }
                }
            }
            Event::FileCreate(payload) => self.start_explorer_operation(
                |root| {
                    files::ExplorerOperation::create(
                        files::ExplorerOperationKind::FileCreate,
                        root,
                        Path::new(&payload.parent),
                        &payload.name,
                    )
                },
                &payload.root,
                &payload.parent,
                payload.device_id.as_deref(),
            ),
            Event::DirCreate(payload) => self.start_explorer_operation(
                |root| {
                    files::ExplorerOperation::create(
                        files::ExplorerOperationKind::DirCreate,
                        root,
                        Path::new(&payload.parent),
                        &payload.name,
                    )
                },
                &payload.root,
                &payload.parent,
                payload.device_id.as_deref(),
            ),
            Event::PathRename(payload) => self.start_explorer_operation(
                |root| {
                    files::ExplorerOperation::rename(root, Path::new(&payload.path), &payload.name)
                },
                &payload.root,
                &payload.path,
                payload.device_id.as_deref(),
            ),
            Event::PathMove(payload) => self.start_explorer_operation(
                |root| {
                    files::ExplorerOperation::move_into(
                        root,
                        Path::new(&payload.path),
                        Path::new(&payload.destination),
                    )
                },
                &payload.root,
                &payload.path,
                payload.device_id.as_deref(),
            ),
            Event::PathTrash(payload) => self.start_explorer_operation(
                |root| {
                    files::ExplorerOperation::trash(
                        root,
                        Path::new(&payload.path),
                        Path::new(&payload.select_after),
                        payload.inode,
                    )
                },
                &payload.root,
                &payload.path,
                payload.device_id.as_deref(),
            ),
            Event::TerminalClick(payload) => {
                // D8 explicitly chooses Herdr's detected agent as the policy
                // boundary until its frame protocol carries mouse mode.
                // Do not infer it from titles, output, or a session id.
                let agent = self
                    .snapshot
                    .navigator
                    .agents
                    .iter()
                    .find(|agent| agent.pane_id == payload.pane_id);
                let report = agent.is_some_and(|agent| agent.agent_kind == "claude");
                crate::diagnostic!(serde_json::json!({
                    "component": "terminal", "kind": "terminal.click_routed",
                    "pane_id": payload.pane_id,
                    "basis": if agent.is_some() { "herdr.agent.list" } else { "not_detected" },
                    "agent_kind": agent.map(|agent| agent.agent_kind.as_str()),
                    "route": if report { "sgr_mouse" } else { "local_selection" },
                    "column": payload.column, "row": payload.row,
                }));
                if report {
                    // SGR uses one-based cells, uppercase M for press and
                    // lowercase m for release. No newline or Enter is sent.
                    let column = u32::from(payload.column) + 1;
                    let row = u32::from(payload.row) + 1;
                    // The shell sends crossterm's bitset, the one Herdr's
                    // terminal.scroll takes. SGR has bits for shift, alt and
                    // control only, so command (bit 8) is dropped here on
                    // purpose: a TUI has no way to receive it.
                    let flags = ((payload.modifiers & 1) * 4)
                        | ((payload.modifiers & 2) * 8)
                        | ((payload.modifiers & 4) * 2);
                    let bytes =
                        format!("\x1b[<{flags};{column};{row}M\x1b[<{flags};{column};{row}m");
                    self.write_terminal_control(
                        &payload.pane_id,
                        &live::encode_base64(bytes.as_bytes()),
                        None,
                    );
                    return self.snapshot.status.last_error.is_some();
                }
                false
            }
            Event::TerminalScroll(payload) => {
                if !matches!(payload.direction.as_str(), "up" | "down") || payload.lines == 0 {
                    self.set_error(
                        "terminal.invalid_scroll",
                        "Scroll needs a direction and positive line count",
                        false,
                    );
                    return true;
                }
                // Herdr owns the pane's history, so the wheel is a request it
                // answers with a fresh frame rather than a local buffer move.
                // A pane another client controls is read-only, not broken, so
                // it simply does not scroll - the same shape as resize.
                if let Some(said) = self.scroll_withheld_for_missing_size(&payload.pane_id) {
                    return said;
                }
                let lines =
                    i32::from(payload.lines) * if payload.direction == "up" { 1 } else { -1 };
                if let Some(session) = self.terminal_sessions.get_mut(&payload.pane_id)
                    && session.mode == TerminalSessionMode::Control
                    && let Err(message) = session.scroll(live::ScrollRequest {
                        lines,
                        column: payload.column,
                        row: payload.row,
                        modifiers: payload.modifiers,
                    })
                {
                    self.set_error("terminal.scroll_failed", message, true);
                    return true;
                }
                false
            }
            Event::TerminalViewport(payload) => {
                let size = (payload.rows, payload.cols);
                let changed = self
                    .terminal_view_sizes
                    .insert(payload.pane_id.clone(), size)
                    != Some(size);
                if changed || payload.new_view {
                    self.terminal_frames_need_full.insert(payload.pane_id);
                }
                false
            }
            Event::TerminalResize(payload) => {
                if payload.rows == 0 || payload.cols == 0 {
                    self.set_error(
                        "terminal.invalid_resize",
                        "Terminal dimensions must be positive",
                        false,
                    );
                    return true;
                }
                let size = (payload.rows, payload.cols);
                self.panes_scrolled_before_size.remove(&payload.pane_id);
                let previous = self.terminal_sizes.insert(payload.pane_id.clone(), size);
                // A view reporting the size the pane is already running at is
                // the common case right after an attach. Sending it on would
                // make Herdr answer with a second full frame for a size that
                // never changed.
                // A pane's first size is what the next launch attaches with,
                // so it is written now. Later sizes ride the next UI-state
                // save rather than putting a file write in the middle of a
                // window drag.
                if previous.is_none() {
                    self.persist_ui_state();
                }
                if self.panes_awaiting_size.remove(&payload.pane_id) {
                    self.terminal_recovery.remove(&payload.pane_id);
                    self.terminal_session_lifecycles
                        .entry(payload.pane_id.clone())
                        .or_default()
                        .state = "idle";
                    self.request_terminal_control(&payload.pane_id);
                    return true;
                }
                if previous == Some(size)
                    && !self.terminal_frames_need_full.contains(&payload.pane_id)
                {
                    return false;
                }
                crate::diagnostic!(
                    serde_json::json!({"kind":"terminal.resize_settled", "pane_id":payload.pane_id, "rows":payload.rows, "cols":payload.cols})
                );
                if self.terminal_sessions.contains_key(&payload.pane_id) {
                    if let Some(session) = self.terminal_sessions.get_mut(&payload.pane_id)
                        && session.mode == TerminalSessionMode::Control
                        && let Err(message) = session.resize(payload.rows, payload.cols)
                    {
                        self.set_error("terminal.resize_failed", message, true);
                        return true;
                    }
                    return false;
                }
                false
            }
            Event::PaneFind(payload) => {
                if payload.term.is_empty() {
                    if self.snapshot.find == PaneFindSnapshot::default() {
                        return false;
                    }
                    self.snapshot.find = PaneFindSnapshot::default();
                    return true;
                }
                let Some(context) = self.live.as_ref().cloned() else {
                    self.snapshot.find = PaneFindSnapshot {
                        pane_id: Some(payload.pane_id.clone()),
                        term: payload.term.clone(),
                        unavailable_reason: Some(
                            "Searching a pane's history needs a live Herdr connection".to_owned(),
                        ),
                        ..PaneFindSnapshot::default()
                    };
                    return true;
                };
                // A step continues from the match the operator is on, which is
                // only the stored one when it belongs to this pane and term.
                let current_index = if self.snapshot.find.pane_id.as_deref()
                    == Some(payload.pane_id.as_str())
                    && self.snapshot.find.term == payload.term
                {
                    self.snapshot.find.index
                } else {
                    0
                };
                let request = live::PaneFindRequest {
                    pane_id: payload.pane_id.clone(),
                    term: payload.term.clone(),
                    options: crate::find::PaneFindOptions {
                        case_sensitive: payload.case_sensitive,
                        whole_word: payload.whole_word,
                        regex: payload.regex,
                    },
                    step: payload.step,
                    current_index,
                };
                if let Err(message) = live::spawn_pane_find(context, request) {
                    self.set_error("pane.find_worker_failed", message, true);
                }
                false
            }
            Event::PaneTextScale(payload) => {
                let current = self
                    .snapshot
                    .ui_state
                    .pane_text_scales
                    .get(&payload.pane_id)
                    .copied()
                    .unwrap_or(DEFAULT_PANE_TEXT_SCALE);
                let Some(next) = self.stepped_text_scale(current, &payload.direction) else {
                    return true;
                };
                // A pane at the default is absent rather than stored at 1.0,
                // so resetting every pane leaves an empty map rather than a
                // row per pane the user ever touched.
                let changed = if next == DEFAULT_PANE_TEXT_SCALE {
                    self.snapshot
                        .ui_state
                        .pane_text_scales
                        .remove(&payload.pane_id)
                        .is_some()
                } else {
                    self.snapshot
                        .ui_state
                        .pane_text_scales
                        .insert(payload.pane_id.clone(), next)
                        != Some(next)
                };
                if changed {
                    self.persist_ui_state();
                }
                changed
            }
            Event::GithubRequest(payload) => {
                let project = self
                    .snapshot
                    .navigator
                    .workspaces
                    .iter()
                    .find(|workspace| {
                        workspace.id == payload.workspace_id
                            && workspace.is_git
                            && workspace.remote_target_id.is_none()
                    })
                    .map(|workspace| workspace.path.clone());
                let Some(project) = project else {
                    self.set_error(
                        "github.invalid_project",
                        "GitHub lookup requires a registered local Git project".to_owned(),
                        false,
                    );
                    return true;
                };
                let first = self.sidebar_github_projects.insert(project.clone());
                if payload.refresh {
                    self.refresh_pull_requests(&project);
                    if let Some(cached) = self
                        .github
                        .projects
                        .iter_mut()
                        .find(|cached| cached.root_path == project)
                    {
                        cached.status.loading = true;
                    }
                }
                if first || payload.refresh {
                    self.apply_pull_requests();
                }
                first || payload.refresh
            }
            Event::CleanupReview => self.review_cleanup(),
            Event::CleanupConfirm(payload) => self.confirm_cleanup(payload),
            Event::CleanupDismiss => {
                if self.cleanup.as_ref().is_none_or(|r| r.phase == "removing") {
                    return false;
                }
                self.cleanup = None;
                self.refresh_worktree_projection();
                true
            }
            Event::OverviewOpenSection(payload) => self.overview_open_section(payload),
            Event::AgentStartInCheckout(payload) => self.agent_start_in_checkout(payload),
            Event::CardRefresh => {
                // The card's one refresh button re-reads both the remote
                // answer and the local counts, because the operator pressing
                // it means "this is out of date", not "gh is out of date".
                if let Some((workspace, _)) = self.focused_local_checkout() {
                    let project = workspace.path.clone();
                    self.refresh_pull_requests(&project);
                }
                self.refresh_worktrees();
                self.remeasure_disk();
                true
            }
            Event::CardMeasureDisk => {
                // The delete confirmation states the size it is about to
                // delete, so it is measured when the dialog opens rather than
                // shown from whenever the card last looked.
                self.remeasure_disk();
                true
            }
            Event::GitWorktreeOpen(payload) => self.open_git_worktree(payload.checkout_path),
            Event::GitWorktreeSetBase(payload) => {
                self.set_git_worktree_base(payload.repository_root, payload.branch)
            }
            Event::CreateWorktree(payload) => self.create_project_worktree(payload),
            Event::SetCheckoutIssue(payload) => self.set_checkout_issue(payload),
            Event::SetCheckoutPurpose(payload) => self.set_checkout_purpose(payload),
            Event::MigrateMainBranch(payload) => self.migrate_main_branch(payload),
            Event::TaskOperationAck(payload) => self.acknowledge_task_operation(payload.id),
            Event::TaskAgentRetry(payload) => self.retry_task_agent(payload.id),
            Event::RemoveWorktree(payload) => self.remove_git_worktree(payload),
            Event::EditorTextScale(payload) => {
                let current = self.snapshot.ui_state.editor_text_scale;
                let Some(next) = self.stepped_text_scale(current, &payload.direction) else {
                    return true;
                };
                if next == current {
                    return false;
                }
                self.snapshot.ui_state.editor_text_scale = next;
                self.persist_ui_state();
                true
            }
            Event::ChangesSelect(payload) => {
                let Some(path) = payload.path else {
                    if self.snapshot.changes.selected_path.is_none() {
                        return false;
                    }
                    self.snapshot.changes.selected_path = None;
                    self.snapshot.changes.diff = None;
                    return true;
                };
                if self.snapshot.changes.root_path.as_deref()
                    != self.snapshot.navigator.changes_root_path.as_deref()
                    || self.snapshot.navigator.changes_root_path.is_none()
                {
                    self.set_error(
                        "diff.invalid_context",
                        "History no longer belongs to the selected checkout",
                        false,
                    );
                    return true;
                }
                let entries = if payload.committed {
                    &self.snapshot.changes.committed
                } else {
                    &self.snapshot.changes.entries
                };
                if !entries.iter().any(|entry| entry.path == path) {
                    self.set_error(
                        "diff.selection_unavailable",
                        format!("{path} is no longer in the selected Changes group"),
                        false,
                    );
                    return true;
                }
                // The checkout in front on the selected device, whose History
                // this is, owns the diff tab.
                let Some((workspace_id, checkout_id)) = self.front_checkout_owned() else {
                    self.set_error(
                        "diff.invalid_context",
                        "A diff tab requires the selected workspace and checkout",
                        false,
                    );
                    return true;
                };
                let tab_id =
                    Self::diff_tab_id(&workspace_id, &checkout_id, &path, payload.committed);
                if self.snapshot.editor.active_tab_id.as_deref() == Some(tab_id.as_str()) {
                    return if payload.preview {
                        false
                    } else {
                        self.promote_editor_tab(&tab_id)
                    };
                }
                self.show_diff_tab(
                    &workspace_id,
                    &checkout_id,
                    &path,
                    payload.committed,
                    payload.preview,
                );
                self.persist_current_ui_state();
                true
            }
            Event::AgentTreeToggle(payload) => {
                let exists = self
                    .snapshot
                    .navigator
                    .agents
                    .iter()
                    .any(|agent| agent.pane_id == payload.pane_id)
                    || self
                        .snapshot
                        .status
                        .remote
                        .iter()
                        .filter_map(|remote| remote.session.as_ref())
                        .any(|session| {
                            session
                                .agents
                                .iter()
                                .any(|agent| agent.pane_id == payload.pane_id)
                        });
                if !exists {
                    self.set_error(
                        "agent_tree.parent_unavailable",
                        format!("Agent pane {} is no longer available", payload.pane_id),
                        true,
                    );
                    return true;
                }
                let expanded = &mut self.snapshot.ui_state.expanded_agent_pane_ids;
                if expanded.contains(&payload.pane_id) {
                    expanded.retain(|pane| pane != &payload.pane_id);
                } else {
                    expanded.push(payload.pane_id);
                }
                self.refresh_agent_lineage();
                self.persist_ui_state();
                true
            }
            Event::UiStateUpdate(payload) => {
                let payload = *payload;
                if let Some(visible) = payload.usage_window_visible {
                    self.usage_window_visible = visible;
                }
                if let Some(open) = payload.usage_popover_open {
                    if open && !self.usage_popover_open {
                        self.usage_popover_open_generation =
                            self.usage_popover_open_generation.saturating_add(1);
                    }
                    self.usage_popover_open = open;
                }
                // Pet placement, visibility, and shortcut belong to the pet
                // events; a navigator or keyboard save must not erase them.
                let current = self.snapshot.ui_state.clone();
                let previous_ui_state = current.clone();
                let git_was_visible = current.right_panel_visible
                    && matches!(current.right_panel_section, RightPanelSection::Overview);
                let sessions_were_visible = current.right_panel_visible
                    && matches!(current.right_panel_section, RightPanelSection::Sessions);
                self.snapshot.ui_state = UiStateSnapshot {
                    left_sidebar_visible: payload
                        .left_sidebar_visible
                        .unwrap_or(current.left_sidebar_visible),
                    right_panel_visible: payload
                        .right_panel_visible
                        .unwrap_or(current.right_panel_visible),
                    // An unrecognised section name keeps the current one
                    // rather than silently resetting the panel to Explorer.
                    right_panel_section: payload
                        .right_panel_section
                        .as_deref()
                        .and_then(RightPanelSection::parse)
                        .unwrap_or(current.right_panel_section),
                    sessions_mode_by_project: current.sessions_mode_by_project,
                    expanded_paths: payload.expanded_paths,
                    device_expanded_paths: payload
                        .device_expanded_paths
                        .unwrap_or(current.device_expanded_paths),
                    collapsed_workspace_ids: payload.collapsed_workspace_ids,
                    collapsed_checkout_ids: payload
                        .collapsed_checkout_ids
                        .unwrap_or(current.collapsed_checkout_ids),
                    expanded_inactive_checkout_project_paths: current
                        .expanded_inactive_checkout_project_paths,
                    expanded_inactive_project_device_ids: current
                        .expanded_inactive_project_device_ids,
                    project_base_branches: current.project_base_branches,
                    expanded_agent_pane_ids: payload
                        .expanded_agent_pane_ids
                        .unwrap_or(current.expanded_agent_pane_ids),
                    selected_path: payload.selected_path,
                    selected_pane_id: payload.selected_pane_id,
                    shortcut_bindings: payload.shortcut_bindings,
                    browser_shortcut_bindings: match payload.browser_shortcut_bindings {
                        Some(bindings) if browser_bindings_fit(&bindings) => bindings,
                        Some(_) => {
                            self.set_error(
                                "ui_state.browser_shortcuts_invalid",
                                "Browser shortcuts were not saved: too many or too long",
                                false,
                            );
                            current.browser_shortcut_bindings.clone()
                        }
                        None => current.browser_shortcut_bindings.clone(),
                    },
                    pet_visible: self.snapshot.ui_state.pet_visible,
                    pet_origin: self.snapshot.ui_state.pet_origin,
                    pet_shortcut: self.snapshot.ui_state.pet_shortcut.clone(),
                    focused_device_id: payload
                        .focused_device_id
                        .unwrap_or(current.focused_device_id),
                    focused_checkout_id: payload
                        .focused_checkout_id
                        .unwrap_or(current.focused_checkout_id),
                    workspace_registrations: payload
                        .workspace_registrations
                        .unwrap_or(current.workspace_registrations),
                    device_registrations: payload
                        .device_registrations
                        .unwrap_or(current.device_registrations),
                    accent_hex: payload.accent_hex.unwrap_or(current.accent_hex),
                    font_size: payload.font_size.unwrap_or(current.font_size),
                    // The zoom chords own this map; a navigator or keyboard
                    // save must not erase it, for the same reason the pet
                    // fields above are carried through.
                    pane_text_scales: current.pane_text_scales,
                    editor_text_scale: current.editor_text_scale,
                    conversation_pane_ids: current.conversation_pane_ids,
                    pane_read_records: current.pane_read_records,
                };
                // Visibility and popover activity wake the provider reader,
                // but they are not durable preferences. The shell sends the
                // current durable values with the shared UI-state event; if
                // those values did not change, do not rewrite state.json or
                // run unrelated catalog reconciliation.
                if self.snapshot.ui_state == previous_ui_state {
                    return true;
                }
                self.apply_selected_pane_anchor(self.snapshot.ui_state.selected_pane_id.clone());
                self.snapshot.navigator.focused_device_id =
                    self.snapshot.ui_state.focused_device_id.clone();
                self.snapshot.navigator.focused_checkout_id =
                    self.snapshot.ui_state.focused_checkout_id.clone();
                // UI-state writes can carry a focus anchor after a checkout
                // event. Reconcile the whole navigator identity in this frame.
                self.resync_navigator_focus();
                self.refresh_inactive_groups();
                self.reconcile_remote_terminal_selection();
                Self::apply_workspace_expansion(
                    &mut self.snapshot.navigator.workspaces,
                    &self.snapshot.ui_state.collapsed_workspace_ids,
                );
                self.refresh_agent_lineage();
                let git_is_visible = self.snapshot.ui_state.right_panel_visible
                    && matches!(
                        self.snapshot.ui_state.right_panel_section,
                        RightPanelSection::Overview
                    );
                if git_is_visible && !git_was_visible {
                    if let Some((workspace, _)) = self.focused_local_checkout() {
                        let project = workspace.path.clone();
                        self.refresh_pull_requests(&project);
                    }
                    self.refresh_worktrees();
                    self.remeasure_disk();
                }
                if git_is_visible != git_was_visible {
                    self.refresh_card();
                }
                let sessions_are_visible = self.snapshot.ui_state.right_panel_visible
                    && matches!(
                        self.snapshot.ui_state.right_panel_section,
                        RightPanelSection::Sessions
                    );
                if sessions_are_visible && !sessions_were_visible {
                    self.request_sessions_refresh();
                }
                // Session sync owns session-derived temporary workspaces.
                // UI-state persistence must not rebuild from an empty session
                // and erase the catalog that the user is currently viewing.
                match self.write_ui_state() {
                    Ok(()) => true,
                    Err(message) => {
                        self.set_error("ui_state.save_failed", message, true);
                        true
                    }
                }
            }
        }
    }
}

impl Event {
    /// Terminal input and output, which arrive per keystroke and per chunk
    /// and never move a Workspace's areas.
    pub(super) fn is_terminal_io(&self) -> bool {
        matches!(
            self,
            Event::Key(_)
                | Event::TerminalOutput(_)
                | Event::TerminalResize(_)
                | Event::TerminalViewport(_)
                | Event::TerminalScroll(_)
                | Event::TerminalClick(_)
                | Event::Attachment(_)
                | Event::AttachmentReady(_)
                | Event::AttachmentAction(_)
        )
    }
}
