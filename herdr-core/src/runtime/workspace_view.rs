//! The runtime side of per-Workspace presentation (PRD S6 D-04, D-05, D-08,
//! D-10; S7; PRD three-column-panel D-05, D-08, D-10): whether the front
//! Workspace's File Views and Tools columns are on, their tool and widths,
//! its View area tree, when that tree's documents are read back, and the
//! file that keeps it all. What the areas hold and how an open lands in them is
//! `view_areas.rs`.
//!
//! Everything here is inert unless the shell passed
//! `CoreOptions::workspace_views_path`; without it the rule is that a
//! document takes the terminal canvas and a terminal takes it back.

use std::collections::VecDeque;

use serde::Deserialize;

use super::view_areas::Reconciled;
use super::*;
use crate::model::{BrowserAreaScopeRow, BrowserViewInventoryRow, WorkspaceViewSnapshot};
use crate::view_layout::DisplayKind;
use crate::workspace_views::{self, Tool, WorkspaceView, WorkspaceViews};

/// A Workspace's identity: the device and the checkout path.
pub(super) type WorkspaceKey = (String, String);

struct PendingChoice {
    device_id: String,
    request_id: String,
    key: WorkspaceKey,
}

pub(super) struct WorkspaceViewStore {
    pub(super) path: PathBuf,
    pub(super) views: WorkspaceViews,
    /// Workspaces whose displays were read back in this process. A document
    /// of one that no display shows gets a display; any other Workspace keeps
    /// the tree the file carried until it is shown.
    pub(super) live: HashSet<WorkspaceKey>,
    pub(super) agent_live: HashSet<WorkspaceKey>,
    /// What the bookmark pass saw of each Workspace it has had in front
    /// (`view_bookmarks.rs`). Runtime only: a Workspace not seen yet counts
    /// every shown tab as newly shown.
    pub(super) bookmark_seen: HashMap<WorkspaceKey, super::view_bookmarks::Seen>,
    pub(super) agent_admissions: BTreeMap<WorkspaceKey, HashSet<String>>,
    pub(super) agent_placements: BTreeMap<String, (WorkspaceKey, String, Option<usize>)>,
    /// The Workspace the last sync saw in front.
    pub(super) front: Option<WorkspaceKey>,
    /// One bounded acknowledgement slot for the last column-width intent.
    /// It is published only for its Workspace and never saved.
    width_request: Option<(WorkspaceKey, String)>,
    /// Counts the layout changes made outside the reconcile, so an unchanged
    /// state costs the reconcile one comparison.
    pub(super) generation: u64,
    /// The live catalog identities behind the browser inventory, at most
    /// MAX_WORKSPACES saved layouts. A removed or disconnected checkout
    /// keeps its saved layout but cannot keep a native page eligible.
    browser_inventory_scope: Vec<WorkspaceKey>,
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
    /// The last load stamp a browser display was given (`next_browser_load`).
    /// Runtime only.
    pub(super) browser_load: u64,
    /// Numbers the area intents that called File Views, whichever page, CLI
    /// `--reveal` or event asked, and remembers the number of each
    /// Workspace's last call. The shell draws a called column in a body too
    /// narrow for all of them (PRD three-column-panel D-07, B26, B27), and a
    /// call that did not start in that page reaches it only as the front
    /// Workspace's number passing the last one it read, so a call into
    /// another Workspace never moves the front one's columns. Runtime only:
    /// which column a narrow body shows is never stored (B28).
    pub(super) views_calls: u64,
    pub(super) views_called: HashMap<WorkspaceKey, u64>,
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

/// What an event asks of a Workspace's columns once it has moved the screen
/// (D-03, D-08): a document, diff or page opened or focused turns the front
/// Workspace's File Views on and numbers that call, and so does a file a
/// terminal link names, applied to its own checkout's Workspace when it
/// settles (`settle_reveal_path`), leaving Tools as it was (B4). A folder a
/// link names opens nothing in File Views, so it turns only Tools on with the
/// Explorer and is no File Views call. The Explorer's own reveal (B5) is a
/// `workspace_view` `reveal`, not an intent.
/// Choosing an agent or a tab changes no column (D-17); nothing else moves
/// a column on its own.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum AreaIntent {
    Views,
    /// A folder a link names: Tools on, on the Explorer, and File Views as it was.
    RevealFolder,
}

impl AreaIntent {
    pub(super) fn of(event: &Event) -> Option<Self> {
        match event {
            Event::FileOpen(_) | Event::FileFocus(_) => Some(Self::Views),
            // A deselect opens nothing.
            Event::ChangesSelect(payload) if payload.path.is_some() => Some(Self::Views),
            _ => None,
        }
    }
}

/// The payload of `workspace_view`: any subset of the front Workspace's
/// presentation, each field the value it should end at, so the same event
/// sent twice lands where it did once (D-05, B13). Absent fields keep their
/// value.
#[derive(Debug, Deserialize)]
pub(super) struct WorkspaceViewPayload {
    /// Whether the File Views column is on. On with no view open, it opens
    /// one New tab page in the same event (D-10).
    #[serde(default)]
    pub(super) views: Option<bool>,
    /// Whether the Tools column is on.
    #[serde(default)]
    pub(super) tools: Option<bool>,
    /// `explorer` or `changes` (History): the Tools column's one tool.
    /// Choosing one turns the column on unless `tools` says otherwise.
    #[serde(default)]
    pub(super) tool: Option<String>,
    /// The File Views column's width, in CSS pixels.
    #[serde(default)]
    pub(super) views_width: Option<u32>,
    /// The Tools column's width, in CSS pixels.
    #[serde(default)]
    pub(super) tools_width: Option<u32>,
    /// An optional opaque identity for a width intent, at most 128 bytes.
    #[serde(default)]
    pub(super) width_request_id: Option<String>,
    /// A file of the front Workspace to reveal: Tools turns on with the
    /// Explorer and the file's folders unfolded, in the same event, and
    /// nothing is opened; File Views stays as it is (B5).
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
                upgraded,
            } => {
                if upgraded > 0 {
                    diagnostics.push((
                        "workspace_views.columns_migrated",
                        format!(
                            "{upgraded} Workspaces were stored with the side panel or an older layout; each restarts with File Views on where its views showed, Tools and its tool as they were, and both column widths at their defaults"
                        ),
                    ));
                }
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
                agent_live: HashSet::new(),
                bookmark_seen: HashMap::new(),
                agent_admissions: BTreeMap::new(),
                agent_placements: BTreeMap::new(),
                front: None,
                width_request: None,
                generation: 0,
                browser_inventory_scope: Vec::new(),
                reconciled: None,
                derived_active: None,
                split_requests: HashMap::new(),
                browser_load: 0,
                views_calls: 0,
                views_called: HashMap::new(),
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

    /// The terminal took the surface. Without View areas the document gives
    /// it back; with separate View areas the documents stay where they are.
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
        if intent == AreaIntent::Views {
            store.views_calls += 1;
            store.views_called.insert(key.clone(), store.views_calls);
        }
        let entry = store.views.entry(&key.0, &key.1);
        let before = (entry.views, entry.tool, entry.tools);
        match intent {
            AreaIntent::Views => entry.views = true,
            AreaIntent::RevealFolder => {
                entry.tool = Tool::Explorer;
                entry.tools = true;
            }
        }
        let changed = before != (entry.views, entry.tool, entry.tools);
        // A new Workspace can evict the store's least recently used entry.
        // Call history has the same bound; prune only when crossing it, not
        // on repeated calls or any terminal input/output path.
        if store.views_called.len() > workspace_views::MAX_WORKSPACES {
            let views = &store.views;
            store
                .views_called
                .retain(|(device, path), _| views.get(device, path).is_some());
        }
        if changed {
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
        if payload
            .width_request_id
            .as_ref()
            .is_some_and(|id| id.is_empty() || id.len() > 128)
        {
            self.set_error(
                "workspace_view.invalid_width_request",
                "The column width request identity must contain 1 to 128 bytes",
                false,
            );
            return true;
        }
        let tool = match payload.tool.as_deref() {
            None => None,
            Some(value) => match Tool::parse(value) {
                Some(tool) => Some(tool),
                None => {
                    self.set_error(
                        "workspace_view.unknown_tool",
                        format!("{value} is not a Workspace tool; expected explorer or changes"),
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
            && crate::files::path_inside_root(root, path, false).is_err()
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
        // File Views is never an empty column: turned on with no view, it
        // opens the New tab page in the same event (D-10, B16). That is the
        // one step that can fail, so it runs before anything else changes and
        // a refusal leaves the entry as it was.
        if payload.views == Some(true) && entry.layout.displays().next().is_none() {
            let load = self.next_browser_load();
            if let Err(error) = self.change_view_layout(&key, |layout, stamp| {
                let display = layout.new_browser_display("", load);
                let area = layout.active_area().id.clone();
                layout.insert(&area, display, stamp)?;
                Ok(((), true))
            }) {
                self.set_error(error.kind(), error.message(), false);
                return true;
            }
        }
        let store = self.workspace_views.as_mut().expect("checked above");
        let entry = store.views.entry(&key.0, &key.1);
        // A reveal is the Explorer's; a tool chosen turns its column on.
        let tool = if payload.reveal.is_some() {
            Some(Tool::Explorer)
        } else {
            tool
        };
        let tools = payload.tools.or(tool.is_some().then_some(true));
        if let Some(tool) = tool {
            entry.tool = tool;
        }
        if let Some(tools) = tools {
            entry.tools = tools;
        }
        if let Some(width) = payload.views_width {
            entry.views_width = Some(workspace_views::clamp_column_width(width));
        }
        if let Some(width) = payload.tools_width {
            entry.tools_width = Some(workspace_views::clamp_column_width(width));
        }
        if let Some(views) = payload.views {
            entry.views = views;
        }
        let width_request_changed =
            if payload.views_width.is_some() || payload.tools_width.is_some() {
                let next = payload.width_request_id.map(|id| (key.clone(), id));
                let changed = store.width_request != next;
                store.width_request = next;
                changed
            } else {
                false
            };
        let revealed = payload
            .reveal
            .is_some_and(|path| self.unfold_to(&key, &path));
        let entry_changed = self
            .workspace_views
            .as_ref()
            .and_then(|store| store.views.get(&key.0, &key.1))
            != Some(&before);
        if !entry_changed && !revealed && !width_request_changed {
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
    /// of which there are at most 64. The Agent tab bookmark pass adds a
    /// comparison of the shown tabs and area fronts against what it saw last,
    /// in a few small vectors and with no write.
    pub(super) fn sync_workspace_view(&mut self) {
        // The recent list follows the front checkout wherever the layout
        // store exists or not, and writes only when the front moved.
        self.track_recent_checkouts();
        let Some(store) = self.workspace_views.as_ref() else {
            return;
        };
        let front = self.front_workspace_key();
        if front != store.front {
            let store = self.workspace_views.as_mut().expect("checked above");
            store.front = front.clone();
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
        self.sync_agent_selection();
        // The one comparison point for Agent tab bookmarks: a restore lands
        // before the editor is reconciled and the snapshot published, so the
        // tab and the panel's fronts leave in the same frame.
        if self.track_view_bookmarks() {
            self.reconcile_view_displays();
        }
        self.refresh_browser_inventory_scope();
        if let Some(store) = self.workspace_views.as_ref()
            && self.snapshot.browser_views_revision != Some(store.generation)
        {
            // Retain authority across ordinary layout changes, but not
            // across a revoke/regrant coalesced into one outgoing frame.
            // This bounded map is rebuilt only on a changed generation.
            let incarnations: HashMap<_, _> = self
                .snapshot
                .browser_scopes
                .iter()
                .map(|scope| {
                    (
                        (
                            scope.device_id.as_str(),
                            scope.path.as_str(),
                            scope.area_id.as_str(),
                        ),
                        scope.incarnation,
                    )
                })
                .collect();
            let mut displays = Vec::new();
            let mut scopes = Vec::new();
            for view in store.views.workspaces.iter().filter(|view| {
                store
                    .browser_inventory_scope
                    .iter()
                    .any(|key| key.0 == view.device_id && key.1 == view.path)
            }) {
                for area in view.layout.areas() {
                    scopes.push(BrowserAreaScopeRow {
                        device_id: view.device_id.clone(),
                        path: view.path.clone(),
                        area_id: area.id.clone(),
                        incarnation: incarnations
                            .get(&(
                                view.device_id.as_str(),
                                view.path.as_str(),
                                area.id.as_str(),
                            ))
                            .copied()
                            .unwrap_or(store.generation),
                    });
                    displays.extend(
                        area.displays
                            .iter()
                            .filter(|display| display.kind == DisplayKind::Browser)
                            .map(|display| BrowserViewInventoryRow {
                                device_id: view.device_id.clone(),
                                path: view.path.clone(),
                                area_id: area.id.clone(),
                                view_id: display.id.clone(),
                            }),
                    );
                }
            }
            self.snapshot.browser_views = displays;
            self.snapshot.browser_scopes = scopes;
            self.snapshot.browser_views_revision = Some(store.generation);
        }
        self.publish_workspace_view(front.as_ref());
    }

    /// Records authority transitions before an asynchronous catalog/session
    /// update can be coalesced with a later regrant. This does not reconcile
    /// the front tree or publish another notification.
    pub(super) fn refresh_browser_inventory_scope(&mut self) -> bool {
        // Compare borrowed identities without allocating on unchanged input.
        let changed = self.workspace_views.as_ref().is_some_and(|store| {
            !self.browser_inventory_scope().eq(store
                .browser_inventory_scope
                .iter()
                .map(|key| (key.0.as_str(), key.1.as_str())))
        });
        if !changed {
            return false;
        }
        let scope: Vec<_> = self
            .browser_inventory_scope()
            .map(|(device, path)| (device.to_owned(), path.to_owned()))
            .collect();
        let allowed: HashSet<_> = scope
            .iter()
            .map(|key| (key.0.as_str(), key.1.as_str()))
            .collect();
        // Forget the old incarnation at the authority boundary, even if no
        // snapshot read occurs before the same checkout becomes live again.
        self.snapshot
            .browser_scopes
            .retain(|row| allowed.contains(&(row.device_id.as_str(), row.path.as_str())));
        let store = self
            .workspace_views
            .as_mut()
            .expect("scope requires a store");
        store.browser_inventory_scope = scope;
        store.generation += 1;
        true
    }

    /// Positive page authority follows the current connected catalog, never
    /// a saved layout alone. The iterator is bounded by MAX_WORKSPACES (256)
    /// and allocates only when its identity sequence changes.
    fn browser_inventory_scope(&self) -> impl Iterator<Item = (&str, &str)> {
        self.workspace_views
            .as_ref()
            .into_iter()
            .flat_map(|store| &store.views.workspaces)
            .filter(|view| {
                let connected = if view.device_id == workspace::LOCAL_DEVICE_ID {
                    self.snapshot.status.herdr.state == "connected"
                } else {
                    self.snapshot.status.remote.iter().any(|remote| {
                        remote.target_id == view.device_id && remote.state == "connected"
                    })
                };
                connected
                    && self.catalog_workspaces().any(|workspace| {
                        workspace.device_id == view.device_id
                            && workspace
                                .checkouts
                                .iter()
                                .any(|checkout| checkout.path == view.path)
                    })
            })
            .map(|view| (view.device_id.as_str(), view.path.as_str()))
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
            views: view.views,
            tools: view.tools,
            tool: view.tool,
            views_width: view.views_width,
            tools_width: view.tools_width,
            width_request_id: store
                .width_request
                .as_ref()
                .filter(|(workspace, _)| workspace == key)
                .map(|(_, request)| request.clone()),
            views_called: store.views_called.get(key).copied().unwrap_or(0),
            views_calls: store.views_calls,
            resumed: Some(key) == store.resumable.as_ref(),
            layout: self.view_layout_snapshot(key, &view.layout),
            agent_layout: self.agent_layout_snapshot(key, &view.agent_layout),
        });
        self.snapshot.workspace_view = published;
        // The one global panel every existing reader gates on (the Changes
        // reader, the device Explorer watch) follows the front Workspace's
        // Tools column and its tool, so those readers keep one owner (A5)
        // and read nothing while Tools is off.
        if let Some(view) = self.snapshot.workspace_view.as_ref() {
            self.snapshot.ui_state.right_panel_visible = view.tools;
            self.snapshot.ui_state.right_panel_section = match view.tool {
                Tool::Explorer => RightPanelSection::Explorer,
                Tool::Changes => RightPanelSection::Changes,
            };
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
            wrap: true,
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
            .views_called
            .retain(|(device, _), _| device != device_id);
        store
            .bookmark_seen
            .retain(|(device, _), _| device != device_id);
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

/// The last name of a path, which may be any device's, read by names alone.
pub(super) fn file_label(path: &str) -> String {
    hide_platform::path::wire_name(path).to_owned()
}
