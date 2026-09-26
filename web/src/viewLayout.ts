// The View areas' rules (PRD S7 B6-B13, B20; contract sections 2 and 6): the
// core owns the tree of areas and displays (`workspace_view.layout`), and
// these pure functions answer what the page asks of it - where each area and
// divider sits for a body rectangle, which area lies in a direction, whether
// a split fits, where a dragged tab would land, which commands a display's
// menu offers, and what a narrow window shows. They take values and return
// values, so every rule is testable without a page; nothing here dispatches.

import type { ViewAreaSnapshot, ViewDisplaySnapshot, ViewLayoutSnapshot, ViewNode, ViewSplitSnapshot } from "./snapshot";

export type Rect = { x: number; y: number; width: number; height: number };
export type Point = { x: number; y: number };
export type Edge = "left" | "right" | "up" | "down";

/** The pixel sizes the rules read, each from its design token. */
export type LayoutSizes = {
  /** `--size-workspace-area-min`: the narrowest a View area may be. */
  areaMinWidth: number;
  /** `--size-view-area-min-height`: the shortest a View area may be, its tab bar included. */
  areaMinHeight: number;
  /** `--size-resize-handle`: a divider's thickness. */
  divider: number;
  /** `--size-tab-strip`: an area's tab bar height. */
  tabStrip: number;
};

/** The core clamps a split's ratio to this range (contract 1). */
export const RATIO_MIN = 0.15;
export const RATIO_MAX = 0.85;
/** One arrow key or Grow/Shrink moves a divider by this share. */
export const RESIZE_STEP = 0.05;
/** How far into an area's content, as a share of its size, an edge's drop zone reaches. */
export const EDGE_ZONE = 0.3;

const EDGE_NAME: Record<Edge, string> = { right: "right", left: "left", up: "up", down: "down" };
/** The order a display's menu offers directions in. */
const MENU_EDGES: readonly Edge[] = ["right", "left", "up", "down"];

// --- the Workspace an action names ---------------------------------------------

/** The Workspace a View action was taken on, as its frame's `workspace_view` names it (contract 4.1). */
export type ViewWorkspace = { device_id: string; path: string };

/** What the operator acted on: one Workspace's View areas as a frame drew them. */
export type ViewFrame = { workspace: ViewWorkspace; layout: ViewLayoutSnapshot };

/** One string per Workspace, for keys and comparisons. */
export function workspaceKey(workspace: ViewWorkspace): string {
  return `${workspace.device_id}\u0000${workspace.path}`;
}

/**
 * A `view_layout` payload (contract 4.1): the action and its fields with the
 * Workspace of the frame it was taken on. Display, area and split ids repeat
 * across Workspaces and the front can move before the event lands, so the
 * core applies the action only while that Workspace is still in front.
 */
export function viewLayoutPayload(workspace: ViewWorkspace, action: { action: string } & Record<string, unknown>): Record<string, unknown> {
  return { workspace: { device_id: workspace.device_id, path: workspace.path }, ...action };
}

/**
 * What the operator is told when the core refuses a View action or open
 * (B19): the core's reason, for every `view_layout.*` refusal. An action
 * that arrived for a Workspace no longer in front (`stale_workspace`) is the
 * log's alone, since the screen already shows another Workspace.
 */
export function viewRefusal(error: { kind: string; message: string } | null | undefined): string | null {
  if (!error || !error.kind.startsWith("view_layout.") || error.kind === "view_layout.stale_workspace") return null;
  return error.message;
}

// --- tree queries ------------------------------------------------------------

/** Every area, in tree order: first child before second, depth first. */
export function areasOf(node: ViewNode): ViewAreaSnapshot[] {
  return "area" in node ? [node.area] : [...areasOf(node.split.first), ...areasOf(node.split.second)];
}

export function findArea(root: ViewNode, areaId: string): ViewAreaSnapshot | null {
  return areasOf(root).find((area) => area.id === areaId) ?? null;
}

export type LocatedDisplay = { area: ViewAreaSnapshot; display: ViewDisplaySnapshot; index: number };

export function locateDisplay(root: ViewNode, displayId: string): LocatedDisplay | null {
  for (const area of areasOf(root)) {
    const index = area.displays.findIndex((display) => display.id === displayId);
    const display = area.displays[index];
    if (display) return { area, display, index };
  }
  return null;
}

/** Whether two displays show one document: its path and kind, and a diff's History group (the core's `Display::shows`). */
export function showsSameDocument(a: ViewDisplaySnapshot, b: ViewDisplaySnapshot): boolean {
  return a.path === b.path && a.kind === b.kind && (a.kind === "file" || a.committed === b.committed);
}

/** Every display that shows one document (editor tab), in tree order. */
export function displaysOfDocument(root: ViewNode, tabId: string): ViewDisplaySnapshot[] {
  return areasOf(root).flatMap((area) => area.displays.filter((display) => display.tab_id === tabId));
}

/** The active area's active display: what the operator last worked in. */
export function activeDisplay(layout: ViewLayoutSnapshot): LocatedDisplay | null {
  const area = findArea(layout.root, layout.active_area);
  if (!area?.active) return null;
  return locateDisplay(layout.root, area.active);
}

/** The display each area shows, one per area. */
export function shownDisplays(root: ViewNode): ViewDisplaySnapshot[] {
  return areasOf(root).flatMap((area) => area.displays.filter((display) => display.id === area.active));
}

/** How many splits lie between the root and an area (contract 2's depth). */
export function areaDepth(root: ViewNode, areaId: string): number | null {
  if ("area" in root) return root.area.id === areaId ? 0 : null;
  for (const child of [root.split.first, root.split.second]) {
    const depth = areaDepth(child, areaId);
    if (depth !== null) return depth + 1;
  }
  return null;
}

/** The split an area sits directly in, and on which side of it. */
export function parentSplit(root: ViewNode, areaId: string): { split: ViewSplitSnapshot; side: "first" | "second" } | null {
  if ("area" in root) return null;
  const { split } = root;
  for (const side of ["first", "second"] as const) {
    const child = split[side];
    if ("area" in child && child.area.id === areaId) return { split, side };
    const inner = parentSplit(child, areaId);
    if (inner) return inner;
  }
  return null;
}

/** The area after or before `areaId` in tree order, wrapping; null when there is no other. */
export function adjacentInOrder(root: ViewNode, areaId: string, step: 1 | -1): ViewAreaSnapshot | null {
  const areas = areasOf(root);
  const index = areas.findIndex((area) => area.id === areaId);
  if (index === -1 || areas.length < 2) return null;
  return areas[(index + step + areas.length) % areas.length] ?? null;
}

// --- geometry ----------------------------------------------------------------

/** The smallest a subtree may be drawn: each area at its minimum, a divider between siblings. */
export function minimumSize(node: ViewNode, sizes: LayoutSizes): { width: number; height: number } {
  if ("area" in node) return { width: sizes.areaMinWidth, height: sizes.areaMinHeight };
  const first = minimumSize(node.split.first, sizes);
  const second = minimumSize(node.split.second, sizes);
  return node.split.axis === "row"
    ? { width: first.width + sizes.divider + second.width, height: Math.max(first.height, second.height) }
    : { width: Math.max(first.width, second.width), height: first.height + sizes.divider + second.height };
}

export type AreaBox = { id: string; rect: Rect; bar: Rect; content: Rect };

export type DividerBox = {
  /** The split's id. */
  id: string;
  axis: "row" | "column";
  /** The divider itself. */
  rect: Rect;
  /** The whole split it divides. */
  span: Rect;
  ratio: number;
  /** The first child's drawn size along the axis, in pixels. */
  first: number;
  /** The first and second subtrees' minimum sizes along the axis. */
  minFirst: number;
  minSecond: number;
};

/**
 * Where everything sits for one body rectangle. `fits` is false when some
 * split cannot give both of its sides their minimum; the page then shows one
 * area at a time (B13, A7). While it fits, each split's first side keeps the
 * stored ratio within both sides' minimums, the way the Agent/View boundary
 * does (S6 B6).
 */
export type Geometry = { areas: AreaBox[]; dividers: DividerBox[]; fits: boolean };

export function viewGeometry(root: ViewNode, body: Rect, sizes: LayoutSizes): Geometry {
  const areas: AreaBox[] = [];
  const dividers: DividerBox[] = [];
  let fits = true;
  const place = (node: ViewNode, rect: Rect) => {
    if ("area" in node) {
      const bar = Math.min(sizes.tabStrip, rect.height);
      areas.push({
        id: node.area.id,
        rect,
        bar: { x: rect.x, y: rect.y, width: rect.width, height: bar },
        content: { x: rect.x, y: rect.y + bar, width: rect.width, height: rect.height - bar },
      });
      return;
    }
    const { split } = node;
    const row = split.axis === "row";
    const available = Math.max(0, (row ? rect.width : rect.height) - sizes.divider);
    const minFirst = along(minimumSize(split.first, sizes), row);
    const minSecond = along(minimumSize(split.second, sizes), row);
    let first: number;
    if (available < minFirst + minSecond) {
      fits = false;
      first = Math.round(available * split.ratio);
    } else {
      first = Math.round(clamp(split.ratio * available, minFirst, available - minSecond));
    }
    const second = available - first;
    const firstRect = row ? { ...rect, width: first } : { ...rect, height: first };
    const dividerRect = row
      ? { x: rect.x + first, y: rect.y, width: sizes.divider, height: rect.height }
      : { x: rect.x, y: rect.y + first, width: rect.width, height: sizes.divider };
    const secondRect = row
      ? { x: rect.x + first + sizes.divider, y: rect.y, width: second, height: rect.height }
      : { x: rect.x, y: rect.y + first + sizes.divider, width: rect.width, height: second };
    dividers.push({ id: split.id, axis: split.axis, rect: dividerRect, span: rect, ratio: split.ratio, first, minFirst, minSecond });
    place(split.first, firstRect);
    place(split.second, secondRect);
  };
  place(root, body);
  return { areas, dividers, fits };
}

/**
 * The ratio a divider lands at for a first side of `first` pixels: within
 * the core's range and both sides' minimums. When the minimums do not fit
 * the core's range alone applies.
 */
export function ratioForFirst(divider: DividerBox, first: number): number {
  const available = along(divider.span, divider.axis === "row") - along(divider.rect, divider.axis === "row");
  if (available <= 0) return divider.ratio;
  const low = Math.max(RATIO_MIN, divider.minFirst / available);
  const high = Math.min(RATIO_MAX, 1 - divider.minSecond / available);
  const ratio = first / available;
  return low > high ? clamp(ratio, RATIO_MIN, RATIO_MAX) : clamp(ratio, low, high);
}

/** The ratio a divider dragged to `offset` pixels from its split's start stands for. */
export function ratioAtOffset(divider: DividerBox, offset: number): number {
  const thickness = along(divider.rect, divider.axis === "row");
  return ratioForFirst(divider, offset - thickness / 2);
}

/** The ratio one keyboard step moves a divider to, or null when it cannot move that way. */
export function steppedRatio(divider: DividerBox, delta: number): number | null {
  const available = along(divider.span, divider.axis === "row") - along(divider.rect, divider.axis === "row");
  return moved(divider.ratio, ratioForFirst(divider, (divider.ratio + delta) * available));
}

/**
 * The geometry of one area filling the body, for a window too narrow for the
 * whole tree (B13, A7): only the active area shows, and no split fits.
 */
export function singleAreaGeometry(area: ViewAreaSnapshot, body: Rect, sizes: LayoutSizes): Geometry {
  return { ...viewGeometry({ area }, body, sizes), fits: false };
}

// --- directions --------------------------------------------------------------

/** Only the topology matters for a neighbour, so it is measured on a fixed square with no minimums. */
const TOPOLOGY_SIZES: LayoutSizes = { areaMinWidth: 0, areaMinHeight: 0, divider: 0, tabStrip: 0 };
const TOPOLOGY_BODY: Rect = { x: 0, y: 0, width: 1024, height: 1024 };

/**
 * The area that lies directly in `direction` from `areaId`: it shares that
 * edge and overlaps it across; the widest overlap wins, then the nearest
 * centre, then tree order. Null when that edge is the Views' own edge.
 */
export function neighbourArea(root: ViewNode, areaId: string, direction: Edge): string | null {
  const boxes = viewGeometry(root, TOPOLOGY_BODY, TOPOLOGY_SIZES).areas;
  const from = boxes.find((box) => box.id === areaId)?.rect;
  if (!from) return null;
  const across = direction === "left" || direction === "right";
  let best: { id: string; overlap: number; distance: number } | null = null;
  for (const box of boxes) {
    if (box.id === areaId || !touches(from, box.rect, direction)) continue;
    const overlap = across
      ? Math.min(from.y + from.height, box.rect.y + box.rect.height) - Math.max(from.y, box.rect.y)
      : Math.min(from.x + from.width, box.rect.x + box.rect.width) - Math.max(from.x, box.rect.x);
    if (overlap <= 0) continue;
    const distance = across
      ? Math.abs(from.y + from.height / 2 - (box.rect.y + box.rect.height / 2))
      : Math.abs(from.x + from.width / 2 - (box.rect.x + box.rect.width / 2));
    if (!best || overlap > best.overlap || (overlap === best.overlap && distance < best.distance)) {
      best = { id: box.id, overlap, distance };
    }
  }
  return best?.id ?? null;
}

function touches(from: Rect, to: Rect, direction: Edge): boolean {
  switch (direction) {
    case "right":
      return to.x === from.x + from.width;
    case "left":
      return to.x + to.width === from.x;
    case "down":
      return to.y === from.y + from.height;
    case "up":
      return to.y + to.height === from.y;
  }
}

/**
 * The scroll position of a tab strip that shows the tab at `offset` (its
 * left edge within the strip's content) with `width`: unchanged when the
 * tab is already whole in the `viewport`, else the least move that shows
 * it, its left edge first when it cannot fit whole.
 */
export function revealedScroll(scroll: number, viewport: number, offset: number, width: number): number {
  if (offset < scroll || width >= viewport) return offset;
  if (offset + width > scroll + viewport) return offset + width - viewport;
  return scroll;
}

// --- eligibility -------------------------------------------------------------

export type Eligibility = { ok: true } | { ok: false; reason: string };

const refuse = (reason: string): Eligibility => ({ ok: false, reason });

/**
 * Whether moving `displayId` into a new area at `edge` of `areaId` can land
 * (B9, B19, contract 2 and 6): the area and depth caps the core publishes,
 * the target area holding two minimums along the axis plus a divider, and a
 * split that would change nothing - a display alone in its own area - is
 * refused. The caps are counted before an emptied source area collapses,
 * which is the stricter reading of the core's rule.
 */
export function splitEligibility(
  layout: ViewLayoutSnapshot,
  geometry: Geometry,
  sizes: LayoutSizes,
  displayId: string,
  areaId: string,
  edge: Edge,
): Eligibility {
  const located = locateDisplay(layout.root, displayId);
  if (!located) return refuse("This view is no longer open.");
  const target = findArea(layout.root, areaId);
  if (!target) return refuse("That view area is gone.");
  if (located.area.id === areaId && located.area.displays.length === 1) return refuse("This is the only view in its area.");
  return roomToSplit(layout, geometry, sizes, areaId, edge);
}

/**
 * Why Open to the side cannot land now, or null (B4, B9, D-06). With two
 * or more areas it goes to a neighbour and splits nothing, and into an
 * empty Views it opens in the empty area; from the only area with a view
 * it makes a new area on the right, so that area has to hold two minimums
 * side by side, exactly as Split right does. While the View areas are not
 * drawn (the side panel closed) their room is unknown, and the core's caps decide.
 */
export function besideUnavailable(
  layout: ViewLayoutSnapshot | null | undefined,
  drawn: { geometry: Geometry; sizes: LayoutSizes } | null,
): string | null {
  if (!layout || !drawn) return null;
  const areas = areasOf(layout.root);
  const only = areas.length === 1 ? areas[0] : undefined;
  if (!only || only.displays.length === 0) return null;
  const room = roomToSplit(layout, drawn.geometry, drawn.sizes, only.id, "right");
  if (room.ok) return null;
  return room.reason === NARROW ? "This view area is too narrow to open a second view beside it." : room.reason;
}

const NARROW = "This view area is too narrow to split.";

/** The caps, the depth and the pixel room a new area at `edge` of `areaId` needs. */
function roomToSplit(layout: ViewLayoutSnapshot, geometry: Geometry, sizes: LayoutSizes, areaId: string, edge: Edge): Eligibility {
  if (areasOf(layout.root).length >= layout.limits.areas) return refuse(`This Workspace already shows ${count(layout.limits.areas, "view area")}, the most it can.`);
  if ((areaDepth(layout.root, areaId) ?? 0) >= layout.limits.depth) return refuse(`View areas can be split only ${count(layout.limits.depth, "level")} deep.`);
  if (!geometry.fits) return refuse("The window is too small to show more view areas.");
  const box = geometry.areas.find((area) => area.id === areaId);
  if (!box) return refuse("That view area is not on screen.");
  const across = edge === "left" || edge === "right";
  const needed = across ? 2 * sizes.areaMinWidth + sizes.divider : 2 * sizes.areaMinHeight + sizes.divider;
  if ((across ? box.rect.width : box.rect.height) < needed) {
    return refuse(across ? NARROW : "This view area is too short to split.");
  }
  return { ok: true };
}

// --- a display's menu --------------------------------------------------------

export type ViewMenuId =
  | "keep_open"
  | `split_${Edge}`
  | `move_${Edge}`
  | "copy_path"
  | "reveal"
  | "close_view";

export type ViewMenuEntry = { id: ViewMenuId; label: string; unavailable: string | null; separated?: boolean };

/** The menu's items in docs/UI_BEHAVIOR.md's order, with its fixed labels. */
const MENU_ITEMS: readonly { id: ViewMenuId; label: string }[] = [
  { id: "keep_open", label: "Keep open" },
  ...MENU_EDGES.map((edge) => ({ id: `split_${edge}` as const, label: `Split ${EDGE_NAME[edge]}` })),
  ...MENU_EDGES.map((edge) => ({ id: `move_${edge}` as const, label: `Move ${EDGE_NAME[edge]}` })),
  { id: "copy_path", label: "Copy path" },
  { id: "reveal", label: "Reveal in Explorer" },
  { id: "close_view", label: "Close view" },
];

const NO_AREA: Record<Edge, string> = {
  right: "There is no view area to the right.",
  left: "There is no view area to the left.",
  up: "There is no view area above.",
  down: "There is no view area below.",
};

/**
 * Every command of a display's menu with the reason it cannot run now, or
 * null: Keep open for a preview, a split that can land (B9, B19), a move
 * toward an area that exists, Reveal while the file can be read. `drawn` is
 * what the page last drew; without it a split's room cannot be judged.
 */
function displayCommands(layout: ViewLayoutSnapshot, drawn: { geometry: Geometry; sizes: LayoutSizes } | null, located: LocatedDisplay): (ViewMenuEntry & { hidden: boolean })[] {
  const { area, display } = located;
  return MENU_ITEMS.map(({ id, label }) => {
    const edge = menuEdge(id);
    let unavailable: string | null = null;
    let hidden = false;
    if (id === "keep_open") {
      unavailable = display.preview ? null : "This view is already kept open.";
      hidden = !display.preview;
    } else if (edge && id.startsWith("split_")) {
      const eligibility = drawn ? splitEligibility(layout, drawn.geometry, drawn.sizes, display.id, area.id, edge) : refuse("The View areas are not on screen.");
      unavailable = eligibility.ok ? null : eligibility.reason;
    } else if (edge) {
      unavailable = neighbourArea(layout.root, area.id, edge) ? null : NO_AREA[edge];
      hidden = unavailable !== null;
    } else if (id === "reveal") {
      unavailable = revealBlocked(display);
      // A page is not a file of the checkout.
      hidden = display.kind === "browser";
    }
    const named = id === "copy_path" && display.kind === "browser" ? "Copy address" : label;
    return { id, label: named, unavailable, hidden, separated: id === "copy_path" || id === "close_view" };
  });
}

/**
 * The commands a display's tab menu and its area's overflow button offer, and
 * nothing else (B11, D-07): Keep open while it is a preview, a split in each
 * direction (disabled with the reason when it cannot land), a move toward
 * each direction that has an area, the path, Reveal in Explorer, and Close
 * view. There is no file deletion here.
 */
export function displayMenu(layout: ViewLayoutSnapshot, geometry: Geometry, sizes: LayoutSizes, displayId: string): ViewMenuEntry[] {
  const located = locateDisplay(layout.root, displayId);
  if (!located) return [];
  return displayCommands(layout, { geometry, sizes }, located)
    .filter((entry) => !entry.hidden)
    .map(({ id, label, unavailable, separated }) => ({ id, label, unavailable, ...(separated ? { separated } : {}) }));
}

/**
 * Where a display's selection and scroll are remembered: per Workspace,
 * display and document. A preview display is retargeted in place to the
 * next document, which starts at its own top rather than at the place the
 * last one was left (B1, B4).
 */
export function placeKey(workspace: string, display: Pick<ViewDisplaySnapshot, "id" | "tab_id">): string {
  return `${workspace}\u0000${display.id}\u0000${display.tab_id ?? ""}`;
}

function revealBlocked(display: ViewDisplaySnapshot): string | null {
  if (display.state === "unavailable") return "The file is unavailable";
  if (display.state === "waiting") return display.reason ?? "The file cannot be read yet";
  return null;
}

/** The edge a `split_*` or `move_*` menu id names. */
export function menuEdge(id: ViewMenuId): Edge | null {
  const match = /^(?:split|move)_(left|right|up|down)$/.exec(id);
  return match ? (match[1] as Edge) : null;
}

/** What a display is, for its tooltip and accessible name (B21): kind, full path and state. */
export function displayIdentity(display: ViewDisplaySnapshot): string {
  if (display.kind === "browser") return `Page: ${display.title ? `${display.title} · ` : ""}${display.url ?? ""}`;
  const kind = display.kind === "diff" ? `${display.committed ? "Branch" : "Working"} diff` : "File";
  const reason = display.reason ? `: ${display.reason}` : "";
  const state =
    display.state === "unavailable"
      ? ` · Unavailable${reason}`
      : display.state === "waiting"
        ? ` · Waiting${reason}`
        : display.state === "opening"
          ? " · Opening"
          : display.preview
            ? " · Preview"
            : "";
  return `${kind}: ${display.path}${state}`;
}

// --- dropping a dragged tab --------------------------------------------------

export type TabSlot = { displayId: string; rect: Rect };

export type DropTarget =
  /** A thin insertion line in a tab bar: reorder there, or move into that area (B6). `index` is the display's final position. */
  | { kind: "bar"; areaId: string; index: number; line: { x: number; y: number; height: number } }
  /** An area's content edge: a new area there takes half of it (B7). */
  | { kind: "edge"; areaId: string; edge: Edge; preview: Rect; label: string }
  /** Nothing lands here; a reason means the place is refused and the cursor says so (B8). */
  | { kind: "none"; reason: string | null };

const NOWHERE = "Drop on a tab bar to move this view, or near an area's edge to split it.";

/**
 * Where a dragged display would land at `point` (B6-B8, D-04, D-05): over a
 * tab bar, a thin insertion line between tabs; over an area's content, the
 * edge nearest the pointer when it is within `EDGE_ZONE` of that edge, and
 * only when that split can land. One target wins at a time. A drop back into
 * its own slot changes nothing and targets nothing. `tabs` are each area's
 * tab rectangles in the same coordinates as `geometry`.
 */
export function dropTarget(input: {
  layout: ViewLayoutSnapshot;
  geometry: Geometry;
  sizes: LayoutSizes;
  tabs: Record<string, TabSlot[]>;
  displayId: string;
  point: Point;
}): DropTarget {
  const { layout, geometry, sizes, tabs, displayId, point } = input;
  const located = locateDisplay(layout.root, displayId);
  if (!located) return { kind: "none", reason: "This view is no longer open." };
  for (const box of geometry.areas) {
    if (contains(box.bar, point)) return barTarget(box, findArea(layout.root, box.id), tabs[box.id] ?? [], located, point);
  }
  for (const box of geometry.areas) {
    if (!contains(box.content, point)) continue;
    const edge = nearestEdge(box.content, point);
    if (!edge) return { kind: "none", reason: NOWHERE };
    const eligibility = splitEligibility(layout, geometry, sizes, displayId, box.id, edge);
    if (!eligibility.ok) return { kind: "none", reason: eligibility.reason };
    return { kind: "edge", areaId: box.id, edge, preview: half(box.rect, edge), label: `Split ${EDGE_NAME[edge]}` };
  }
  return { kind: "none", reason: NOWHERE };
}

function barTarget(box: AreaBox, area: ViewAreaSnapshot | null, slots: TabSlot[], located: LocatedDisplay, point: Point): DropTarget {
  const slot = slots.filter((tab) => tab.rect.x + tab.rect.width / 2 < point.x).length;
  const at = slots[slot]?.rect;
  const last = slots.at(-1)?.rect;
  const x = at ? at.x : last ? last.x + last.width : box.bar.x;
  const line = { x, y: box.bar.y, height: box.bar.height };
  if (located.area.id !== box.id) {
    // A view of this document already in that area gives way to the moved
    // one, so one left of the line leaves the final place a slot earlier.
    const twin = area?.displays.find((other) => showsSameDocument(other, located.display));
    const twinSlot = twin ? slots.findIndex((tab) => tab.displayId === twin.id) : -1;
    return { kind: "bar", areaId: box.id, index: twinSlot !== -1 && twinSlot < slot ? slot - 1 : slot, line };
  }
  const from = slots.findIndex((tab) => tab.displayId === located.display.id);
  const index = from !== -1 && slot > from ? slot - 1 : slot;
  if (from === -1 || index === from) return { kind: "none", reason: null };
  return { kind: "bar", areaId: box.id, index, line };
}

/** The edge `point` is nearest to, when it is within the edge zone; ties go right, left, up, down. */
function nearestEdge(rect: Rect, point: Point): Edge | null {
  if (rect.width <= 0 || rect.height <= 0) return null;
  const distances: [Edge, number][] = [
    ["right", (rect.x + rect.width - point.x) / rect.width],
    ["left", (point.x - rect.x) / rect.width],
    ["up", (point.y - rect.y) / rect.height],
    ["down", (rect.y + rect.height - point.y) / rect.height],
  ];
  let best: [Edge, number] | null = null;
  for (const entry of distances) {
    if (entry[1] < EDGE_ZONE && (!best || entry[1] < best[1])) best = entry;
  }
  return best ? best[0] : null;
}

/** The half of `rect` a new area at `edge` would take. */
function half(rect: Rect, edge: Edge): Rect {
  switch (edge) {
    case "right":
      return { x: rect.x + rect.width / 2, y: rect.y, width: rect.width / 2, height: rect.height };
    case "left":
      return { x: rect.x, y: rect.y, width: rect.width / 2, height: rect.height };
    case "down":
      return { x: rect.x, y: rect.y + rect.height / 2, width: rect.width, height: rect.height / 2 };
    case "up":
      return { x: rect.x, y: rect.y, width: rect.width, height: rect.height / 2 };
  }
}

/** Whether two targets are the same place, so a drop lands only where the preview said it would (D-05). */
export function sameTarget(a: DropTarget, b: DropTarget): boolean {
  if (a.kind === "bar" && b.kind === "bar") return a.areaId === b.areaId && a.index === b.index;
  if (a.kind === "edge" && b.kind === "edge") return a.areaId === b.areaId && a.edge === b.edge;
  return false;
}

// --- palette commands --------------------------------------------------------

export type ViewCommandId = ViewMenuId | "focus_next" | "focus_previous" | "grow" | "shrink";

export type ViewCommand = { id: ViewCommandId; title: string; unavailable: string | null };

/**
 * The View commands the palette offers (B20, D-13, docs/UI_BEHAVIOR.md "The View tab
 * menu"): every item of the active view's menu, a Move toward each
 * direction and Keep open included, then focus to the next or previous area
 * and resizing the active area; each with the reason it cannot run now.
 * `drawn` is what the page last drew of the areas; without it a split's
 * room cannot be judged, so every split is offered disabled with that reason.
 */
export function viewCommands(layout: ViewLayoutSnapshot, drawn: { geometry: Geometry; sizes: LayoutSizes } | null): ViewCommand[] {
  const active = activeDisplay(layout);
  const alone = areasOf(layout.root).length < 2 ? "There is only one view area." : null;
  const resize = (grow: boolean): string | null => {
    const target = resizeTarget(layout, drawn?.geometry ?? null, grow);
    return "reason" in target ? target.reason : null;
  };
  const display: ViewCommand[] = active
    ? displayCommands(layout, drawn, active).map(({ id, label, unavailable }) => ({ id, title: label, unavailable }))
    : MENU_ITEMS.map(({ id, label }) => ({ id, title: label, unavailable: "No view is open in the active view area." }));
  return [
    ...display,
    { id: "focus_next", title: "Focus next view area", unavailable: alone },
    { id: "focus_previous", title: "Focus previous view area", unavailable: alone },
    { id: "grow", title: "Grow view area", unavailable: resize(true) },
    { id: "shrink", title: "Shrink view area", unavailable: resize(false) },
  ];
}

/** Whether a palette command is one of a display's menu commands, run on the active view. */
export function isMenuCommand(id: ViewCommandId): id is ViewMenuId {
  return MENU_ITEMS.some((item) => item.id === id);
}

// --- where the keyboard goes -------------------------------------------------

/**
 * Where the keyboard goes once the core shows it (B20): a display a menu,
 * the palette or a drop moved or split, with where it stood when asked, or
 * an area a focus command chose; each in the Workspace it was asked in.
 */
export type ViewFocusRequest =
  | { workspace: string; displayId: string; from: { areaId: string; index: number } | null }
  | { workspace: string; areaId: string };

/**
 * Whether the core now shows what a focus request asked for, so the keyboard
 * can follow: the named area active, or the display active in the active
 * area and no longer where it stood when asked. A move or split of the
 * active view therefore resolves once it has landed, never on the frame it
 * was asked from, and a refused one never resolves.
 */
export function focusRequestArrived(request: ViewFocusRequest, workspace: string, layout: ViewLayoutSnapshot): boolean {
  if (request.workspace !== workspace) return false;
  if ("areaId" in request) return layout.active_area === request.areaId;
  const located = locateDisplay(layout.root, request.displayId);
  if (!located || layout.active_area !== located.area.id || located.area.active !== request.displayId) return false;
  return !request.from || located.area.id !== request.from.areaId || located.index !== request.from.index;
}

/**
 * Where Grow or Shrink moves the split the active area sits in: one step,
 * within the core's range and, when the areas are drawn, both sides'
 * minimums; or why it cannot move that way.
 */
export function resizeTarget(layout: ViewLayoutSnapshot, geometry: Geometry | null, grow: boolean): { splitId: string; ratio: number } | { reason: string } {
  const parent = parentSplit(layout.root, layout.active_area);
  if (!parent) return { reason: "There is only one view area." };
  const delta = (grow ? 1 : -1) * (parent.side === "first" ? RESIZE_STEP : -RESIZE_STEP);
  const divider = geometry?.dividers.find((box) => box.id === parent.split.id);
  const ratio = divider ? steppedRatio(divider, delta) : moved(parent.split.ratio, clamp(parent.split.ratio + delta, RATIO_MIN, RATIO_MAX));
  if (ratio === null) return { reason: grow ? "This view area cannot grow any further." : "This view area cannot shrink any further." };
  return { splitId: parent.split.id, ratio };
}

function moved(before: number, after: number): number | null {
  return Math.abs(after - before) < 1e-6 ? null : after;
}

// --- narrow panels -----------------------------------------------------------

/**
 * How the Workspace tools stand (B12, D-08): the `column` beside the View
 * areas while the side panel has room for it; in a narrower panel an overlay
 * that is `closed` until the operator asks for a tool and `open` until they
 * dismiss it, so it never opens by itself.
 */
export type ToolsPlacement = "column" | "closed" | "open";

/**
 * Where the tools stand once the window is narrow or not: the column comes
 * back whenever there is room for it, and a window turning narrow closes it
 * into an overlay rather than floating the tools over the work by itself.
 */
export function placementForWidth(current: ToolsPlacement, narrow: boolean): ToolsPlacement {
  if (!narrow) return "column";
  return current === "column" ? "closed" : current;
}

/**
 * The tools a Workspace shows now: the ones the core stores while its side
 * panel shows, except that a narrow panel's overlay shows them only while the
 * operator has it open. The stored tools never change for it, so widening
 * brings the column back.
 */
export function shownTools(stored: { explorer: boolean; changes: boolean; panel: string }, placement: ToolsPlacement): { explorer: boolean; changes: boolean } {
  const shown = stored.panel !== "closed" && placement !== "closed";
  return { explorer: stored.explorer && shown, changes: stored.changes && shown };
}

function count(n: number, noun: string): string {
  return `${n} ${noun}${n === 1 ? "" : "s"}`;
}

function contains(rect: Rect, point: Point): boolean {
  return point.x >= rect.x && point.x < rect.x + rect.width && point.y >= rect.y && point.y < rect.y + rect.height;
}

function along(size: { width: number; height: number }, row: boolean): number {
  return row ? size.width : size.height;
}

function clamp(value: number, low: number, high: number): number {
  return Math.min(Math.max(value, low), high);
}
