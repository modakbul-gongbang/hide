import { legacyAgentRow } from "../test/legacyAgentRow";
import { describe, expect, it } from "vitest";
import { foldedLineage } from "./lineageSummary";
import type { AgentRow, Workspace } from "./snapshot";

function agent(pane: string, patch: Partial<AgentRow> = {}): AgentRow {
  return legacyAgentRow({
    id: pane,
    pane_id: pane,
    identity_label: pane,
    agent_kind: "codex",
    symbol: "●",
    group: "working",
    status_code: "working",
    detail: null,
    changed_at_unix_ms: null,
    emphasized: false,
    unread: false,
    demand: "none",
    activity: "working",
    device_id: "local",
    device_label: "This Mac",
    ...patch,
  });
}

function workspace(device: string, rows: { id: string; branch: string; panes: string[]; pr?: number }[]): Workspace {
  return {
    id: `${device}-project`,
    label: "hide",
    path: `/${device}/hide`,
    device_id: device,
    registered: true,
    temporary: false,
    pinned: false,
    inactive_checkouts: { expanded: false, checkout_ids: [] },
    checkouts: rows.map((row) => ({
      id: row.id,
      workspace_id: `${device}-workspace`,
      label: row.branch,
      branch: row.branch,
      path: `/${device}/${row.branch}`,
      purpose: null,
      is_worktree: row.branch !== "main",
      exists: true,
      has_panes: true,
      pull_request: row.pr ? { number: row.pr, title: "PR", url: "https://example.test", badge: "open", review: null, is_draft: false } : null,
      tabs: [{ id: "tab", workspace_id: null, checkout_id: row.id, label: null, empty: false, delegated: false, panes: row.panes.map((id) => ({ id }) as never) }],
      active_tab_id: null,
      strip: [],
      next_tab_label: "2",
    })),
  } as Workspace;
}

describe("folded checkout lineage", () => {
  it("keeps same-checkout descendants in the badge and summarizes other checkouts in priority order", () => {
    const parent = agent("parent", { checkout_label: "main", lineage_child_pane_ids: ["same", "review", "remote", "fourth", "fifth"] });
    const same = agent("same", { checkout_label: "main", delegated: true, demand: "question", activity: "stopped", symbol: "?", group: "working" });
    const review = agent("review", { checkout_label: "review-branch-with-a-very-long-name", delegated: true, group: "done", activity: "stopped", symbol: "✓" });
    const remote = agent("remote", { checkout_label: "remote-work", delegated: true, device_id: "mini", device_label: "mini" });
    const fourth = agent("fourth", { checkout_label: "zeta", delegated: true, group: "seen", activity: "stopped", symbol: "○" });
    const fifth = agent("fifth", { checkout_label: "alpha", delegated: true, group: "needs_you", demand: "error", activity: "stopped", symbol: "×" });
    const workspaces = [
      workspace("local", [
        { id: "main", branch: "main", panes: ["parent", "same"] },
        { id: "review", branch: "review-branch-with-a-very-long-name", panes: ["review"], pr: 173 },
        { id: "zeta", branch: "zeta", panes: ["fourth"] },
        { id: "alpha", branch: "alpha", panes: ["fifth"] },
      ]),
      workspace("mini", [{ id: "remote-work", branch: "remote-work", panes: ["remote"] }]),
    ];

    const presentation = foldedLineage(parent, [parent, same, review, remote, fourth, fifth], workspaces);
    expect(presentation.badgeDescendants).toBe(1);
    expect(presentation.badgeCounts.question).toBe(1);
    expect(presentation.badgeChildren.map((row) => row.pane_id)).toEqual(["same"]);
    expect(presentation.lines.map((line) => line.branch)).toEqual(["alpha", "review-branch-with-a-very-long-name", "remote-work"]);
    expect(presentation.lines[1]?.pullRequest).toBe(173);
    expect(presentation.lines[2]?.device).toBe("mini");
    expect(presentation.overflow).toBe(1);
  });
});
