import { describe, expect, it } from "vitest";
import { agentClosing, closeDecision, subtreeOf } from "./close";
import type { AgentRow, PaneRow } from "./snapshot";

function pane(id: string, extra: Partial<PaneRow> = {}): PaneRow {
  return {
    id,
    herdr_label: id,
    terminal_title: null,
    cwd: "/",
    status_label: "Idle",
    requires_close_confirmation: false,
    requires_close_status_check: false,
    identity_label: null,
    ...extra,
  };
}

function agent(paneId: string, extra: Partial<AgentRow> = {}): AgentRow {
  return {
    id: `agent:${paneId}`,
    pane_id: paneId,
    identity_label: "claude",
    agent_kind: "claude",
    symbol: "c",
    group: "working",
    status_label: "Working",
    elapsed: "1m",
    emphasized: false,
    unread: false,
    ...extra,
  };
}

describe("closeDecision", () => {
  it("closes an idle pane without asking", () => {
    expect(closeDecision("pane", [pane("p1")], [])).toEqual({ action: "close" });
  });

  it("asks once for a pane with working or attention state", () => {
    const decision = closeDecision("pane", [pane("p1", { requires_close_confirmation: true })], []);
    expect(decision.action).toBe("confirm");
    expect(decision).toMatchObject({ affected: ["p1"] });
    const byAgent = closeDecision("pane", [pane("p1")], [agent("p1", { requires_close_confirmation: true })]);
    expect(byAgent.action).toBe("confirm");
  });

  it("refuses to close while any pane's status is unknown", () => {
    const decision = closeDecision(
      "tab",
      [pane("p1"), pane("p2", { requires_close_status_check: true, herdr_label: "worker" })],
      [],
    );
    expect(decision).toEqual({ action: "status_unknown", label: "worker" });
  });

  it("lists only the risky panes of a tab", () => {
    const decision = closeDecision(
      "tab",
      [pane("p1"), pane("p2", { requires_close_confirmation: true }), pane("p3", { requires_close_confirmation: true })],
      [],
    );
    expect(decision).toMatchObject({ action: "confirm", affected: ["p2", "p3"] });
  });
});

// p1 spawned p2 and p4; p2 spawned p3. Close lists come from the core,
// deepest first, so a test states them as the core would publish them.
function family(extra: Record<string, Partial<AgentRow>> = {}): AgentRow[] {
  return [
    agent("p1", { lineage_depth: 0, lineage_child_pane_ids: ["p2", "p4"], close_descendant_pane_ids: ["p3", "p2", "p4"], ...extra.p1 }),
    agent("p2", { lineage_depth: 1, lineage_parent_pane_id: "p1", lineage_child_pane_ids: ["p3"], close_descendant_pane_ids: ["p3"], ...extra.p2 }),
    agent("p3", { lineage_depth: 2, lineage_parent_pane_id: "p2", ...extra.p3 }),
    agent("p4", { lineage_depth: 1, lineage_parent_pane_id: "p1", activity: "idle", group: "idle", ...extra.p4 }),
  ];
}

describe("subtreeOf", () => {
  it("leaves the ordinary close alone for an agent with no descendants", () => {
    expect(subtreeOf(["p3"], family())).toBeNull();
    expect(subtreeOf(["p9"], family())).toBeNull();
  });

  it("lists the target then its descendants in tree order, the target not counted", () => {
    const subtree = subtreeOf(["p1"], family({ p2: { activity: "working" }, p3: { demand: "question" } }));
    expect(subtree?.ids).toEqual(["p3", "p2", "p4"]);
    expect(subtree?.rows.map((row) => [row.agent.pane_id, row.depth, row.target, row.state])).toEqual([
      ["p1", 0, true, "quiet"],
      ["p2", 1, false, "working"],
      ["p3", 2, false, "waiting"],
      ["p4", 1, false, "quiet"],
    ]);
    expect(subtree?.counts).toEqual({ working: 1, waiting: 1, unread: 0, unknown: 0 });
    expect(subtree?.unknown).toBe(false);
  });

  it("counts a tab's descendants as the union outside the tab", () => {
    const subtree = subtreeOf(["p1", "p2"], family());
    expect(subtree?.ids).toEqual(["p3", "p4"]);
  });

  it("marks the choice blocked while a descendant's status is unknown", () => {
    const subtree = subtreeOf(["p1"], family({ p4: { requires_close_status_check: true } }));
    expect(subtree?.unknown).toBe(true);
    expect(subtree?.counts.unknown).toBe(1);
  });

  it("follows the snapshot: a descendant that appears is listed and one whose row went is dropped", () => {
    // p4's row is gone while p1's list still names it; p5 was just spawned by p3.
    const agents = [
      ...family({
        p1: { close_descendant_pane_ids: ["p5", "p3", "p2", "p4"] },
        p2: { close_descendant_pane_ids: ["p5", "p3"] },
        p3: { lineage_child_pane_ids: ["p5"], close_descendant_pane_ids: ["p5"] },
      }).filter((row) => row.pane_id !== "p4"),
      agent("p5", { lineage_depth: 3, lineage_parent_pane_id: "p3", activity: "working" }),
    ];
    const subtree = subtreeOf(["p1"], agents);
    expect(subtree?.ids).toEqual(["p5", "p3", "p2"]);
    expect(subtree?.rows.map((row) => [row.agent.pane_id, row.depth])).toEqual([["p1", 0], ["p2", 1], ["p3", 2], ["p5", 3]]);
    expect(subtree?.counts.working).toBe(1);
  });
});

describe("agentClosing", () => {
  const op = (kind: string, target_id: string, phase: string) => ({ id: `${kind}:${target_id}`, kind, target_id, scope_id: "s", phase, stage: "", message: null, retryable: false });

  it("says closing while a tree close or a pane close still names the pane", () => {
    expect(agentClosing([op("tree.close", "p2", "waiting")], "p2")).toBe(true);
    expect(agentClosing([op("pane.close", "p2", "awaiting_topology")], "p2")).toBe(true);
    expect(agentClosing([op("tree.close", "p2", "failed")], "p2")).toBe(false);
    expect(agentClosing([op("tree.close", "p3", "closing")], "p2")).toBe(false);
    expect(agentClosing(undefined, "p2")).toBe(false);
  });
});
