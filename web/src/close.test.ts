import { describe, expect, it } from "vitest";
import { agentClosing, closeDecision, closeSheet, stopWorkOf, subtreeOf } from "./close";
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
    changed_at_unix_ms: null,
    emphasized: false,
    unread: false,
    ...extra,
  };
}

describe("closeDecision", () => {
  it("closes an idle pane without asking", () => {
    expect(closeDecision([pane("p1")], [])).toEqual({ action: "close" });
  });

  it("asks once for a pane with working or attention state", () => {
    expect(closeDecision([pane("p1", { requires_close_confirmation: true })], [])).toEqual({ action: "confirm" });
    expect(closeDecision([pane("p1")], [agent("p1", { requires_close_confirmation: true })])).toEqual({ action: "confirm" });
  });

  it("refuses to close while any pane's status is unknown", () => {
    const decision = closeDecision([pane("p1"), pane("p2", { requires_close_status_check: true, herdr_label: "worker" })], []);
    expect(decision).toEqual({ action: "status_unknown", label: "worker" });
  });
});

// The open Stop-work sheet (B28, D-39): what it lists and whether it can close
// follow the snapshot, never the moment it opened.
describe("stopWorkOf", () => {
  it("lists every pane that closes, a quiet one dimmed rather than dropped", () => {
    const busy = stopWorkOf([pane("p1"), pane("p2", { requires_close_confirmation: true })], []);
    expect(busy.rows.map((row) => [row.label, row.state])).toEqual([
      ["p1", "quiet"],
      ["p2", "active"],
    ]);
    expect(busy.unknown).toBeNull();
  });

  it("follows a pane that settles or starts while the sheet is open", () => {
    const working = [agent("p1", { requires_close_confirmation: true })];
    expect(stopWorkOf([pane("p1")], working).rows[0]).toMatchObject({ state: "active", agent: { status_label: "Working" } });
    const settled = [agent("p1", { status_label: "Idle", activity: "idle" })];
    expect(stopWorkOf([pane("p1")], settled).rows[0]).toMatchObject({ state: "quiet", agent: { status_label: "Idle" } });
  });

  it("blocks Stop work and close while a pane's status is unknown", () => {
    const sheet = stopWorkOf([pane("p1", { requires_close_confirmation: true }), pane("p2")], [agent("p2", { identity_label: "worker", requires_close_status_check: true })]);
    expect(sheet.unknown).toMatchObject({ label: "worker", state: "unknown" });
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

// D-40: the open sheet turns into the other in place and never closes itself
// while its target is there.
describe("closeSheet", () => {
  it("turns the subtree sheet into the target's Stop-work sheet when the last descendant leaves", () => {
    const target = [pane("p3")];
    const quiet = { lineage_depth: 0, status_label: "Idle", activity: "idle" };
    const alone = [agent("p3", quiet)];
    const withChild = [agent("p3", { ...quiet, lineage_child_pane_ids: ["p9"], close_descendant_pane_ids: ["p9"] }), agent("p9", { lineage_depth: 1, lineage_parent_pane_id: "p3" })];
    expect(closeSheet(target, withChild, withChild)).toMatchObject({ sheet: "subtree", subtree: { ids: ["p9"] } });
    const after = closeSheet(target, alone, alone);
    expect(after).toMatchObject({ sheet: "stop_work", stopWork: { unknown: null } });
    expect(after.sheet === "stop_work" && after.stopWork.rows.map((row) => [row.pane.id, row.state])).toEqual([["p3", "quiet"]]);
  });

  it("lists every agent of a closing tab as a target, so a working one beside the parent is shown", () => {
    const agents = [...family(), agent("p5", { lineage_depth: 0, activity: "working", requires_close_confirmation: true })];
    const sheet = closeSheet([pane("p1"), pane("p5")], agents, agents);
    expect(sheet.sheet === "subtree" && sheet.subtree.rows.filter((row) => row.target).map((row) => [row.agent.pane_id, row.state])).toEqual([
      ["p1", "quiet"],
      ["p5", "working"],
    ]);
    // The removal dialogs keep listing only the agents whose descendants run outside.
    expect(subtreeOf(["p1", "p5"], agents)?.rows.filter((row) => row.target).map((row) => row.agent.pane_id)).toEqual(["p1"]);
  });

  it("blocks both closes while a target's own status is unknown", () => {
    const agents = [...family(), agent("p5", { lineage_depth: 0, requires_close_status_check: true })];
    expect(closeSheet([pane("p1"), pane("p5")], agents, agents)).toMatchObject({ sheet: "subtree", subtree: { unknown: false, targetUnknown: true } });
    expect(closeSheet([pane("p1")], family(), family())).toMatchObject({ sheet: "subtree", subtree: { targetUnknown: false } });
  });

  it("turns the Stop-work sheet into the subtree sheet when a descendant appears", () => {
    const target = [pane("p1", { requires_close_confirmation: true })];
    expect(closeSheet(target, [agent("p1")], [agent("p1")]).sheet).toBe("stop_work");
    expect(closeSheet(target, family(), family())).toMatchObject({ sheet: "subtree", subtree: { ids: ["p3", "p2", "p4"] } });
  });
});
