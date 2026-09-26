import { describe, expect, it } from "vitest";
import type { AgentRow, Checkout, StripTab, Tab } from "./snapshot";
import { agentEntries, panelFrame, panelShareAt, tabAgent, tabIdentity, type WorkspaceView } from "./workspace";

const strip: StripTab[] = [
  { id: "herdr:1", kind: "herdr", source_id: "t1", label: "1", preview: false },
  { id: "file:a", kind: "file", source_id: "f-a", label: "a.md", preview: true },
  { id: "herdr:2", kind: "herdr", source_id: "t2", label: "2", preview: false },
  { id: "diff:b", kind: "diff", source_id: "d-b", label: "b.rs (working diff)", preview: false },
];

const checkout = { id: "c1", strip, tabs: [], active_tab_id: "t1" } as unknown as Checkout;

function agent(pane: string, kind: string): AgentRow {
  return { id: pane, pane_id: pane, identity_label: `task ${pane}`, agent_kind: kind, symbol: "●", group: "working", status_label: "Working", elapsed: "1m", emphasized: false, unread: false };
}

describe("the Workspace strips", () => {
  it("takes the Agent tabs from the core strip in the core's order", () => {
    expect(agentEntries(checkout).map((entry) => entry.source_id)).toEqual(["t1", "t2"]);
  });
});

describe("tab identity", () => {
  const tab = { id: "t1", panes: [{ id: "p1" }, { id: "p2" }] } as unknown as Tab;

  it("marks a Herdr tab with the focused pane's agent, else the first agent, else none", () => {
    const agents = [agent("p1", "codex"), agent("p2", "claude")];
    expect(tabAgent(tab, agents, "p2")?.agent_kind).toBe("claude");
    expect(tabAgent(tab, agents, "elsewhere")?.agent_kind).toBe("codex");
    expect(tabAgent(tab, [], "p1")).toBeNull();
  });

  it("names the kind and the full identity without replacing Herdr's tab name", () => {
    const entry = strip[0]!;
    expect(tabIdentity(entry, agent("p1", "codex"))).toBe("codex agent tab 1 · task p1 · Working");
    expect(tabIdentity(entry, null)).toBe("Terminal tab 1");
  });
});

describe("the side panel", () => {
  const sizes = { areaMin: 224, toolColumn: 260, toolMin: 200 };
  const view = (panel: WorkspaceView["panel"], extra: Partial<WorkspaceView> = {}) => ({ panel, pinned: false, views_over_share: 0.6, explorer: true, changes: false, ...extra });
  const frame = (panel: WorkspaceView["panel"], views: boolean, body: number, extra: Partial<WorkspaceView> = {}) => panelFrame({ view: view(panel, extra), views, body, sizes });

  it("floats at its share over agents that keep the body's width, and a pinned one narrows them to its edge", () => {
    expect(frame("open", true, 1400)).toMatchObject({ shown: "open", content: "views", width: 840, agentsRight: 0, resizable: true, narrow: false });
    expect(frame("open", true, 1400, { pinned: true })).toMatchObject({ width: 840, agentsRight: 840 });
    expect(frame("closed", true, 1400, { pinned: true })).toMatchObject({ shown: "closed", width: 0, agentsRight: 0 });
  });

  it("covers the whole body when expanded, and a pinned one leaves the agents at their docked width underneath", () => {
    expect(frame("expanded", true, 1400)).toMatchObject({ shown: "expanded", width: 1400, agentsRight: 0, resizable: false });
    expect(frame("expanded", true, 1400, { pinned: true })).toMatchObject({ width: 1400, agentsRight: 840 });
  });

  it("is only as wide as the tool column while no view is open, and says nothing is open only without a tool", () => {
    expect(frame("open", false, 1400)).toMatchObject({ content: "tools", width: 260, resizable: false });
    expect(frame("expanded", false, 1400)).toMatchObject({ shown: "open", content: "tools", width: 260 });
    expect(frame("open", false, 1400, { explorer: false })).toMatchObject({ content: "empty", width: 840, resizable: true });
    // Only views expand, so the empty state never covers the agents.
    expect(frame("expanded", false, 1400, { explorer: false })).toMatchObject({ shown: "open", content: "empty", width: 840 });
  });

  it("places nothing before the body is measured, so no terminal fits to a guess", () => {
    expect(frame("open", true, 0, { pinned: true })).toMatchObject({ width: 0, agentsRight: 0, toolsOverlay: false });
    expect(frame("open", false, 0, { pinned: true, explorer: false })).toMatchObject({ width: 0, agentsRight: 0 });
  });

  it("keeps a View area and the tool column beside it, and the agents left of it, at their minimum", () => {
    expect(frame("open", true, 1400, { views_over_share: 0.2 }).width).toBe(484);
    expect(frame("open", true, 1400, { views_over_share: 0.2, explorer: false }).width).toBe(280);
    expect(frame("open", true, 1000, { views_over_share: 0.8 }).width).toBe(776);
  });

  it("takes the whole body in a window too narrow for both, unsaved, and a pinned one floats there", () => {
    expect(frame("open", true, 707, { pinned: true })).toMatchObject({ shown: "expanded", narrow: true, width: 707, agentsRight: 0, resizable: false });
    expect(frame("open", true, 708, { pinned: true })).toMatchObject({ shown: "open", narrow: false, agentsRight: 484 });
  });

  it("folds the tools into an overlay when the panel cannot hold a View area beside them", () => {
    expect(frame("expanded", true, 423).toolsOverlay).toBe(true);
    expect(frame("expanded", true, 424).toolsOverlay).toBe(false);
    expect(frame("expanded", true, 424, { explorer: false }).toolsOverlay).toBe(false);
    expect(frame("open", true, 1400).toolsOverlay).toBe(false);
  });

  it("follows its left edge to a share the core keeps where it was released", () => {
    expect(panelShareAt(560, 1400, 484, 224)).toBeCloseTo(0.6);
    expect(panelShareAt(1300, 1400, 484, 224)).toBeCloseTo(484 / 1400);
    expect(panelShareAt(10, 1400, 484, 224)).toBeCloseTo(0.8);
    expect(panelShareAt(10, 1000, 484, 224)).toBeCloseTo(0.776);
    expect(panelShareAt(10, 0, 484, 224)).toBe(0.2);
  });
});
