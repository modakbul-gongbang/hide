import { describe, expect, it } from "vitest";
import type { AgentRow, Checkout, EditorSnapshot, EditorTabSnapshot, StripTab, Tab } from "./snapshot";
import { activeViewTab, agentEntries, agentWidth, shareAt, tabAgent, tabIdentity, viewEntries } from "./workspace";

const strip: StripTab[] = [
  { id: "herdr:1", kind: "herdr", source_id: "t1", label: "1", preview: false },
  { id: "file:a", kind: "file", source_id: "f-a", label: "a.md", preview: true },
  { id: "herdr:2", kind: "herdr", source_id: "t2", label: "2", preview: false },
  { id: "diff:b", kind: "diff", source_id: "d-b", label: "b.rs (working diff)", preview: false },
];

const checkout = { id: "c1", strip, tabs: [], active_tab_id: "t1" } as unknown as Checkout;

function fileTab(id: string, checkoutId: string, kind: EditorTabSnapshot["kind"] = "file"): EditorTabSnapshot {
  return { id, workspace_id: "w", checkout_id: checkoutId, path: `/repo/${id}`, label: id, kind, diff_committed: kind === "diff" ? false : null, markdown_live: true, wrap: false, dirty: false, preview: false };
}

function agent(pane: string, kind: string): AgentRow {
  return { id: pane, pane_id: pane, identity_label: `task ${pane}`, agent_kind: kind, symbol: "●", group: "working", status_label: "Working", elapsed: "1m", emphasized: false, unread: false };
}

describe("the Workspace strips", () => {
  it("splits one core strip into Agent tabs and View tabs in the core's order", () => {
    expect(agentEntries(checkout).map((entry) => entry.source_id)).toEqual(["t1", "t2"]);
    expect(viewEntries(checkout).map((entry) => entry.source_id)).toEqual(["f-a", "d-b"]);
  });

  it("shows the editor's active tab only when it is a file or diff of this checkout", () => {
    const editor = (active: string | null, tabs: EditorTabSnapshot[]): EditorSnapshot => ({ active_tab_id: active, document: null, tabs });
    expect(activeViewTab(editor("f-a", [fileTab("f-a", "c1")]), checkout)?.id).toBe("f-a");
    expect(activeViewTab(editor("f-x", [fileTab("f-x", "c2")]), checkout)).toBeNull();
    expect(activeViewTab(editor("s", [fileTab("s", "c1", "session")]), checkout)).toBeNull();
    expect(activeViewTab(editor(null, [fileTab("f-a", "c1")]), checkout)).toBeNull();
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
    expect(tabIdentity(entry, agent("p1", "codex"), null)).toBe("codex agent tab 1 · task p1 · Working");
    expect(tabIdentity(entry, null, null)).toBe("Terminal tab 1");
    const unavailable = { ...fileTab("f-a", "c1"), unavailable_reason: "gone" };
    expect(tabIdentity(strip[1]!, null, unavailable)).toBe("File: /repo/f-a · Unavailable");
    expect(tabIdentity(strip[3]!, null, fileTab("d-b", "c1", "diff"))).toBe("Working diff: /repo/d-b");
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
