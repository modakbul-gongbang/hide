import { describe, expect, it } from "vitest";
import type { ViewDisplaySnapshot, ViewLayoutSnapshot, ViewNode } from "./snapshot";
import {
  RATIO_MIN,
  displayIdentity,
  displayMenu,
  dropTarget,
  narrowWorkspace,
  neighbourArea,
  ratioForFirst,
  splitEligibility,
  steppedRatio,
  viewGeometry,
  type LayoutSizes,
  type Rect,
  type TabSlot,
} from "./viewLayout";

const SIZES: LayoutSizes = { areaMinWidth: 224, areaMinHeight: 144, divider: 2, tabStrip: 32 };

function display(id: string, overrides: Partial<ViewDisplaySnapshot> = {}): ViewDisplaySnapshot {
  return { id, tab_id: `file:${id}`, path: `/repo/${id}.md`, label: `${id}.md`, kind: "file", committed: null, preview: false, state: "open", reason: null, ...overrides };
}

function area(id: string, displays: (string | ViewDisplaySnapshot)[]): ViewNode {
  const rows = displays.map((entry) => (typeof entry === "string" ? display(entry) : entry));
  return { area: { id, active: rows[0]?.id ?? null, displays: rows } };
}

function split(id: string, axis: "row" | "column", ratio: number, first: ViewNode, second: ViewNode): ViewNode {
  return { split: { id, axis, ratio, first, second } };
}

function layout(root: ViewNode, limits: Partial<ViewLayoutSnapshot["limits"]> = {}): ViewLayoutSnapshot {
  return { root, active_area: "a1", limits: { areas: 6, depth: 3, displays: 64, ...limits }, display_count: 0 };
}

const body = (width: number, height = 600): Rect => ({ x: 0, y: 0, width, height });

describe("geometry", () => {
  it("keeps the stored ratio while both sides get their minimum, and a divider between them", () => {
    const g = viewGeometry(split("s1", "row", 0.5, area("a1", ["d1"]), area("a2", ["d2"])), body(1002), SIZES);
    expect(g.fits).toBe(true);
    expect(g.areas.map((box) => [box.id, box.rect.x, box.rect.width])).toEqual([["a1", 0, 500], ["a2", 502, 500]]);
    expect(g.areas[0]?.bar).toEqual({ x: 0, y: 0, width: 500, height: 32 });
    expect(g.areas[0]?.content).toEqual({ x: 0, y: 32, width: 500, height: 568 });
    expect(g.dividers[0]?.rect).toEqual({ x: 500, y: 0, width: 2, height: 600 });
  });

  it("holds a side at its minimum rather than the ratio", () => {
    const g = viewGeometry(split("s1", "row", 0.15, area("a1", ["d1"]), area("a2", ["d2"])), body(1002), SIZES);
    expect(g.areas.map((box) => box.rect.width)).toEqual([224, 776]);
  });

  it("fits exactly two minimums and a divider, and no less", () => {
    const tree = split("s1", "row", 0.5, area("a1", ["d1"]), area("a2", ["d2"]));
    expect(viewGeometry(tree, body(451), SIZES).fits).toBe(true);
    expect(viewGeometry(tree, body(450), SIZES).fits).toBe(true);
    expect(viewGeometry(tree, body(449), SIZES).fits).toBe(false);
  });

  it("sums the minimums of a nested split along its axis", () => {
    const tree = split("s1", "row", 0.5, area("a1", ["d1"]), split("s2", "row", 0.5, area("a2", ["d2"]), area("a3", ["d3"])));
    expect(viewGeometry(tree, body(676), SIZES).fits).toBe(true);
    expect(viewGeometry(tree, body(675), SIZES).fits).toBe(false);
  });

  it("measures a column split by height with the height minimum", () => {
    const tree = split("s1", "column", 0.5, area("a1", ["d1"]), area("a2", ["d2"]));
    expect(viewGeometry(tree, body(300, 290), SIZES).fits).toBe(true);
    expect(viewGeometry(tree, body(300, 289), SIZES).fits).toBe(false);
  });
});

describe("dividers", () => {
  const divider = (width: number, ratio = 0.5) => viewGeometry(split("s1", "row", ratio, area("a1", ["d1"]), area("a2", ["d2"])), body(width), SIZES).dividers[0]!;

  it("lands within both minimums and the core's range", () => {
    expect(ratioForFirst(divider(1002), 100)).toBeCloseTo(0.224);
    expect(ratioForFirst(divider(1002), 900)).toBeCloseTo(0.776);
    expect(ratioForFirst(divider(3002), 300)).toBe(RATIO_MIN);
    expect(ratioForFirst(divider(1002), 600)).toBeCloseTo(0.6);
  });

  it("steps by the keyboard step and stops at the bound", () => {
    expect(steppedRatio(divider(1002, 0.5), 0.05)).toBeCloseTo(0.55);
    expect(steppedRatio(divider(1002, 0.776), 0.05)).toBeNull();
    expect(steppedRatio(divider(1002, 0.75), 0.05)).toBeCloseTo(0.776);
  });
});

describe("neighbours", () => {
  // a1 on the left; a2 above a3 on the right, a2 the smaller.
  const tree = split("s1", "row", 0.5, area("a1", ["d1"]), split("s2", "column", 0.3, area("a2", ["d2"]), area("a3", ["d3"])));

  it("finds the area across a shared edge, the widest overlap first", () => {
    expect(neighbourArea(tree, "a1", "right")).toBe("a3");
    expect(neighbourArea(tree, "a2", "left")).toBe("a1");
    expect(neighbourArea(tree, "a2", "down")).toBe("a3");
    expect(neighbourArea(tree, "a3", "up")).toBe("a2");
  });

  it("has none past the Views' own edge", () => {
    expect(neighbourArea(tree, "a1", "left")).toBeNull();
    expect(neighbourArea(tree, "a2", "right")).toBeNull();
    expect(neighbourArea(tree, "a3", "down")).toBeNull();
  });
});

describe("split eligibility", () => {
  const two = area("a1", ["d1", "d2"]);

  it("needs two minimums and a divider along the split's axis", () => {
    const at = (width: number, height = 600) => viewGeometry(two, body(width, height), SIZES);
    expect(splitEligibility(layout(two), at(450), SIZES, "d1", "a1", "right")).toEqual({ ok: true });
    expect(splitEligibility(layout(two), at(449), SIZES, "d1", "a1", "right")).toEqual({ ok: false, reason: "This view area is too narrow to split." });
    expect(splitEligibility(layout(two), at(300, 290), SIZES, "d1", "a1", "down")).toEqual({ ok: true });
    expect(splitEligibility(layout(two), at(300, 289), SIZES, "d1", "a1", "up")).toEqual({ ok: false, reason: "This view area is too short to split." });
  });

  it("stops at the area cap and the depth cap the core publishes", () => {
    const g = viewGeometry(two, body(1200), SIZES);
    expect(splitEligibility(layout(two, { areas: 2 }), g, SIZES, "d1", "a1", "right").ok).toBe(true);
    expect(splitEligibility(layout(two, { areas: 1 }), g, SIZES, "d1", "a1", "right")).toMatchObject({ ok: false, reason: expect.stringContaining("1 view area,") });
    const nested = split("s1", "row", 0.5, area("a1", ["d1", "d2"]), area("a2", ["d3"]));
    const wide = viewGeometry(nested, body(2000), SIZES);
    expect(splitEligibility(layout(nested, { depth: 2 }), wide, SIZES, "d1", "a1", "right").ok).toBe(true);
    expect(splitEligibility(layout(nested, { depth: 1 }), wide, SIZES, "d1", "a1", "right")).toMatchObject({ ok: false, reason: expect.stringContaining("1 level deep") });
  });

  it("refuses a split that would change nothing, but not another area's view splitting a single-view area", () => {
    const tree = split("s1", "row", 0.5, area("a1", ["d1", "d2"]), area("a2", ["d3"]));
    const g = viewGeometry(tree, body(2000), SIZES);
    expect(splitEligibility(layout(tree), g, SIZES, "d3", "a2", "right")).toEqual({ ok: false, reason: "This is the only view in its area." });
    expect(splitEligibility(layout(tree), g, SIZES, "d1", "a2", "right")).toEqual({ ok: true });
  });

  it("refuses every split while the tree does not fit the window", () => {
    const tree = split("s1", "row", 0.5, area("a1", ["d1", "d2"]), area("a2", ["d3"]));
    const g = viewGeometry(tree, body(449), SIZES);
    expect(splitEligibility(layout(tree), g, SIZES, "d1", "a1", "down")).toEqual({ ok: false, reason: "The window is too small to show more view areas." });
  });
});

describe("a display's menu", () => {
  const tree = split("s1", "row", 0.5, area("a1", [display("d1", { preview: true }), "d2"]), area("a2", ["d3"]));
  const g = viewGeometry(tree, body(2000), SIZES);
  const ids = (displayId: string) => displayMenu(layout(tree), g, SIZES, displayId).map((entry) => entry.id);

  it("offers Keep open only for a preview, moves only toward an area, and nothing else", () => {
    expect(ids("d1")).toEqual(["keep_open", "split_right", "split_left", "split_up", "split_down", "move_right", "copy_path", "reveal", "close_view"]);
    expect(ids("d2")).toEqual(["split_right", "split_left", "split_up", "split_down", "move_right", "copy_path", "reveal", "close_view"]);
    expect(ids("d3")).toEqual(["split_right", "split_left", "split_up", "split_down", "move_left", "copy_path", "reveal", "close_view"]);
  });

  it("disables what cannot land with its reason", () => {
    const menu = displayMenu(layout(tree), g, SIZES, "d3");
    expect(menu.find((entry) => entry.id === "split_right")?.unavailable).toBe("This is the only view in its area.");
    const gone = split("s1", "row", 0.5, area("a1", [display("d1", { state: "unavailable", reason: "No such file" }), "d2"]), area("a2", ["d3"]));
    expect(displayMenu(layout(gone), g, SIZES, "d1").find((entry) => entry.id === "reveal")?.unavailable).toBe("The file is unavailable");
  });
});

describe("a dragged tab's target", () => {
  // a1 [d1 d2 d3] on the left (0..500), a2 [d4] on the right (502..1002).
  const tree = split("s1", "row", 0.5, area("a1", ["d1", "d2", "d3"]), area("a2", ["d4"]));
  const g = viewGeometry(tree, body(1002), SIZES);
  const tab = (displayId: string, x: number): TabSlot => ({ displayId, rect: { x, y: 0, width: 100, height: 32 } });
  const tabs = { a1: [tab("d1", 0), tab("d2", 100), tab("d3", 200)], a2: [tab("d4", 502)] };
  const at = (displayId: string, x: number, y: number, geometry = g, root = tree) => dropTarget({ layout: layout(root), geometry, sizes: SIZES, tabs, displayId, point: { x, y } });

  it("reorders within a bar by tab centres, naming the final position", () => {
    expect(at("d1", 250, 10)).toMatchObject({ kind: "bar", areaId: "a1", index: 1, line: { x: 200 } });
    expect(at("d1", 251, 10)).toMatchObject({ kind: "bar", areaId: "a1", index: 2, line: { x: 300 } });
    expect(at("d3", 40, 10)).toMatchObject({ kind: "bar", areaId: "a1", index: 0, line: { x: 0 } });
  });

  it("targets nothing where the tab already is", () => {
    expect(at("d2", 120, 10)).toEqual({ kind: "none", reason: null });
    expect(at("d2", 240, 10)).toEqual({ kind: "none", reason: null });
  });

  it("moves into another area's bar at the slot under the pointer", () => {
    expect(at("d1", 700, 10)).toMatchObject({ kind: "bar", areaId: "a2", index: 1 });
    expect(at("d1", 540, 10)).toMatchObject({ kind: "bar", areaId: "a2", index: 0 });
  });

  it("splits at an edge only inside its zone, previewing the half the new area takes", () => {
    // a2's content is 500 wide: the right zone starts past 1002 - 0.3 * 500 = 852.
    expect(at("d1", 853, 300)).toEqual({ kind: "edge", areaId: "a2", edge: "right", preview: { x: 752, y: 0, width: 250, height: 600 }, label: "Split right" });
    expect(at("d1", 852, 300)).toMatchObject({ kind: "none", reason: expect.any(String) });
    expect(at("d1", 600, 200)).toMatchObject({ kind: "edge", edge: "left", areaId: "a2" });
  });

  it("shows no split where it cannot land and says why", () => {
    const narrow = viewGeometry(tree, body(900), SIZES);
    expect(at("d1", 890, 300, narrow)).toEqual({ kind: "none", reason: "This view area is too narrow to split." });
    expect(at("d4", 990, 300)).toEqual({ kind: "none", reason: "This is the only view in its area." });
  });

  it("targets nothing outside the Views or for a view that is gone", () => {
    expect(at("d1", 1100, 300)).toMatchObject({ kind: "none", reason: expect.any(String) });
    expect(at("gone", 700, 10)).toEqual({ kind: "none", reason: "This view is no longer open." });
  });
});

describe("narrow windows", () => {
  const narrow = (bodyWidth: number, mode: "agents" | "together" | "views") => narrowWorkspace({ bodyWidth, mode, areaMin: 224, panelMin: 260, divider: 2 });

  it("turns the tools into an overlay when the work area cannot keep its minimum beside them", () => {
    expect(narrow(710, "together").toolsOverlay).toBe(false);
    expect(narrow(709, "together").toolsOverlay).toBe(true);
    expect(narrow(484, "views").toolsOverlay).toBe(false);
    expect(narrow(483, "views").toolsOverlay).toBe(true);
  });

  it("shows one working region when Together cannot fit both minimums", () => {
    expect(narrow(450, "together").singleRegion).toBe(false);
    expect(narrow(449, "together").singleRegion).toBe(true);
    expect(narrow(300, "views").singleRegion).toBe(false);
  });

  it("decides nothing before the body is measured", () => {
    expect(narrow(0, "together")).toEqual({ toolsOverlay: false, singleRegion: false });
  });
});

describe("a display's identity", () => {
  it("names the kind, the full path and the state", () => {
    expect(displayIdentity(display("d1", { preview: true }))).toBe("File: /repo/d1.md · Preview");
    expect(displayIdentity(display("d1", { state: "unavailable", reason: "No such file" }))).toBe("File: /repo/d1.md · Unavailable: No such file");
    expect(displayIdentity(display("d1", { kind: "diff", committed: true }))).toBe("Branch diff: /repo/d1.md");
    expect(displayIdentity(display("d1", { state: "waiting", reason: "Waiting for mini to connect" }))).toBe("File: /repo/d1.md · Waiting: Waiting for mini to connect");
  });
});
