import { describe, expect, it } from "vitest";
import type { AgentRow, Checkout, StripTab, Tab } from "./snapshot";
import { agentEntries, agentWidth, drawnMode, shareAt, tabAgent, tabIdentity, type WorkspaceView } from "./workspace";

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

describe("the Agent/View boundary", () => {
  it("keeps each area at its minimum and splits evenly when both minimums cannot fit", () => {
    expect(agentWidth(0.5, 1000, 320)).toBe(500);
    expect(agentWidth(0.1, 1000, 320)).toBe(320);
    expect(agentWidth(0.95, 1000, 320)).toBe(680);
    expect(agentWidth(0.2, 600, 320)).toBe(300);
    expect(shareAt(100, 1000, 320)).toBeCloseTo(0.32);
  });
});

describe("the drawn layout", () => {
  const view = (mode: WorkspaceView["mode"], displays: number): WorkspaceView =>
    ({ device_id: "local", path: "/r", mode, explorer: true, changes: false, agent_share: 0.5, layout: { display_count: displays } }) as unknown as WorkspaceView;

  it("leaves View areas with nothing open out of Together and Views only, keeping the stored mode's return", () => {
    expect(drawnMode(view("together", 0), false)).toBe("agents");
    expect(drawnMode(view("views", 0), false)).toBe("agents");
    expect(drawnMode(view("together", 1), false)).toBe("together");
    expect(drawnMode(view("views", 2), false)).toBe("views");
  });

  it("keeps the View areas while a file is opening, so the opening state has a place", () => {
    expect(drawnMode(view("together", 0), true)).toBe("together");
    expect(drawnMode(view("agents", 3), false)).toBe("agents");
  });
});
