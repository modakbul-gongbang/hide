import { describe, expect, it } from "vitest";
import type { AgentRow, Checkout, StripTab, Tab } from "./snapshot";
import { agentEntries, panelCoversToSend, panelFrame, panelShareAt, tabAgent, tabIdentity, type WorkspaceView } from "./workspace";

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
  const sizes = { areaMin: 224, toolColumn: 260, chrome: 0 };
  const view = (panel: WorkspaceView["panel"], extra: Partial<WorkspaceView> = {}) => ({ panel, pinned: false, views_over_share: 0.6, tools_share: null, tools: true, ...extra });
  const frame = (panel: WorkspaceView["panel"], views: boolean, body: number, extra: Partial<WorkspaceView> = {}) => panelFrame({ view: view(panel, extra), views, body, sizes });

  it("floats at its share over agents that keep the body's width, and a pinned one narrows them to its edge", () => {
    expect(frame("open", true, 1400)).toMatchObject({ shown: "open", content: "views", width: 840, agentsRight: 0, resize: "views_over_share", narrow: false });
    expect(frame("open", true, 1400, { pinned: true })).toMatchObject({ width: 840, agentsRight: 840 });
    expect(frame("closed", true, 1400, { pinned: true })).toMatchObject({ shown: "closed", width: 0, agentsRight: 0 });
  });

  it("covers the whole body when expanded, and a pinned one leaves the agents at their docked width underneath", () => {
    expect(frame("expanded", true, 1400)).toMatchObject({ shown: "expanded", width: 1400, agentsRight: 0, resize: null });
    expect(frame("expanded", true, 1400, { pinned: true })).toMatchObject({ width: 1400, agentsRight: 840 });
  });

  it("is only as wide as the tool column while no view is open, and says nothing is open only without a tool", () => {
    expect(frame("open", false, 1400)).toMatchObject({ content: "tools", width: 260, resize: "tools_share", need: 260 });
    expect(frame("expanded", false, 1400)).toMatchObject({ shown: "open", content: "tools", width: 260 });
    expect(frame("open", false, 1400, { tools: false })).toMatchObject({ content: "empty", width: 840, resize: "views_over_share" });
    // Only views expand, so the empty state never covers the agents.
    expect(frame("expanded", false, 1400, { tools: false })).toMatchObject({ shown: "open", content: "empty", width: 840 });
  });

  it("resizes the tools-only panel to a width of its own, never under the tool column or over the agents' minimum", () => {
    expect(frame("open", false, 1400, { tools_share: 0.4 })).toMatchObject({ content: "tools", width: 560, resize: "tools_share" });
    expect(frame("open", false, 1400, { tools_share: 0.4, pinned: true })).toMatchObject({ width: 560, agentsRight: 560 });
    expect(frame("open", false, 1400, { tools_share: 0.2 }).width).toBe(280);
    expect(frame("open", false, 1000, { tools_share: 0.2 }).width).toBe(260);
    expect(frame("open", false, 1000, { tools_share: 0.8 }).width).toBe(776);
    // The View areas keep their own width beside it.
    expect(frame("open", true, 1400, { tools_share: 0.4 }).width).toBe(840);
  });

  it("places nothing before the body is measured, so no terminal fits to a guess", () => {
    expect(frame("open", true, 0, { pinned: true })).toMatchObject({ width: 0, agentsRight: 0, toolsOverlay: false });
    expect(frame("open", false, 0, { pinned: true, tools: false })).toMatchObject({ width: 0, agentsRight: 0 });
  });

  it("keeps a View area and the tool column beside it, and the agents left of it, at their minimum", () => {
    expect(frame("open", true, 1400, { views_over_share: 0.2 }).width).toBe(484);
    expect(frame("open", true, 1400, { views_over_share: 0.2, tools: false }).width).toBe(280);
    expect(frame("open", true, 1000, { views_over_share: 0.8 }).width).toBe(776);
  });

  it("takes the whole body in a window too narrow for both, unsaved, and a pinned one floats there", () => {
    expect(frame("open", true, 707, { pinned: true })).toMatchObject({ shown: "expanded", narrow: true, width: 707, agentsRight: 0, resize: null, toolsOverlay: true });
    // With no view the tool column is the panel, never an overlay.
    expect(frame("open", false, 400)).toMatchObject({ narrow: true, content: "tools", toolsOverlay: false });
    expect(frame("open", true, 708, { pinned: true })).toMatchObject({ shown: "open", narrow: false, agentsRight: 484 });
  });

  it("counts its gap and hairlines in every minimum, so a View area and the tool column keep theirs", () => {
    const framed = { ...sizes, chrome: 10 };
    const at = (views: boolean, body: number, extra: Partial<WorkspaceView> = {}) => panelFrame({ view: view("open", extra), views, body, sizes: framed });
    expect(at(false, 1400)).toMatchObject({ content: "tools", width: 270 });
    expect(at(true, 1400, { views_over_share: 0.2 }).width).toBe(494);
    expect(at(true, 717)).toMatchObject({ narrow: true });
  });

  it("folds the tools into an overlay in a window too narrow for both, and only there", () => {
    expect(frame("expanded", true, 707).toolsOverlay).toBe(true);
    expect(frame("expanded", true, 708).toolsOverlay).toBe(false);
    expect(frame("expanded", true, 707, { tools: false }).toolsOverlay).toBe(false);
    expect(frame("open", true, 1400).toolsOverlay).toBe(false);
  });

  it("reports whether it covers the body once per change, and two pages that disagree each report once", () => {
    expect(panelCoversToSend(true, false, null)).toBe(true);
    expect(panelCoversToSend(true, true, null)).toBe(false);
    // Sent and not echoed yet, or undone by another page: not sent again.
    expect(panelCoversToSend(true, false, true)).toBe(false);
    // Forgotten after another Workspace or a reconnect: sent again.
    expect(panelCoversToSend(false, true, null)).toBe(true);
    expect(panelCoversToSend(false, true, true)).toBe(true);
  });

  it("follows its left edge to a share the core keeps where it was released", () => {
    expect(panelShareAt(560, 1400, 484, 224)).toBeCloseTo(0.6);
    expect(panelShareAt(1300, 1400, 484, 224)).toBeCloseTo(484 / 1400);
    expect(panelShareAt(10, 1400, 484, 224)).toBeCloseTo(0.8);
    expect(panelShareAt(10, 1000, 484, 224)).toBeCloseTo(0.776);
    expect(panelShareAt(10, 0, 484, 224)).toBe(0.2);
  });
});
