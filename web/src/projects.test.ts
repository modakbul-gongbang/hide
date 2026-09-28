import { describe, expect, it } from "vitest";
import { cardSingleValue, checkoutCard, checkoutHasSecondLine, checkoutNameParts, checkoutRowExpansion, overviewRowSelected, projectRowExpansion, checkoutPresentation, projectMarks, projectRows, pullRequestBadge, relativeActivity, shownPullRequest } from "./projects";
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

const NO_MARKS = { error: 0, approval: 0, question: 0, working: 0, done: 0, idle: 0 };

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
  it("rounds recency to the coarsest unit that fits", () => {
    const now = 1_000_000_000;
    expect(relativeActivity(null, now)).toBeNull();
    expect(relativeActivity(now + 5000, now)).toBe("now");
    expect(relativeActivity(now - 30_000, now)).toBe("now");
    expect(relativeActivity(now - 5 * 60_000, now)).toBe("5m");
    expect(relativeActivity(now - 3 * 3_600_000, now)).toBe("3h");
    expect(relativeActivity(now - 49 * 3_600_000, now)).toBe("2d");
  });
});

describe("pullRequestBadge", () => {
  it("names the lifecycle, or the review decision for a pull request under review (D-08)", () => {
    const base = { number: 1, title: "", url: "", is_draft: false };
    expect(pullRequestBadge({ ...base, badge: "merged", review: null })).toEqual({ label: "Merged", color: "text-pr-merged", draft: false });
    expect(pullRequestBadge({ ...base, badge: "closed", review: null })).toEqual({ label: "Closed", color: "text-pr-closed", draft: false });
    expect(pullRequestBadge({ ...base, badge: "open", review: null })).toEqual({ label: "Open", color: "text-pr-open", draft: false });
    expect(pullRequestBadge({ ...base, badge: "open", review: null, is_draft: true })).toEqual({ label: "Draft", color: "text-pr-draft", draft: false });
    expect(pullRequestBadge({ ...base, badge: "review", review: "approved" })).toEqual({ label: "Approved", color: "text-success", draft: false });
    expect(pullRequestBadge({ ...base, badge: "review", review: "changes_requested" })).toEqual({ label: "Changes requested", color: "text-destructive", draft: false });
    expect(pullRequestBadge({ ...base, badge: "review", review: "review_required" })).toEqual({ label: "Review required", color: "text-muted-foreground", draft: false });
  });

  it("keeps the decision on a draft under review and says Draft beside it", () => {
    const base = { number: 1, title: "", url: "", is_draft: true };
    expect(pullRequestBadge({ ...base, badge: "review", review: "approved" })).toEqual({ label: "Approved", color: "text-success", draft: true });
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

  it("draws one shape for a pull request on the row and in its card, a draft under review included", () => {
    for (const request of [pr(), pr({ is_draft: true }), pr({ badge: "review", is_draft: true }), pr({ badge: "merged" }), pr({ badge: "closed", is_draft: true })]) {
      const row = checkout({ pull_request: request });
      const header = checkoutCard(project, row, now).header;
      expect(header?.kind === "pull_request" ? header.glyph : null).toBe(checkoutPresentation(project, row, now).kind);
    }
    expect(checkoutCard(project, checkout({ pull_request: pr({ badge: "review", is_draft: true }) }), now).header).toMatchObject({ glyph: "pr_draft" });
  });

  it("mutes a stale pull request and falls back to the branch when GitHub could not answer", () => {
    expect(checkoutPresentation(project, checkout({ pull_request: pr(), github: github({ stale: true }) }), now)).toMatchObject({ kind: "pr_open", kindTone: "text-muted-foreground" });
    expect(checkoutPresentation(project, checkout({ pull_request: pr(), github: github({ available: false, unavailable_reason: "gh is not signed in" }) }), now).kind).toBe("branch");
  });

  it("names the primary checkout, a detached one and a plain folder", () => {
    expect(checkoutPresentation(project, checkout({ is_primary: true, is_worktree: false, path: "/h/repo" }), now).kind).toBe("primary");
    expect(checkoutPresentation(project, checkout({ worktree: { branch: null, head_sha: "abc1234" } as Checkout["worktree"] }), now).kind).toBe("detached");
    expect(checkoutPresentation(workspace("notes", { is_git: false }), checkout({ worktree: null, is_worktree: false }), now).kind).toBe("folder");
  });

  it("uses the snapshot home choice even on a linked worktree with a pull request", () => {
    expect(checkoutPresentation(project, checkout({ is_primary: true, pull_request: pr() }), now).kind).toBe("primary");
    expect(checkoutPresentation(project, checkout({ is_primary: false, is_worktree: false, path: project.path }), now).kind).toBe("branch");
    expect(checkoutPresentation({ ...project, device_id: "remote" }, checkout({ is_primary: true }), now).kind).toBe("primary");
  });

  it("dates the last commit, and draws a missing folder in danger with no age", () => {
    expect(checkoutPresentation(project, checkout(), now).age).toBe("2h");
    expect(checkoutPresentation(project, checkout({ exists: false }), now)).toMatchObject({ kindTone: "text-destructive", age: null });
  });

  it("holds line two back until Git has been read", () => {
    expect(checkoutPresentation(project, checkout({ worktree: null }), now)).toMatchObject({ secondLineReady: false, age: null });
  });

  it("lists the agents by state in the tooltip", () => {
    const view = checkoutPresentation(project, checkout({ agent_summary: { representative_pane_id: "p1", needs_you: 1, done: 0, working: 2, seen: 1, unknown: 1, marks: NO_MARKS } }), now);
    expect(view.detail.split("\n")).toEqual(["Needs You: 1 · Working: 2 · Seen: 1 (1 Unknown)", "feature", "/h/repo.worktrees/feature"]);
  });
});

describe("checkoutCard", () => {
  const now = 1_000_000_000_000;
  const project = workspace("repo", { path: "/h/repo", is_git: true });
  const pr = (extra: Partial<PullRequest> = {}): PullRequest => ({ number: 180, title: "Sidebar readability", url: "https://example.invalid/pull/180", badge: "review", review: "approved", is_draft: false, checks: "passing", ...extra });
  const summary = (marks: Partial<typeof NO_MARKS>, counts: Partial<{ needs_you: number; done: number; working: number; seen: number }> = {}) => ({
    representative_pane_id: null,
    needs_you: 0,
    done: 0,
    working: 0,
    seen: 0,
    unknown: 0,
    ...counts,
    marks: { ...NO_MARKS, ...marks },
  });
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
      worktree: { branch: "feature", head_sha: "abc1234def", last_commit_unix_seconds: (now - 2 * 3_600_000) / 1000 },
      pull_request: null,
      tabs: [],
      active_tab_id: null,
      strip: [],
      next_tab_label: "Tab 1",
      ...extra,
    }) as Checkout;
  const keys = (card: ReturnType<typeof checkoutCard>) => card.rows.map((row) => row.key);

  it("heads a pull request's card with its badge, number, url and title, then every row with a value (B5)", () => {
    const card = checkoutCard(project, checkout({ pull_request: pr(), agent_summary: summary({ question: 1, working: 2 }, { needs_you: 1, working: 2 }) }), now);
    expect(card.header).toEqual({
      kind: "pull_request",
      badge: { label: "Approved", color: "text-success", draft: false },
      glyph: "pr_open",
      number: 180,
      url: "https://example.invalid/pull/180",
      title: "Sidebar readability",
    });
    expect(card.rows).toEqual([
      { key: "review", label: "Review", value: "Approved", tone: "text-success" },
      { key: "checks", label: "Checks", value: "Passing", tone: "text-success" },
      { key: "branch", label: "Branch", value: "feature" },
      { key: "agents", label: "Agents", marks: { ...NO_MARKS, question: 1, working: 2 } },
      { key: "commit", label: "Commit", value: "2h ago" },
      { key: "path", label: "Path", value: "/h/repo.worktrees/feature" },
    ]);
  });

  it("colors Checks by result and leaves the row out when there are none or they are unknown (B6)", () => {
    expect(checkoutCard(project, checkout({ pull_request: pr({ checks: "failed" }) }), now).rows[1]).toEqual({ key: "checks", label: "Checks", value: "Failed", tone: "text-destructive" });
    expect(checkoutCard(project, checkout({ pull_request: pr({ checks: "pending" }) }), now).rows[1]).toMatchObject({ value: "Pending", tone: "text-muted-foreground" });
    expect(keys(checkoutCard(project, checkout({ pull_request: pr({ checks: "none" }) }), now))).toEqual(["review", "branch", "commit", "path"]);
    expect(keys(checkoutCard(project, checkout({ pull_request: pr({ checks: "unknown" }) }), now))).toEqual(["review", "branch", "commit", "path"]);
    expect(keys(checkoutCard(project, checkout({ pull_request: pr({ checks: undefined }) }), now))).toEqual(["review", "branch", "commit", "path"]);
  });

  it("leaves Review out of an open pull request with no decision, and names the others (B6)", () => {
    expect(keys(checkoutCard(project, checkout({ pull_request: pr({ badge: "open", review: null }) }), now))).toEqual(["checks", "branch", "commit", "path"]);
    expect(checkoutCard(project, checkout({ pull_request: pr({ review: "changes_requested" }) }), now).rows[0]).toMatchObject({ value: "Changes requested", tone: "text-destructive" });
    expect(checkoutCard(project, checkout({ pull_request: pr({ review: "review_required" }) }), now).rows[0]).toMatchObject({ value: "Review required", tone: "text-muted-foreground" });
  });

  it("has no header without a pull request and draws only the rows with a value (B8, D-09)", () => {
    const plain = checkoutCard(project, checkout(), now);
    expect(plain.header).toBeNull();
    expect(keys(plain)).toEqual(["branch", "commit", "path"]);
    // Git not read yet: neither branch nor commit, no loading row.
    expect(keys(checkoutCard(project, checkout({ worktree: null }), now))).toEqual(["path"]);
    // No agents: no Agents row; agents with a zero sum: none either.
    expect(keys(checkoutCard(project, checkout({ agent_summary: summary({}) }), now))).toEqual(["branch", "commit", "path"]);
    // A commit whose age is not known has no row.
    expect(keys(checkoutCard(project, checkout({ worktree: { branch: "feature", head_sha: "abc1234def" } as Checkout["worktree"] }), now))).toEqual(["branch", "path"]);
  });

  it("names a detached HEAD by its short sha, dates the first minute as now, and a folder has only its path and agents", () => {
    expect(checkoutCard(project, checkout({ worktree: { branch: null, head_sha: "abc1234def", last_commit_unix_seconds: now / 1000 } as Checkout["worktree"] }), now).rows).toEqual([
      { key: "branch", label: "Branch", value: "Detached HEAD at abc1234" },
      { key: "commit", label: "Commit", value: "now" },
      { key: "path", label: "Path", value: "/h/repo.worktrees/feature" },
    ]);
    const folder = workspace("notes", { is_git: false });
    expect(keys(checkoutCard(folder, checkout({ worktree: null, is_worktree: false, path: "/h/notes", agent_summary: summary({ idle: 1 }, { seen: 1 }) }), now))).toEqual(["agents", "path"]);
  });

  it("heads a missing folder with Folder missing over its path alone, whatever else is known", () => {
    const card = checkoutCard(project, checkout({ exists: false, pull_request: pr(), agent_summary: summary({ idle: 1 }, { seen: 1 }) }), now);
    expect(card.header).toEqual({ kind: "missing", label: "Folder missing" });
    expect(card.rows).toEqual([{ key: "path", label: "Path", value: "/h/repo.worktrees/feature" }]);
  });

  it("falls back to a text tooltip only when the card would hold one value and no header (D-09)", () => {
    const folder = workspace("notes", { is_git: false });
    expect(cardSingleValue(checkoutCard(folder, checkout({ worktree: null, is_worktree: false, path: "/h/notes" }), now))).toBe("/h/notes");
    expect(cardSingleValue(checkoutCard(project, checkout(), now))).toBeNull();
    expect(cardSingleValue(checkoutCard(project, checkout({ exists: false }), now))).toBeNull();
  });

  it("shows the pull request a stale refresh kept and none an unavailable lookup lost", () => {
    const github = (extra: Partial<GithubStatus> = {}): GithubStatus =>
      ({ failure_category: null, available: true, loading: false, stale: false, last_success_at_unix_ms: now, unavailable_reason: null, ...extra }) as GithubStatus;
    expect(shownPullRequest(checkout({ pull_request: pr(), github: github({ stale: true, available: false }) }))?.number).toBe(180);
    expect(shownPullRequest(checkout({ pull_request: pr(), github: github({ available: false, unavailable_reason: "gh is not signed in" }) }))).toBeNull();
    expect(shownPullRequest(checkout({ pull_request: null }))).toBeNull();
    expect(checkoutCard(project, checkout({ pull_request: pr(), github: github({ available: false, unavailable_reason: "gh is not signed in" }) }), now).header).toBeNull();
  });
});

describe("projectMarks", () => {
  it("adds every checkout's marks, and a checkout the core has not summarised yet adds none", () => {
    const summary = (marks: Partial<typeof NO_MARKS>) => ({ representative_pane_id: null, needs_you: 0, done: 0, working: 0, seen: 0, unknown: 0, marks: { ...NO_MARKS, ...marks } });
    const project = workspace("repo", {
      checkouts: [
        { agent_summary: summary({ question: 1, idle: 2 }) },
        { agent_summary: summary({ question: 1, working: 3 }) },
        {},
      ] as Checkout[],
    });
    expect(projectMarks(project)).toEqual({ ...NO_MARKS, question: 2, working: 3, idle: 2 });
  });
});

describe("sidebar row activation", () => {
  it("opens folded rows and preserves an expanded row when returning from another scope", () => {
    expect(checkoutRowExpansion(true, false, false)).toBe(true);
    expect(checkoutRowExpansion(true, true, false)).toBe(true);
    expect(checkoutRowExpansion(true, false, true)).toBe(true);
    expect(checkoutRowExpansion(true, true, true)).toBe(false);
    expect(checkoutRowExpansion(false, true, true)).toBeUndefined();
  });

  it("unfolds a project row unless its own Overview is open and it is unfolded", () => {
    expect(projectRowExpansion(workspace("repo", { expanded: false }), null, true)).toBe(true);
    expect(projectRowExpansion(workspace("repo", { expanded: false }), "repo", true)).toBe(true);
    expect(projectRowExpansion(workspace("repo", { expanded: true }), "other", true)).toBe(true);
    expect(projectRowExpansion(workspace("repo", { expanded: true }), "repo", true)).toBe(false);
    expect(projectRowExpansion(workspace("repo", {}), "repo", true)).toBe(false);
    expect(projectRowExpansion(workspace("repo", { expanded: true }), "repo", false)).toBeUndefined();
  });

  it("selects the Git project's Overview child only for its own Overview", () => {
    const project = workspace("repo", { is_git: true });
    expect(overviewRowSelected(project, "repo")).toBe(true);
    expect(overviewRowSelected(project, "other")).toBe(false);
    expect(overviewRowSelected(project, null)).toBe(false);
    expect(overviewRowSelected(workspace("notes", { is_git: false }), "notes")).toBe(false);
  });
});

describe("checkout row lines (PRD sidebar-typography D-04, D-06)", () => {
  const project = workspace("p", { is_git: true });
  const now = Date.UTC(2026, 8, 27);
  const read = (extra: Partial<Checkout> = {}) =>
    checkoutPresentation(
      project,
      {
        id: "c",
        workspace_id: "w",
        label: "feature",
        path: "/h/p/feature",
        branch: "feature",
        purpose: null,
        is_worktree: true,
        exists: true,
        has_panes: false,
        worktree: { branch: "feature", head_sha: "abc1234", last_commit_unix_seconds: now / 1000 - 3600 },
        pull_request: null,
        tabs: [],
        active_tab_id: null,
        strip: [],
        next_tab_label: "Tab 1",
        ...extra,
      } as Checkout,
      now,
    );

  it("draws line two only for a purpose or a raised-from parent, agents or not", () => {
    const withAgents = read({ agent_summary: { representative_pane_id: null, needs_you: 0, done: 0, working: 2, seen: 0, unknown: 0, marks: { ...NO_MARKS, working: 2 } } });
    expect(checkoutHasSecondLine(withAgents, null, null)).toBe(false);
    expect(checkoutHasSecondLine(withAgents, "사이드바 가독성 개선", null)).toBe(true);
    expect(checkoutHasSecondLine(withAgents, null, "메인 체크아웃 정리에서")).toBe(true);
    expect(checkoutHasSecondLine(read(), "purpose", null)).toBe(true);
    expect(checkoutHasSecondLine(read({ worktree: null }), "purpose", null)).toBe(false);
  });

  it("mutes a name's path prefix up to its first slash", () => {
    expect(checkoutNameParts("prd/sidebar-typography")).toEqual({ prefix: "prd/", rest: "sidebar-typography" });
    expect(checkoutNameParts("gen-prd/a/b")).toEqual({ prefix: "gen-prd/", rest: "a/b" });
    expect(checkoutNameParts("main")).toEqual({ prefix: "", rest: "main" });
    expect(checkoutNameParts("/abs")).toEqual({ prefix: "", rest: "/abs" });
    expect(checkoutNameParts("trailing/")).toEqual({ prefix: "", rest: "trailing/" });
  });
});
