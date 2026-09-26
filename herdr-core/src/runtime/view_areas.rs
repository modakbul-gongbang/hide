//! The View areas of each Workspace in a shell that draws them (PRD S7 A2,
//! A3): where an opened document shows, which editor tab each display shows,
//! the operator's `view_layout` actions, and what each display can show now.
//!
//! The tree and its pure operations are `crate::view_layout`; the stored file
//! is `crate::workspace_views`. A display names its document by path, kind
//! and Changes group and binds at runtime to the editor tab that holds the
//! document (the S5.5 buffer), so several displays can show one buffer and a
//! restart binds them again. The reconcile after every event and before every
//! snapshot read keeps the two in step: a display whose tab closed goes, a
//! tab no display shows gets one, a dirty document is never a preview, and
//! the editor's active tab is the active area's active display.
//!
//! A browser display (issue 155) shows a page, not a document: it binds to
//! no tab, so the reconcile never removes or binds one, and its address and
//! title are what the desktop host reports of the page (`browser_state`).
//!
//! Inert without `CoreOptions::workspace_views_path`: the Swift shell keeps
//! one canvas and one preview slot per checkout.

use serde::Deserialize;

use super::documents::OpenRequestFields;
use super::workspace_view::{AreaIntent, WorkspaceKey, file_label};
use super::*;
use crate::model::{
    BrowserOpenReceiptSnapshot, ViewAreaSnapshot, ViewDisplaySnapshot, ViewDisplayState,
    ViewLayoutSnapshot, ViewLimitsSnapshot, ViewNodeSnapshot, ViewSplitSnapshot,
};
use crate::view_layout::{
    Display, DisplayKind, Edge, Layout, LayoutError, MAX_SPLIT_DEPTH, MAX_VIEW_AREAS,
    MAX_VIEW_DISPLAYS, Node, browser_address, browser_title, is_file_address,
};

/// Split request ids remembered per Workspace, so a split sent twice splits
/// once (engineering principle 11). Runtime only.
const SPLIT_REQUESTS_KEPT: usize = 32;

/// `browser_open` receipts kept for the requests that named themselves; a
/// CLI waiting on one reads it within a frame or two of its event.
const BROWSER_OPEN_RECEIPTS_KEPT: usize = 8;

const UNLOADABLE_ADDRESS: &str = "A page opens from an http, https or file address, or about:blank";
const FILE_ELSEWHERE: &str = "A file on this Mac cannot open in a Workspace on another device";

/// Where Open to the side looks for an area next to the one in use.
const BESIDE_ORDER: [Edge; 4] = [Edge::Right, Edge::Left, Edge::Down, Edge::Up];

// Every View diff is taken in the one Changes read, one per area at most.
const _: () = assert!(MAX_VIEW_AREAS <= hide_host::git::MAX_DIFFS);

/// The payload of `view_layout`: one operator action on the View areas of
/// the Workspace the operator saw. Each is one event and one frame; a
/// refusal says why and changes nothing.
#[derive(Debug, Deserialize)]
pub(super) struct ViewLayoutPayload {
    /// The Workspace of the frame the action was taken on, as that frame's
    /// `workspace_view` named it. Display, area and split ids repeat across
    /// Workspaces and the front can move between that frame and this event,
    /// so an action is applied only while its Workspace is still in front.
    workspace: ViewWorkspace,
    #[serde(flatten)]
    action: ViewLayoutAction,
}

#[derive(Debug, Deserialize)]
pub(super) struct ViewWorkspace {
    device_id: String,
    path: String,
}

/// The payload of `browser_open` (issue 155): show `url` as a page in a View
/// area of a Workspace, which need not be the one in front: the one named,
/// else the one the pane `pane_id` works in (`hide browser open` from an
/// agent's pane), else the one in front. A Workspace already showing `url`
/// shows that page again, loaded again, rather than a second one.
#[derive(Debug, Deserialize)]
pub(super) struct BrowserOpenPayload {
    url: String,
    #[serde(default)]
    workspace: Option<ViewWorkspace>,
    #[serde(default)]
    pane_id: Option<String>,
    /// Names the receipt in `status.browser_opens` the sender waits for.
    #[serde(default)]
    request_id: Option<String>,
}

/// The payload of `browser_state`: what a browser display's page says now,
/// its address after it navigated and its title, as the desktop host reports
/// it. It records; it never loads anything.
#[derive(Debug, Deserialize)]
pub(super) struct BrowserStatePayload {
    workspace: ViewWorkspace,
    display_id: String,
    url: String,
    #[serde(default)]
    title: String,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
enum ViewLayoutAction {
    Focus {
        display_id: String,
    },
    FocusArea {
        area_id: String,
    },
    /// `index` is the display's place after the move, so the same move sent
    /// twice changes nothing the second time.
    Move {
        display_id: String,
        area_id: String,
        index: usize,
    },
    Split {
        display_id: String,
        area_id: String,
        edge: Edge,
        request_id: String,
    },
    Resize {
        split_id: String,
        ratio: f32,
    },
    /// The save a dirty document's last display waits on before it closes,
    /// or `discard`, the operator's Don't Save; a last display of unsaved
    /// work with neither is refused.
    Close {
        display_id: String,
        #[serde(default)]
        pending_save: Option<FileSavePayload>,
        #[serde(default)]
        discard: bool,
    },
    KeepOpen {
        display_id: String,
    },
    Retry {
        display_id: String,
    },
    /// The operator typed an address into a browser display's toolbar.
    Navigate {
        display_id: String,
        url: String,
    },
}

impl ViewLayoutAction {
    fn name(&self) -> &'static str {
        match self {
            Self::Focus { .. } => "focus",
            Self::FocusArea { .. } => "focus_area",
            Self::Move { .. } => "move",
            Self::Split { .. } => "split",
            Self::Resize { .. } => "resize",
            Self::Close { .. } => "close",
            Self::KeepOpen { .. } => "keep_open",
            Self::Retry { .. } => "retry",
            Self::Navigate { .. } => "navigate",
        }
    }
}

/// What every display of one Workspace's tree reads to say its state.
struct DisplayFacts<'a> {
    /// The checkout in front, whose reads a display can be waiting on.
    front: Option<(&'a str, &'a str)>,
    tabs: HashMap<&'a str, &'a EditorTabSnapshot>,
    /// Why the Workspace's root cannot be read yet.
    wait: Option<String>,
}

/// Where a document being read shows once it lands: the Workspace and the
/// area the operator asked in, and how.
#[derive(Clone, Debug)]
pub(super) struct ViewPlacement {
    pub(super) key: WorkspaceKey,
    pub(super) area: String,
    pub(super) preview: bool,
    pub(super) beside: bool,
}

/// What the last reconcile saw, so an unchanged editor costs a comparison.
pub(super) struct Reconciled {
    tabs: Vec<EditorTabSnapshot>,
    active: Option<String>,
    front: Option<WorkspaceKey>,
    generation: u64,
}

/// What a display needs to know about an editor tab.
struct TabFacts {
    id: String,
    key: Option<WorkspaceKey>,
    path: String,
    kind: DisplayKind,
    committed: Option<bool>,
    /// Dirty, saving or holding a save's outcome: never a preview (B5).
    kept: bool,
}

fn display_kind(kind: EditorTabKind) -> Option<DisplayKind> {
    match kind {
        EditorTabKind::File => Some(DisplayKind::File),
        EditorTabKind::Diff => Some(DisplayKind::Diff),
        EditorTabKind::Session | EditorTabKind::Memory => None,
    }
}

/// The display that shows a document and was focused last.
fn last_focused_showing(
    displays: &[&Display],
    path: &str,
    kind: DisplayKind,
    committed: Option<bool>,
) -> Option<String> {
    displays
        .iter()
        .filter(|display| display.shows(path, kind, committed))
        .max_by_key(|display| display.last_focused_unix_ms)
        .map(|display| display.id.clone())
}

/// The cap with the opens still being read counted, so reads in flight
/// cannot pass it together.
fn display_limit(layout: &Layout, pending: usize) -> Option<LayoutRefusal> {
    (layout.display_count() + pending >= MAX_VIEW_DISPLAYS)
        .then(|| LayoutError::DisplayLimit.into())
}

struct LayoutRefusal {
    kind: &'static str,
    message: String,
}

impl From<LayoutError> for LayoutRefusal {
    fn from(error: LayoutError) -> Self {
        Self {
            kind: error.kind(),
            message: error.message(),
        }
    }
}

/// Brings one Workspace's displays in line with the editor's tabs. Returns
/// whether anything the file stores changed, and the documents the display
/// cap kept off screen.
fn reconcile_layout<'t>(
    layout: &mut Layout,
    key: &WorkspaceKey,
    live: bool,
    tabs: &'t [TabFacts],
    requested: Option<&str>,
    now: u64,
) -> (bool, Vec<&'t TabFacts>) {
    let mut stored = false;
    let mut unshown = Vec::new();
    // A display whose document closed, by any path, goes with it.
    let gone: Vec<String> = layout
        .displays()
        .filter(|display| {
            display
                .tab_id
                .as_ref()
                .is_some_and(|id| !tabs.iter().any(|tab| &tab.id == id))
        })
        .map(|display| display.id.clone())
        .collect();
    for display_id in gone {
        layout.remove(&display_id);
        stored = true;
    }
    for display in layout.displays_mut() {
        let tab = match display
            .tab_id
            .as_ref()
            .and_then(|id| tabs.iter().find(|tab| &tab.id == id))
        {
            Some(tab) => tab,
            // A restored display binds to its document once it is read.
            None => match tabs.iter().find(|tab| {
                tab.key.as_ref() == Some(key) && display.shows(&tab.path, tab.kind, tab.committed)
            }) {
                Some(tab) => {
                    display.tab_id = Some(tab.id.clone());
                    tab
                }
                None => continue,
            },
        };
        // A rename keeps the tab, and the display follows it.
        if display.path != tab.path {
            display.path = tab.path.clone();
            stored = true;
        }
        if tab.kept && display.preview {
            display.preview = false;
            stored = true;
        }
    }
    // A document of this Workspace that no display shows gets one, at the
    // end of the area in use: a Reopen Closed, a created file. The actions
    // that make one are refused at the cap; one that landed after the tree
    // filled waits off screen until a view is closed.
    if live {
        for tab in tabs.iter().filter(|tab| tab.key.as_ref() == Some(key)) {
            if layout
                .displays()
                .any(|display| display.tab_id.as_deref() == Some(tab.id.as_str()))
            {
                continue;
            }
            if layout.display_count() >= MAX_VIEW_DISPLAYS {
                unshown.push(tab);
                continue;
            }
            let mut display = layout.new_display(&tab.path, tab.kind, tab.committed, false);
            display.tab_id = Some(tab.id.clone());
            let area = layout.active_area().id.clone();
            if layout.append(&area, display).is_ok() {
                stored = true;
            }
        }
    }
    // A tab an event activated (File Focus, Reopen Closed) shows in the
    // display that showed it last.
    if let Some(requested) = requested {
        let bound: Vec<&Display> = layout
            .displays()
            .filter(|display| display.tab_id.as_deref() == Some(requested))
            .collect();
        if let Some(display_id) = bound
            .iter()
            .max_by_key(|display| display.last_focused_unix_ms)
            .map(|display| display.id.clone())
        {
            let stamp = layout.next_stamp(now);
            stored |= layout.focus(&display_id, stamp).unwrap_or(false);
        }
    }
    (stored, unshown)
}

impl Runtime {
    fn view_layout_of(&self, key: &WorkspaceKey) -> Option<&Layout> {
        self.workspace_views
            .as_ref()?
            .views
            .get(&key.0, &key.1)
            .map(|view| &view.layout)
    }

    /// Whether the Workspace of a checkout holds `display_id`, before a
    /// `focus_checkout` that names it moves anything.
    pub(super) fn view_display_known(
        &self,
        workspace_id: &str,
        checkout_id: &str,
        display_id: &str,
    ) -> Result<(), LayoutError> {
        if !self.separate_view_areas() {
            return Err(LayoutError::UnknownDisplay(display_id.to_owned()));
        }
        self.workspace_key(workspace_id, checkout_id)
            .and_then(|key| self.view_layout_of(&key))
            .and_then(|layout| layout.display(display_id))
            .map(|_| ())
            .ok_or_else(|| LayoutError::UnknownDisplay(display_id.to_owned()))
    }

    /// A display of the checkout just brought forward takes its View area,
    /// shown with the Agent area if only Agents showed (D-08). Returns
    /// whether the layout changed.
    pub(super) fn focus_view_display_of(
        &mut self,
        workspace_id: &str,
        checkout_id: &str,
        display_id: &str,
    ) -> bool {
        let Some(key) = self.workspace_key(workspace_id, checkout_id) else {
            return false;
        };
        self.apply_area_intent_to(&key, AreaIntent::Views);
        match self.change_view_layout(&key, |layout, stamp| {
            layout
                .focus(display_id, stamp)
                .map(|changed| (changed, changed))
        }) {
            Ok(changed) => changed,
            Err(error) => {
                self.set_error(error.kind(), error.message(), false);
                true
            }
        }
    }

    /// Applies one change to a Workspace's layout, created when the file has
    /// none for it, and saves it when it changed something.
    fn change_view_layout<T>(
        &mut self,
        key: &WorkspaceKey,
        change: impl FnOnce(&mut Layout, u64) -> Result<(T, bool), LayoutError>,
    ) -> Result<T, LayoutError> {
        let now = unix_milliseconds();
        let Some(store) = self.workspace_views.as_mut() else {
            return Err(LayoutError::UnknownArea(key.1.clone()));
        };
        let layout = &mut store.views.entry(&key.0, &key.1).layout;
        let stamp = layout.next_stamp(now);
        let (value, changed) = change(layout, stamp)?;
        if changed {
            store.generation += 1;
            self.persist_workspace_views();
        }
        Ok(value)
    }

    /// Whether an open may add what it would add to the Workspace `key`; a
    /// refusal is set as the error and nothing changes (contract 2). An open
    /// of a document already shown is a focus and is never refused.
    pub(super) fn admit_view_open(
        &mut self,
        key: &WorkspaceKey,
        path: &str,
        kind: DisplayKind,
        committed: Option<bool>,
        preview: bool,
        beside: bool,
    ) -> bool {
        let pending = self.pending_view_placements(key);
        let Some(layout) = self.view_layout_of(key) else {
            return true;
        };
        let base = layout.active_area();
        let refusal = if beside {
            match BESIDE_ORDER
                .iter()
                .find_map(|edge| layout.neighbour(&base.id, *edge))
            {
                Some(target)
                    if layout.area(&target).is_some_and(|area| {
                        area.displays
                            .iter()
                            .any(|display| display.shows(path, kind, committed))
                    }) =>
                {
                    None
                }
                Some(_) => display_limit(layout, pending),
                None if base.displays.is_empty() => display_limit(layout, pending),
                None => match layout.can_split(&base.id, Edge::Right) {
                    Err(error) => Some(error.into()),
                    Ok(()) => display_limit(layout, pending),
                },
            }
        } else if layout
            .displays()
            .any(|display| display.shows(path, kind, committed))
        {
            None
        } else if preview
            && base.displays.iter().any(|display| {
                display.preview
                    && !display
                        .tab_id
                        .as_deref()
                        .is_some_and(|tab_id| self.document_kept(tab_id))
            })
        {
            // The area's preview display takes the document in place; one
            // whose document holds work stays and the open adds a display.
            None
        } else {
            display_limit(layout, pending)
        };
        match refusal {
            Some(refusal) => {
                self.set_error(refusal.kind, refusal.message, false);
                false
            }
            None => true,
        }
    }

    /// `file_open` with View areas: `beside` asks for the area next to the
    /// one in use (Open to the side, B4).
    pub(super) fn open_file_in_view(
        &mut self,
        workspace_id: &str,
        checkout_id: &str,
        path: &str,
        preview: bool,
        beside: bool,
    ) {
        match self.prepare_file_tab(workspace_id, checkout_id, path) {
            Ok(prepared) => {
                self.show_file_in_view(prepared, workspace_id, checkout_id, path, preview, beside)
            }
            // The read failed before anything moved, so every display keeps
            // what it shows (B15).
            Err(message) => self.set_error("file.open_failed", message, true),
        }
    }

    /// Opens a file into the front Workspace's View areas (contract 4.2).
    /// A file being read shows when the read lands, where it was asked for;
    /// a failed read moves nothing.
    pub(super) fn show_file_in_view(
        &mut self,
        prepared: super::editor::PreparedFileTab,
        workspace_id: &str,
        checkout_id: &str,
        path: &str,
        preview: bool,
        beside: bool,
    ) {
        self.reconcile_view_displays();
        let Some(key) = self.workspace_key(workspace_id, checkout_id) else {
            self.set_error(
                "file.invalid_context",
                "The file's project or checkout is no longer available",
                false,
            );
            return;
        };
        if !self.admit_view_open(&key, path, DisplayKind::File, None, preview, beside) {
            return;
        }
        self.snapshot.ui_state.selected_path = Some(path.to_owned());
        match prepared {
            super::editor::PreparedFileTab::Open(tab_id) => {
                self.place_document(&key, &tab_id, preview, beside, None);
            }
            read @ super::editor::PreparedFileTab::Read { .. } => {
                if let Some(tab_id) =
                    self.insert_file_tab(read, workspace_id, checkout_id, path, preview)
                {
                    self.place_document(&key, &tab_id, preview, beside, None);
                }
            }
            super::editor::PreparedFileTab::Reading { root, channel } => {
                // A display that shows the file but could not read it is
                // focused now, and the read fills its tab (S6 B20).
                let shown = !beside
                    && self.focus_view_document(&key, path, DisplayKind::File, None, !preview);
                let placement = (!shown).then(|| ViewPlacement {
                    area: self
                        .view_layout_of(&key)
                        .map(|layout| layout.active_area().id.clone())
                        .unwrap_or_else(|| Layout::default().active_area),
                    key: key.clone(),
                    preview,
                    beside,
                });
                self.start_document_open(
                    root,
                    channel,
                    OpenRequestFields {
                        workspace_id: workspace_id.to_owned(),
                        checkout_id: checkout_id.to_owned(),
                        path: path.to_owned(),
                        preview,
                        reload: false,
                        reveal: None,
                        restore: false,
                        placement,
                    },
                );
            }
        }
    }

    /// Opens a diff into the front Workspace's View areas by the same rules
    /// as a file (A10). Its text comes from the Changes read. Returns whether
    /// anything changed.
    pub(super) fn show_diff_in_view(
        &mut self,
        workspace_id: &str,
        checkout_id: &str,
        path: &str,
        committed: bool,
        preview: bool,
        beside: bool,
    ) -> bool {
        self.reconcile_view_displays();
        let Some(key) = self.workspace_key(workspace_id, checkout_id) else {
            self.set_error(
                "diff.invalid_context",
                "The diff's project or checkout is no longer available",
                false,
            );
            return true;
        };
        if !self.admit_view_open(
            &key,
            path,
            DisplayKind::Diff,
            Some(committed),
            preview,
            beside,
        ) {
            return true;
        }
        let tab_id = Self::diff_tab_id(workspace_id, checkout_id, path, committed);
        if !self.snapshot.editor.tabs.iter().any(|tab| tab.id == tab_id) {
            self.insert_diff_tab(workspace_id, checkout_id, path, committed, preview);
        }
        self.place_document(&key, &tab_id, preview, beside, None)
    }

    /// Focuses the display of a document that was focused last, keeping it
    /// open when `keep_open`. Returns whether the Workspace shows it at all.
    fn focus_view_document(
        &mut self,
        key: &WorkspaceKey,
        path: &str,
        kind: DisplayKind,
        committed: Option<bool>,
        keep_open: bool,
    ) -> bool {
        let Some(display_id) = self.view_layout_of(key).and_then(|layout| {
            last_focused_showing(
                &layout.displays().collect::<Vec<_>>(),
                path,
                kind,
                committed,
            )
        }) else {
            return false;
        };
        let _ = self.change_view_layout(key, |layout, stamp| {
            let mut changed = layout.focus(&display_id, stamp)?;
            if keep_open && let Some(display) = layout.display_mut(&display_id) {
                changed |= std::mem::replace(&mut display.preview, false);
            }
            Ok(((), changed))
        });
        true
    }

    /// Shows the document of editor tab `tab_id` in the Workspace `key`
    /// (contract 4.2). A display that shows it already is focused, and kept
    /// open unless a preview was asked for. Otherwise it opens in `area`, the
    /// area in use when the open was asked (the one in use now if that area
    /// is gone): a preview takes the area's preview display in place, and
    /// anything else is a new display at the end. `beside` puts it in the
    /// area next to that one, or a new area to its right. Returns whether the
    /// layout changed.
    ///
    /// It decides by the displays' bindings, so every caller reconciles them
    /// before it adds the tab it places: reads land on workers, and one that
    /// landed since the last reconcile leaves a display unbound to the
    /// document it shows.
    pub(super) fn place_document(
        &mut self,
        key: &WorkspaceKey,
        tab_id: &str,
        preview: bool,
        beside: bool,
        area: Option<&str>,
    ) -> bool {
        let Some((path, kind, committed)) = self
            .snapshot
            .editor
            .tabs
            .iter()
            .find(|tab| tab.id == tab_id)
            .and_then(|tab| {
                Some((
                    tab.path.clone(),
                    display_kind(tab.kind)?,
                    tab.diff_committed,
                ))
            })
        else {
            return false;
        };
        let base = match (self.view_layout_of(key), area) {
            (Some(layout), Some(area)) if layout.area(area).is_some() => area.to_owned(),
            (Some(layout), asked) => {
                if let Some(asked) = asked {
                    crate::diagnostic!(serde_json::json!({
                        "component": "view_areas",
                        "kind": "view_layout.area_gone",
                        "device": key.0,
                        "area": asked,
                        "tab": tab_id,
                    }));
                }
                layout.active_area().id.clone()
            }
            (None, _) => Layout::default().active_area,
        };
        if beside {
            return self.place_beside(key, &base, tab_id, &path, kind, committed);
        }
        if self.focus_view_document(key, &path, kind, committed, !preview) {
            return true;
        }
        // The preview display the open takes, with the document it showed.
        let slot = preview
            .then(|| {
                let area = self.view_layout_of(key)?.area(&base)?;
                area.displays
                    .iter()
                    .find(|display| display.preview)
                    .map(|display| {
                        (
                            display.id.clone(),
                            display.tab_id.clone(),
                            display.path.clone(),
                            display.kind,
                        )
                    })
            })
            .flatten();
        let old_kept = slot
            .as_ref()
            .and_then(|(_, old_tab, _, _)| old_tab.as_deref())
            .is_some_and(|old_tab| self.document_kept(old_tab));
        let placed = self.change_view_layout(key, |layout, stamp| {
            match &slot {
                Some((slot_id, _, _, _)) if !old_kept => {
                    let display = layout
                        .display_mut(slot_id)
                        .ok_or_else(|| LayoutError::UnknownDisplay(slot_id.clone()))?;
                    display.tab_id = Some(tab_id.to_owned());
                    display.path = path.clone();
                    display.kind = kind;
                    display.committed =
                        (kind == DisplayKind::Diff).then_some(committed.unwrap_or(false));
                    layout.focus(slot_id, stamp)?;
                }
                // A document with unsaved work keeps its display, opened for
                // good, and the new preview goes beside it.
                kept => {
                    if let Some((slot_id, _, _, _)) = kept
                        && let Some(display) = layout.display_mut(slot_id)
                    {
                        display.preview = false;
                    }
                    let mut display = layout.new_display(&path, kind, committed, preview);
                    display.tab_id = Some(tab_id.to_owned());
                    layout.insert(&base, display, stamp)?;
                }
            }
            Ok(((), true))
        });
        if let Err(error) = placed {
            self.refuse_placement(key, tab_id, error);
            return true;
        }
        if let Some((_, old_tab, old_path, old_kind)) = slot
            && !old_kept
        {
            self.release_previewed(key, old_tab, (&old_path, old_kind), tab_id);
        }
        true
    }

    /// Open to the side (B4): a kept-open display in the area next to
    /// `base`, or in a new area to its right when there is none. A display of
    /// the document already in that area is focused instead.
    fn place_beside(
        &mut self,
        key: &WorkspaceKey,
        base: &str,
        tab_id: &str,
        path: &str,
        kind: DisplayKind,
        committed: Option<bool>,
    ) -> bool {
        let placed = self.change_view_layout(key, |layout, stamp| {
            let neighbour = BESIDE_ORDER
                .iter()
                .find_map(|edge| layout.neighbour(base, *edge));
            let Some(target) = neighbour else {
                let mut display = layout.new_display(path, kind, committed, false);
                display.tab_id = Some(tab_id.to_owned());
                // An empty area is only ever the root of a Workspace with no
                // view yet: there is nothing to stand beside, and a split
                // would leave that empty area on screen next to the new one.
                if layout
                    .area(base)
                    .is_some_and(|area| area.displays.is_empty())
                {
                    layout.insert(base, display, stamp)?;
                    return Ok(((), true));
                }
                if let Err(error) = layout.split_new(base, Edge::Right, display, stamp) {
                    // The tree changed while the file was read: a display
                    // that shows the document already is focused, and with
                    // none the open is refused rather than doubled up in the
                    // area it was asked from.
                    let Some(shown) = last_focused_showing(
                        &layout.displays().collect::<Vec<_>>(),
                        path,
                        kind,
                        committed,
                    ) else {
                        return Err(error);
                    };
                    crate::diagnostic!(serde_json::json!({
                        "component": "view_areas",
                        "kind": "view_layout.beside_refused",
                        "device": key.0,
                        "reason": error.kind(),
                    }));
                    layout.focus(&shown, stamp)?;
                }
                return Ok(((), true));
            };
            let shown = layout.area(&target).and_then(|area| {
                last_focused_showing(
                    &area.displays.iter().collect::<Vec<_>>(),
                    path,
                    kind,
                    committed,
                )
            });
            match shown {
                Some(display_id) => {
                    layout.focus(&display_id, stamp)?;
                    if let Some(display) = layout.display_mut(&display_id) {
                        display.preview = false;
                    }
                }
                None => {
                    let mut display = layout.new_display(path, kind, committed, false);
                    display.tab_id = Some(tab_id.to_owned());
                    layout.insert(&target, display, stamp)?;
                }
            }
            Ok(((), true))
        });
        if let Err(error) = placed {
            self.refuse_placement(key, tab_id, error);
        }
        true
    }

    /// An open whose display cannot be added says why, and the document it
    /// read goes with it unless a display shows it or it holds work, so the
    /// reconcile does not put it on screen anyway.
    fn refuse_placement(&mut self, key: &WorkspaceKey, tab_id: &str, error: LayoutError) {
        self.set_error(error.kind(), error.message(), false);
        let shown = self.view_layout_of(key).is_some_and(|layout| {
            layout
                .displays()
                .any(|display| display.tab_id.as_deref() == Some(tab_id))
        });
        if shown || self.document_kept(tab_id) {
            return;
        }
        if let Some(index) = self
            .snapshot
            .editor
            .tabs
            .iter()
            .position(|tab| tab.id == tab_id)
        {
            self.retire_editor_tab(index);
            self.rebuild_tab_strips();
        }
    }

    /// The document a preview display showed before it took `new_tab`'s
    /// goes when nothing else shows it, without a Recent Closed entry, as the
    /// preview tab it replaces did (D-04); one still being read is dropped.
    fn release_previewed(
        &mut self,
        key: &WorkspaceKey,
        old_tab: Option<String>,
        (old_path, old_kind): (&str, DisplayKind),
        new_tab: &str,
    ) {
        let Some(layout) = self.view_layout_of(key) else {
            return;
        };
        match old_tab {
            Some(old_tab) if old_tab != new_tab => {
                let shown = layout
                    .displays()
                    .any(|display| display.tab_id.as_deref() == Some(old_tab.as_str()));
                if shown {
                    return;
                }
                if let Some(index) = self
                    .snapshot
                    .editor
                    .tabs
                    .iter()
                    .position(|tab| tab.id == old_tab)
                {
                    self.retire_editor_tab(index);
                    self.rebuild_tab_strips();
                    self.push_diagnostic(
                        "editor.preview_replaced",
                        format!("Preview tab {old_tab} replaced by {new_tab}"),
                    );
                }
            }
            None if old_kind == DisplayKind::File
                && !layout
                    .displays()
                    .any(|display| display.shows(old_path, DisplayKind::File, None)) =>
            {
                // The Workspace's checkout is the one the new document is in.
                if let Some((workspace_id, checkout_id)) = self
                    .snapshot
                    .editor
                    .tabs
                    .iter()
                    .find(|tab| tab.id == new_tab)
                    .map(|tab| (tab.workspace_id.clone(), tab.checkout_id.clone()))
                {
                    self.cancel_document_read(&workspace_id, &checkout_id, old_path);
                }
            }
            _ => {}
        }
    }

    /// Whether a document holds work a preview would lose: a draft, a save
    /// running, or a save's outcome the operator has not seen settle.
    fn document_kept(&self, tab_id: &str) -> bool {
        self.snapshot
            .editor
            .tabs
            .iter()
            .any(|tab| tab.id == tab_id && tab.dirty)
            || self
                .editor_documents
                .get(tab_id)
                .is_some_and(|document| document.save.is_some())
    }

    /// Keep Open for a document: every preview display of it stays open, and
    /// the tab's flag follows (B5). A display binds only to a tab of its own
    /// Workspace, so that Workspace is the only one looked at.
    pub(super) fn promote_view_displays(&mut self, tab_id: &str) -> bool {
        let mut changed = false;
        let key = self
            .snapshot
            .editor
            .tabs
            .iter()
            .find(|tab| tab.id == tab_id)
            .and_then(|tab| self.workspace_key(&tab.workspace_id, &tab.checkout_id));
        if let Some(store) = self.workspace_views.as_mut()
            && let Some((device, path)) = key
            && let Some(view) = store.views.get_mut(&device, &path)
        {
            for display in view.layout.displays_mut() {
                if display.tab_id.as_deref() == Some(tab_id) && display.preview {
                    display.preview = false;
                    changed = true;
                }
            }
            if changed {
                store.generation += 1;
            }
        }
        if changed {
            self.persist_workspace_views();
        }
        if let Some(tab) = self
            .snapshot
            .editor
            .tabs
            .iter_mut()
            .find(|tab| tab.id == tab_id && tab.preview)
        {
            tab.preview = false;
            self.rebuild_tab_strips();
            changed = true;
        }
        if changed {
            self.push_diagnostic("editor.preview_promoted", format!("Tab {tab_id} kept open"));
        }
        changed
    }

    /// Whether `tab_id` is an open file with a document, which a draft or a
    /// conflict choice can name.
    pub(super) fn file_document_open(&self, tab_id: &str) -> bool {
        self.editor_documents.contains_key(tab_id)
            && self
                .snapshot
                .editor
                .tabs
                .iter()
                .any(|tab| tab.id == tab_id && tab.kind == EditorTabKind::File)
    }

    /// One operator action on the front Workspace's View areas (contract
    /// 4.1). Returns whether the snapshot changed.
    pub(super) fn apply_view_layout(&mut self, payload: ViewLayoutPayload) -> bool {
        if !self.separate_view_areas() {
            self.set_error(
                "view_layout.unsupported",
                "This shell does not draw View areas",
                false,
            );
            return true;
        }
        let ViewLayoutPayload { workspace, action } = payload;
        let key = (workspace.device_id, workspace.path);
        let front = self.front_workspace_key();
        if front.as_ref() != Some(&key) {
            // The screen already shows another Workspace, so there is nothing
            // on it to explain; the log keeps both (design principle 13).
            let front = front.map_or_else(
                || "no Workspace".to_owned(),
                |(device, path)| format!("{path} on {device}"),
            );
            self.push_diagnostic(
                "view_layout.stale_workspace",
                format!(
                    "A {} for {} on {} arrived while {front} is in front; nothing changed",
                    action.name(),
                    key.1,
                    key.0
                ),
            );
            return true;
        }
        // The action names displays by what the last frame showed; bring
        // their bindings up to date first.
        self.reconcile_view_displays();
        let outcome = match action {
            ViewLayoutAction::Focus { display_id } => {
                self.change_view_layout(&key, |layout, stamp| {
                    layout
                        .focus(&display_id, stamp)
                        .map(|changed| (changed, changed))
                })
            }
            ViewLayoutAction::FocusArea { area_id } => {
                self.change_view_layout(&key, |layout, stamp| {
                    layout
                        .focus_area(&area_id, stamp)
                        .map(|changed| (changed, changed))
                })
            }
            ViewLayoutAction::Move {
                display_id,
                area_id,
                index,
            } => self.change_view_layout(&key, |layout, stamp| {
                layout
                    .move_display(&display_id, &area_id, index, stamp)
                    .map(|changed| (changed, changed))
            }),
            ViewLayoutAction::Split {
                display_id,
                area_id,
                edge,
                request_id,
            } => {
                if self.split_request_seen(&key, &request_id) {
                    return false;
                }
                let split = self.change_view_layout(&key, |layout, stamp| {
                    layout
                        .split(&display_id, &area_id, edge, stamp)
                        .map(|_| (true, true))
                });
                if split.is_ok() {
                    self.remember_split_request(&key, request_id);
                }
                split
            }
            ViewLayoutAction::Resize { split_id, ratio } => {
                self.change_view_layout(&key, |layout, _| {
                    layout
                        .resize(&split_id, ratio)
                        .map(|changed| (changed, changed))
                })
            }
            ViewLayoutAction::KeepOpen { display_id } => {
                self.change_view_layout(&key, |layout, _| {
                    let display = layout
                        .display_mut(&display_id)
                        .ok_or_else(|| LayoutError::UnknownDisplay(display_id.clone()))?;
                    let changed = std::mem::replace(&mut display.preview, false);
                    Ok((changed, changed))
                })
            }
            ViewLayoutAction::Close {
                display_id,
                pending_save,
                discard,
            } => return self.close_view_display(&key, &display_id, pending_save, discard),
            ViewLayoutAction::Retry { display_id } => {
                return self.retry_view_display(&key, &display_id);
            }
            ViewLayoutAction::Navigate { display_id, url } => {
                return self.navigate_browser(&key, &display_id, url);
            }
        };
        match outcome {
            Ok(changed) => changed,
            Err(error) => {
                self.set_error(error.kind(), error.message(), false);
                true
            }
        }
    }

    /// `browser_open`: a page in a View area of the Workspace the payload
    /// names (see `BrowserOpenPayload`), in its area in use, and the View
    /// area brought back if only Agents showed. A refusal says why and
    /// changes nothing; a request that named itself gets a receipt either way.
    pub(super) fn open_browser(&mut self, payload: BrowserOpenPayload) -> bool {
        let BrowserOpenPayload {
            url,
            workspace,
            pane_id,
            request_id,
        } = payload;
        let outcome = self.place_browser(&url, workspace, pane_id.as_deref());
        if let Err(message) = &outcome {
            self.set_error("browser.open_refused", message.clone(), false);
        }
        if let Some(request_id) = request_id {
            let receipt = match outcome {
                Ok(((device_id, path), display_id)) => BrowserOpenReceiptSnapshot {
                    request_id,
                    ok: true,
                    message: None,
                    device_id: Some(device_id),
                    path: Some(path),
                    display_id: Some(display_id),
                },
                Err(message) => BrowserOpenReceiptSnapshot {
                    request_id,
                    ok: false,
                    message: Some(message),
                    device_id: None,
                    path: None,
                    display_id: None,
                },
            };
            let receipts = &mut self.snapshot.status.browser_opens;
            receipts.push(receipt);
            if receipts.len() > BROWSER_OPEN_RECEIPTS_KEPT {
                receipts.remove(0);
            }
        }
        true
    }

    fn place_browser(
        &mut self,
        url: &str,
        workspace: Option<ViewWorkspace>,
        pane_id: Option<&str>,
    ) -> Result<(WorkspaceKey, String), String> {
        if !self.separate_view_areas() {
            return Err("This shell does not draw View areas".to_owned());
        }
        if !browser_address(url) {
            return Err(UNLOADABLE_ADDRESS.to_owned());
        }
        let key = match (workspace, pane_id) {
            (Some(workspace), _) => {
                let key = (workspace.device_id, workspace.path);
                if !self.catalog_has_workspace(&key) {
                    return Err(format!("{} is not a Workspace hide shows", key.1));
                }
                key
            }
            (None, Some(pane_id)) => self.pane_workspace_key(pane_id).ok_or_else(|| {
                format!("Pane {pane_id} is not in a Workspace hide shows on this Mac")
            })?,
            (None, None) => self
                .front_workspace_key()
                .ok_or_else(|| "No Workspace is in front to open the page in".to_owned())?,
        };
        if is_file_address(url) && key.0 != workspace::LOCAL_DEVICE_ID {
            return Err(FILE_ELSEWHERE.to_owned());
        }
        let load = self.next_browser_load();
        let display_id = self
            .change_view_layout(&key, |layout, stamp| {
                let shown = layout
                    .displays()
                    .find(|display| {
                        display.kind == DisplayKind::Browser && display.url.as_deref() == Some(url)
                    })
                    .map(|display| display.id.clone());
                if let Some(display_id) = shown {
                    layout.focus(&display_id, stamp)?;
                    if let Some(display) = layout.display_mut(&display_id) {
                        display.load = load;
                    }
                    return Ok((display_id, true));
                }
                let area = layout.active_area().id.clone();
                let display = layout.new_browser_display(url, load);
                let display_id = display.id.clone();
                layout.insert(&area, display, stamp)?;
                Ok((display_id, true))
            })
            .map_err(|error| error.message())?;
        self.apply_area_intent_to(&key, AreaIntent::Views);
        Ok((key, display_id))
    }

    /// `view_layout` navigate: the operator's own address in a browser
    /// display's toolbar, which its page loads.
    fn navigate_browser(&mut self, key: &WorkspaceKey, display_id: &str, url: String) -> bool {
        if !browser_address(&url) {
            self.set_error("browser.navigate_refused", UNLOADABLE_ADDRESS, false);
            return true;
        }
        if is_file_address(&url) && key.0 != workspace::LOCAL_DEVICE_ID {
            self.set_error("browser.navigate_refused", FILE_ELSEWHERE, false);
            return true;
        }
        let load = self.next_browser_load();
        let outcome = self.change_view_layout(key, |layout, _| {
            let display = layout
                .display_mut(display_id)
                .filter(|display| display.kind == DisplayKind::Browser)
                .ok_or_else(|| LayoutError::UnknownDisplay(display_id.to_owned()))?;
            let changed = display.url.as_deref() != Some(url.as_str());
            if changed {
                display.url = Some(url);
                display.title = None;
            }
            display.load = load;
            Ok(((), changed))
        });
        if let Err(error) = outcome {
            self.set_error(error.kind(), error.message(), false);
        }
        true
    }

    /// `browser_state`: what a browser display's page says now. It applies to
    /// any Workspace, since a page keeps loading while another is in front,
    /// and records only: the page is already where it says.
    pub(super) fn record_browser_state(&mut self, payload: BrowserStatePayload) -> bool {
        let BrowserStatePayload {
            workspace,
            display_id,
            url,
            title,
        } = payload;
        let key = (workspace.device_id, workspace.path);
        let file_elsewhere = is_file_address(&url) && key.0 != workspace::LOCAL_DEVICE_ID;
        if !browser_address(&url) || file_elsewhere {
            // A page that moved somewhere a display does not hold keeps the
            // last address it may; the log names only the scheme.
            let scheme: String = url
                .split(':')
                .next()
                .unwrap_or("")
                .chars()
                .take(16)
                .collect();
            crate::diagnostic!(serde_json::json!({
                "component": "view_areas",
                "kind": "browser.state_refused",
                "device": key.0,
                "display": display_id,
                "scheme": scheme,
            }));
            return false;
        }
        let title = Some(browser_title(&title)).filter(|title| !title.is_empty());
        let Some(display) = self
            .view_layout_of(&key)
            .and_then(|layout| layout.display(&display_id))
            .filter(|display| display.kind == DisplayKind::Browser)
        else {
            // A page's last report can land after its display closed.
            crate::diagnostic!(serde_json::json!({
                "component": "view_areas",
                "kind": "browser.state_stale",
                "device": key.0,
                "display": display_id,
            }));
            return false;
        };
        if display.url.as_deref() == Some(url.as_str()) && display.title == title {
            return false;
        }
        self.change_view_layout(&key, |layout, _| {
            let display = layout
                .display_mut(&display_id)
                .ok_or_else(|| LayoutError::UnknownDisplay(display_id.clone()))?;
            display.url = Some(url);
            display.title = title;
            Ok(((), true))
        })
        .is_ok()
    }

    /// A load stamp later than every one given in this process, and than the
    /// clock, so a host that outlived a daemon restart still sees a new one
    /// as newer than any it loaded.
    fn next_browser_load(&mut self) -> u64 {
        let now = unix_milliseconds();
        let Some(store) = self.workspace_views.as_mut() else {
            return now;
        };
        store.browser_load = now.max(store.browser_load + 1);
        store.browser_load
    }

    /// The Workspace of a pane on this Mac: the checkout whose tab holds it.
    fn pane_workspace_key(&self, pane_id: &str) -> Option<WorkspaceKey> {
        self.snapshot
            .navigator
            .workspaces
            .iter()
            .find_map(|workspace| {
                workspace
                    .checkouts
                    .iter()
                    .find(|checkout| {
                        checkout
                            .tabs
                            .iter()
                            .any(|tab| tab.panes.iter().any(|pane| pane.id == pane_id))
                    })
                    .map(|checkout| (workspace.device_id.clone(), checkout.path.clone()))
            })
    }

    fn catalog_has_workspace(&self, key: &WorkspaceKey) -> bool {
        self.catalog_workspaces().any(|workspace| {
            workspace.device_id == key.0
                && workspace
                    .checkouts
                    .iter()
                    .any(|checkout| checkout.path == key.1)
        })
    }

    fn split_request_seen(&self, key: &WorkspaceKey, request_id: &str) -> bool {
        self.workspace_views.as_ref().is_some_and(|store| {
            store
                .split_requests
                .get(key)
                .is_some_and(|seen| seen.iter().any(|id| id == request_id))
        })
    }

    fn remember_split_request(&mut self, key: &WorkspaceKey, request_id: String) {
        if let Some(store) = self.workspace_views.as_mut() {
            let seen = store.split_requests.entry(key.clone()).or_default();
            seen.push_back(request_id);
            if seen.len() > SPLIT_REQUESTS_KEPT {
                seen.pop_front();
            }
        }
    }

    /// Close View (B10): a display another display's document also shows,
    /// or one whose file was never read, goes alone, and a save sent with it
    /// is not needed. The last display of a document closes the document the
    /// way its file tab closes, after the save it waits on when there is
    /// one, or without saving on the operator's Don't Save (`discard`); the
    /// display goes when the document does, so a refused save keeps both.
    /// With neither, a last display of unsaved work is refused and nothing
    /// changes (contract 4.1): the web's frame may still have shown another
    /// view of it, so a close it sent bare can never drop the text.
    fn close_view_display(
        &mut self,
        key: &WorkspaceKey,
        display_id: &str,
        pending_save: Option<FileSavePayload>,
        discard: bool,
    ) -> bool {
        let Some(layout) = self.view_layout_of(key) else {
            return self.close_absent_display(key, display_id);
        };
        let Some(display) = layout.display(display_id) else {
            return self.close_absent_display(key, display_id);
        };
        let (path, kind, committed) = (display.path.clone(), display.kind, display.committed);
        let tab = display.tab_id.clone();
        let shared = layout.displays().any(|other| {
            other.id != display_id
                && match &tab {
                    Some(tab) => other.tab_id.as_ref() == Some(tab),
                    None => other.shows(&path, kind, committed),
                }
        });
        if let Some(tab_id) = tab.as_ref()
            && !shared
        {
            if let Some(save) = pending_save {
                return self.start_file_save_then_close(tab_id.clone(), save);
            }
            if !discard && self.document_kept(tab_id) {
                let label = self
                    .snapshot
                    .editor
                    .tabs
                    .iter()
                    .find(|held| &held.id == tab_id)
                    .map_or(path.as_str(), |held| held.label.as_str());
                let message = format!(
                    "{label} has changes that are not saved, so its last view stays open; close it with Save or Don't Save"
                );
                self.set_error("view_layout.unsaved", message, false);
                return true;
            }
            return self.close_file_tab_now(tab_id);
        }
        let _ = self.change_view_layout(key, |layout, _| {
            let removed = layout.remove(display_id).is_some();
            Ok(((), removed))
        });
        if tab.is_none()
            && !shared
            && kind == DisplayKind::File
            && let Some((workspace_id, checkout_id)) = self.front_checkout_owned()
        {
            self.cancel_document_read(&workspace_id, &checkout_id, &path);
        }
        true
    }

    /// A close of a display that is already gone, by an earlier send of the
    /// same close or by its document closing, has nothing left to do.
    fn close_absent_display(&self, key: &WorkspaceKey, display_id: &str) -> bool {
        crate::diagnostic!(serde_json::json!({
            "component": "view_areas",
            "kind": "view_layout.close_absent",
            "device": key.0,
            "display": display_id,
        }));
        false
    }

    /// Retry (B16): reads an unavailable display's file again into its tab.
    /// A display waiting for its device says what it waits for.
    fn retry_view_display(&mut self, key: &WorkspaceKey, display_id: &str) -> bool {
        let Some(display) = self
            .view_layout_of(key)
            .and_then(|layout| layout.display(display_id))
            .cloned()
        else {
            let error = LayoutError::UnknownDisplay(display_id.to_owned());
            self.set_error(error.kind(), error.message(), false);
            return true;
        };
        let front = self.front_checkout_owned();
        let (state, reason) = self.display_state(&self.display_facts(key), &display);
        match state {
            ViewDisplayState::Waiting => {
                self.set_error(
                    "view_layout.waiting",
                    reason.unwrap_or_else(|| "This view waits for its device".to_owned()),
                    false,
                );
                true
            }
            ViewDisplayState::Open | ViewDisplayState::Opening => false,
            ViewDisplayState::Unavailable => {
                let Some((workspace_id, checkout_id)) = front else {
                    return false;
                };
                // A diff is taken by the Changes read; its view needs only
                // its tab back, which the reconcile binds it to.
                if display.kind == DisplayKind::Diff {
                    self.insert_diff_tab(
                        &workspace_id,
                        &checkout_id,
                        &display.path,
                        display.committed.unwrap_or(false),
                        display.preview,
                    );
                    return true;
                }
                match self.document_source(&workspace_id, &checkout_id) {
                    Ok((root, channel)) => self.start_document_open(
                        root,
                        channel,
                        OpenRequestFields {
                            workspace_id,
                            checkout_id,
                            path: display.path,
                            preview: display.preview,
                            reload: false,
                            reveal: None,
                            restore: true,
                            placement: None,
                        },
                    ),
                    Err(message) => self.insert_unavailable_file_tab(
                        &workspace_id,
                        &checkout_id,
                        &display.path,
                        display.preview,
                        message,
                    ),
                }
                true
            }
        }
    }

    /// Reads back the documents of a Workspace's displays, the first time in
    /// this process that its files can be read (contract 5). Files are read
    /// on the document worker like any open; a file that cannot be read
    /// becomes an unavailable tab that says why. Diffs show at once. Nothing
    /// is started in Herdr, and nothing takes the screen: the tree was
    /// published already, and each display binds to its tab when it lands.
    pub(super) fn restore_view_displays(&mut self, key: &WorkspaceKey) {
        let Some((workspace_id, checkout_id)) = self.front_checkout_owned() else {
            return;
        };
        let Some(layout) = self.view_layout_of(key) else {
            return;
        };
        let areas = layout.area_count();
        let mut targets: Vec<(String, DisplayKind, Option<bool>)> = Vec::new();
        // A page has no document to read back; the host loads it once shown.
        for display in layout
            .displays()
            .filter(|display| display.kind != DisplayKind::Browser)
        {
            let target = (display.path.clone(), display.kind, display.committed);
            if display.tab_id.is_none() && !targets.contains(&target) {
                targets.push(target);
            }
        }
        let mut reads = 0usize;
        for (path, kind, committed) in &targets {
            let open = self.snapshot.editor.tabs.iter().any(|tab| {
                tab.workspace_id == workspace_id
                    && tab.checkout_id == checkout_id
                    && tab.path == *path
                    && display_kind(tab.kind) == Some(*kind)
                    && (*kind == DisplayKind::File || tab.diff_committed == *committed)
            });
            if open {
                continue;
            }
            match kind {
                DisplayKind::Browser => {}
                DisplayKind::Diff => {
                    self.insert_diff_tab(
                        &workspace_id,
                        &checkout_id,
                        path,
                        committed.unwrap_or(false),
                        false,
                    );
                }
                DisplayKind::File => {
                    reads += 1;
                    match self.document_source(&workspace_id, &checkout_id) {
                        Ok((root, channel)) => self.start_document_open(
                            root,
                            channel,
                            OpenRequestFields {
                                workspace_id: workspace_id.clone(),
                                checkout_id: checkout_id.clone(),
                                path: path.clone(),
                                preview: false,
                                reload: false,
                                reveal: None,
                                restore: true,
                                placement: None,
                            },
                        ),
                        Err(message) => self.insert_unavailable_file_tab(
                            &workspace_id,
                            &checkout_id,
                            path,
                            false,
                            message,
                        ),
                    }
                }
            }
        }
        crate::diagnostic!(serde_json::json!({
            "component": "workspace_views",
            "kind": "workspace_views.restore",
            "device": key.0,
            "areas": areas,
            "documents": targets.len(),
            "reads": reads,
        }));
    }

    /// Keeps every Workspace's displays in step with the editor's tabs, and
    /// the editor's active tab on the front Workspace's active display. Runs
    /// after every event and before every snapshot read; with nothing
    /// changed since the last pass it costs one comparison of the editor's
    /// tabs.
    pub(super) fn reconcile_view_displays(&mut self) {
        let Some(store) = self.workspace_views.as_ref() else {
            return;
        };
        if store.reconciled.as_ref().is_some_and(|seen| {
            seen.generation == store.generation
                && seen.front == store.front
                && seen.active == self.snapshot.editor.active_tab_id
                && seen.tabs == self.snapshot.editor.tabs
        }) {
            return;
        }
        // An active tab the last pass did not set was activated by an event.
        let requested = self
            .snapshot
            .editor
            .active_tab_id
            .clone()
            .filter(|active| store.derived_active.as_ref() != Some(active));
        let tabs: Vec<TabFacts> = self
            .snapshot
            .editor
            .tabs
            .iter()
            .filter_map(|tab| {
                Some(TabFacts {
                    id: tab.id.clone(),
                    key: self.workspace_key(&tab.workspace_id, &tab.checkout_id),
                    path: tab.path.clone(),
                    kind: display_kind(tab.kind)?,
                    committed: tab.diff_committed,
                    kept: tab.dirty
                        || self
                            .editor_documents
                            .get(&tab.id)
                            .is_some_and(|document| document.save.is_some()),
                })
            })
            .collect();
        let now = unix_milliseconds();
        let store = self.workspace_views.as_mut().expect("checked above");
        let mut stored = false;
        let mut every_preview: HashMap<String, bool> = HashMap::new();
        let mut unshown: Vec<(String, String, String)> = Vec::new();
        for view in &mut store.views.workspaces {
            let key = (view.device_id.clone(), view.path.clone());
            let requested = requested.as_deref().filter(|requested| {
                tabs.iter()
                    .any(|tab| tab.id == *requested && tab.key.as_ref() == Some(&key))
            });
            let (changed, capped) = reconcile_layout(
                &mut view.layout,
                &key,
                store.live.contains(&key),
                &tabs,
                requested,
                now,
            );
            stored |= changed;
            unshown.extend(
                capped
                    .into_iter()
                    .map(|tab| (tab.id.clone(), tab.path.clone(), key.1.clone())),
            );
            for display in view.layout.displays() {
                if let Some(tab_id) = &display.tab_id {
                    *every_preview.entry(tab_id.clone()).or_insert(true) &= display.preview;
                }
            }
        }
        // A document the cap keeps off screen is reported once, not on every
        // pass that still finds it there.
        let reported = std::mem::take(&mut store.unshown);
        store.unshown = unshown
            .iter()
            .map(|(tab_id, _, _)| tab_id.clone())
            .collect();
        if stored {
            store.generation += 1;
            self.persist_workspace_views();
        }
        for (tab_id, path, workspace) in unshown {
            if !reported.contains(&tab_id) {
                self.push_diagnostic(
                    "view_layout.display_limit",
                    format!(
                        "{path} is open but not shown: {workspace} already has {MAX_VIEW_DISPLAYS} views, and it shows once one is closed"
                    ),
                );
            }
        }
        // The tab's own flag says what its displays say.
        let mut strips = false;
        for tab in &mut self.snapshot.editor.tabs {
            if let Some(every) = every_preview.get(&tab.id)
                && tab.preview != *every
            {
                tab.preview = *every;
                strips = true;
            }
        }
        if strips {
            self.rebuild_tab_strips();
        }
        let derived = self.front_active_view_tab();
        if derived != self.snapshot.editor.active_tab_id {
            match derived.as_deref() {
                Some(tab_id) => {
                    if let Err(message) = self.activate_editor_tab(tab_id) {
                        self.set_error("editor.focus_failed", message, false);
                    }
                }
                None => self.deactivate_editor_tab(),
            }
        }
        let store = self.workspace_views.as_mut().expect("checked above");
        store.derived_active = self.snapshot.editor.active_tab_id.clone();
        store.reconciled = Some(Reconciled {
            tabs: self.snapshot.editor.tabs.clone(),
            active: self.snapshot.editor.active_tab_id.clone(),
            front: store.front.clone(),
            generation: store.generation,
        });
    }

    /// The document the front Workspace's active area shows, while it can
    /// show it (contract 3).
    fn front_active_view_tab(&self) -> Option<String> {
        let store = self.workspace_views.as_ref()?;
        let key = store.front.as_ref()?;
        let layout = &store.views.get(&key.0, &key.1)?.layout;
        let area = layout.active_area();
        let tab_id = area
            .active
            .as_ref()
            .and_then(|active| area.displays.iter().find(|display| &display.id == active))?
            .tab_id
            .as_ref()?;
        let tab = self
            .snapshot
            .editor
            .tabs
            .iter()
            .find(|tab| &tab.id == tab_id)?;
        let open = tab.kind == EditorTabKind::Diff
            || (tab.unavailable_reason.is_none() && self.editor_documents.contains_key(tab_id));
        open.then(|| tab_id.clone())
    }

    /// The front Workspace's area-active displays while its mode shows
    /// Views: what is on screen, one per area, in tree order.
    fn visible_view_displays(&self) -> Vec<&Display> {
        let Some(store) = self.workspace_views.as_ref() else {
            return Vec::new();
        };
        let Some(view) = store
            .front
            .as_ref()
            .and_then(|key| store.views.get(&key.0, &key.1))
        else {
            return Vec::new();
        };
        if !view.mode.shows_views() {
            return Vec::new();
        }
        view.layout
            .areas()
            .into_iter()
            .filter_map(|area| {
                let active = area.active.as_ref()?;
                area.displays.iter().find(|display| &display.id == active)
            })
            .collect()
    }

    /// The file documents on screen, which the snapshot's `documents`
    /// section carries (contract 3.1).
    pub(super) fn visible_view_documents(&self) -> Vec<String> {
        let mut visible: Vec<String> = Vec::new();
        for display in self.visible_view_displays() {
            if display.kind != DisplayKind::File {
                continue;
            }
            let Some(tab_id) = display.tab_id.as_ref() else {
                continue;
            };
            if self.editor_documents.contains_key(tab_id) && !visible.contains(tab_id) {
                visible.push(tab_id.clone());
            }
        }
        visible
    }

    /// The diffs on screen, which the Changes read takes with its own
    /// (contract 3.2), by absolute path.
    pub(super) fn visible_view_diffs(&self) -> Vec<hide_host::git::DiffTarget> {
        let mut visible: Vec<hide_host::git::DiffTarget> = Vec::new();
        for display in self.visible_view_displays() {
            if display.kind != DisplayKind::Diff || display.tab_id.is_none() {
                continue;
            }
            let target = hide_host::git::DiffTarget {
                path: display.path.clone(),
                committed: display.committed.unwrap_or(false),
            };
            if !visible.contains(&target) {
                visible.push(target);
            }
        }
        visible
    }

    /// Why a Workspace's files cannot be read yet, or `None` when they can.
    /// A device's files are read through its helper, and its Workspace also
    /// waits for its catalog, which moves the device's checkouts into their
    /// Projects; this machine's wait for the daemon to open the checkout's
    /// root. A helper that cannot be used gives the device's own reason, the
    /// one History and Settings show, and its displays still wait, so they
    /// open once the device is fixed.
    pub(super) fn view_root_wait(&self, key: &WorkspaceKey) -> Option<String> {
        if key.0 != workspace::LOCAL_DEVICE_ID {
            let label = self
                .snapshot
                .ui_state
                .device_registrations
                .iter()
                .find(|device| device.id == key.0)
                .map_or(key.0.as_str(), |device| device.label.as_str());
            match self.device_hosts.get(&key.0).map(|host| &host.phase) {
                None | Some(hosts::HostPhase::Connecting) => {
                    return Some(format!("Waiting for {label} to connect"));
                }
                Some(hosts::HostPhase::Ready { host, .. }) if host.closed_reason().is_none() => {}
                Some(_) => {
                    return Some(
                        self.host_snapshot(&key.0)
                            .message
                            .unwrap_or_else(|| format!("Waiting for {label} to connect")),
                    );
                }
            }
            let catalog_ready = self
                .snapshot
                .status
                .remote
                .iter()
                .find(|status| status.target_id == key.0)
                .is_some_and(|status| status.catalog.state == "ready");
            return (!catalog_ready).then(|| format!("Waiting for {label} to list its projects"));
        }
        let pinned = self
            .file_roots
            .as_ref()
            .is_some_and(|roots| roots.pinned_root(Path::new(&key.1)).is_some());
        (!pinned).then(|| "Waiting for this checkout to open".to_owned())
    }

    /// What a display's state reads that is the same for every display of
    /// the Workspace `key`, taken once per tree rather than per display.
    fn display_facts(&self, key: &WorkspaceKey) -> DisplayFacts<'_> {
        DisplayFacts {
            front: self.front_checkout(),
            tabs: self
                .snapshot
                .editor
                .tabs
                .iter()
                .map(|tab| (tab.id.as_str(), tab))
                .collect(),
            wait: self.view_root_wait(key),
        }
    }

    /// What one display can show now, and why not when it cannot.
    fn display_state(
        &self,
        facts: &DisplayFacts<'_>,
        display: &Display,
    ) -> (ViewDisplayState, Option<String>) {
        // The host loads a page wherever it is; whether it loaded is the
        // page's own state, which the host draws.
        if display.kind == DisplayKind::Browser {
            return (ViewDisplayState::Open, None);
        }
        if let Some(tab) = display.tab_id.as_deref().and_then(|id| facts.tabs.get(id)) {
            return match (tab.kind, &tab.unavailable_reason) {
                (EditorTabKind::Diff, _) => (ViewDisplayState::Open, None),
                // Retry reads the file again into the same tab, which keeps
                // its old reason until the read lands.
                (_, Some(_))
                    if self.document_opens_path(&tab.workspace_id, &tab.checkout_id, &tab.path) =>
                {
                    (ViewDisplayState::Opening, None)
                }
                (_, Some(reason)) => (ViewDisplayState::Unavailable, Some(reason.clone())),
                _ if self.editor_documents.contains_key(&tab.id) => (ViewDisplayState::Open, None),
                _ => (
                    ViewDisplayState::Unavailable,
                    Some("This file has no document to show".to_owned()),
                ),
            };
        }
        if let Some(reason) = &facts.wait {
            return (ViewDisplayState::Waiting, Some(reason.clone()));
        }
        if display.kind == DisplayKind::File
            && facts.front.is_some_and(|(workspace_id, checkout_id)| {
                self.document_opens_path(workspace_id, checkout_id, &display.path)
            })
        {
            return (ViewDisplayState::Opening, None);
        }
        let reason = match display.kind {
            DisplayKind::File | DisplayKind::Browser => {
                "This view's file is not open; Retry reads it again"
            }
            DisplayKind::Diff => "This view's diff is not open; Retry opens it again",
        };
        (ViewDisplayState::Unavailable, Some(reason.to_owned()))
    }

    /// The front Workspace's tree as the snapshot carries it. It runs on
    /// every snapshot read, over at most `MAX_VIEW_DISPLAYS` displays: one
    /// map of the editor's tabs and one root check, then a map lookup per
    /// display.
    pub(super) fn view_layout_snapshot(
        &self,
        key: &WorkspaceKey,
        layout: &Layout,
    ) -> ViewLayoutSnapshot {
        let facts = self.display_facts(key);
        ViewLayoutSnapshot {
            root: self.view_node_snapshot(&facts, &layout.root),
            active_area: layout.active_area().id.clone(),
            limits: ViewLimitsSnapshot {
                areas: MAX_VIEW_AREAS,
                depth: MAX_SPLIT_DEPTH,
                displays: MAX_VIEW_DISPLAYS,
            },
            display_count: layout.display_count(),
        }
    }

    fn view_node_snapshot(&self, facts: &DisplayFacts<'_>, node: &Node) -> ViewNodeSnapshot {
        match node {
            Node::Area(area) => ViewNodeSnapshot::Area(ViewAreaSnapshot {
                id: area.id.clone(),
                active: area.active.clone(),
                displays: area
                    .displays
                    .iter()
                    .map(|display| {
                        let (state, reason) = self.display_state(facts, display);
                        let tab = display
                            .tab_id
                            .as_deref()
                            .and_then(|id| facts.tabs.get(id).copied());
                        ViewDisplaySnapshot {
                            id: display.id.clone(),
                            tab_id: tab.map(|tab| tab.id.clone()),
                            path: display.path.clone(),
                            label: match (tab, display.kind) {
                                (_, DisplayKind::Browser) => browser_label(display),
                                (Some(tab), _) => tab.label.clone(),
                                (None, DisplayKind::File) => file_label(&display.path),
                                (None, DisplayKind::Diff) => super::editor::diff_label(
                                    &display.path,
                                    display.committed.unwrap_or(false),
                                ),
                            },
                            kind: display.kind,
                            committed: display.committed,
                            preview: display.preview,
                            state,
                            reason,
                            url: display.url.clone(),
                            title: display.title.clone(),
                            load: display.load,
                        }
                    })
                    .collect(),
            }),
            Node::Split(split) => ViewNodeSnapshot::Split(ViewSplitSnapshot {
                id: split.id.clone(),
                axis: split.axis,
                ratio: split.ratio,
                first: Box::new(self.view_node_snapshot(facts, &split.first)),
                second: Box::new(self.view_node_snapshot(facts, &split.second)),
            }),
        }
    }
}

/// A page's name on its tab: its title once it has one, else its host (for
/// a file, its file name), else its address.
fn browser_label(display: &Display) -> String {
    if let Some(title) = display.title.as_deref().filter(|title| !title.is_empty()) {
        return title.to_owned();
    }
    let url = display
        .url
        .as_deref()
        .unwrap_or(crate::view_layout::BLANK_PAGE);
    if is_file_address(url) {
        let path = url.split(['?', '#']).next().unwrap_or(url);
        let name = path.rsplit('/').next().unwrap_or(path);
        return file_label(&percent_encoding::percent_decode_str(name).decode_utf8_lossy());
    }
    url.split_once("://")
        .map(|(_, rest)| rest.split(['/', '?', '#']).next().unwrap_or(rest))
        .filter(|host| !host.is_empty())
        .unwrap_or(url)
        .to_owned()
}
