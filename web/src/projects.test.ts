import { describe, expect, it } from "vitest";
import { activityLabel, checkoutPresentation, projectRows, pullRequestBadge, relativeActivity } from "./projects";
import type { Checkout, GithubStatus, PullRequest, Workspace } from "./snapshot";

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

describe("checkoutPresentation", () => {
  const now = 1_000_000_000_000;
  const github = (extra: Partial<GithubStatus> = {}): GithubStatus =>
    ({ failure_category: null, available: true, loading: false, stale: false, last_success_at_unix_ms: now, unavailable_reason: null, ...extra }) as GithubStatus;
  const pr = (extra: Partial<PullRequest> = {}): PullRequest => ({ number: 155, title: "Browser display", url: "", badge: "open", review: null, is_draft: false, ...extra });
  const project = workspace("repo", { path: "/h/repo", is_git: true });
  const checkout = (extra: Partial<Checkout> = {}): Checkout =>
    ({
      id: "c",
      workspace_id: "w",
      label: "feature",
      path: "/h/repo.worktrees/feature",
      branch: "feature",
      purpose: null,
      is_worktree: true,
      exists: true,
      has_panes: false,
      worktree: { branch: "feature", head_sha: "abc1234", last_commit_unix_seconds: (now - 2 * 3_600_000) / 1000 },
      pull_request: null,
      github: github(),
      tabs: [],
      active_tab_id: null,
      strip: [],
      next_tab_label: "Tab 1",
      ...extra,
    }) as Checkout;

  it("draws a known pull request's lifecycle before the kind of checkout", () => {
    expect(checkoutPresentation(project, checkout({ pull_request: pr() }), now)).toMatchObject({ kind: "pr_open", kindTone: "text-pr-open" });
    expect(checkoutPresentation(project, checkout({ pull_request: pr({ is_draft: true }) }), now).kind).toBe("pr_draft");
    expect(checkoutPresentation(project, checkout({ pull_request: pr({ badge: "merged" }) }), now)).toMatchObject({ kind: "pr_merged", settled: true });
    expect(checkoutPresentation(project, checkout({ pull_request: pr({ badge: "review" }) }), now).kind).toBe("pr_open");
  });

  it("mutes a stale pull request and falls back to the branch when GitHub could not answer", () => {
    expect(checkoutPresentation(project, checkout({ pull_request: pr(), github: github({ stale: true }) }), now)).toMatchObject({ kind: "pr_open", kindTone: "text-muted-foreground" });
    expect(checkoutPresentation(project, checkout({ pull_request: pr(), github: github({ available: false, unavailable_reason: "gh is not signed in" }) }), now).kind).toBe("branch");
  });

  it("names the primary checkout, a detached one and a plain folder", () => {
    expect(checkoutPresentation(project, checkout({ is_worktree: false, path: "/h/repo" }), now).kind).toBe("primary");
    expect(checkoutPresentation(project, checkout({ worktree: { branch: null, head_sha: "abc1234" } as Checkout["worktree"] }), now).kind).toBe("detached");
    expect(checkoutPresentation(workspace("notes", { is_git: false }), checkout({ worktree: null, is_worktree: false }), now).kind).toBe("folder");
  });

  it("dates the last commit, and draws a missing folder in danger with no age", () => {
    expect(checkoutPresentation(project, checkout(), now).age).toBe("2h");
    expect(checkoutPresentation(project, checkout({ exists: false }), now)).toMatchObject({ kindTone: "text-destructive", age: null });
  });

  it("holds line two back until Git has been read", () => {
    expect(checkoutPresentation(project, checkout({ worktree: null }), now)).toMatchObject({ secondLineReady: false, age: null });
  });

  it("counts the agents once each and lists them by state in the tooltip", () => {
    const view = checkoutPresentation(project, checkout({ agent_summary: { representative_pane_id: "p1", needs_you: 1, done: 0, working: 2, seen: 1, unknown: 1 } }), now);
    expect(view.agentCount).toBe(4);
    expect(view.detail.split("\n")).toEqual(["Needs You: 1 · Working: 2 · Seen: 1 (1 Unknown)", "feature", "/h/repo.worktrees/feature"]);
  });
});
