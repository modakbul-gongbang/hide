import { describe, expect, it } from "vitest";
import type { AgentLayout } from "./agentLayout";
import type { TreeRow } from "./agentRow";
import { numberedTarget } from "./keyboard";
import { agentListOrder, numberedAgents, numberedTabs, numberOf, projectListNumbers } from "./numbering";
import { projectRows } from "./projects";
import type { AgentRow, Checkout, SnapshotRest, Workspace } from "./snapshot";

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
  return { id: paneId, pane_id: paneId, identity_label: paneId, agent_kind: "claude", symbol: "●", group: "working", status_code: "working", changed_at_unix_ms: null, emphasized: false, unread: false, ...extra };
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

  it("uses placed area order for both keycaps and numbered focus after a move", () => {
    const checkout = { ...checkoutWith(["t1", "t2", "waiting"]), path: "/fixture" };
    const layout: AgentLayout = {
      root: { split: { id: "s1", axis: "row", ratio: .5,
        first: { area: { id: "a1", active: "t2", displays: [{ id: "t2" }] } },
        second: { area: { id: "a2", active: "t1", displays: [{ id: "t1" }] } },
      } },
      active_area: "a2", canvases: {}, limits: { areas: 6, depth: 3, displays: 64 }, display_count: 2,
    };
    const rest = {
      navigator: { focused_checkout_id: "c1", focused_workspace_id: "w1", focused_device_id: "local", devices: [], workspaces: [{ id: "w1", checkouts: [checkout] }] },
      workspace_view: { device_id: "local", path: checkout.path, agent_layout: layout },
    } as unknown as SnapshotRest;
    const numbers = numberedTabs(checkout, layout);
    expect([...numbers]).toEqual([[1, "t2"], [2, "t1"]]);
    expect(numberOf(numbers, "waiting")).toBeNull();
    expect(numberedTarget("tabs", 1, { rest, agents: [] })).toBe("t2");
    expect(numberedTarget("tabs", 2, { rest, agents: [] })).toBe("t1");
    expect(numberedTarget("tabs", 3, { rest, agents: [] })).toBeNull();
    // The layout from a different Workspace cannot number this checkout.
    rest.workspace_view!.path = "/other";
    expect(numberedTarget("tabs", 1, { rest, agents: [] })).toBe("t1");
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

  it("numbers only the device in front's Agents list, so another device's agents hold no number (quick device-rail-badges B3)", () => {
    const devices = [
      { id: "local", label: "This Mac", kind: "local" },
      { id: "mini", label: "mini", kind: "remote" },
    ];
    const status = { remote: [{ target_id: "mini", state: "connected", session: { agents: [agent("r1", { group: "needs_you" })], workspaces: [] } }] };
    const local = [agent("l1")];
    const at = (front: string) => agentListOrder({ rest: { navigator: { focused_device_id: front, devices }, status } as unknown as SnapshotRest, agents: local }).map((row) => row.agent.pane_id);
    expect(at("local")).toEqual(["l1"]);
    expect(at("mini")).toEqual(["r1"]);
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

  it("shows each agent's Agents-list number once in Projects: its raised row, else its own checkout's row", () => {
    const checkout = (id: string, panes: string[]) => ({ id, tabs: [{ id: `${id}-t`, panes: panes.map((pane) => ({ id: pane })) }] });
    const workspaces = [{ id: "w", device_id: "local", pinned: false, checkouts: [checkout("main", ["ask", "parent"]), checkout("wt", ["child"])] } as unknown as Workspace];
    const listed = [agent("ask", { group: "needs_you" }), agent("parent"), agent("child")].map((row) => ({ agent: row, device: null }));
    const rows = projectRows(workspaces, [], listed);
    const numbered = new Map([[1, "ask"], [2, "parent"], [3, "child"]] as const);
    const numberOfPane = projectListNumbers(numbered, rows, workspaces);
    // Raised: the number sits there, and not again on its tree row.
    expect(numberOfPane("ask", null)).toBe(1);
    expect(numberOfPane("ask", "main")).toBeNull();
    // Not raised: only the checkout that owns the pane shows it, so a child
    // drawn under its parent in another checkout carries none there.
    expect(numberOfPane("parent", null)).toBeNull();
    expect(numberOfPane("parent", "main")).toBe(2);
    expect(numberOfPane("child", "main")).toBeNull();
    expect(numberOfPane("child", "wt")).toBe(3);
  });

  it("leaves a raised agent folded past its section's cap numbered on its tree row until the section opens", () => {
    const panes = ["d1", "d2", "d3", "d4"];
    const workspaces = [{ id: "w", device_id: "local", pinned: false, checkouts: [{ id: "main", tabs: [{ id: "t", panes: panes.map((id) => ({ id })) }] }] } as unknown as Workspace];
    const listed = panes.map((pane) => ({ agent: agent(pane, { group: "done" }), device: null }));
    const numbered = new Map([[4, "d4"]] as const);
    const folded = projectListNumbers(numbered, projectRows(workspaces, [], listed), workspaces);
    expect(folded("d4", null)).toBeNull();
    expect(folded("d4", "main")).toBe(4);
    const opened = projectListNumbers(numbered, projectRows(workspaces, [], listed, null, ["done"]), workspaces);
    expect(opened("d4", null)).toBe(4);
    expect(opened("d4", "main")).toBeNull();
  });
});
