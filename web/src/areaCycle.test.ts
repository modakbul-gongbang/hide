import { beforeEach, describe, expect, it } from "vitest";
import { areaCycle, focusedCycleScope } from "./areaCycle";
import { noteAreaFrame } from "./areaFrames";
import { areaGeometry } from "./areaLayout";
import { createActions } from "./actions";
import { commitCycle, reconcileHeldCycle } from "./keyboard";
import { observeEntries, resetRecent, tabSurface } from "./recent";
import type { SnapshotRest } from "./snapshot";
import { useShellStore } from "./store";
import { useUiStore } from "./ui";

// a1 exists independently in Agent and View; another Agent area and
// checkout both contain tabs that must never leak into the cycle.
function world(): SnapshotRest {
  const limits = { areas: 6, depth: 5, displays: 64 };
  const tabs = ["t1", "t2", "t3", "outside"].map((id) => ({ id, label: id, panes: [{ id: `${id}-pane` }], workspace_id: "w", checkout_id: "c", empty: false, delegated: false }));
  const checkout = { id: "c", workspace_id: "w", path: "/fixture", label: "fixture", tabs, active_tab_id: "t1", strip: [] };
  return {
    navigator: { focused_device_id: "local", focused_checkout_id: "c", focused_workspace_id: "w", devices: [{ id: "local", label: "This Mac", kind: "local", state: "local" }], workspaces: [{ id: "w", label: "fixture", device_id: "local", checkouts: [checkout, { ...checkout, id: "other", path: "/other", tabs: [{ ...tabs[0], id: "other-tab" }] }] }] },
    workspace_view: { device_id: "local", path: "/fixture", panel: "open", layout: { root: { area: { id: "a1", active: "d1", displays: ["d1", "d2"].map((id) => ({ id, kind: "browser", label: id, url: "about:blank", state: "open", tab_id: null })) } }, active_area: "a1", display_count: 2, limits }, agent_layout: { root: { split: { id: "s1", axis: "row", ratio: 0.5, first: { area: { id: "a1", active: "t1", displays: ["t1", "t2", "t3"].map((id) => ({ id })) } }, second: { area: { id: "a2", active: "outside", displays: [{ id: "outside" }] } } } }, active_area: "a1", canvases: {}, display_count: 4, limits } },
  } as unknown as SnapshotRest;
}
function draw(rest: SnapshotRest) {
  const view = rest.workspace_view!;
  const sizes = { areaMinWidth: 100, areaMinHeight: 100, divider: 4, tabStrip: 30 };
  for (const kind of ["agent", "view"] as const) {
    const layout = kind === "agent" ? view.agent_layout! : view.layout!;
    noteAreaFrame(kind, { workspace: { device_id: "local", path: "/fixture" }, layout, geometry: areaGeometry(layout.root, { x: 0, y: 0, width: 1200, height: 800 }, sizes), sizes } as never);
  }
  useShellStore.setState({ rest });
}
const ids = (cycle: ReturnType<typeof areaCycle>) => cycle?.items.map((row) => row.target.kind === "surface" ? row.target.surface.id : "wrong-kind");
const owner = { kind: "pane", workspace: "c", paneId: "t1-pane" } as const;

describe("focused-area recent tabs", () => {
  beforeEach(() => { resetRecent(); useUiStore.setState({ screen: { kind: "workspace" }, cycle: null }); noteAreaFrame("agent", null); noteAreaFrame("view", null); });
  it("uses exact Agent membership and MRU despite colliding area IDs and other checkouts", () => {
    const rest = world(); draw(rest);
    const checkout = rest.navigator!.workspaces![0]!.checkouts[0]!;
    observeEntries(rest, tabSurface(checkout, "t2"));
    observeEntries(rest, tabSurface(checkout, "outside"));
    observeEntries(rest, tabSurface(checkout, "t3"));
    expect(ids(areaCycle(rest, owner))).toEqual(["t1", "t3", "t2"]);
    expect(ids(areaCycle(rest, { kind: "agent", workspace: "c", areaId: "a1" }))).toEqual(["t1", "t3", "t2"]);
    expect(ids(areaCycle(rest, { kind: "view", workspace: "c", areaId: "a1" }))).toEqual(["d1", "d2"]);
  });
  it("commits exactly one in-place selection and never dispatches a preview", () => {
    const rest = world(); draw(rest);
    const sent: unknown[] = [];
    const actions = createActions((event) => { sent.push(event); });
    const cycle = areaCycle(rest, owner)!;
    expect(sent).toEqual([]);
    commitCycle(cycle, actions);
    expect(sent).toEqual([]);
    commitCycle({ ...cycle, index: 1 }, actions);
    expect(sent).toEqual([expect.objectContaining({ kind: "focus_tab", payload: expect.objectContaining({ tab_id: "t2", in_place: true }) })]);
    sent.length = 0;
    const views = areaCycle(rest, { kind: "view", workspace: "c", areaId: "a1" })!;
    commitCycle({ ...views, index: 1 }, actions);
    expect(sent).toEqual([expect.objectContaining({ kind: "view_layout", payload: expect.objectContaining({ action: "focus", display_id: "d2" }) })]);
    expect(useUiStore.getState().viewFocusRequest).toEqual({ workspace: "local\u0000/fixture", displayId: "d2", from: null });
  });
  it("prunes a moved tab using the new snapshot, keeps the frozen order and cancels a removed origin", () => {
    const rest = world(); draw(rest);
    const held = { ...areaCycle(rest, owner)!, index: 1 };
    const root = rest.workspace_view!.agent_layout!.root;
    if (!("split" in root)) throw new Error("fixture split missing");
    root.split.first = { area: { id: "a1", active: "t1", displays: [{ id: "t1" }, { id: "t3" }] } };
    expect(ids(reconcileHeldCycle(held, rest))).toEqual(["t1", "t3"]);
    expect(reconcileHeldCycle(held, rest)?.index).toBe(1);
    root.split.first = { area: { id: "new-area", active: "t1", displays: [{ id: "t1" }, { id: "t3" }] } };
    expect(reconcileHeldCycle(held, rest)).toBeNull();
  });
  it("never falls back from a tool, outside owner, Overview, hidden View or delegated canvas", () => {
    const rest = world(); draw(rest);
    expect(areaCycle(rest, { kind: "tool", workspace: "c" })).toBeNull();
    expect(areaCycle(rest, { kind: "view", workspace: "other", areaId: "a1" })).toBeNull();
    useUiStore.setState({ screen: { kind: "main" } });
    expect(areaCycle(rest, owner)).toBeNull();
    useUiStore.setState({ screen: { kind: "workspace" } });
    rest.workspace_view!.panel = "closed";
    expect(areaCycle(rest, { kind: "view", workspace: "c", areaId: "a1" })).toBeNull();
    rest.workspace_view!.agent_layout!.canvases.a1 = "outside";
    expect(focusedCycleScope(rest, { kind: "pane", workspace: "c", paneId: "outside-pane" })?.areaId).toBe("a2");
    expect(focusedCycleScope(rest, owner)).toBeNull();
  });
});
