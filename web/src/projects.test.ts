import { describe, expect, it } from "vitest";
import { activityLabel, projectRows, pullRequestBadge, relativeActivity } from "./projects";
import type { Workspace } from "./snapshot";

function workspace(id: string, extra: Partial<Workspace> = {}): Workspace {
  return {
    id,
    label: id,
    path: `/h/${id}`,
    device_id: "local",
    registered: true,
    temporary: false,
    pinned: false,
    checkouts: [],
    inactive_checkouts: { expanded: false, checkout_ids: [] },
    ...extra,
  };
}

describe("projectRows", () => {
  it("draws pinned rows under their header, then the activity list with the device fold", () => {
    const rows = projectRows(
      [workspace("a", { pinned: true }), workspace("b"), workspace("c"), workspace("d")],
      [{ device_id: "local", expanded: false, project_ids: ["c", "d"] }],
    );
    expect(rows.map((row) => (row.kind === "workspace" ? `${row.kind}:${row.workspace.id}:${row.level}` : row.kind))).toEqual([
      "header",
      "workspace:a:root",
      "header",
      "workspace:b:root",
      "inactive_projects",
    ]);
    const expanded = projectRows(
      [workspace("b"), workspace("c")],
      [{ device_id: "local", expanded: true, project_ids: ["c"] }],
    );
    expect(expanded.at(-1)).toMatchObject({ kind: "workspace", level: "child" });
  });

  it("omits the pinned header when nothing is pinned", () => {
    const rows = projectRows([workspace("b")], []);
    expect(rows[0]).toEqual({ kind: "header", title: "Projects · Recent activity", count: 1 });
  });
});

describe("activity", () => {
  it("rounds recency like the Swift row", () => {
    const now = 1_000_000_000;
    expect(relativeActivity(null, now)).toBeNull();
    expect(relativeActivity(now + 5000, now)).toBe("now");
    expect(relativeActivity(now - 30_000, now)).toBe("now");
    expect(relativeActivity(now - 5 * 60_000, now)).toBe("5m");
    expect(relativeActivity(now - 3 * 3_600_000, now)).toBe("3h");
    expect(relativeActivity(now - 49 * 3_600_000, now)).toBe("2d");
  });

  it("counts agents before workspaces", () => {
    const w = workspace("a", {
      last_activity_unix_ms: 0,
      checkouts: [
        {
          id: "c1",
          workspace_id: "a",
          label: "a",
          path: "/h/a",
          branch: "main",
          purpose: null,
          is_worktree: false,
          exists: true,
          has_panes: true,
          pull_request: null,
          tabs: [{ id: "t1", workspace_id: "a", checkout_id: "c1", label: "1", empty: false, delegated: false, panes: [{ id: "p1", herdr_label: null, terminal_title: null, workspace_label: null, cwd: "/h/a", status_label: "", requires_close_confirmation: false, requires_close_status_check: false, identity_label: null }] }],
          active_tab_id: "t1",
          strip: [],
          next_tab_label: "2",
        },
      ],
    });
    expect(activityLabel(w, [], 120_000)).toBe("1 workspace · 2m");
    expect(activityLabel(w, [{ id: "x", pane_id: "p1", identity_label: "", agent_kind: "", symbol: "", group: "working", status_label: "", elapsed: "", emphasized: false, unread: false }], 120_000)).toBe("1 agent · 2m");
  });
});

describe("pullRequestBadge", () => {
  it("names the decision for a review badge and the lifecycle otherwise", () => {
    const base = { number: 1, title: "", url: "", is_draft: false };
    expect(pullRequestBadge({ ...base, badge: "merged", review: null })).toEqual({ label: "merged", color: "text-pr-merged" });
    expect(pullRequestBadge({ ...base, badge: "open", review: null, is_draft: true })).toEqual({ label: "draft", color: "text-pr-draft" });
    expect(pullRequestBadge({ ...base, badge: "review", review: "approved" })).toEqual({ label: "approved", color: "text-success" });
    expect(pullRequestBadge({ ...base, badge: "review", review: "changes_requested" })).toEqual({ label: "changes", color: "text-destructive" });
    expect(pullRequestBadge({ ...base, badge: "review", review: null })).toEqual({ label: "review", color: "text-warning" });
  });
});
