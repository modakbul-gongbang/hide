import { describe, expect, it } from "vitest";
import { closeDecision } from "./close";
import type { AgentRow, PaneRow } from "./snapshot";

function pane(id: string, extra: Partial<PaneRow> = {}): PaneRow {
  return {
    id,
    herdr_label: id,
    terminal_title: null,
    workspace_label: null,
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
