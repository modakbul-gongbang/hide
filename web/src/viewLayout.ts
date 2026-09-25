// The View areas' rules (PRD S7 B6-B13, B20; contract sections 2 and 6): the
// core owns the tree of areas and displays (`workspace_view.layout`), and
// these pure functions answer what the page asks of it - where each area and
// divider sits for a body rectangle, which area lies in a direction, whether
// a split fits, where a dragged tab would land, which commands a display's
// menu offers, and what a narrow window shows. They take values and return
// values, so every rule is testable without a page; nothing here dispatches.

import type { ViewAreaSnapshot, ViewDisplaySnapshot, ViewLayoutSnapshot, ViewNode, ViewSplitSnapshot } from "./snapshot";
import type { ViewMode } from "./workspace";

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
  const next = ratioForFirst(divider, (divider.ratio + delta) * available);
  return Math.abs(next - divider.ratio) < 1e-6 ? null : next;
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
  if (areasOf(layout.root).length >= layout.limits.areas) return refuse(`This Workspace already shows ${count(layout.limits.areas, "view area")}, the most it can.`);
  if ((areaDepth(layout.root, areaId) ?? 0) >= layout.limits.depth) return refuse(`View areas can be split only ${count(layout.limits.depth, "level")} deep.`);
  if (!geometry.fits) return refuse("The window is too small to show more view areas.");
  const box = geometry.areas.find((area) => area.id === areaId);
  if (!box) return refuse("That view area is not on screen.");
  const across = edge === "left" || edge === "right";
  const needed = across ? 2 * sizes.areaMinWidth + sizes.divider : 2 * sizes.areaMinHeight + sizes.divider;
  if ((across ? box.rect.width : box.rect.height) < needed) {
    return refuse(across ? "This view area is too narrow to split." : "This view area is too short to split.");
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
  const { area, display } = located;
  const entries: ViewMenuEntry[] = [];
  if (display.preview) entries.push({ id: "keep_open", label: "Keep open", unavailable: null });
  for (const edge of MENU_EDGES) {
    const eligibility = splitEligibility(layout, geometry, sizes, displayId, area.id, edge);
    entries.push({ id: `split_${edge}`, label: `Split ${EDGE_NAME[edge]}`, unavailable: eligibility.ok ? null : eligibility.reason });
  }
  for (const edge of MENU_EDGES) {
    if (neighbourArea(layout.root, area.id, edge)) entries.push({ id: `move_${edge}`, label: `Move ${EDGE_NAME[edge]}`, unavailable: null });
  }
  entries.push({ id: "copy_path", label: "Copy path", unavailable: null, separated: true });
  entries.push({ id: "reveal", label: "Reveal in Explorer", unavailable: revealBlocked(display) });
  entries.push({ id: "close_view", label: "Close view", unavailable: null, separated: true });
  return entries;
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
    if (contains(box.bar, point)) return barTarget(box, tabs[box.id] ?? [], located, point);
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

function barTarget(box: AreaBox, slots: TabSlot[], located: LocatedDisplay, point: Point): DropTarget {
  const slot = slots.filter((tab) => tab.rect.x + tab.rect.width / 2 < point.x).length;
  const at = slots[slot]?.rect;
  const last = slots.at(-1)?.rect;
  const x = at ? at.x : last ? last.x + last.width : box.bar.x;
  const line = { x, y: box.bar.y, height: box.bar.height };
  if (located.area.id !== box.id) return { kind: "bar", areaId: box.id, index: slot, line };
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

// --- narrow windows ----------------------------------------------------------

/**
 * What a narrow Workspace body shows (A7, B12, B13), decided from its width
 * alone and never stored: the tools become an overlay when the work area
 * cannot keep its minimum beside the tool column, and Agents and Views show
 * one at a time when both minimums do not fit even without the tools. An
 * unmeasured body (0) is neither.
 */
export function narrowWorkspace(input: {
  bodyWidth: number;
  mode: ViewMode;
  areaMin: number;
  panelMin: number;
  divider: number;
}): { toolsOverlay: boolean; singleRegion: boolean } {
  const { bodyWidth, mode, areaMin, panelMin, divider } = input;
  if (bodyWidth <= 0) return { toolsOverlay: false, singleRegion: false };
  const together = 2 * areaMin + divider;
  const work = mode === "together" ? together : areaMin;
  return {
    toolsOverlay: bodyWidth < work + panelMin,
    singleRegion: mode === "together" && bodyWidth < together,
  };
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
