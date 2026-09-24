//! The runtime side of per-Workspace presentation (PRD S6 D-04, D-05, D-08,
//! D-10): which areas the front Workspace shows, its tools, its View tabs and
//! the editor's active tab kept on that Workspace's View tab.
//!
//! Everything here is inert unless the shell passed
//! `CoreOptions::workspace_views_path`; the Swift shell keeps the rule that a
//! document takes the terminal canvas and a terminal takes it back.

use serde::Deserialize;

use super::*;
use crate::model::WorkspaceViewSnapshot;
use crate::workspace_views::{
    self, ViewMode, ViewTabKind, ViewTabRecord, WorkspaceView, WorkspaceViews,
};

/// A Workspace's identity: the device and the checkout path.
pub(super) type WorkspaceKey = (String, String);

pub(super) struct WorkspaceViewStore {
    path: PathBuf,
    views: WorkspaceViews,
    /// Workspaces whose remembered tabs were restored in this process. Their
    /// tab list follows the editor from then on; any other Workspace keeps the
    /// list the file carried until it is shown.
    live: HashSet<WorkspaceKey>,
    /// The Workspace the last sync saw in front.
    front: Option<WorkspaceKey>,
    /// The editor state the last sync recorded, so an unchanged editor costs
    /// one comparison rather than a pass over the catalog.
    recorded: Option<(Vec<EditorTabSnapshot>, Option<String>)>,
    /// Set when an unreadable file could not be moved aside: nothing is ever
    /// written over it.
    frozen: bool,
    save_pending: bool,
    save_active: bool,
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
            Event::FileOpen(_) | Event::FileFocus(_) | Event::ChangesSelect(_) => Some(Self::Views),
            Event::RevealPath(_) => Some(Self::RevealInViews),
            Event::FocusPane(_) | Event::FocusTab(_) => Some(Self::Agents),
            Event::RemoteControl(payload) => match payload.request {
                RemoteControlRequest::FocusPane { .. } | RemoteControlRequest::FocusTab { .. } => {
                    Some(Self::Agents)
                }
                _ => None,
            },
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
}

impl WorkspaceViewStore {
    pub(super) fn open(path: PathBuf) -> (Self, Option<(&'static str, String)>) {
        let (views, outcome) = workspace_views::load(&path, unix_milliseconds());
        let (frozen, diagnostic) = match outcome {
            workspace_views::LoadOutcome::Loaded | workspace_views::LoadOutcome::Missing => {
                (false, None)
            }
            workspace_views::LoadOutcome::Unreadable {
                reason,
                preserved_as,
            } => match preserved_as {
                Some(preserved) => (
                    false,
                    Some((
                        "workspace_views.unreadable",
                        format!(
                            "Workspace view state could not be read ({reason}); the original was kept at {} and defaults were loaded",
                            preserved.display()
                        ),
                    )),
                ),
                None => (
                    true,
                    Some((
                        "workspace_views.unreadable",
                        format!(
                            "Workspace view state could not be read ({reason}) and could not be moved aside; defaults are used and nothing will be written over it"
                        ),
                    )),
                ),
            },
        };
        (
            Self {
                path,
                views,
                live: HashSet::new(),
                front: None,
                recorded: None,
                frozen,
                save_pending: false,
                save_active: false,
            },
            diagnostic,
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

    fn front_workspace_key(&self) -> Option<WorkspaceKey> {
        let (workspace_id, checkout_id) = self.front_checkout()?;
        self.workspace_key(workspace_id, checkout_id)
    }

    /// The terminal took the surface. The Swift shell's document gives it
    /// back; with separate View areas the document stays, and only moves to
    /// the front Workspace's own View tab when the Workspace changed.
    pub(super) fn yield_surface_to_terminal(&mut self) {
        if self.separate_view_areas() {
            self.align_editor_with_front();
        } else {
            self.deactivate_editor_tab();
        }
    }

    /// Keeps the editor's active tab on the front Workspace's View tab: the
    /// one it last showed when that tab is open, else its newest open View
    /// tab, else none.
    fn align_editor_with_front(&mut self) {
        let Some((workspace_id, checkout_id)) = self.front_checkout_owned() else {
            return;
        };
        let in_front = |tab: &EditorTabSnapshot| {
            tab.workspace_id == workspace_id
                && tab.checkout_id == checkout_id
                && matches!(tab.kind, EditorTabKind::File | EditorTabKind::Diff)
        };
        let active = self.snapshot.editor.active_tab_id.as_deref();
        if self
            .snapshot
            .editor
            .tabs
            .iter()
            .any(|tab| Some(tab.id.as_str()) == active && in_front(tab))
        {
            return;
        }
        let remembered =
            self.workspace_key(&workspace_id, &checkout_id)
                .and_then(|(device, path)| {
                    self.workspace_views
                        .as_ref()?
                        .views
                        .get(&device, &path)?
                        .active
                        .clone()
                });
        let remembered_tab = remembered.as_ref().and_then(|record| {
            self.snapshot
                .editor
                .tabs
                .iter()
                .find(|tab| in_front(tab) && record_of(tab).same_target(record))
                .map(|tab| tab.id.clone())
        });
        // The remembered tab is still being read back after a restart; the
        // screen waits for it rather than showing a neighbour for a moment.
        let remembered_reading = remembered_tab.is_none()
            && remembered.as_ref().is_some_and(|record| {
                record.kind == ViewTabKind::File
                    && self.document_opens_path(&workspace_id, &checkout_id, &record.path)
            });
        let target = remembered_tab.or_else(|| {
            if remembered_reading {
                return None;
            }
            self.snapshot
                .editor
                .tabs
                .iter()
                .rev()
                .find(|tab| in_front(tab))
                .map(|tab| tab.id.clone())
        });
        match target {
            Some(tab_id) => {
                if let Err(message) = self.activate_editor_tab(&tab_id) {
                    self.set_error("editor.focus_failed", message, false);
                }
            }
            None => self.deactivate_editor_tab(),
        }
    }

    /// Applies what the event that just ran asks of the areas.
    pub(super) fn apply_area_intent(&mut self, intent: AreaIntent) {
        let Some(key) = self.front_workspace_key() else {
            return;
        };
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
        if *entry == before {
            return false;
        }
        self.persist_workspace_views();
        self.sync_workspace_view();
        true
    }

    /// Brings the snapshot, the editor and the file in line with the front
    /// Workspace. It runs after every event that can move the screen and
    /// before every snapshot read, so a front moved by Herdr or by a device's
    /// own focus is followed too. With nothing changed it costs a key lookup
    /// and one comparison of the editor's tabs.
    pub(super) fn sync_workspace_view(&mut self) {
        let Some(store) = self.workspace_views.as_ref() else {
            return;
        };
        let front = self.front_workspace_key();
        if front != store.front {
            let store = self.workspace_views.as_mut().expect("checked above");
            store.front = front.clone();
            let restore = front
                .as_ref()
                .is_some_and(|key| store.live.insert(key.clone()));
            if let Some(key) = front.as_ref() {
                store.views.entry(&key.0, &key.1).last_used_unix_ms = unix_milliseconds();
            }
            if restore && let Some(key) = front.clone() {
                self.restore_view_tabs(&key);
            }
            self.persist_workspace_views();
        }
        if front.is_some() {
            self.align_editor_with_front();
        }
        self.record_view_tabs();
        self.publish_workspace_view(front.as_ref());
    }

    fn publish_workspace_view(&mut self, front: Option<&WorkspaceKey>) {
        let Some(store) = self.workspace_views.as_ref() else {
            return;
        };
        let view = front.map(|(device, path)| {
            store
                .views
                .get(device, path)
                .cloned()
                .unwrap_or_else(|| WorkspaceView::new(device, path))
        });
        self.snapshot.workspace_view = view.as_ref().map(|view| WorkspaceViewSnapshot {
            device_id: view.device_id.clone(),
            path: view.path.clone(),
            mode: view.mode,
            explorer: view.explorer,
            changes: view.changes,
            agent_share: view.agent_share,
        });
        // The one global panel every existing reader gates on (the Changes
        // reader, the device Explorer watch) follows the front Workspace's
        // tools, so those readers keep one owner (A5). The Explorer wins the
        // section while both show, because its decorations need Changes too.
        if let Some(view) = view {
            self.snapshot.ui_state.right_panel_visible = view.explorer || view.changes;
            if view.explorer {
                self.snapshot.ui_state.right_panel_section = RightPanelSection::Explorer;
            } else if view.changes {
                self.snapshot.ui_state.right_panel_section = RightPanelSection::Changes;
            }
        }
    }

    /// Records the open View tabs and the active one of every Workspace
    /// restored in this process, in strip order, when the editor changed.
    fn record_view_tabs(&mut self) {
        let Some(store) = self.workspace_views.as_ref() else {
            return;
        };
        if store.recorded.as_ref().is_some_and(|(tabs, active)| {
            *tabs == self.snapshot.editor.tabs && *active == self.snapshot.editor.active_tab_id
        }) {
            return;
        }
        let current = (
            self.snapshot.editor.tabs.clone(),
            self.snapshot.editor.active_tab_id.clone(),
        );
        let live = store.live.clone();
        let mut lists: HashMap<WorkspaceKey, Vec<ViewTabRecord>> = HashMap::new();
        let mut active: Option<(WorkspaceKey, ViewTabRecord)> = None;
        // A Workspace the catalog cannot place right now (a device that is
        // reconnecting) keeps what it had rather than recording no tabs.
        let mut placed: HashSet<WorkspaceKey> = HashSet::new();
        for workspace in self.catalog_workspaces() {
            for checkout in &workspace.checkouts {
                placed.insert((workspace.device_id.clone(), checkout.path.clone()));
            }
        }
        for tab in &self.snapshot.editor.tabs {
            if !matches!(tab.kind, EditorTabKind::File | EditorTabKind::Diff) {
                continue;
            }
            let Some(key) = self.workspace_key(&tab.workspace_id, &tab.checkout_id) else {
                continue;
            };
            let record = record_of(tab);
            if self.snapshot.editor.active_tab_id.as_deref() == Some(tab.id.as_str()) {
                active = Some((key.clone(), record.clone()));
            }
            lists.entry(key).or_default().push(record);
        }
        let store = self.workspace_views.as_mut().expect("checked above");
        let mut changed = false;
        for key in live.iter().filter(|key| placed.contains(*key)) {
            let mut tabs = lists.remove(key).unwrap_or_default();
            let overflow = tabs.len().saturating_sub(workspace_views::MAX_VIEW_TABS);
            tabs.drain(..overflow);
            let entry = store.views.entry(&key.0, &key.1);
            if entry.tabs != tabs {
                entry.tabs = tabs;
                changed = true;
            }
            let still_open = entry
                .active
                .as_ref()
                .is_some_and(|record| entry.tabs.iter().any(|tab| tab.same_target(record)));
            if !still_open && entry.active.is_some() {
                entry.active = None;
                changed = true;
            }
        }
        if let Some((key, record)) = active
            && live.contains(&key)
        {
            let entry = store.views.entry(&key.0, &key.1);
            if entry.active.as_ref() != Some(&record) {
                entry.active = Some(record);
                changed = true;
            }
        }
        store.recorded = Some(current);
        if changed {
            self.persist_workspace_views();
        }
    }

    /// Opens the View tabs a Workspace had when Hide last ran, the first time
    /// it comes to the front in this process. Files are read on the document
    /// worker like any open; a file that cannot be read becomes a tab that
    /// says why and offers Close (B19, B20). Nothing is started in Herdr.
    fn restore_view_tabs(&mut self, key: &WorkspaceKey) {
        let Some((workspace_id, checkout_id)) = self.front_checkout_owned() else {
            return;
        };
        let Some(entry) = self
            .workspace_views
            .as_ref()
            .and_then(|store| store.views.get(&key.0, &key.1))
            .cloned()
        else {
            return;
        };
        if entry.tabs.is_empty() {
            return;
        }
        let mut restored = 0usize;
        for record in &entry.tabs {
            let open = self.snapshot.editor.tabs.iter().any(|tab| {
                tab.workspace_id == workspace_id
                    && tab.checkout_id == checkout_id
                    && record_of(tab).same_target(record)
            });
            if open {
                continue;
            }
            restored += 1;
            let is_active = entry
                .active
                .as_ref()
                .is_some_and(|active| active.same_target(record));
            match record.kind {
                ViewTabKind::Diff => {
                    self.insert_diff_tab(
                        &workspace_id,
                        &checkout_id,
                        &record.path,
                        record.committed.unwrap_or(false),
                        record.preview,
                    );
                }
                ViewTabKind::File => match self.document_source(&workspace_id, &checkout_id) {
                    Ok((root, channel)) => self.start_document_open(
                        root,
                        channel,
                        documents::OpenRequestFields {
                            workspace_id: workspace_id.clone(),
                            checkout_id: checkout_id.clone(),
                            path: record.path.clone(),
                            preview: record.preview,
                            reload: false,
                            reveal: None,
                            restore: Some(is_active),
                        },
                    ),
                    Err(message) => self.insert_unavailable_file_tab(
                        &workspace_id,
                        &checkout_id,
                        &record.path,
                        record.preview,
                        message,
                    ),
                },
            }
        }
        crate::diagnostic!(serde_json::json!({
            "component": "workspace_views",
            "kind": "workspace_views.restore",
            "device": key.0,
            "tabs": restored,
        }));
    }

    /// A View tab whose file could not be read back after a restart.
    pub(super) fn insert_unavailable_file_tab(
        &mut self,
        workspace_id: &str,
        checkout_id: &str,
        path: &str,
        preview: bool,
        reason: String,
    ) {
        let tab_id = self.new_file_tab_id(workspace_id, checkout_id, path);
        if self.snapshot.editor.tabs.iter().any(|tab| tab.id == tab_id) {
            return;
        }
        self.push_diagnostic(
            "workspace_views.tab_unavailable",
            format!("A restored View tab could not be read: {reason}"),
        );
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
        if let Err(error) = spawned {
            let store = self.workspace_views.as_mut().expect("checked above");
            store.save_active = false;
            self.set_error(
                "workspace_views.save_failed",
                format!("Workspace view state save worker could not start: {error}"),
                true,
            );
        }
    }
}

fn record_of(tab: &EditorTabSnapshot) -> ViewTabRecord {
    ViewTabRecord {
        path: tab.path.clone(),
        kind: if tab.kind == EditorTabKind::Diff {
            ViewTabKind::Diff
        } else {
            ViewTabKind::File
        },
        committed: tab.diff_committed,
        preview: tab.preview,
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
