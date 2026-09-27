import { beforeEach, describe, expect, it } from "vitest";
import {
  currentSurface,
  expectSurface,
  focusSignature,
  isScreenVisit,
  lastSurfaceOf,
  observeEntries,
  observeProject,
  panelItem,
  projectItem,
  reconcileCycle,
  recentEntries,
  recentProjectOrder,
  resetRecent,
  visibleWindow,
} from "./recent";
import { createActions } from "./actions";
import { commitCycle, observeRecent, panelCycle, reconcileHeldCycle } from "./keyboard";
import { openingProgress } from "./navigation";
import type { AgentRow, Checkout, SnapshotRest, ViewDisplaySnapshot, Workspace } from "./snapshot";
import { useShellStore } from "./store";
import { useUiStore, type Screen } from "./ui";

// Two projects: "hide" with checkouts main (tabs t1, t2) and feature (tab t3,
// labelled like its project so its place collapses), and "notes" (tab t4).

function display(id: string, label: string, kind: ViewDisplaySnapshot["kind"] = "file"): ViewDisplaySnapshot {
  return { id, tab_id: null, path: `/${label}`, label, kind, committed: null, preview: false, state: "open", reason: null };
}

function checkout(id: string, workspaceId: string, label: string, tabs: Record<string, string[]>): Checkout {
  return {
    id,
    workspace_id: workspaceId,
    label,
    path: `/src/${id}`,
    tabs: Object.entries(tabs).map(([tabId, panes]) => ({ id: tabId, workspace_id: workspaceId, checkout_id: id, label: `${tabId} label`, empty: false, delegated: false, panes: panes.map((pane) => ({ id: pane })) })),
    active_tab_id: Object.keys(tabs)[0] ?? null,
    strip: [],
  } as unknown as Checkout;
}

const workspaces = (): Workspace[] =>
  [
    { id: "w-hide", label: "hide", device_id: "local", checkouts: [checkout("c-main", "w-hide", "main", { t1: ["p1"], t2: ["p2", "p3"] }), checkout("c-feature", "w-hide", "hide", { t3: ["p4"] })] },
    { id: "w-notes", label: "notes", device_id: "local", checkouts: [checkout("c-notes", "w-notes", "notes", { t4: ["p5"] })] },
  ] as unknown as Workspace[];

const agents = [
  { pane_id: "p1", identity_label: "Fix the build", symbol: "●", status_label: "Working" },
  { pane_id: "p2", identity_label: "Reviewer", symbol: "?", status_label: "Needs input" },
  { pane_id: "p3", identity_label: "Writer", symbol: "●", status_label: "Working" },
] as AgentRow[];

/** The session with `checkoutId` in front on `tabId`, its Workspace showing `displays`. */
function session(checkoutId: string, tabId: string, displays: ViewDisplaySnapshot[] = [], shape: Workspace[] = workspaces()): SnapshotRest {
  const front = shape.flatMap((row) => row.checkouts).find((row) => row.id === checkoutId)!;
  front.active_tab_id = tabId;
  return {
    navigator: { focused_workspace_id: front.workspace_id, focused_checkout_id: checkoutId, workspaces: shape, agents },
    workspace_view: {
      device_id: "local",
      path: front.path,
      panel: "open",
      pinned: true,
      tool: "explorer",
      tools: false,
      views_over_share: 0.5,
      layout: { root: { area: { id: "a1", active: displays[0]?.id ?? null, displays } }, active_area: "a1", limits: { areas: 4, depth: 3, displays: 64 }, display_count: displays.length },
    },
  } as unknown as SnapshotRest;
}

function use(rest: SnapshotRest, inView = false) {
  observeEntries(rest, currentSurface(rest, inView));
}

const order = () => recentEntries().map((entry) => (isScreenVisit(entry) ? entry.key : `${entry.checkoutId}/${entry.id}`));

describe("Recent Panels order", () => {
  beforeEach(() => resetRecent());

  it("is one order over tabs and displays across checkouts, unused surfaces after", () => {
    use(session("c-main", "t1"));
    use(session("c-notes", "t4", [display("d1", "plan.md")]), true);
    use(session("c-main", "t2"));
    expect(order()).toEqual(["c-main/t2", "c-notes/d1", "c-main/t1", "c-feature/t3", "c-notes/t4"]);
  });

  it("keeps a display of a checkout not in front, and drops one its Workspace closed", () => {
    use(session("c-notes", "t4", [display("d1", "plan.md"), display("d2", "todo.md")]), true);
    use(session("c-main", "t1"));
    expect(order()).toContain("c-notes/d1");
    expect(order()).toContain("c-notes/d2");
    // Back on notes, d2 was closed: the snapshot's tree no longer has it.
    use(session("c-notes", "t4", [display("d1", "plan.md")]));
    expect(order()).toContain("c-notes/d1");
    expect(order()).not.toContain("c-notes/d2");
  });

  it("drops every surface of a checkout that is gone", () => {
    use(session("c-feature", "t3"));
    use(session("c-main", "t1"));
    const shape = workspaces();
    shape[0]!.checkouts.pop();
    use(session("c-main", "t1", [], shape));
    expect(order().some((row) => row.startsWith("c-feature/"))).toBe(false);
  });

  it("records a commit's target, not the frames it passes through on the way", () => {
    use(session("c-main", "t1"));
    const target = displayKey("c-notes", "d1");
    expectSurface(target);
    // The checkout arrives with the keyboard still in the terminal.
    use(session("c-notes", "t4", [display("d1", "plan.md")]));
    expect(order()[0]).toBe("c-main/t1");
    // Then the keyboard reaches the display.
    use(session("c-notes", "t4", [display("d1", "plan.md")]), true);
    expect(order().slice(0, 2)).toEqual(["c-notes/d1", "c-main/t1"]);
  });

  it("treats the next use as a visit again once the wait is ended", () => {
    use(session("c-main", "t1"));
    expectSurface(displayKey("c-notes", "d9"));
    expectSurface(null);
    use(session("c-notes", "t4"));
    expect(order()[0]).toBe("c-notes/t4");
  });
});

describe("what counts as a move", () => {
  it("changes with the checkout, tab or active display in front, not with an agent's status", () => {
    const base = session("c-main", "t1", [display("d1", "plan.md"), display("d2", "todo.md")]);
    const statusOnly = { ...base, navigator: { ...base.navigator, agents: [] } } as SnapshotRest;
    expect(focusSignature(statusOnly)).toBe(focusSignature(base));
    expect(focusSignature(session("c-main", "t2", [display("d1", "plan.md")]))).not.toBe(focusSignature(session("c-main", "t1", [display("d1", "plan.md")])));
    expect(focusSignature(session("c-main", "t1", [display("d2", "todo.md")]))).not.toBe(focusSignature(session("c-main", "t1", [display("d1", "plan.md")])));
  });
});

function displayKey(checkoutId: string, displayId: string) {
  return `display\u0000${checkoutId}\u0000${displayId}`;
}

describe("Recent Panels rows", () => {
  beforeEach(() => resetRecent());

  it("names a one-agent tab by its agent with its mark, and keeps a shared tab's label", () => {
    const rest = session("c-main", "t1");
    use(rest);
    const [one, two] = ["t1", "t2"].map((id) => panelItem(rest, recentEntries().find((entry) => !isScreenVisit(entry) && entry.id === id)!)!);
    expect(one).toMatchObject({ title: "Fix the build", detail: "hide · main · Terminal", agent: { symbol: "●" } });
    expect(two).toMatchObject({ title: "t2 label", agent: null });
  });

  it("collapses the place to the checkout when it shares the project's name, and names a display's type", () => {
    const rest = session("c-feature", "t3", [display("d1", "a.diff", "diff")]);
    use(rest, true);
    expect(panelItem(rest, recentEntries()[0]!)).toMatchObject({ title: "a.diff", detail: "hide · Diff" });
  });
});

describe("Recent Panels over All projects and each Overview", () => {
  beforeEach(() => {
    resetRecent();
    useUiStore.setState({ screen: null, cycle: null, opening: null });
  });

  /** The page showing `screen` over `rest`, observed as the page observes a move. */
  function show(rest: SnapshotRest, screen: Screen) {
    useShellStore.setState({ rest });
    useUiStore.setState({ screen });
    observeRecent(rest, true);
  }

  const rows = (rest: SnapshotRest) => panelCycle(rest)!.items.map((item) => `${item.kind} ${item.title} (${item.detail})`);

  it("puts the Overview just left one chord back, and from the Overview the Workspace surface it was left from", () => {
    const rest = session("c-main", "t1");
    show(rest, { kind: "workspace" });
    show(rest, { kind: "overview", projectId: "w-notes" });
    expect(rows(rest).slice(0, 2)).toEqual(["overview notes (Overview)", "herdr Fix the build (hide · main · Terminal)"]);
    show(rest, { kind: "workspace" });
    expect(rows(rest).slice(0, 2)).toEqual(["herdr Fix the build (hide · main · Terminal)", "overview notes (Overview)"]);
  });

  it("keeps one row per Overview, moves a revisited one to the front, and has All projects as its own row", () => {
    const rest = session("c-main", "t1");
    show(rest, { kind: "main" });
    show(rest, { kind: "overview", projectId: "w-notes" });
    show(rest, { kind: "workspace" });
    show(rest, { kind: "overview", projectId: "w-hide" });
    show(rest, { kind: "overview", projectId: "w-notes" });
    expect(rows(rest)).toEqual([
      "overview notes (Overview)",
      "overview hide (Overview)",
      "herdr Fix the build (hide · main · Terminal)",
      "main All projects (Overview)",
      "herdr t2 label (hide · main · Terminal)",
      "herdr t3 label (hide · Terminal)",
      "herdr t4 label (notes · Terminal)",
    ]);
    // Recent Projects still restores a Project's Workspace surface, not its Overview.
    expect(lastSurfaceOf("w-hide")?.id).toBe("t1");
  });

  it("drops the Overview of a Project that left the catalog, from the order and from a held cycle", () => {
    const rest = session("c-main", "t1");
    show(rest, { kind: "overview", projectId: "w-notes" });
    show(rest, { kind: "workspace" });
    const held = { ...panelCycle(rest)!, index: 1 };
    expect(held.items[1]).toMatchObject({ kind: "overview", title: "notes" });
    const gone = session("c-main", "t1", [], workspaces().slice(0, 1));
    show(gone, { kind: "workspace" });
    expect(rows(gone).some((row) => row.startsWith("overview"))).toBe(false);
    expect(reconcileHeldCycle(held, gone)!.items.map((item) => item.kind)).not.toContain("overview");
  });

  it("commits an Overview row by showing it, and a Workspace surface from an Overview by bringing its checkout forward", () => {
    const sent: { kind: string; payload: Record<string, unknown> }[] = [];
    const actions = createActions((event) => {
      sent.push(event as { kind: string; payload: Record<string, unknown> });
      return true;
    });
    const rest = session("c-main", "t1");
    show(rest, { kind: "overview", projectId: "w-notes" });
    show(rest, { kind: "workspace" });
    // A commit still on its way does not keep the Overview from being the visit.
    expectSurface(displayKey("c-notes", "d9"));
    commitCycle({ ...panelCycle(rest)!, index: 1 }, actions);
    expect(useUiStore.getState().screen).toEqual({ kind: "overview", projectId: "w-notes" });
    expect(sent).toEqual([]);
    observeRecent(rest, true);
    expect(rows(rest)[0]).toBe("overview notes (Overview)");

    commitCycle({ ...panelCycle(rest)!, index: 1 }, actions);
    expect(sent).toMatchObject([{ kind: "focus_tab", payload: { workspace_id: "w-hide", checkout_id: "c-main", tab_id: "t1" } }]);
    // The checkout is already in front, so the Workspace shows at once.
    expect(openingProgress(rest, useUiStore.getState().opening!)).toBe("landed");
  });
});

describe("Recent Projects", () => {
  beforeEach(() => resetRecent());

  it("orders projects by use and restores each one's last surface", () => {
    const notes = session("c-notes", "t4", [display("d1", "plan.md", "browser")]);
    use(notes, true);
    observeProject("w-notes");
    const main = session("c-main", "t2");
    use(main);
    observeProject("w-hide");
    expect(recentProjectOrder(["w-notes", "w-hide", "w-new"])).toEqual(["w-hide", "w-notes", "w-new"]);
    expect(lastSurfaceOf("w-notes")?.id).toBe("d1");
    const row = projectItem(main.navigator!.workspaces![1]!, main, true);
    expect(row).toMatchObject({ title: "notes", detail: "plan.md · notes", target: { kind: "surface", surface: { id: "d1", kind: "browser" } } });
  });

  it("comes forward on its checkout when no surface of it was used", () => {
    const rest = session("c-main", "t1");
    const row = projectItem(rest.navigator!.workspaces![1]!, rest, false);
    expect(row).toMatchObject({ target: { kind: "checkout", checkoutId: "c-notes" }, detail: "notes" });
  });
});

describe("the held cycle", () => {
  it("shows at most nine rows with the highlight among them", () => {
    const items = Array.from({ length: 20 }, (_, index) => index);
    expect(visibleWindow(items, 0)).toEqual({ start: 0, rows: items.slice(0, 9) });
    expect(visibleWindow(items, 10)).toEqual({ start: 6, rows: items.slice(6, 15) });
    expect(visibleWindow(items, 19)).toEqual({ start: 11, rows: items.slice(11, 20) });
    expect(visibleWindow(items.slice(0, 3), 2)).toEqual({ start: 0, rows: [0, 1, 2] });
  });

  it("moves a highlight that went to the next surviving row without reordering, and ends with none left", () => {
    const rows = ["a", "b", "c", "d"].map((key) => ({ key }));
    expect(reconcileCycle(rows, 1, (row) => row.key !== "b")).toEqual({ items: [{ key: "a" }, { key: "c" }, { key: "d" }], index: 1 });
    expect(reconcileCycle(rows, 3, (row) => row.key === "a")).toEqual({ items: [{ key: "a" }], index: 0 });
    expect(reconcileCycle(rows, 2, () => false)).toBeNull();
  });
});
