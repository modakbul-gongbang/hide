import { describe, expect, it } from "vitest";
import type { TreeRow } from "./agentRow";
import { numberedTarget } from "./keyboard";
import { numberedAgents, numberedTabs, numberOf } from "./numbering";
import type { AgentRow, Checkout, SnapshotRest } from "./snapshot";

function checkoutWith(tabs: string[], extra: { id: string; kind: "file" | "diff" }[] = []): Checkout {
  return {
    id: "c1",
    workspace_id: "w1",
    strip: [
      ...tabs.map((id) => ({ id: `herdr:${id}`, kind: "herdr" as const, source_id: id, label: id, preview: false })),
      ...extra.map((row) => ({ id: `${row.kind}:${row.id}`, kind: row.kind, source_id: row.id, label: row.id, preview: false })),
    ],
    tabs: tabs.map((id) => ({ id, label: id, panes: [] })),
    active_tab_id: tabs[0] ?? null,
  } as unknown as Checkout;
}

function agent(paneId: string, extra: Partial<AgentRow> = {}): AgentRow {
  return { id: paneId, pane_id: paneId, identity_label: paneId, agent_kind: "claude", symbol: "●", group: "working", status_label: "Working", elapsed: "1m", emphasized: false, unread: false, ...extra };
}

describe("numbering (electron-digit-shortcuts-hints D-02)", () => {
  it("numbers the strip's Herdr tabs left to right, nine at most, and only those", () => {
    const tabs = Array.from({ length: 11 }, (_, index) => `t${index + 1}`);
    const numbered = numberedTabs(checkoutWith(tabs, [{ id: "f1", kind: "file" }]));
    expect([...numbered]).toEqual(tabs.slice(0, 9).map((id, index) => [index + 1, id]));
    expect(numberOf(numbered, "t10")).toBeNull();
    expect(numberOf(numbered, "f1")).toBeNull();
    expect(numberOf(numbered, "t4")).toBe(4);
  });

  it("numbers the Agents list's drawn rows top to bottom, folded children left out", () => {
    const rows: TreeRow[] = [
      { agent: agent("p1", { lineage_collapsed: false, lineage_child_pane_ids: ["p2"] }), device: null, depth: 0, descendants: 1 },
      { agent: agent("p2", { delegated: true }), device: null, depth: 1, descendants: 0 },
      { agent: agent("p3"), device: "mini", depth: 0, descendants: 0 },
    ];
    expect([...numberedAgents(rows)]).toEqual([
      [1, "p1"],
      [2, "p2"],
      [3, "p3"],
    ]);
    expect(numberedAgents([]).size).toBe(0);
  });

  it("selects what holds the number, and nothing past the end", () => {
    const parent = agent("p1", { lineage_child_pane_ids: ["p2"], lineage_collapsed: true });
    const child = agent("p2", { delegated: true, lineage_parent_pane_id: "p1" });
    const other = agent("p3", { group: "needs_you" });
    const rest = {
      navigator: { focused_checkout_id: "c1", focused_workspace_id: "w1", focused_device_id: "local", devices: [], workspaces: [{ id: "w1", label: "w", checkouts: [checkoutWith(["t1", "t2"])] }] },
      status: { remote: [] },
    } as unknown as SnapshotRest;
    const state = { rest, agents: [parent, child, other] };
    expect(numberedTarget("tabs", 2, state)).toBe("t2");
    expect(numberedTarget("tabs", 3, state)).toBeNull();
    // Needs You draws first; the folded child is not a row, so p1 is second and 3 is empty.
    expect(numberedTarget("agents", 1, state)).toBe("p3");
    expect(numberedTarget("agents", 2, state)).toBe("p1");
    expect(numberedTarget("agents", 3, state)).toBeNull();
    expect(numberedTarget("tabs", 1, { rest: null, agents: [] })).toBeNull();
  });
});
