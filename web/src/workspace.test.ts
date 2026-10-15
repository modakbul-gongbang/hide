import { describe, expect, it } from "vitest";
import type { Checkout, StripTab } from "./snapshot";
import { agentEntries, laneFrame, laneLanding, NO_CALLS_SEEN, readCalls, treeLanding, type LaneInput, type LaneSizes } from "./workspace";

const strip: StripTab[] = [
  { id: "herdr:1", kind: "herdr", source_id: "t1", label: "1", preview: false },
  { id: "file:a", kind: "file", source_id: "f-a", label: "a.md", preview: true },
  { id: "herdr:2", kind: "herdr", source_id: "t2", label: "2", preview: false },
  { id: "diff:b", kind: "diff", source_id: "d-b", label: "b.rs (working diff)", preview: false },
];

const checkout = { id: "c1", strip, tabs: [], active_tab_id: "t1" } as unknown as Checkout;

describe("the Workspace strips", () => {
  it("takes the Agent tabs from the core strip in the core's order", () => {
    expect(agentEntries(checkout).map((entry) => entry.source_id)).toEqual(["t1", "t2"]);
  });
});

// The token values (`--size-agent-views-min`, `--size-lane-min`,
// `--size-lane-ideal`, `--size-tree-min`, `--size-tree-ideal`,
// `--spacing-sm`), so the widths read as the PRD and the boards state them.
const sizes: LaneSizes = { agentMin: 480, laneMin: 360, laneIdeal: 520, treeMin: 180, treeIdeal: 240, divider: 8 };

const frame = (input: Partial<Omit<LaneInput, "sizes">> & { body: number }) =>
  laneFrame({ tool: null, pinned: false, tree: false, laneWidth: null, treeWidth: null, hasViews: true, shown: false, expanded: false, ...input, sizes });

describe("the right lane (PRD right-lane)", () => {
  it("gives Agent Views the whole body while the lane is closed or put away (B1, B4)", () => {
    expect(frame({ body: 1200 })).toMatchObject({ mode: "closed", agents: 1200 });
    expect(frame({ tool: "files", body: 1200 })).toMatchObject({ mode: "hidden", agents: 1200, width: 520 });
  });

  it("draws an unpinned lane over Agent Views at its width, Agent Views keeping its own (B3)", () => {
    expect(frame({ tool: "files", shown: true, body: 1200 })).toMatchObject({ mode: "overlay", agents: 1200, width: 520, document: 520, tree: null });
    expect(frame({ tool: "work", shown: true, laneWidth: 700, body: 1200 })).toMatchObject({ mode: "overlay", agents: 1200, width: 700 });
  });

  it("docks a pinned lane, Agent Views giving up its width and the divider's (B5, B7)", () => {
    expect(frame({ tool: "files", pinned: true, body: 1200 })).toMatchObject({ mode: "docked", agents: 1200 - 8 - 520, width: 520 });
    // The pin is the lane's, whichever tool shows and whether or not it was called.
    expect(frame({ tool: "memory", pinned: true, body: 1200 })).toMatchObject({ mode: "docked", agents: 672 });
  });

  it("keeps the stored width the same docked or drawn over, never under the lane's minimum (B6)", () => {
    expect(frame({ tool: "files", pinned: true, laneWidth: 600, body: 1400 }).width).toBe(600);
    expect(frame({ tool: "files", shown: true, laneWidth: 600, body: 1400 }).width).toBe(600);
    expect(frame({ tool: "files", shown: true, laneWidth: 100, body: 1400 }).width).toBe(360);
  });

  it("widens the lane by the tree beside a document, so the document keeps its width (B10)", () => {
    const tree = frame({ tool: "files", shown: true, tree: true, body: 1400 });
    expect(tree).toMatchObject({ width: 520 + 8 + 240, document: 520, tree: 240, treeOnly: false });
    // Docked, Agent Views gives up the tree's width too.
    expect(frame({ tool: "files", pinned: true, tree: true, body: 1400 })).toMatchObject({ mode: "docked", agents: 1400 - 8 - 768, document: 520, tree: 240 });
    // Another tool keeps the tree on without drawing it.
    expect(frame({ tool: "changes", shown: true, tree: true, body: 1400 })).toMatchObject({ width: 520, tree: null });
  });

  it("gives the tree the whole lane while no view is open, whether or not it is on (B12)", () => {
    for (const tree of [true, false]) {
      expect(frame({ tool: "files", shown: true, tree, hasViews: false, body: 1400 })).toMatchObject({ width: 520, tree: null, treeOnly: true });
    }
  });

  it("draws a pinned lane over Agent Views and keeps it up where docking would leave Agent Views under 480, and docks it again once the body widens (D-07, B19)", () => {
    // A document of 520 and a tree of 240 with their divider, beside the lane's divider and 480, need 1256.
    const narrow = frame({ tool: "files", pinned: true, tree: true, body: 1255 });
    expect(narrow).toMatchObject({ mode: "overlay", narrowPin: true, agents: 1255, width: 768 });
    const wide = frame({ tool: "files", pinned: true, tree: true, body: 1256 });
    expect(wide).toMatchObject({ mode: "docked", narrowPin: false, agents: 480, width: 768 });
    // Shrinking the window while pinned goes back over Agent Views, never hiding the lane.
    expect(frame({ tool: "files", pinned: true, body: 1007 })).toMatchObject({ mode: "overlay", narrowPin: true });
    expect(frame({ tool: "files", pinned: true, body: 1008 })).toMatchObject({ mode: "docked", agents: 480 });
  });

  it("opens the lane at any width, a body narrower than it giving up document width before tree width (B19)", () => {
    expect(frame({ tool: "files", shown: true, body: 400 })).toMatchObject({ mode: "overlay", width: 400, document: 400 });
    expect(frame({ tool: "files", shown: true, tree: true, body: 700 })).toMatchObject({ width: 700, document: 452, tree: 240 });
    expect(frame({ tool: "files", shown: true, tree: true, body: 560 })).toMatchObject({ width: 560, document: 360, tree: 192 });
  });

  it("expands over the whole body while Agent Views keeps the width it had (B8)", () => {
    expect(frame({ tool: "files", shown: true, expanded: true, tree: true, body: 1400 })).toMatchObject({ mode: "expanded", agents: 1400, width: 1400, tree: 240, document: 1400 - 8 - 240 });
    expect(frame({ tool: "files", pinned: true, expanded: true, body: 1400 })).toMatchObject({ mode: "expanded", agents: 1400 - 8 - 520, width: 1400 });
  });

  it("places nothing before the body is measured, so no terminal fits to a guess", () => {
    expect(frame({ tool: "files", pinned: true, body: 0 })).toMatchObject({ agents: 0, width: 0 });
  });

  it("lands the docked lane's divider as the document width, within the minimums (B6)", () => {
    const docked = frame({ tool: "files", pinned: true, body: 1600 });
    // The divider dragged 100px left widens the lane by 100px.
    expect(laneLanding({ x: 1600 - 520 - 8 - 100, body: 1600, frame: docked, sizes })).toEqual({ lane_width: 620 });
    expect(laneLanding({ x: 0, body: 1600, frame: docked, sizes })).toEqual({ lane_width: 1600 - 8 - 480 });
    expect(laneLanding({ x: 1590, body: 1600, frame: docked, sizes })).toEqual({ lane_width: 360 });
    // With the tree beside the document, the tree keeps its width.
    const tree = frame({ tool: "files", pinned: true, tree: true, body: 1600 });
    expect(laneLanding({ x: 1600 - 768 - 8 - 32, body: 1600, frame: tree, sizes })).toEqual({ lane_width: 552 });
  });

  it("trades width between the tree and the document, so the lane and Agent Views keep theirs (B10)", () => {
    const tree = frame({ tool: "files", pinned: true, tree: true, body: 1600 });
    const landed = treeLanding({ x: 300, frame: tree, sizes });
    expect(landed).toEqual({ tree_width: 300, lane_width: 460 });
    expect(treeLanding({ x: 10, frame: tree, sizes })).toEqual({ tree_width: 180, lane_width: 580 });
    expect(treeLanding({ x: 700, frame: tree, sizes })).toEqual({ tree_width: 400, lane_width: 360 });
    // Expanded, only the tree's width is kept, so the stored document width is not the body's.
    const expanded = frame({ tool: "files", shown: true, expanded: true, tree: true, body: 1600 });
    expect(treeLanding({ x: 300, frame: expanded, sizes })).toEqual({ tree_width: 300 });
  });
});

describe("readCalls", () => {
  const view = (path: string, lane_called: number, lane_calls: number) => ({ device_id: "local", path, lane_called, lane_calls });

  it("takes the first count as history, then shows the lane for a call to the Workspace in front", () => {
    const first = readCalls(NO_CALLS_SEEN, view("/a", 3, 5));
    expect(first).toMatchObject({ reset: true, call: false });
    const same = readCalls(first.seen, view("/a", 3, 5));
    expect(same).toMatchObject({ reset: false, call: false });
    const called = readCalls(same.seen, view("/a", 6, 6));
    expect(called).toMatchObject({ reset: false, call: true });
    expect(readCalls(called.seen, view("/a", 6, 6)).call).toBe(false);
  });

  it("leaves the front Workspace alone for a call into another, and shows the one a call brings in front", () => {
    const start = readCalls(NO_CALLS_SEEN, view("/a", 0, 2)).seen;
    const elsewhere = readCalls(start, view("/a", 0, 3));
    expect(elsewhere.call).toBe(false);
    // That older call does not move its Workspace when it is chosen later.
    expect(readCalls(elsewhere.seen, view("/b", 3, 3))).toMatchObject({ reset: true, call: false });
    // A reveal from the Overview: the called Workspace comes in front with the call.
    const away = readCalls(elsewhere.seen, null).seen;
    expect(readCalls(away, view("/c", 4, 4))).toMatchObject({ reset: true, call: true });
  });

  it("counts again from a core that started again", () => {
    const old = readCalls(NO_CALLS_SEEN, view("/a", 9, 9)).seen;
    const restarted = readCalls(old, view("/a", 0, 0));
    expect(restarted.call).toBe(false);
    expect(readCalls(restarted.seen, view("/a", 1, 1)).call).toBe(true);
    // A core first read at zero that starts again at zero still counts its calls.
    const fresh = readCalls(readCalls(NO_CALLS_SEEN, view("/a", 0, 0)).seen, view("/a", 3, 3)).seen;
    const again = readCalls(fresh, view("/a", 0, 0)).seen;
    expect(readCalls(again, view("/a", 1, 1)).call).toBe(true);
  });
});
