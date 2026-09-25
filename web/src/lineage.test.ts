import { describe, expect, it } from "vitest";
import { relationEntries, relationState, type Relation } from "./lineage";
import type { AgentChip, PaneRow } from "./snapshot";

const relation: Relation = { requestId: "r1", sourcePaneId: "parent", targetPaneId: "child", label: "child task" };

function chip(pane_id: string): AgentChip {
  return { pane_id, label: pane_id, detail: null, status_word_visible: false, agent_kind: "codex", demand: "none", activity: "working", emphasized: false, symbol: "●", status_label: "Working", delegated: true };
}

describe("a relationship focus", () => {
  it("is in flight until the core's receipt names it, and needs no mark once it landed", () => {
    expect(relationState(relation, null)).toEqual({ phase: "pending" });
    expect(relationState(relation, { request_id: "older", target_pane_id: "child", phase: "failed", message: "gone", retryable: true })).toEqual({ phase: "pending" });
    expect(relationState(relation, { request_id: "r1", target_pane_id: "child", phase: "pending", message: null, retryable: false })).toEqual({ phase: "pending" });
    expect(relationState(relation, { request_id: "r1", target_pane_id: "child", phase: "succeeded", message: null, retryable: false })).toBeNull();
  });

  it("carries the core's reason and whether it can be retried when it failed", () => {
    expect(relationState(relation, { request_id: "r1", target_pane_id: "child", phase: "failed", message: "Pane child is no longer available.", retryable: true })).toEqual({
      phase: "failed",
      message: "Pane child is no longer available.",
      retryable: true,
    });
  });

  it("reads as a retryable failure when no answer arrived in time, unless the core answered", () => {
    const late = { ...relation, timedOut: true };
    expect(relationState(late, null)).toEqual({ phase: "failed", message: "child task did not open: Hide did not answer in time.", retryable: true });
    expect(relationState(late, { request_id: "r1", target_pane_id: "child", phase: "pending", message: null, retryable: false })?.phase).toBe("failed");
    expect(relationState(late, { request_id: "r1", target_pane_id: "child", phase: "succeeded", message: null, retryable: false })).toBeNull();
  });
});

describe("the relationship menu", () => {
  it("lists the parent, the other siblings, then the pane's own children", () => {
    const pane = {
      id: "me",
      children: { instrumented: true, uninstrumented_reason: null, uninstrumented_label: null, chips: [chip("kid")] },
      lineage_path: [
        { pane_id: "root", label: "root", siblings: [] },
        { pane_id: "me", label: "me", siblings: [chip("me"), chip("sister")] },
      ],
    } as unknown as PaneRow;
    expect(relationEntries(pane).map((entry) => [entry.relation, entry.paneId])).toEqual([
      ["parent", "root"],
      ["sibling", "sister"],
      ["child", "kid"],
    ]);
  });
});
