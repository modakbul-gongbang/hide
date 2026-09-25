//! One Workspace's View areas (PRD S7 D-04, D-06, D-11, D-12): a binary tree
//! of splits whose leaves are areas, each holding an ordered strip of
//! displays, and the pure operations that focus, move, split, resize and
//! close them.
//!
//! A display shows one document, a file or a diff, and names it by what it
//! shows (path, kind, Changes group) so the tree survives a restart that
//! mints new editor tab ids. The runtime binds each display to the editor tab
//! that holds its document (`tab_id`, never stored); several displays may
//! bind one tab, which is how two areas show one buffer (D-03).
//!
//! Nothing here knows the runtime. Every operation applies completely or
//! returns why it cannot and leaves the tree as it was, so one operator
//! action is one change (B18). The caps are the core's; the shell enforces
//! pixel minimums on top of them because only it has the geometry.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

/// View areas one Workspace may hold at once.
pub const MAX_VIEW_AREAS: usize = 6;
/// Split nodes on the path from the root to any area.
pub const MAX_SPLIT_DEPTH: usize = 3;
/// Displays one Workspace may hold at once, across all its areas.
pub const MAX_VIEW_DISPLAYS: usize = 64;
/// The first child's share of a split; the bounds keep either side usable.
pub const MIN_SPLIT_RATIO: f32 = 0.15;
pub const MAX_SPLIT_RATIO: f32 = 0.85;
const DEFAULT_SPLIT_RATIO: f32 = 0.5;

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DisplayKind {
    File,
    Diff,
}

/// `row` puts the first child left of the second, `column` above it.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SplitAxis {
    Row,
    Column,
}

/// A side of an area: where a split puts the new area, or which way a
/// neighbour lies.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Edge {
    Left,
    Right,
    Up,
    Down,
}

impl Edge {
    fn axis(self) -> SplitAxis {
        match self {
            Self::Left | Self::Right => SplitAxis::Row,
            Self::Up | Self::Down => SplitAxis::Column,
        }
    }

    /// Whether this side comes second in a split: right of or below.
    fn after(self) -> bool {
        matches!(self, Self::Right | Self::Down)
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Display {
    pub id: String,
    pub path: String,
    pub kind: DisplayKind,
    /// The Changes group a diff compares against; absent for a file.
    #[serde(default)]
    pub committed: Option<bool>,
    #[serde(default)]
    pub preview: bool,
    #[serde(default)]
    pub last_focused_unix_ms: u64,
    /// The editor tab holding this display's document, bound at runtime and
    /// never stored: a restart mints new tab ids.
    #[serde(skip)]
    pub tab_id: Option<String>,
}

impl Display {
    /// Whether this display shows the document named by `path`, `kind` and,
    /// for a diff, its Changes group.
    pub fn shows(&self, path: &str, kind: DisplayKind, committed: Option<bool>) -> bool {
        self.path == path
            && self.kind == kind
            && (kind == DisplayKind::File || self.committed == committed)
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Area {
    pub id: String,
    /// The display the area shows; `None` only when it holds none.
    #[serde(default)]
    pub active: Option<String>,
    #[serde(default)]
    pub displays: Vec<Display>,
}

impl Area {
    fn empty(id: String) -> Self {
        Self {
            id,
            active: None,
            displays: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Split {
    pub id: String,
    pub axis: SplitAxis,
    pub ratio: f32,
    pub first: Box<Node>,
    pub second: Box<Node>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Node {
    Area(Area),
    Split(Split),
}

impl Node {
    fn collect_areas<'a>(&'a self, out: &mut Vec<&'a Area>) {
        match self {
            Self::Area(area) => out.push(area),
            Self::Split(split) => {
                split.first.collect_areas(out);
                split.second.collect_areas(out);
            }
        }
    }

    fn collect_displays_mut<'a>(&'a mut self, out: &mut Vec<&'a mut Display>) {
        match self {
            Self::Area(area) => out.extend(area.displays.iter_mut()),
            Self::Split(split) => {
                split.first.collect_displays_mut(out);
                split.second.collect_displays_mut(out);
            }
        }
    }

    fn area_mut(&mut self, id: &str) -> Option<&mut Area> {
        match self {
            Self::Area(area) => (area.id == id).then_some(area),
            Self::Split(split) => match split.first.area_mut(id) {
                Some(area) => Some(area),
                None => split.second.area_mut(id),
            },
        }
    }

    fn split_mut(&mut self, id: &str) -> Option<&mut Split> {
        let Self::Split(split) = self else {
            return None;
        };
        if split.id == id {
            return Some(split);
        }
        match split.first.split_mut(id) {
            Some(found) => Some(found),
            None => split.second.split_mut(id),
        }
    }

    fn first_area(&self) -> &Area {
        match self {
            Self::Area(area) => area,
            Self::Split(split) => split.first.first_area(),
        }
    }

    fn last_area(&self) -> &Area {
        match self {
            Self::Area(area) => area,
            Self::Split(split) => split.second.last_area(),
        }
    }

    fn depth(&self) -> usize {
        match self {
            Self::Area(_) => 0,
            Self::Split(split) => 1 + split.first.depth().max(split.second.depth()),
        }
    }

    /// Puts `fresh` at `edge` of the area `area_id`, the two sharing that
    /// area's space in a new split. Returns whether the area was found.
    fn wrap_area(
        &mut self,
        area_id: &str,
        edge: Edge,
        fresh: &mut Option<Area>,
        split_id: &str,
    ) -> bool {
        if matches!(self, Self::Area(area) if area.id == area_id) {
            let Some(fresh) = fresh.take() else {
                return false;
            };
            let old = std::mem::replace(self, Self::Area(Area::empty(String::new())));
            let fresh = Self::Area(fresh);
            let (first, second) = if edge.after() {
                (old, fresh)
            } else {
                (fresh, old)
            };
            *self = Self::Split(Split {
                id: split_id.to_owned(),
                axis: edge.axis(),
                ratio: DEFAULT_SPLIT_RATIO,
                first: Box::new(first),
                second: Box::new(second),
            });
            return true;
        }
        match self {
            Self::Area(_) => false,
            Self::Split(split) => {
                split.first.wrap_area(area_id, edge, fresh, split_id)
                    || split.second.wrap_area(area_id, edge, fresh, split_id)
            }
        }
    }

    /// Replaces the split that directly holds the area `area_id` with its
    /// other child, which takes the space. Returns the area of that child
    /// nearest the removed one.
    fn remove_area(&mut self, area_id: &str) -> Option<String> {
        let (first_is, second_is) = match self {
            Self::Area(_) => return None,
            Self::Split(split) => (
                matches!(&*split.first, Self::Area(area) if area.id == area_id),
                matches!(&*split.second, Self::Area(area) if area.id == area_id),
            ),
        };
        if first_is || second_is {
            let Self::Split(split) =
                std::mem::replace(self, Self::Area(Area::empty(String::new())))
            else {
                unreachable!("matched a split above");
            };
            let sibling = if first_is {
                *split.second
            } else {
                *split.first
            };
            let nearest = if first_is {
                sibling.first_area().id.clone()
            } else {
                sibling.last_area().id.clone()
            };
            *self = sibling;
            return Some(nearest);
        }
        match self {
            Self::Area(_) => None,
            Self::Split(split) => split
                .first
                .remove_area(area_id)
                .or_else(|| split.second.remove_area(area_id)),
        }
    }

    /// Replaces every split whose areas would sit deeper than the cap with
    /// one area holding their displays in order. `depth` is this node's.
    fn flatten_below(&mut self, depth: usize, notes: &mut Vec<String>) {
        if !matches!(self, Self::Split(_)) {
            return;
        }
        if depth >= MAX_SPLIT_DEPTH {
            let merged = merge_areas(self);
            notes.push(format!(
                "areas deeper than {MAX_SPLIT_DEPTH} splits were merged into {}",
                merged.id
            ));
            *self = Self::Area(merged);
            return;
        }
        if let Self::Split(split) = self {
            split.first.flatten_below(depth + 1, notes);
            split.second.flatten_below(depth + 1, notes);
        }
    }

    /// Merges the last split whose children are both areas into one area.
    fn merge_last_pair(&mut self) -> Option<String> {
        let Self::Split(split) = self else {
            return None;
        };
        if let Some(merged) = split.second.merge_last_pair() {
            return Some(merged);
        }
        if let Some(merged) = split.first.merge_last_pair() {
            return Some(merged);
        }
        let merged = merge_areas(self);
        let id = merged.id.clone();
        *self = Self::Area(merged);
        Some(id)
    }

    fn visit_mut(&mut self, visit: &mut impl FnMut(&mut Self)) {
        visit(self);
        if let Self::Split(split) = self {
            split.first.visit_mut(visit);
            split.second.visit_mut(visit);
        }
    }
}

/// One area holding every display of `node` in order, named after its first
/// area.
fn merge_areas(node: &Node) -> Area {
    let mut areas = Vec::new();
    node.collect_areas(&mut areas);
    let mut merged = Area::empty(areas[0].id.clone());
    merged.active = areas[0].active.clone();
    for area in areas {
        merged.displays.extend(area.displays.iter().cloned());
    }
    merged
}

/// Why an operation was refused. Nothing changed when one is returned.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LayoutError {
    UnknownDisplay(String),
    UnknownArea(String),
    UnknownSplit(String),
    AreaLimit,
    DepthLimit,
    DisplayLimit,
    NothingToSplit,
    InvalidRatio,
}

impl LayoutError {
    /// The stable error kind the shell matches a refusal by.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::UnknownDisplay(_) => "view_layout.unknown_display",
            Self::UnknownArea(_) => "view_layout.unknown_area",
            Self::UnknownSplit(_) => "view_layout.unknown_split",
            Self::AreaLimit | Self::DepthLimit => "view_layout.limit",
            Self::DisplayLimit => "view_layout.display_limit",
            Self::NothingToSplit => "view_layout.nothing_to_split",
            Self::InvalidRatio => "view_layout.invalid_ratio",
        }
    }

    pub fn message(&self) -> String {
        match self {
            Self::UnknownDisplay(id) => format!("View {id} is not open in this Workspace"),
            Self::UnknownArea(id) => format!("View area {id} is not in this Workspace"),
            Self::UnknownSplit(id) => format!("Divider {id} is not in this Workspace"),
            Self::AreaLimit => format!(
                "This Workspace already has {MAX_VIEW_AREAS} view areas. Close a view to split again."
            ),
            Self::DepthLimit => format!(
                "This view area is already split {MAX_SPLIT_DEPTH} levels deep. Split another area instead."
            ),
            Self::DisplayLimit => format!(
                "This Workspace has {MAX_VIEW_DISPLAYS} views open. Close a view to open another."
            ),
            Self::NothingToSplit => {
                "This view is the only one in its area, so splitting it there would change nothing"
                    .to_owned()
            }
            Self::InvalidRatio => "A view area's share must be a number".to_owned(),
        }
    }
}

/// A split ratio inside the bounds, or `None` for a value that is not one.
pub fn clamp_ratio(ratio: f32) -> Option<f32> {
    ratio
        .is_finite()
        .then(|| ratio.clamp(MIN_SPLIT_RATIO, MAX_SPLIT_RATIO))
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Layout {
    pub root: Node,
    /// The area used last: where an open lands (B1).
    pub active_area: String,
    /// The next number an id takes; ids are never reused in a Workspace.
    pub next_id: u64,
}

impl Default for Layout {
    /// One empty area, which is what a Workspace with no View has.
    fn default() -> Self {
        Self {
            root: Node::Area(Area::empty("a1".to_owned())),
            active_area: "a1".to_owned(),
            next_id: 2,
        }
    }
}

impl Layout {
    /// The areas in tree order: left to right, top to bottom.
    pub fn areas(&self) -> Vec<&Area> {
        let mut areas = Vec::new();
        self.root.collect_areas(&mut areas);
        areas
    }

    pub fn area(&self, id: &str) -> Option<&Area> {
        self.areas().into_iter().find(|area| area.id == id)
    }

    pub fn area_mut(&mut self, id: &str) -> Option<&mut Area> {
        self.root.area_mut(id)
    }

    /// The area in use; the first one if the stored name is stale.
    pub fn active_area(&self) -> &Area {
        self.area(&self.active_area)
            .unwrap_or_else(|| self.root.first_area())
    }

    pub fn area_count(&self) -> usize {
        self.areas().len()
    }

    pub fn depth(&self) -> usize {
        self.root.depth()
    }

    /// Every display in tree order.
    pub fn displays(&self) -> impl Iterator<Item = &Display> {
        self.areas()
            .into_iter()
            .flat_map(|area| area.displays.iter())
    }

    pub fn displays_mut(&mut self) -> Vec<&mut Display> {
        let mut displays = Vec::new();
        self.root.collect_displays_mut(&mut displays);
        displays
    }

    pub fn display_count(&self) -> usize {
        self.areas().iter().map(|area| area.displays.len()).sum()
    }

    pub fn display(&self, id: &str) -> Option<&Display> {
        self.displays().find(|display| display.id == id)
    }

    pub fn display_mut(&mut self, id: &str) -> Option<&mut Display> {
        self.displays_mut()
            .into_iter()
            .find(|display| display.id == id)
    }

    /// The area holding the display `id`.
    pub fn area_of(&self, display_id: &str) -> Option<&Area> {
        self.areas()
            .into_iter()
            .find(|area| area.displays.iter().any(|display| display.id == display_id))
    }

    /// A focus stamp later than every one in the tree, so the most recently
    /// focused display is unambiguous even within one millisecond.
    pub fn next_stamp(&self, now_unix_ms: u64) -> u64 {
        let latest = self
            .displays()
            .map(|display| display.last_focused_unix_ms)
            .max()
            .unwrap_or(0);
        now_unix_ms.max(latest + 1)
    }

    fn mint(&mut self, prefix: char) -> String {
        let id = format!("{prefix}{}", self.next_id);
        self.next_id += 1;
        id
    }

    /// A new display with a fresh id, not yet in any area.
    pub fn new_display(
        &mut self,
        path: &str,
        kind: DisplayKind,
        committed: Option<bool>,
        preview: bool,
    ) -> Display {
        Display {
            id: self.mint('d'),
            path: path.to_owned(),
            kind,
            committed: (kind == DisplayKind::Diff).then_some(committed.unwrap_or(false)),
            preview,
            last_focused_unix_ms: 0,
            tab_id: None,
        }
    }

    /// Adds `display` to the end of `area_id` as its active display, and
    /// makes that area the one in use.
    pub fn insert(
        &mut self,
        area_id: &str,
        mut display: Display,
        stamp: u64,
    ) -> Result<(), LayoutError> {
        self.check_room()?;
        let area = self
            .root
            .area_mut(area_id)
            .ok_or_else(|| LayoutError::UnknownArea(area_id.to_owned()))?;
        display.last_focused_unix_ms = stamp;
        area.active = Some(display.id.clone());
        area.displays.push(display);
        self.active_area = area_id.to_owned();
        Ok(())
    }

    /// Adds `display` to the end of `area_id` without moving the focus: the
    /// area shows it only when it had nothing to show.
    pub fn append(&mut self, area_id: &str, display: Display) -> Result<(), LayoutError> {
        self.check_room()?;
        let area = self
            .root
            .area_mut(area_id)
            .ok_or_else(|| LayoutError::UnknownArea(area_id.to_owned()))?;
        if area.active.is_none() {
            area.active = Some(display.id.clone());
        }
        area.displays.push(display);
        Ok(())
    }

    /// Takes a display out. Its area shows its most recently focused
    /// remaining display; an area left empty collapses and its sibling takes
    /// the space, except the last area, which stays empty (B10).
    pub fn remove(&mut self, display_id: &str) -> Option<Display> {
        let area_id = self.area_of(display_id)?.id.clone();
        let display = self.take(&area_id, display_id)?;
        self.collapse_if_empty(&area_id);
        Some(display)
    }

    fn take(&mut self, area_id: &str, display_id: &str) -> Option<Display> {
        let area = self.root.area_mut(area_id)?;
        let index = area
            .displays
            .iter()
            .position(|display| display.id == display_id)?;
        let display = area.displays.remove(index);
        if area.active.as_deref() == Some(display_id) {
            area.active = successor(&area.displays, index);
        }
        Some(display)
    }

    fn collapse_if_empty(&mut self, area_id: &str) {
        if !self
            .area(area_id)
            .is_some_and(|area| area.displays.is_empty())
        {
            return;
        }
        if let Some(nearest) = self.root.remove_area(area_id)
            && self.active_area == area_id
        {
            self.active_area = nearest;
        }
    }

    /// Makes a display its area's active display and that area the one in
    /// use. Returns whether that changed anything; focusing the display
    /// already in use is a no-op and stamps nothing.
    pub fn focus(&mut self, display_id: &str, stamp: u64) -> Result<bool, LayoutError> {
        let area_id = self
            .area_of(display_id)
            .ok_or_else(|| LayoutError::UnknownDisplay(display_id.to_owned()))?
            .id
            .clone();
        let area = self.root.area_mut(&area_id).expect("found above");
        if self.active_area == area_id && area.active.as_deref() == Some(display_id) {
            return Ok(false);
        }
        area.active = Some(display_id.to_owned());
        if let Some(display) = area
            .displays
            .iter_mut()
            .find(|display| display.id == display_id)
        {
            display.last_focused_unix_ms = stamp;
        }
        self.active_area = area_id;
        Ok(true)
    }

    /// Makes an area the one in use; its active display counts as focused.
    pub fn focus_area(&mut self, area_id: &str, stamp: u64) -> Result<bool, LayoutError> {
        if self.area(area_id).is_none() {
            return Err(LayoutError::UnknownArea(area_id.to_owned()));
        }
        if self.active_area == area_id {
            return Ok(false);
        }
        self.active_area = area_id.to_owned();
        let area = self.root.area_mut(area_id).expect("found above");
        if let Some(active) = area.active.clone()
            && let Some(display) = area
                .displays
                .iter_mut()
                .find(|display| display.id == active)
        {
            display.last_focused_unix_ms = stamp;
        }
        Ok(true)
    }

    /// Puts a display at `index` of `area_id` (clamped), reordering within
    /// its area or moving it to another; it becomes that area's active
    /// display, the area becomes the one in use, and an area it leaves empty
    /// collapses. A display that changes place is kept open (a drag keeps a
    /// preview, D-02). The index is the display's place after the move, so
    /// the same move twice changes nothing the second time.
    pub fn move_display(
        &mut self,
        display_id: &str,
        area_id: &str,
        index: usize,
        stamp: u64,
    ) -> Result<bool, LayoutError> {
        let source = self
            .area_of(display_id)
            .ok_or_else(|| LayoutError::UnknownDisplay(display_id.to_owned()))?;
        let source_id = source.id.clone();
        let from = source
            .displays
            .iter()
            .position(|display| display.id == display_id)
            .expect("the area holds it");
        let target = self
            .area(area_id)
            .ok_or_else(|| LayoutError::UnknownArea(area_id.to_owned()))?;
        let to = if source_id == area_id {
            index.min(target.displays.len() - 1)
        } else {
            index.min(target.displays.len())
        };
        let placed = source_id == area_id && from == to;
        if placed && self.active_area == area_id && target.active.as_deref() == Some(display_id) {
            return Ok(false);
        }
        let mut display = self.take(&source_id, display_id).expect("found above");
        if !placed {
            display.preview = false;
        }
        display.last_focused_unix_ms = stamp;
        let area = self.root.area_mut(area_id).expect("found above");
        area.active = Some(display_id.to_owned());
        area.displays.insert(to, display);
        self.active_area = area_id.to_owned();
        if source_id != area_id {
            self.collapse_if_empty(&source_id);
        }
        Ok(true)
    }

    /// Moves a display into a new area at `edge` of `area_id`, which gives
    /// the new area half its space (B7). Refused when the result would pass
    /// the area or depth cap, or when the display is the only one of the
    /// area it would split. Returns the new area's id.
    pub fn split(
        &mut self,
        display_id: &str,
        area_id: &str,
        edge: Edge,
        stamp: u64,
    ) -> Result<String, LayoutError> {
        let source = self
            .area_of(display_id)
            .ok_or_else(|| LayoutError::UnknownDisplay(display_id.to_owned()))?;
        let source_id = source.id.clone();
        let alone = source.displays.len() == 1;
        if self.area(area_id).is_none() {
            return Err(LayoutError::UnknownArea(area_id.to_owned()));
        }
        if source_id == area_id && alone {
            return Err(LayoutError::NothingToSplit);
        }
        let mut next = self.clone();
        let mut display = next.take(&source_id, display_id).expect("found above");
        display.preview = false;
        let fresh = next.put_beside(area_id, edge, display, stamp);
        next.collapse_if_empty(&source_id);
        next.check_caps()?;
        *self = next;
        Ok(fresh)
    }

    /// Puts a display that is in no area yet into a new area at `edge` of
    /// `area_id` (Open to the side, B4). Returns the new area's id.
    pub fn split_new(
        &mut self,
        area_id: &str,
        edge: Edge,
        display: Display,
        stamp: u64,
    ) -> Result<String, LayoutError> {
        if self.area(area_id).is_none() {
            return Err(LayoutError::UnknownArea(area_id.to_owned()));
        }
        self.check_room()?;
        let mut next = self.clone();
        let fresh = next.put_beside(area_id, edge, display, stamp);
        next.check_caps()?;
        *self = next;
        Ok(fresh)
    }

    /// Whether a new area at `edge` of `area_id` would stay inside the caps.
    pub fn can_split(&self, area_id: &str, edge: Edge) -> Result<(), LayoutError> {
        let mut probe = self.clone();
        let display = probe.new_display("", DisplayKind::File, None, false);
        probe.split_new(area_id, edge, display, 0).map(|_| ())
    }

    fn put_beside(
        &mut self,
        area_id: &str,
        edge: Edge,
        mut display: Display,
        stamp: u64,
    ) -> String {
        let fresh = self.mint('a');
        let split_id = self.mint('s');
        display.last_focused_unix_ms = stamp;
        let mut area = Some(Area {
            id: fresh.clone(),
            active: Some(display.id.clone()),
            displays: vec![display],
        });
        let wrapped = self.root.wrap_area(area_id, edge, &mut area, &split_id);
        debug_assert!(wrapped, "the caller checked the area exists");
        self.active_area = fresh.clone();
        fresh
    }

    /// Every operation that adds a display refuses the one past the cap, so
    /// no path can take a Workspace there.
    fn check_room(&self) -> Result<(), LayoutError> {
        if self.display_count() >= MAX_VIEW_DISPLAYS {
            return Err(LayoutError::DisplayLimit);
        }
        Ok(())
    }

    fn check_caps(&self) -> Result<(), LayoutError> {
        if self.area_count() > MAX_VIEW_AREAS {
            return Err(LayoutError::AreaLimit);
        }
        if self.depth() > MAX_SPLIT_DEPTH {
            return Err(LayoutError::DepthLimit);
        }
        Ok(())
    }

    /// Sets a split's first-child share, clamped. A target state: the same
    /// ratio twice changes nothing the second time.
    pub fn resize(&mut self, split_id: &str, ratio: f32) -> Result<bool, LayoutError> {
        let ratio = clamp_ratio(ratio).ok_or(LayoutError::InvalidRatio)?;
        let split = self
            .root
            .split_mut(split_id)
            .ok_or_else(|| LayoutError::UnknownSplit(split_id.to_owned()))?;
        if split.ratio == ratio {
            return Ok(false);
        }
        split.ratio = ratio;
        Ok(true)
    }

    /// The area next to `area_id` toward `edge` in the tree: the nearest one
    /// across the closest enclosing split along that axis.
    pub fn neighbour(&self, area_id: &str, edge: Edge) -> Option<String> {
        fn path_to<'a>(node: &'a Node, area_id: &str, path: &mut Vec<(&'a Split, bool)>) -> bool {
            match node {
                Node::Area(area) => area.id == area_id,
                Node::Split(split) => {
                    path.push((split, true));
                    if path_to(&split.first, area_id, path) {
                        return true;
                    }
                    path.pop();
                    path.push((split, false));
                    if path_to(&split.second, area_id, path) {
                        return true;
                    }
                    path.pop();
                    false
                }
            }
        }
        let mut path = Vec::new();
        if !path_to(&self.root, area_id, &mut path) {
            return None;
        }
        path.iter().rev().find_map(|(split, in_first)| {
            if split.axis != edge.axis() {
                return None;
            }
            match (edge.after(), in_first) {
                (true, true) => Some(split.second.first_area().id.clone()),
                (false, false) => Some(split.first.last_area().id.clone()),
                _ => None,
            }
        })
    }

    /// Gives every id a new number in tree order, keeping the display each
    /// area shows and the area in use.
    fn renumber(&mut self) {
        let in_use = self
            .areas()
            .iter()
            .position(|area| area.id == self.active_area);
        let mut next = 1u64;
        let mut mint = |prefix: char| {
            let id = format!("{prefix}{next}");
            next += 1;
            id
        };
        self.root.visit_mut(&mut |node| match node {
            Node::Area(area) => {
                let shown = area.active.as_ref().and_then(|active| {
                    area.displays
                        .iter()
                        .position(|display| &display.id == active)
                });
                area.id = mint('a');
                for display in &mut area.displays {
                    display.id = mint('d');
                }
                area.active = shown.map(|index| area.displays[index].id.clone());
            }
            Node::Split(split) => split.id = mint('s'),
        });
        self.next_id = next;
        if let Some(index) = in_use {
            self.active_area = self.areas()[index].id.clone();
        }
    }

    /// Replaces focus stamps near the end of their numbers by their order,
    /// so the display focused last stays last and the next stamp has room.
    /// Returns whether it had to.
    fn renumber_stamps(&mut self) -> bool {
        if self
            .displays()
            .all(|display| display.last_focused_unix_ms < NUMBER_LIMIT)
        {
            return false;
        }
        let mut stamps: Vec<u64> = self
            .displays()
            .map(|display| display.last_focused_unix_ms)
            .filter(|stamp| *stamp > 0)
            .collect();
        stamps.sort_unstable();
        stamps.dedup();
        for display in self.displays_mut() {
            if let Ok(rank) = stamps.binary_search(&display.last_focused_unix_ms) {
                display.last_focused_unix_ms = rank as u64 + 1;
            }
        }
        true
    }

    /// Makes a stored tree hold the invariants every operation keeps: unique
    /// ids and a `next_id` past them with room to mint, ratios in bounds, the
    /// display, depth and area caps, no empty area but the root, at most one
    /// preview per area, active names that exist, focus stamps with room.
    /// Returns what it had to change as counts, for the diagnostic log, in
    /// one pass over the tree per rule; a tree this build wrote returns
    /// nothing.
    pub fn repair(&mut self) -> Vec<String> {
        let mut notes = Vec::new();
        let mut highest = 0u64;
        self.root.visit_mut(&mut |node| {
            let mut note = |id: &str| {
                if let Some(number) = id.get(1..).and_then(|digits| digits.parse::<u64>().ok()) {
                    highest = highest.max(number);
                }
            };
            match node {
                Node::Area(area) => {
                    note(&area.id);
                    area.displays.iter().for_each(|display| note(&display.id));
                }
                Node::Split(split) => note(&split.id),
            }
        });
        // Only a crafted file holds numbers this high; minting past them
        // would overflow, so the tree takes new ones.
        if highest >= NUMBER_LIMIT || self.next_id >= NUMBER_LIMIT {
            self.renumber();
            notes.push("ids near the end of their numbers were renumbered".to_owned());
        } else if self.next_id <= highest {
            self.next_id = highest + 1;
        }
        if self.renumber_stamps() {
            notes.push("focus stamps near the end of their numbers were renumbered".to_owned());
        }

        let mut seen = HashSet::new();
        let mut renamed = 0usize;
        let mut ratios = 0usize;
        let mut groups = 0usize;
        let Layout { root, next_id, .. } = self;
        let mut claim = |id: &mut String, prefix: char| {
            if !seen.insert(id.clone()) {
                *id = format!("{prefix}{next_id}");
                *next_id += 1;
                seen.insert(id.clone());
                renamed += 1;
            }
        };
        root.visit_mut(&mut |node| match node {
            Node::Area(area) => {
                claim(&mut area.id, 'a');
                for display in &mut area.displays {
                    claim(&mut display.id, 'd');
                    // A diff shows one Changes group, the working one unless
                    // stored otherwise, and a file none: a diff stored
                    // without its group would never find its tab again.
                    let group = match display.kind {
                        DisplayKind::Diff => Some(display.committed.unwrap_or(false)),
                        DisplayKind::File => None,
                    };
                    if display.committed != group {
                        display.committed = group;
                        groups += 1;
                    }
                }
            }
            Node::Split(split) => {
                claim(&mut split.id, 's');
                let bounded = clamp_ratio(split.ratio).unwrap_or(DEFAULT_SPLIT_RATIO);
                if bounded != split.ratio {
                    split.ratio = bounded;
                    ratios += 1;
                }
            }
        });
        if renamed > 0 {
            notes.push(format!("{renamed} repeated ids were renamed"));
        }
        if ratios > 0 {
            notes.push(format!("{ratios} split ratios were brought into bounds"));
        }
        if groups > 0 {
            notes.push(format!(
                "{groups} views had a Changes group that did not fit their kind"
            ));
        }

        let mut kept = 0usize;
        let mut dropped = 0usize;
        self.root.visit_mut(&mut |node| {
            if let Node::Area(area) = node {
                let room = MAX_VIEW_DISPLAYS.saturating_sub(kept);
                if area.displays.len() > room {
                    dropped += area.displays.len() - room;
                    area.displays.truncate(room);
                }
                kept += area.displays.len();
            }
        });
        if dropped > 0 {
            notes.push(format!(
                "{dropped} views past the cap of {MAX_VIEW_DISPLAYS} were dropped"
            ));
        }

        // One pass however many areas a file holds; the last area stays,
        // empty, when every one is.
        let survivor = self.root.first_area().id.clone();
        let root = std::mem::replace(&mut self.root, Node::Area(Area::empty(String::new())));
        let mut emptied = 0usize;
        self.root = without_empty_areas(root, &mut emptied).unwrap_or_else(|| {
            emptied -= 1;
            Node::Area(Area::empty(survivor))
        });
        if emptied > 0 {
            notes.push(format!("{emptied} empty areas were removed"));
        }

        self.root.flatten_below(0, &mut notes);
        while self.area_count() > MAX_VIEW_AREAS {
            let Some(merged) = self.root.merge_last_pair() else {
                break;
            };
            notes.push(format!(
                "areas past the cap of {MAX_VIEW_AREAS} were merged into {merged}"
            ));
        }

        let mut previews = 0usize;
        let mut actives = 0usize;
        self.root.visit_mut(&mut |node| {
            let Node::Area(area) = node else {
                return;
            };
            let valid = area
                .active
                .as_ref()
                .is_some_and(|active| area.displays.iter().any(|display| &display.id == active));
            if !valid {
                let fixed = successor(&area.displays, 0);
                if area.active != fixed {
                    area.active = fixed;
                    actives += 1;
                }
            }
            // The active display keeps the preview when it is one; any other
            // preview is kept open, which loses nothing.
            let keep = area
                .displays
                .iter()
                .find(|display| display.preview && area.active.as_ref() == Some(&display.id))
                .or_else(|| area.displays.iter().find(|display| display.preview))
                .map(|display| display.id.clone());
            for display in &mut area.displays {
                if display.preview && Some(&display.id) != keep.as_ref() {
                    display.preview = false;
                    previews += 1;
                }
            }
        });
        if previews > 0 {
            notes.push(format!("{previews} extra preview views were kept open"));
        }
        if actives > 0 {
            notes.push(format!("{actives} areas named a missing active view"));
        }
        if self.area(&self.active_area).is_none() {
            notes.push(format!(
                "the area in use, {}, was missing",
                self.active_area
            ));
            self.active_area = self.root.first_area().id.clone();
        }
        notes
    }
}

/// Ids and focus stamps stay below this, so minting the next one can never
/// overflow; only a crafted file comes near it, and loading renumbers it.
const NUMBER_LIMIT: u64 = u64::MAX / 2;

/// `node` without its empty areas: a split that loses one side becomes its
/// other side, and `None` when nothing is left. Counts what it dropped.
fn without_empty_areas(node: Node, dropped: &mut usize) -> Option<Node> {
    match node {
        Node::Area(area) if area.displays.is_empty() => {
            *dropped += 1;
            None
        }
        Node::Area(area) => Some(Node::Area(area)),
        Node::Split(split) => {
            let Split {
                id,
                axis,
                ratio,
                first,
                second,
            } = split;
            match (
                without_empty_areas(*first, dropped),
                without_empty_areas(*second, dropped),
            ) {
                (Some(first), Some(second)) => Some(Node::Split(Split {
                    id,
                    axis,
                    ratio,
                    first: Box::new(first),
                    second: Box::new(second),
                })),
                (Some(only), None) | (None, Some(only)) => Some(only),
                (None, None) => None,
            }
        }
    }
}

/// The display an area shows once `index` left it: its most recently
/// focused remaining display, else the one that took the index, else the
/// last.
fn successor(displays: &[Display], index: usize) -> Option<String> {
    displays
        .iter()
        .filter(|display| display.last_focused_unix_ms > 0)
        .max_by_key(|display| display.last_focused_unix_ms)
        .or_else(|| displays.get(index).or(displays.last()))
        .map(|display| display.id.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A layout whose root area holds `names`, in order.
    fn with_files(names: &[&str]) -> Layout {
        let mut layout = Layout::default();
        for name in names {
            let display =
                layout.new_display(&format!("/repo/{name}"), DisplayKind::File, None, false);
            layout.insert("a1", display, 0).unwrap();
        }
        layout
    }

    fn id(layout: &Layout, name: &str) -> String {
        layout
            .displays()
            .find(|display| display.path == format!("/repo/{name}"))
            .unwrap()
            .id
            .clone()
    }

    fn area_of(layout: &Layout, name: &str) -> String {
        layout.area_of(&id(layout, name)).unwrap().id.clone()
    }

    fn names(layout: &Layout) -> Vec<Vec<String>> {
        layout
            .areas()
            .iter()
            .map(|area| {
                area.displays
                    .iter()
                    .map(|display| display.path.trim_start_matches("/repo/").to_owned())
                    .collect()
            })
            .collect()
    }

    fn split(layout: &mut Layout, name: &str, target: &str, edge: Edge) -> String {
        let display = id(layout, name);
        layout.split(&display, target, edge, 1).unwrap()
    }

    #[test]
    fn a_split_past_the_area_cap_is_refused_and_changes_nothing() {
        let mut layout = with_files(&["a", "b", "c", "d", "e", "f", "g"]);
        let b = split(&mut layout, "b", "a1", Edge::Right);
        split(&mut layout, "c", "a1", Edge::Down);
        split(&mut layout, "d", &b, Edge::Down);
        split(&mut layout, "e", "a1", Edge::Right);
        split(&mut layout, "f", &b, Edge::Right);
        assert_eq!(layout.area_count(), MAX_VIEW_AREAS);
        assert_eq!(layout.depth(), MAX_SPLIT_DEPTH);

        let before = layout.clone();
        let shallow = area_of(&layout, "c");
        assert_eq!(
            layout.split(&id(&layout, "g"), &shallow, Edge::Right, 2),
            Err(LayoutError::AreaLimit)
        );
        assert_eq!(layout, before);
    }

    /// Review U1: the 64th display fits, and every way of adding one more is
    /// refused and changes nothing, whichever path asked.
    #[test]
    fn every_way_of_adding_a_display_past_the_cap_is_refused() {
        let names: Vec<String> = (1..MAX_VIEW_DISPLAYS).map(|n| format!("f{n}")).collect();
        let mut layout = with_files(&names.iter().map(String::as_str).collect::<Vec<_>>());
        let last = layout.new_display("/repo/last", DisplayKind::File, None, false);
        layout.insert("a1", last, 1).unwrap();
        assert_eq!(layout.display_count(), MAX_VIEW_DISPLAYS);

        let before = layout.clone();
        let mut scratch = layout.clone();
        let mut extra = || scratch.new_display("/repo/x", DisplayKind::File, None, false);
        assert_eq!(
            layout.insert("a1", extra(), 2),
            Err(LayoutError::DisplayLimit)
        );
        assert_eq!(layout.append("a1", extra()), Err(LayoutError::DisplayLimit));
        assert_eq!(
            layout.split_new("a1", Edge::Right, extra(), 2),
            Err(LayoutError::DisplayLimit)
        );
        assert_eq!(layout, before);
    }

    #[test]
    fn a_split_past_the_depth_cap_is_refused_and_changes_nothing() {
        // `a` stays behind in the root area, so no area collapses below.
        let mut layout = with_files(&["a", "b", "c", "d", "e"]);
        let b = split(&mut layout, "b", "a1", Edge::Right);
        let c = split(&mut layout, "c", &b, Edge::Down);
        let d = split(&mut layout, "d", &c, Edge::Right);
        assert_eq!(layout.depth(), MAX_SPLIT_DEPTH, "the cap itself fits");
        assert!(layout.area_count() < MAX_VIEW_AREAS);

        let before = layout.clone();
        assert_eq!(
            layout.split(&id(&layout, "e"), &d, Edge::Down, 2),
            Err(LayoutError::DepthLimit)
        );
        assert_eq!(layout, before);
    }

    #[test]
    fn splitting_an_areas_only_display_into_that_area_is_refused() {
        let mut layout = with_files(&["a"]);
        let before = layout.clone();
        assert_eq!(
            layout.split(&id(&layout, "a"), "a1", Edge::Right, 1),
            Err(LayoutError::NothingToSplit)
        );
        assert_eq!(layout, before);
    }

    #[test]
    fn moving_an_areas_last_display_away_collapses_it_and_the_neighbour_takes_the_space() {
        let mut layout = with_files(&["a", "b"]);
        let fresh = split(&mut layout, "b", "a1", Edge::Right);
        assert_eq!(names(&layout), vec![vec!["a"], vec!["b"]]);
        assert_eq!(layout.active_area, fresh);

        let b = id(&layout, "b");
        assert!(layout.move_display(&b, "a1", 0, 2).unwrap());
        assert_eq!(names(&layout), vec![vec!["b", "a"]]);
        assert!(matches!(layout.root, Node::Area(ref area) if area.id == "a1"));
        assert_eq!(layout.active_area, "a1");
        assert_eq!(layout.area("a1").unwrap().active, Some(b.clone()));
        assert!(
            !layout.move_display(&b, "a1", 0, 3).unwrap(),
            "the same move again is the state already reached"
        );
    }

    #[test]
    fn the_root_area_stays_when_its_last_display_closes() {
        let mut layout = with_files(&["a"]);
        layout.remove(&id(&layout, "a")).unwrap();
        assert_eq!(layout.area_count(), 1);
        let root = layout.area("a1").expect("the root area stays");
        assert!(root.displays.is_empty());
        assert_eq!(root.active, None);
        assert_eq!(layout.active_area, "a1");
    }

    #[test]
    fn a_ratio_is_clamped_into_bounds_and_a_non_number_is_refused() {
        let mut layout = with_files(&["a", "b"]);
        split(&mut layout, "b", "a1", Edge::Down);
        let Node::Split(root) = &layout.root else {
            panic!("a split root")
        };
        let split_id = root.id.clone();
        let ratio = |layout: &Layout| match &layout.root {
            Node::Split(split) => split.ratio,
            Node::Area(_) => panic!("a split root"),
        };
        assert!(layout.resize(&split_id, 0.01).unwrap());
        assert_eq!(ratio(&layout), MIN_SPLIT_RATIO);
        assert!(
            !layout.resize(&split_id, 0.0).unwrap(),
            "same clamped value"
        );
        layout.resize(&split_id, 0.99).unwrap();
        assert_eq!(ratio(&layout), MAX_SPLIT_RATIO);
        assert_eq!(
            layout.resize(&split_id, f32::NAN),
            Err(LayoutError::InvalidRatio)
        );
        assert_eq!(ratio(&layout), MAX_SPLIT_RATIO);
        assert_eq!(
            layout.resize("s404", 0.5),
            Err(LayoutError::UnknownSplit("s404".to_owned()))
        );
    }

    #[test]
    fn a_neighbour_is_found_across_the_nearest_split_along_each_direction() {
        // a | (b over (c | d)): a left, b top right, c and d bottom right.
        let mut layout = with_files(&["a", "b", "c", "d"]);
        let b = split(&mut layout, "b", "a1", Edge::Right);
        let c = split(&mut layout, "c", &b, Edge::Down);
        let d = split(&mut layout, "d", &c, Edge::Right);
        assert_eq!(
            names(&layout),
            vec![vec!["a"], vec!["b"], vec!["c"], vec!["d"]]
        );
        assert_eq!(layout.neighbour("a1", Edge::Right), Some(b.clone()));
        assert_eq!(layout.neighbour("a1", Edge::Left), None);
        assert_eq!(layout.neighbour(&c, Edge::Left), Some("a1".to_owned()));
        assert_eq!(layout.neighbour(&c, Edge::Right), Some(d.clone()));
        assert_eq!(layout.neighbour(&b, Edge::Down), Some(c.clone()));
        assert_eq!(layout.neighbour(&d, Edge::Up), Some(b.clone()));
        assert_eq!(layout.neighbour(&b, Edge::Up), None);
    }

    /// A crafted file can hold ids and focus stamps at the top of their
    /// numbers, where minting the next one overflows: loading renumbers
    /// them and keeps what the area shows and which view was focused last.
    #[test]
    fn a_tree_at_the_top_of_its_numbers_is_renumbered_on_load() {
        let mut layout = with_files(&["a", "b"]);
        let b = id(&layout, "b");
        for display in layout.displays_mut() {
            let last = display.id == b;
            display.last_focused_unix_ms = if last { u64::MAX } else { u64::MAX - 1 };
            if last {
                display.id = format!("d{}", u64::MAX);
            }
        }
        layout.area_mut("a1").unwrap().active = Some(format!("d{}", u64::MAX));
        layout.next_id = u64::MAX;

        let notes = layout.repair();

        assert_eq!(notes.len(), 2, "{notes:?}");
        assert_eq!(names(&layout), vec![vec!["a", "b"]]);
        let area = layout.active_area();
        assert_eq!(area.active, Some(id(&layout, "b")));
        let shown = successor(&area.displays, 0).unwrap();
        assert_eq!(shown, id(&layout, "b"), "b stays the one focused last");
        let area_id = area.id.clone();
        let fresh = layout.new_display("/repo/c", DisplayKind::File, None, false);
        assert!(layout.displays().all(|display| display.id != fresh.id));
        let stamp = layout.next_stamp(1_700_000_000_000);
        layout.insert(&area_id, fresh, stamp).unwrap();
        assert_eq!(names(&layout), vec![vec!["a", "b", "c"]]);
        assert!(
            layout.clone().repair().is_empty(),
            "a repaired tree is stable"
        );
    }

    /// A crafted file can hold thousands of empty areas: loading drops them
    /// in one pass over the tree and says so in one line.
    #[test]
    fn thousands_of_empty_areas_are_dropped_with_one_note() {
        fn empties(depth: u32, next: &mut u64) -> Node {
            *next += 1;
            if depth == 0 {
                return Node::Area(Area::empty(format!("a{next}")));
            }
            let id = format!("s{next}");
            Node::Split(Split {
                id,
                axis: SplitAxis::Row,
                ratio: 0.5,
                first: Box::new(empties(depth - 1, next)),
                second: Box::new(empties(depth - 1, next)),
            })
        }
        let mut layout = with_files(&["a"]);
        let mut next = layout.next_id;
        let second = empties(12, &mut next);
        layout.root = Node::Split(Split {
            id: format!("s{}", next + 1),
            axis: SplitAxis::Row,
            ratio: 0.5,
            first: Box::new(layout.root.clone()),
            second: Box::new(second),
        });
        layout.next_id = next + 2;

        let notes = layout.repair();

        assert_eq!(names(&layout), vec![vec!["a"]]);
        assert_eq!(notes.len(), 1, "{notes:?}");
        assert!(notes[0].contains("4096"), "{notes:?}");
    }

    #[test]
    fn repair_restores_the_invariants_of_a_damaged_tree() {
        let mut layout = with_files(&["a", "b", "c"]);
        let fresh = split(&mut layout, "c", "a1", Edge::Right);
        for display in layout.displays_mut() {
            display.preview = true;
        }
        layout.area_mut("a1").unwrap().active = Some("d404".to_owned());
        layout.area_mut(&fresh).unwrap().displays.clear();
        layout.active_area = "a404".to_owned();
        layout.next_id = 1;

        let notes = layout.repair();

        assert!(!notes.is_empty());
        assert_eq!(names(&layout), vec![vec!["a", "b"]]);
        let root = layout.area("a1").unwrap();
        assert!(root.active.is_some());
        assert_eq!(
            root.displays
                .iter()
                .filter(|display| display.preview)
                .count(),
            1
        );
        assert_eq!(layout.active_area, "a1");
        let minted = layout
            .new_display("/repo/e", DisplayKind::File, None, false)
            .id;
        assert!(
            layout.displays().all(|display| display.id != minted),
            "an id already stored is never minted again"
        );
        assert!(
            layout.clone().repair().is_empty(),
            "a repaired tree is stable"
        );
    }
}
