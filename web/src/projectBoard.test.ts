// The Swift Project Home cases (`ProjectHomeTests.swift`) against the web
// board, so both shells put the same checkout in the same column (D-03).

import { describe, expect, it } from "vitest";
import { agentColumnCards, buildBoard, showsDetail, stageCards, stageMatches, stageOf, waitingOnDescendants } from "./projectBoard";
import type { AgentRow, Checkout, Issue, PullRequest, Workspace } from "./snapshot";

const NOW = 1_800_000_000_000;

function pr(badge: PullRequest["badge"], checks: PullRequest["checks"] = "unknown"): PullRequest {
  return { number: 7, title: "PR", url: "https://github.com/acme/project/pull/7", badge, review: null, is_draft: false, checks };
}

function checkout(id: string, options: { changed?: number; ahead?: number; pr?: PullRequest; worktree?: boolean; panes?: string[]; merged?: boolean } = {}): Checkout {
  const worktree = options.worktree ?? true;
  return {
    id,
    workspace_id: "project",
    label: id,
    path: `/fixture/${id}`,
    branch: id,
    purpose: null,
    is_worktree: worktree,
    exists: true,
    has_panes: (options.panes ?? []).length > 0,
    worktree: { merged: options.merged ?? null, is_main: !worktree } as Checkout["worktree"],
    pull_request: options.pr ?? null,
    issue: null,
    changed_file_count: options.changed ?? 0,
    ahead: options.ahead ?? 0,
    tabs: [{ id: `${id}:tab`, workspace_id: "project", checkout_id: id, label: "Tab 1", empty: false, delegated: false, panes: (options.panes ?? []).map((pane) => ({ id: pane })) as never }],
    active_tab_id: `${id}:tab`,
    strip: [],
    next_tab_label: "Tab 2",
  };
}

function workspace(checkouts: Checkout[], git = true): Workspace {
  return {
    id: "project",
    label: "Project",
    path: "/fixture",
    device_id: "local",
    is_git: git,
    registered: true,
    temporary: false,
    pinned: false,
    checkouts,
    inactive_checkouts: { expanded: false, checkout_ids: [] },
    home_issues: { repository: "acme/project", issues: [], overflow: false },
  };
}

function agent(pane: string, group = "working", extra: Partial<AgentRow> = {}): AgentRow {
  return { id: pane, pane_id: pane, identity_label: pane, agent_kind: "claude", symbol: "●", group, status_label: group, elapsed: "1m", emphasized: false, unread: false, demand: "none", activity: "working", ...extra };
}

function issue(number: number, state: "OPEN" | "CLOSED", title = `Issue ${number}`, projectStatus: string | null = null): Issue {
  return { reference: { repository: "acme/project", number }, title, url: `https://github.com/acme/project/issues/${number}`, state, project_status: projectStatus, updated_at_unix_ms: null };
}

describe("the Git stage", () => {
  it("is merged, then review, then working, else ready; agents and Project status never move it", () => {
    expect(stageOf(checkout("a"))).toBe("ready");
    expect(stageOf(checkout("a", { changed: 1 }))).toBe("working");
    expect(stageOf(checkout("a", { ahead: 1 }))).toBe("working");
    expect(stageOf(checkout("a", { changed: 3, pr: pr("open") }))).toBe("review");
    expect(stageOf(checkout("a", { changed: 3, pr: pr("review") }))).toBe("review");
    expect(stageOf(checkout("a", { changed: 3, pr: pr("merged") }))).toBe("merged");
    expect(stageOf(checkout("a", { changed: 3, pr: pr("closed") }))).toBe("working");
    expect(stageOf(checkout("a", { merged: true }))).toBe("merged");
    expect(stageMatches("review", "In Review")).toBe(true);
    expect(stageMatches("review", "Done")).toBe(false);
  });
});

describe("the Tasks board", () => {
  it("dedupes linked open issues, never shows a closed one as backlog, and keeps main off the columns", () => {
    const open = issue(7, "OPEN", "한국어 작업");
    const linked = { ...checkout("task"), issue: { issue: open, source: "pane 토큰" } };
    const project = { ...workspace([linked, checkout("main", { worktree: false })]), home_issues: { repository: "acme/project", issues: [open, issue(8, "CLOSED")], overflow: true } };
    const board = buildBoard(project, [], NOW);
    expect(board.tasks.map((row) => row.id)).toEqual(["task:task"]);
    expect(board.tasks[0]?.title).toBe("한국어 작업");
    expect(board.adHoc).toEqual([]);
    expect(board.overflow).toBe(true);
    const unlinked = { ...workspace([]), home_issues: project.home_issues };
    const backlog = buildBoard(unlinked, [], NOW).tasks;
    expect(backlog.map((row) => [row.id, row.stage, row.backlog, row.delivery])).toEqual([["issue:acme/project#7", "ready", true, null]]);
  });

  it("puts a checkout with agents that is not a worktree on the ad hoc strip, and one with none nowhere", () => {
    const board = buildBoard(workspace([checkout("main", { worktree: false, panes: ["m"] }), checkout("scratch", { worktree: false })]), [agent("m")], NOW);
    expect(board.adHoc.map((row) => row.id)).toEqual(["task:main"]);
    expect(board.tasks).toEqual([]);
  });

  it("keeps a cross-checkout descendant under its root, names its branch, and warns without moving the stage", () => {
    const parent = agent("p", "needs_you", { demand: "question", lineage_child_pane_ids: ["child"] });
    const child = agent("child", "working", { lineage_parent_pane_id: "p", delegated: true });
    const board = buildBoard(workspace([checkout("parent-branch", { changed: 1, panes: ["p"] }), checkout("child-branch", { changed: 1, panes: ["child"] })]), [child, parent], NOW);
    expect(board.agents).toHaveLength(1);
    expect(board.agents[0]?.rows.map((row) => [row.agent.pane_id, row.depth, row.foreignBranch])).toEqual([
      ["p", 0, null],
      ["child", 1, "child-branch"],
    ]);
    const parentCard = board.tasks.find((row) => row.id === "task:parent-branch");
    expect(parentCard?.needsYou).toBe(true);
    expect(parentCard?.stage).toBe("working");
    // The child's own checkout still carries it as that card's root.
    expect(board.tasks.find((row) => row.id === "task:child-branch")?.rows.map((row) => row.agent.pane_id)).toEqual(["child"]);
  });

  it("raises a needs-you card to the top of its own column and keeps the rest in order", () => {
    const board = buildBoard(
      workspace([checkout("a", { changed: 1, panes: ["a"] }), checkout("b", { changed: 1, panes: ["b"] }), checkout("c", { changed: 1, panes: ["c"] }), checkout("d", { panes: ["d"] })]),
      [agent("a"), agent("b"), agent("c", "needs_you", { demand: "error" }), agent("d", "needs_you")],
      NOW,
    );
    expect(stageCards(board, "working").map((row) => row.id)).toEqual(["task:c", "task:a", "task:b"]);
    expect(stageCards(board, "working")[0]?.error).toBe(true);
    expect(stageCards(board, "ready").map((row) => row.id)).toEqual(["task:d"]);
  });

  it("states one delivery fact per stage", () => {
    const board = buildBoard(
      workspace([
        checkout("ready", { panes: ["r"] }),
        checkout("work", { changed: 2, ahead: 4, panes: ["w"] }),
        checkout("rev", { pr: pr("open", "failed"), panes: ["v"] }),
        checkout("done", { pr: pr("merged", "passing"), panes: ["d"] }),
      ]),
      [agent("r"), agent("w"), agent("v"), agent("d")],
      NOW,
    );
    expect(board.tasks.map((row) => row.delivery)).toEqual([
      { label: "변경 없음", checks: null },
      { label: "변경 2 · ↑4 커밋", checks: null },
      { label: "PR #7", checks: "failed" },
      { label: "머지됨", checks: null },
    ]);
  });

  it("flags an issue whose Project status names another stage, and dates stale GitHub only in the issue tooltip", () => {
    const stale = { failure_category: null, available: true, loading: false, stale: true, last_success_at_unix_ms: NOW - 5 * 60_000, unavailable_reason: "gh failed" };
    const linked = { ...checkout("task", { changed: 1, panes: ["t"] }), issue: { issue: issue(9, "OPEN", "Fix", "Done"), source: "branch" }, github: stale };
    const [value] = buildBoard(workspace([linked]), [agent("t")], NOW).tasks;
    expect(value?.mismatch).toBe("Done");
    expect(value?.mismatchHelp).toBe("Project: Done · git: 변경 1");
    expect(value?.issueHelp).toBe("acme/project#9 · 열림 · Fix · Project: Done · branch · GitHub: 마지막 성공 5분 전");
    const fresh = buildBoard(workspace([{ ...linked, github: { ...stale, stale: false } }]), [agent("t")], NOW).tasks[0];
    expect(fresh?.issueHelp).not.toContain("GitHub");
  });
});

describe("the Agents board", () => {
  it("makes one card per lineage root and places it by the root's group", () => {
    const board = buildBoard(
      workspace([checkout("a", { panes: ["a", "a2"] }), checkout("b", { panes: ["b"] })]),
      [agent("a", "done", { lineage_child_pane_ids: ["a2"] }), agent("a2", "working", { lineage_parent_pane_id: "a" }), agent("b", "seen"), agent("elsewhere", "working")],
      NOW,
    );
    expect(board.agents.map((row) => row.id)).toEqual(["agent:a", "agent:b"]);
    expect(agentColumnCards(board, "done").map((row) => row.id)).toEqual(["agent:a"]);
    expect(agentColumnCards(board, "seen").map((row) => row.id)).toEqual(["agent:b"]);
    expect(agentColumnCards(board, "active")).toEqual([]);
  });
});

describe("what the Overview draws", () => {
  it("is the empty state with no agent at all, the ad hoc strip for a folder, else the board", () => {
    expect(buildBoard(workspace([checkout("a", { changed: 3 })]), [], NOW).state).toBe("empty");
    expect(buildBoard(workspace([checkout("f", { worktree: false })], false), [], NOW).state).toBe("empty");
    const folder = buildBoard(workspace([checkout("f", { worktree: false, panes: ["x"] })], false), [agent("x")], NOW);
    expect(folder.state).toBe("adhoc");
    expect(folder.tasks).toEqual([]);
    expect(folder.adHoc.map((row) => row.id)).toEqual(["task:f"]);
    expect(buildBoard(workspace([checkout("a", { panes: ["x"] })]), [agent("x")], NOW).state).toBe("board");
  });

  it("counts worktrees, open pull requests once GitHub answered, and main behind origin only above zero", () => {
    const main = { ...checkout("main", { worktree: false }), worktree: { is_main: true, behind_upstream: 3 } as Checkout["worktree"] };
    const answered = { failure_category: null, available: true, loading: false, stale: false, last_success_at_unix_ms: NOW, unavailable_reason: null };
    const checkouts = [main, checkout("a", { pr: pr("open") }), checkout("b", { pr: pr("merged") }), checkout("c", { pr: pr("review") })];
    expect(buildBoard(workspace(checkouts), [], NOW).stats).toEqual({ worktrees: 3, openPullRequests: null, behind: { branch: "main", count: 3 } });
    const read = checkouts.map((row) => ({ ...row, github: answered }));
    expect(buildBoard(workspace(read), [], NOW).stats.openPullRequests).toBe(2);
    const even = { ...main, worktree: { is_main: true, behind_upstream: 0 } as Checkout["worktree"] };
    expect(buildBoard(workspace([even]), [], NOW).stats.behind).toBeNull();
  });
});

describe("an agent row", () => {
  it("carries a second line only while it waits, changed unseen, or is selected", () => {
    expect(showsDetail(agent("a", "working", { detail: "진행 중" }), false)).toBe(false);
    expect(showsDetail(agent("a", "needs_you", { detail: "질문" }), false)).toBe(true);
    expect(showsDetail(agent("a", "done", { detail: "완료", unread: true }), false)).toBe(true);
    expect(showsDetail(agent("a", "seen", { detail: "완료" }), true)).toBe(true);
    expect(showsDetail(agent("a", "needs_you", { detail: null }), true)).toBe(false);
  });

  it("is a parent waiting on descendants only when its own turn is over and a descendant still works or asks", () => {
    const counts = { error: 0, approval: 0, question: 0, working: 1, done: 0 };
    expect(waitingOnDescendants(agent("p", "working", { activity: "stopped", descendant_counts: counts }))).toBe(true);
    expect(waitingOnDescendants(agent("p", "working", { activity: "working", descendant_counts: counts }))).toBe(false);
    expect(waitingOnDescendants(agent("p", "needs_you", { activity: "stopped", demand: "question", descendant_counts: counts }))).toBe(false);
    expect(waitingOnDescendants(agent("p", "done", { activity: "stopped", descendant_counts: { ...counts, working: 0, done: 2 } }))).toBe(false);
  });
});
