//! The runtime side of per-Workspace presentation (PRD S6 D-04, D-05, D-08,
//! D-10; S7): which areas the front Workspace shows, its tools, its View
//! area tree, when that tree's documents are read back, and the file that
//! keeps it all. What the areas hold and how an open lands in them is
//! `view_areas.rs`.
//!
//! Everything here is inert unless the shell passed
//! `CoreOptions::workspace_views_path`; the Swift shell keeps the rule that a
//! document takes the terminal canvas and a terminal takes it back.

use std::collections::VecDeque;

use serde::Deserialize;

use super::view_areas::Reconciled;
use super::*;
use crate::model::WorkspaceViewSnapshot;
use crate::workspace_views::{self, ViewMode, WorkspaceView, WorkspaceViews};

/// A Workspace's identity: the device and the checkout path.
pub(super) type WorkspaceKey = (String, String);

struct PendingChoice {
    device_id: String,
    request_id: String,
    key: WorkspaceKey,
}

pub(super) struct WorkspaceViewStore {
    path: PathBuf,
    pub(super) views: WorkspaceViews,
    /// Workspaces whose displays were read back in this process. A document
    /// of one that no display shows gets a display; any other Workspace keeps
    /// the tree the file carried until it is shown.
    pub(super) live: HashSet<WorkspaceKey>,
    /// The Workspace the last sync saw in front.
    pub(super) front: Option<WorkspaceKey>,
    /// Counts the layout changes made outside the reconcile, so an unchanged
    /// state costs the reconcile one comparison.
    pub(super) generation: u64,
    /// What the last reconcile saw (`view_areas.rs`).
    pub(super) reconciled: Option<Reconciled>,
    /// The editor's active tab as the last reconcile set it; any other value
    /// was set by an event, which the reconcile shows.
    pub(super) derived_active: Option<String>,
    /// The last split request ids per Workspace, so a split sent twice
    /// splits once. Runtime only.
    pub(super) split_requests: HashMap<WorkspaceKey, VecDeque<String>>,
    /// Editor tabs the display cap keeps off screen, already reported.
    pub(super) unshown: HashSet<String>,
    /// The Workspace the operator last chose, in this process or before it
    /// started: the one the app opens on (D-11); none on a first run.
    resumable: Option<WorkspaceKey>,
    /// A device Workspace the operator asked for, chosen once it is in front,
    /// with the device and request that asked, so a refusal drops it.
    pending_choice: Option<PendingChoice>,
    /// The right panel the settings file held at start. The snapshot's panel
    /// follows the front Workspace's tools, a projection the settings file
    /// never takes (D-10).
    saved_panel: (bool, RightPanelSection),
    /// Set when an unreadable file could not be moved aside: nothing is ever
    /// written over it.
    frozen: bool,
    save_pending: bool,
    save_active: bool,
    save_worker: Option<thread::JoinHandle<()>>,
}

/// What an event asks of the front Workspace's areas once it has moved the
/// screen: a document opened while only Agents show brings the View area
/// back, and an agent opened while only Views show brings the Agent area
/// back (D-08). Nothing else changes the mode on its own.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum AreaIntent {
    Views,
    Agents,
    /// A reveal also opens the Explorer tool.
    RevealInViews,
}

impl AreaIntent {
    pub(super) fn of(event: &Event) -> Option<Self> {
        match event {
            Event::FileOpen(_) | Event::FileFocus(_) => Some(Self::Views),
            // A deselect opens nothing.
            Event::ChangesSelect(payload) if payload.path.is_some() => Some(Self::Views),
            Event::RevealPath(_) => Some(Self::RevealInViews),
            Event::FocusPane(_) | Event::FocusTab(_) => Some(Self::Agents),
            // A device focus lands where the device's Herdr moves, so
            // `request_remote_control` applies it to that Workspace.
            _ => None,
        }
    }
}

/// The payload of `workspace_view`: any subset of the front Workspace's
/// presentation. Absent fields keep their value.
#[derive(Debug, Deserialize)]
pub(super) struct WorkspaceViewPayload {
    #[serde(default)]
    pub(super) mode: Option<String>,
    #[serde(default)]
    pub(super) explorer: Option<bool>,
    #[serde(default)]
    pub(super) changes: Option<bool>,
    #[serde(default)]
    pub(super) agent_share: Option<f32>,
    /// A file of the front Workspace to reveal: the Explorer shows with the
    /// file's folders unfolded, in the same event, and nothing is opened.
    #[serde(default)]
    pub(super) reveal: Option<String>,
}

impl WorkspaceViewStore {
    /// `saved_panel` is the right panel the older settings file holds, which
    /// is written back unchanged (D-10).
    /// The diagnostics say what the load had to do: move an unreadable file
    /// aside, migrate an older one, or repair a tree past its invariants.
    pub(super) fn open(
        path: PathBuf,
        saved_panel: (bool, RightPanelSection),
    ) -> (Self, Vec<(&'static str, String)>) {
        let (views, outcome) = workspace_views::load(&path, unix_milliseconds());
        let mut diagnostics = Vec::new();
        let frozen = match outcome {
            workspace_views::LoadOutcome::Missing => false,
            workspace_views::LoadOutcome::Loaded {
                migrated_from,
                repairs,
            } => {
                if let Some(version) = migrated_from {
                    diagnostics.push((
                        "workspace_views.migrated",
                        format!(
                            "Workspace view state was migrated from schema {version} to {}; each Workspace's View tabs are one area",
                            workspace_views::SCHEMA_VERSION
                        ),
                    ));
                }
                if !repairs.is_empty() {
                    diagnostics.push((
                        "workspace_views.repaired",
                        format!(
                            "Workspace view state was repaired on load: {}",
                            repairs.join("; ")
                        ),
                    ));
                }
                false
            }
            workspace_views::LoadOutcome::Unreadable {
                reason,
                preserved_as,
            } => match preserved_as {
                Some(preserved) => {
                    diagnostics.push((
                        "workspace_views.unreadable",
                        format!(
                            "Workspace view state could not be read ({reason}); the original was kept at {} and defaults were loaded",
                            preserved.display()
                        ),
                    ));
                    false
                }
                None => {
                    diagnostics.push((
                        "workspace_views.unreadable",
                        format!(
                            "Workspace view state could not be read ({reason}) and could not be moved aside; defaults are used and nothing will be written over it"
                        ),
                    ));
                    true
                }
            },
        };
        let resumable = views
            .workspaces
            .iter()
            .filter(|view| view.last_used_unix_ms > 0)
            .max_by_key(|view| view.last_used_unix_ms)
            .map(|view| (view.device_id.clone(), view.path.clone()));
        (
            Self {
                path,
                views,
                live: HashSet::new(),
                front: None,
                generation: 0,
                reconciled: None,
                derived_active: None,
                split_requests: HashMap::new(),
                unshown: HashSet::new(),
                resumable,
                pending_choice: None,
                saved_panel,
                frozen,
                save_pending: false,
                save_active: false,
                save_worker: None,
            },
            diagnostics,
        )
    }
}

impl Runtime {
    pub(super) fn separate_view_areas(&self) -> bool {
        self.workspace_views.is_some()
    }

    /// The device and path of a catalog checkout.
    pub(super) fn workspace_key(
        &self,
        workspace_id: &str,
        checkout_id: &str,
    ) -> Option<WorkspaceKey> {
        self.catalog_checkout(workspace_id, checkout_id)
            .map(|(workspace, checkout)| (workspace.device_id.clone(), checkout.path.clone()))
    }

    pub(super) fn front_workspace_key(&self) -> Option<WorkspaceKey> {
        let (workspace_id, checkout_id) = self.front_checkout()?;
        self.workspace_key(workspace_id, checkout_id)
    }

    /// The terminal took the surface. The Swift shell's document gives it
    /// back; with separate View areas the documents stay where they are.
    pub(super) fn yield_surface_to_terminal(&mut self) {
        if !self.separate_view_areas() {
            self.deactivate_editor_tab();
        }
    }

    /// The operator chose the Workspace in front: it is the one a reload or a
    /// restart opens on (D-11). A front that only Herdr's own focus moved is
    /// not, so a first run the operator spent on Main starts there again.
    pub(super) fn mark_front_chosen(&mut self) {
        if let Some(key) = self.front_workspace_key() {
            self.mark_workspace_chosen(&key);
        }
    }

    pub(super) fn mark_workspace_chosen(&mut self, key: &WorkspaceKey) {
        let Some(store) = self.workspace_views.as_mut() else {
            return;
        };
        store.pending_choice = None;
        // A click on a pane of the Workspace already chosen writes nothing.
        if store.resumable.as_ref() == Some(key) {
            return;
        }
        store.resumable = Some(key.clone());
        store.views.entry(&key.0, &key.1).last_used_unix_ms = unix_milliseconds();
        self.persist_workspace_views();
    }

    /// A device request that chooses a Workspace: the device's Herdr moves
    /// its front only when it accepts, so the choice is remembered when the
    /// sync sees the front land there, and a refusal remembers nothing.
    pub(super) fn choose_when_in_front(
        &mut self,
        device_id: &str,
        request_id: &str,
        key: WorkspaceKey,
    ) {
        if let Some(store) = self.workspace_views.as_mut() {
            store.pending_choice = Some(PendingChoice {
                device_id: device_id.to_owned(),
                request_id: request_id.to_owned(),
                key,
            });
        }
    }

    /// The device refused the request, or its answer was lost: a later move
    /// Herdr makes there on its own is not the operator's choice.
    pub(super) fn drop_pending_choice(&mut self, device_id: &str, request_id: &str) {
        if let Some(store) = self.workspace_views.as_mut()
            && store.pending_choice.as_ref().is_some_and(|pending| {
                pending.device_id == device_id && pending.request_id == request_id
            })
        {
            store.pending_choice = None;
        }
    }

    /// Applies what the event that just ran asks of the front Workspace.
    pub(super) fn apply_area_intent(&mut self, intent: AreaIntent) {
        if let Some(key) = self.front_workspace_key() {
            self.apply_area_intent_to(&key, intent);
        }
    }

    /// Applies an area intent to one Workspace, in front or about to be.
    pub(super) fn apply_area_intent_to(&mut self, key: &WorkspaceKey, intent: AreaIntent) {
        let Some(store) = self.workspace_views.as_mut() else {
            return;
        };
        let entry = store.views.entry(&key.0, &key.1);
        let before = (entry.mode, entry.explorer);
        match intent {
            AreaIntent::Views | AreaIntent::RevealInViews => {
                if entry.mode == ViewMode::Agents {
                    entry.mode = ViewMode::Together;
                }
                if intent == AreaIntent::RevealInViews {
                    entry.explorer = true;
                }
            }
            AreaIntent::Agents => {
                if entry.mode == ViewMode::Views {
                    entry.mode = ViewMode::Together;
                }
            }
        }
        if before != (entry.mode, entry.explorer) {
            self.persist_workspace_views();
        }
    }

    pub(super) fn apply_workspace_view(&mut self, payload: WorkspaceViewPayload) -> bool {
        if !self.separate_view_areas() {
            self.set_error(
                "workspace_view.unsupported",
                "This shell does not draw separate Agent and View areas",
                false,
            );
            return true;
        }
        let mode = match payload.mode.as_deref() {
            None => None,
            Some(value) => match ViewMode::parse(value) {
                Some(mode) => Some(mode),
                None => {
                    self.set_error(
                        "workspace_view.unknown_mode",
                        format!("{value} is not a layout; expected agents, together or views"),
                        false,
                    );
                    return true;
                }
            },
        };
        let Some(key) = self.front_workspace_key() else {
            self.set_error(
                "workspace_view.no_workspace",
                "No Workspace is open, so there is no layout to change",
                false,
            );
            return true;
        };
        let root = key.1.trim_end_matches('/');
        if let Some(path) = payload.reveal.as_deref()
            && crate::files::path_inside_root(Path::new(root), Path::new(path), false).is_err()
        {
            self.set_error(
                "workspace_view.reveal_outside",
                format!("{path} is not in the Workspace at {root}; nothing was revealed"),
                false,
            );
            return true;
        }
        let store = self.workspace_views.as_mut().expect("checked above");
        let entry = store.views.entry(&key.0, &key.1);
        let before = entry.clone();
        if let Some(mode) = mode {
            entry.mode = mode;
        }
        if let Some(explorer) = payload.explorer {
            entry.explorer = explorer;
        }
        if let Some(changes) = payload.changes {
            entry.changes = changes;
        }
        if let Some(share) = payload.agent_share {
            entry.agent_share = workspace_views::clamp_agent_share(share);
        }
        let revealed = payload
            .reveal
            .is_some_and(|path| self.unfold_to(&key, &path));
        let entry_changed = self
            .workspace_views
            .as_ref()
            .and_then(|store| store.views.get(&key.0, &key.1))
            != Some(&before);
        if !entry_changed && !revealed {
            return false;
        }
        if entry_changed {
            self.persist_workspace_views();
        }
        self.sync_workspace_view();
        true
    }

    /// Unfolds the Explorer folders down to `path`, a file the caller found
    /// inside the Workspace `key`; the shell selects the row.
    fn unfold_to(&mut self, key: &WorkspaceKey, path: &str) -> bool {
        let root = key.1.trim_end_matches('/');
        let folders = reveal_expansion_paths(root, path, false);
        let expanded = self.expanded_paths_on(&key.0);
        let mut unfolded = false;
        for folder in folders {
            if !expanded.contains(&folder) {
                expanded.push(folder);
                unfolded = true;
            }
        }
        if unfolded {
            self.persist_current_ui_state();
        }
        unfolded
    }

    /// Brings the snapshot, the editor and the file in line with the front
    /// Workspace. It runs after every event that can move the screen and
    /// before every snapshot read, so a front moved by Herdr or by a device's
    /// own focus is followed too; terminal input and output and a document's
    /// keystrokes skip the pass after their event, so a keystroke costs the
    /// read's pass alone. With nothing changed it costs a catalog lookup of
    /// the front key, one comparison of the editor's tabs, a copy of the
    /// published scalars, and the front Workspace's tree built again: one map
    /// of the editor's tabs and one root check, then a map lookup per display,
    /// of which there are at most 64.
    pub(super) fn sync_workspace_view(&mut self) {
        let Some(store) = self.workspace_views.as_ref() else {
            return;
        };
        let front = self.front_workspace_key();
        if front != store.front {
            self.workspace_views.as_mut().expect("checked above").front = front.clone();
        }
        if let Some(key) = front.as_ref()
            && self
                .workspace_views
                .as_ref()
                .and_then(|store| store.pending_choice.as_ref())
                .is_some_and(|pending| &pending.key == key)
        {
            self.mark_workspace_chosen(key);
        }
        self.restore_front_when_ready();
        self.reconcile_view_displays();
        self.publish_workspace_view(front.as_ref());
    }

    /// Reads the front Workspace's displays back the first time in this
    /// process that they can be read (contract 5). The tree is published
    /// before that, its displays `waiting`; a display binds when its document
    /// lands, so a Workspace whose files cannot be reached yet keeps what it
    /// saved. On this machine that waits for the daemon to open the
    /// checkout's root: a read before it is refused, and would mark every
    /// restored file unavailable. Returns whether a restore began.
    pub(super) fn restore_front_when_ready(&mut self) -> bool {
        let Some(store) = self.workspace_views.as_ref() else {
            return false;
        };
        // The restore opens into the live front, so it runs only while that is
        // the Workspace the last sync saw; a front that moved since waits for
        // the sync, which records it first.
        let Some(key) = store.front.clone() else {
            return false;
        };
        if store.live.contains(&key)
            || self.front_workspace_key().as_ref() != Some(&key)
            || !self.view_root_ready(&key)
        {
            return false;
        }
        let store = self.workspace_views.as_mut().expect("checked above");
        store.live.insert(key.clone());
        // A live Workspace has an entry, so the reconcile gives a document
        // opened into it by any path a display.
        store.views.entry(&key.0, &key.1);
        store.generation += 1;
        self.restore_view_displays(&key);
        true
    }

    /// Whether a file of this Workspace can be read now. A device's files
    /// are read through its helper, so its Workspace waits for the helper
    /// rather than marking every file unavailable while it starts, and for
    /// its catalog, which moves the device's checkouts into their Projects: a
    /// display restored before that names a Workspace the device no longer
    /// shows. The helper and the catalog each ask again once they are ready.
    /// Only the daemon keeps separate View areas, and it pins roots after its
    /// first snapshot read: a read before that would go around the pinned
    /// root, so a local Workspace waits for it.
    fn view_root_ready(&self, key: &WorkspaceKey) -> bool {
        self.view_root_wait(key).is_none()
    }

    fn publish_workspace_view(&mut self, front: Option<&WorkspaceKey>) {
        let Some(store) = self.workspace_views.as_ref() else {
            return;
        };
        // This runs on every snapshot read: the scalars are copied and the
        // tree is built again (`view_layout_snapshot`).
        let fresh;
        let view = match front {
            Some((device, path)) => Some(match store.views.get(device, path) {
                Some(view) => view,
                None => {
                    fresh = WorkspaceView::new(device, path);
                    &fresh
                }
            }),
            None => None,
        };
        let published = front.zip(view).map(|(key, view)| WorkspaceViewSnapshot {
            device_id: view.device_id.clone(),
            path: view.path.clone(),
            mode: view.mode,
            explorer: view.explorer,
            changes: view.changes,
            agent_share: view.agent_share,
            resumed: Some(key) == store.resumable.as_ref(),
            layout: self.view_layout_snapshot(key, &view.layout),
        });
        self.snapshot.workspace_view = published;
        // The one global panel every existing reader gates on (the Changes
        // reader, the device Explorer watch) follows the front Workspace's
        // tools, so those readers keep one owner (A5). The Explorer wins the
        // section while both show, because its decorations need Changes too.
        if let Some(view) = self.snapshot.workspace_view.as_ref() {
            let (explorer, changes) = (view.explorer, view.changes);
            self.snapshot.ui_state.right_panel_visible = explorer || changes;
            if explorer {
                self.snapshot.ui_state.right_panel_section = RightPanelSection::Explorer;
            } else if changes {
                self.snapshot.ui_state.right_panel_section = RightPanelSection::Changes;
            }
        }
    }

    /// A View display's file that could not be read back after a restart,
    /// or retried: its tab says why, and a retry that fails again says why
    /// this time. A tab that holds the file is left as it is.
    pub(super) fn insert_unavailable_file_tab(
        &mut self,
        workspace_id: &str,
        checkout_id: &str,
        path: &str,
        preview: bool,
        reason: String,
    ) {
        let tab_id = self.new_file_tab_id(workspace_id, checkout_id, path);
        self.push_diagnostic(
            "workspace_views.tab_unavailable",
            format!("A View file could not be read: {reason}"),
        );
        if let Some(tab) = self
            .snapshot
            .editor
            .tabs
            .iter_mut()
            .find(|tab| tab.id == tab_id)
        {
            if tab.unavailable_reason.is_some() {
                tab.unavailable_reason = Some(reason);
            }
            return;
        }
        self.place_editor_tab(EditorTabSnapshot {
            id: tab_id,
            workspace_id: workspace_id.to_owned(),
            checkout_id: checkout_id.to_owned(),
            path: path.to_owned(),
            label: file_label(path),
            kind: EditorTabKind::File,
            diff_committed: None,
            markdown_live: true,
            wrap: false,
            dirty: false,
            preview,
            unavailable_reason: Some(reason),
        });
    }

    /// Queues a write of the Workspace view file on its own worker, outside
    /// the runtime mutex; one pending flag coalesces newer state.
    pub(super) fn persist_workspace_views(&mut self) {
        let Some(store) = self.workspace_views.as_mut() else {
            return;
        };
        if store.frozen {
            return;
        }
        let Some(context) = self.worker_context.clone() else {
            if let Err(message) = workspace_views::save(&store.path, &store.views) {
                self.set_error("workspace_views.save_failed", message, true);
            }
            return;
        };
        store.save_pending = true;
        if store.save_active {
            return;
        }
        store.save_active = true;
        let spawned = thread::Builder::new()
            .name("hide-workspace-views-save".into())
            .spawn(move || {
                let Some(runtime) = context.runtime.upgrade() else {
                    return;
                };
                loop {
                    let (path, views) = {
                        let mut guard = runtime.lock().unwrap_or_else(|e| e.into_inner());
                        let Some(store) = guard.workspace_views.as_mut() else {
                            return;
                        };
                        if !store.save_pending {
                            store.save_active = false;
                            return;
                        }
                        store.save_pending = false;
                        (store.path.clone(), store.views.clone())
                    };
                    if let Err(message) = workspace_views::save(&path, &views) {
                        runtime.lock().unwrap_or_else(|e| e.into_inner()).set_error(
                            "workspace_views.save_failed",
                            message,
                            true,
                        );
                        context.notifier.notify();
                    }
                }
            });
        let store = self.workspace_views.as_mut().expect("checked above");
        match spawned {
            // A finished worker's handle is replaced; the one that may still
            // be writing is joined when the core is dropped.
            Ok(worker) => store.save_worker = Some(worker),
            Err(error) => {
                store.save_active = false;
                self.set_error(
                    "workspace_views.save_failed",
                    format!("Workspace view state save worker could not start: {error}"),
                    true,
                );
            }
        }
    }

    /// The UI state the settings file takes: the snapshot's, with the right
    /// panel the file already held while the panel is a projection.
    pub(super) fn ui_state_to_save(&self) -> UiStateSnapshot {
        let mut state = self.snapshot.ui_state.clone();
        if let Some(store) = self.workspace_views.as_ref() {
            (state.right_panel_visible, state.right_panel_section) = store.saved_panel;
        }
        state
    }

    pub(crate) fn take_workspace_views_save_worker(&mut self) -> Option<thread::JoinHandle<()>> {
        self.workspace_views.as_mut()?.save_worker.take()
    }

    /// Forgets what Hide remembered about a removed device's Workspaces.
    pub(super) fn forget_device_views(&mut self, device_id: &str) {
        let Some(store) = self.workspace_views.as_mut() else {
            return;
        };
        let before = store.views.workspaces.len();
        store
            .views
            .workspaces
            .retain(|view| view.device_id != device_id);
        store.live.retain(|(device, _)| device != device_id);
        store
            .split_requests
            .retain(|(device, _), _| device != device_id);
        store.generation += 1;
        if store
            .pending_choice
            .as_ref()
            .is_some_and(|pending| pending.device_id == device_id)
        {
            store.pending_choice = None;
        }
        if store
            .front
            .as_ref()
            .is_some_and(|(device, _)| device == device_id)
        {
            store.front = None;
        }
        if store
            .resumable
            .as_ref()
            .is_some_and(|(device, _)| device == device_id)
        {
            store.resumable = None;
        }
        if store.views.workspaces.len() != before {
            self.persist_workspace_views();
        }
    }
}

pub(super) fn file_label(path: &str) -> String {
    Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or(path)
        .to_owned()
}
