// The Tasks and Agents boards (PRD task-agents-views): five columns from Git,
// a card per task and per untracked checkout, the backlog, the pull request
// as the result, at most two agents per card, and both scopes.

import { describe, expect, it } from "vitest";
import { agentColumnCards, allProjectsStats, buildAgents, buildTasks, formatBytes, projectStats, shownAgents, stageCards, stageOf, type BoardProject } from "./projectBoard";
import type { AgentRow, Checkout, PullRequest, Task, Workspace } from "./snapshot";

const NOW = 1_800_000_000_000;

function pr(badge: PullRequest["badge"], checks: PullRequest["checks"] = "unknown", draft = false): PullRequest {
  return { number: 7, title: "PR", url: "https://github.com/acme/project/pull/7", badge, review: null, is_draft: draft, checks };
}

function checkout(id: string, options: { changed?: number; ahead?: number; pr?: PullRequest; worktree?: boolean; panes?: string[]; merged?: boolean; task?: string; closes?: string[]; behind?: number } = {}): Checkout {
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
    worktree: { merged: options.merged ?? null, is_main: !worktree, behind_upstream: options.behind ?? null } as Checkout["worktree"],
    pull_request: options.pr ?? null,
    issue: null,
    task_key: options.task ?? null,
    closes_task_keys: options.closes ?? [],
    changed_file_count: options.changed ?? 0,
    ahead: options.ahead ?? 0,
    tabs: [{ id: `${id}:tab`, workspace_id: "project", checkout_id: id, label: "Tab 1", empty: false, delegated: false, panes: (options.panes ?? []).map((pane) => ({ id: pane })) as never }],
    active_tab_id: `${id}:tab`,
    strip: [],
    next_tab_label: "Tab 2",
  };
}

function task(number: number, open = true, title = `Task ${number}`): Task {
  return { key: `github:acme/project#${number}`, source: "github", id: `#${number}`, url: `https://github.com/acme/project/issues/${number}`, title, open };
}

function workspace(checkouts: Checkout[], options: { git?: boolean; tasks?: Task[] | null; failure?: string; id?: string } = {}): Workspace {
  const connected = options.tasks !== null;
  return {
    id: options.id ?? "project",
    label: options.id ?? "Project",
    path: "/fixture",
    device_id: "local",
    is_git: options.git ?? true,
    registered: true,
    temporary: false,
    pinned: false,
    checkouts,
    inactive_checkouts: { expanded: false, checkout_ids: [] },
    tasks: {
      source: connected ? { kind: "github", label: "GitHub", name: "acme/project", reading: false, failure: options.failure ?? null, last_read_at_unix_ms: NOW - 5 * 60_000 } : null,
      unconnected_reason: connected ? null : "gh is not logged in",
      tasks: options.tasks ?? [],
      overflow: false,
    },
  };
}

function agent(pane: string, group = "working", extra: Partial<AgentRow> = {}): AgentRow {
  return { id: pane, pane_id: pane, identity_label: pane, agent_kind: "claude", symbol: "●", group, status_label: group, elapsed: "1m", emphasized: false, unread: false, demand: "none", activity: "working", ...extra };
}

function one(project: Workspace, agents: AgentRow[] = []): BoardProject[] {
  return [{ workspace: project, agents, device: null }];
}

describe("the Git stage", () => {
  it("is done, then review, then working, else ready; agents and the task's state never move it", () => {
    expect(stageOf(checkout("a"))).toBe("ready");
    expect(stageOf(checkout("a", { changed: 1 }))).toBe("working");
    expect(stageOf(checkout("a", { ahead: 1 }))).toBe("working");
    expect(stageOf(checkout("a", { changed: 3, pr: pr("open") }))).toBe("review");
    expect(stageOf(checkout("a", { changed: 3, pr: pr("review") }))).toBe("review");
    expect(stageOf(checkout("a", { changed: 3, pr: pr("merged") }))).toBe("done");
    expect(stageOf(checkout("a", { changed: 3, pr: pr("closed") }))).toBe("working");
    expect(stageOf(checkout("a", { merged: true }))).toBe("done");
  });
});

describe("the Tasks board", () => {
  it("puts an open task no checkout works on in the backlog, never a closed one, and heads a linked card with its task (B1)", () => {
    const board = buildTasks(one(workspace([checkout("feat", { task: "github:acme/project#170", changed: 4 }), checkout("main", { worktree: false })], { tasks: [task(170), task(172), task(8, false)] })), "project", NOW);
    expect(stageCards(board, "backlog").map((card) => card.task?.id)).toEqual(["#172"]);
    const working = stageCards(board, "working")[0];
    expect(working?.title).toBe("Task 170");
    expect(working?.task?.id).toBe("#170");
    expect(working?.facts).toEqual({ files: 4, ahead: null, pr: null, behind: null });
    expect(board.adHoc).toEqual([]);
  });

  it("titles an untracked checkout by its branch and gives it only its pull request (D-07)", () => {
    const board = buildTasks(one(workspace([checkout("fix/tab-crash", { pr: pr("open", "unknown", true) })], { tasks: [] })), "project", NOW);
    const [card] = stageCards(board, "review");
    expect(card?.task).toBeNull();
    expect(card?.title).toBe("fix/tab-crash");
    expect(card?.facts.pr).toEqual({ number: 7, url: "https://github.com/acme/project/pull/7", tone: "draft", checks: null });
  });

  it("colours the pull request by its lifecycle and reads CI only once it was read (D-06)", () => {
    const facts = (value: PullRequest) => buildTasks(one(workspace([checkout("a", { pr: value })])), "project", NOW).cards[0]?.facts.pr;
    expect(facts(pr("open", "passing"))).toMatchObject({ tone: "open", checks: "passing" });
    expect(facts(pr("review", "failed"))).toMatchObject({ tone: "open", checks: "failed" });
    expect(facts(pr("merged", "pending"))).toMatchObject({ tone: "merged", checks: "pending" });
    expect(facts(pr("closed", "none"))).toMatchObject({ tone: "closed", checks: null });
  });

  it("shows the same pull request on every task it closes, and a done card behind its upstream (D-07)", () => {
    const board = buildTasks(
      one(workspace([checkout("both", { pr: pr("merged"), task: "github:acme/project#194", closes: ["github:acme/project#195"], behind: 12 })], { tasks: [task(194, false), task(195)] })),
      "project",
      NOW,
    );
    const done = stageCards(board, "done");
    expect(done.map((card) => card.task?.id)).toEqual(["#194", "#195"]);
    expect(done.every((card) => card.facts.pr?.tone === "merged" && card.facts.behind === 12)).toBe(true);
    // A task the pull request closes is not also waiting in the backlog.
    expect(stageCards(board, "backlog")).toEqual([]);
  });

  it("names at most two agents, the ones that need the operator first, and counts the rest (B6)", () => {
    const rows = ["w", "q", "d"].map((pane) => ({ agent: agent(pane, pane === "q" ? "needs_you" : pane === "d" ? "done" : "working", pane === "q" ? { demand: "question" } : {}), depth: 0 }));
    const { shown, more } = shownAgents(rows);
    expect(shown.map((row) => row.pane_id)).toEqual(["q", "d"]);
    expect(more).toBe(1);
  });

  it("offers Start agent only on a ready card nobody works on, on this machine (B7)", () => {
    const board = buildTasks(one(workspace([checkout("idle"), checkout("busy", { panes: ["b"] }), checkout("work", { changed: 1 })]), [agent("b")]), "project", NOW);
    expect(board.cards.filter((card) => card.canStart).map((card) => card.id)).toEqual(["checkout:idle"]);
  });

  it("raises a needs-you card to the top of its own column and keeps the rest in order", () => {
    const board = buildTasks(
      one(workspace([checkout("a", { changed: 1, panes: ["a"] }), checkout("b", { changed: 1, panes: ["b"] }), checkout("c", { changed: 1, panes: ["c"] })]), [agent("a"), agent("b"), agent("c", "needs_you", { demand: "error" })]),
      "project",
      NOW,
    );
    expect(stageCards(board, "working").map((card) => card.id)).toEqual(["checkout:c", "checkout:a", "checkout:b"]);
    expect(stageCards(board, "working")[0]?.error).toBe(true);
  });

  it("keeps a cross-checkout descendant under its root", () => {
    const parent = agent("p", "needs_you", { demand: "question", lineage_child_pane_ids: ["child"], lineage_collapsed: true });
    const child = agent("child", "working", { lineage_parent_pane_id: "p", delegated: true });
    const board = buildTasks(one(workspace([checkout("parent-branch", { changed: 1, panes: ["p"] }), checkout("child-branch", { changed: 1, panes: ["child"] })]), [child, parent]), "project", NOW);
    const parentCard = board.cards.find((card) => card.id === "checkout:parent-branch");
    expect(parentCard?.rows.map((row) => [row.agent.pane_id, row.depth])).toEqual([
      ["p", 0],
      ["child", 1],
    ]);
    expect(parentCard?.needsYou).toBe(true);
    expect(board.cards.find((card) => card.id === "checkout:child-branch")?.rows.map((row) => row.agent.pane_id)).toEqual(["child"]);
  });

  it("keeps the last tasks when the source could not be read and says so on each task card only (B13)", () => {
    const board = buildTasks(one(workspace([checkout("feat", { task: "github:acme/project#170" }), checkout("loose")], { tasks: [task(170), task(171)], failure: "rate limited" })), "project", NOW);
    expect(board.cards).toHaveLength(3);
    const failures = board.cards.map((card) => card.sourceFailure);
    expect(failures.filter(Boolean)).toHaveLength(2);
    expect(failures[0]).toBe("GitHub 읽기 실패 · 5분 전에 확인한 상태 · 자세한 오류는 진단 로그");
    expect(board.cards.find((card) => card.id === "checkout:loose")?.sourceFailure).toBeNull();
  });

  it("is empty only with no source and no agent; a folder with agents draws only the ad hoc strip (B14)", () => {
    const empty = buildTasks(one(workspace([checkout("a", { changed: 3 })], { tasks: null })), "project", NOW);
    expect(empty.empty).toBe(true);
    expect(empty.unconnectedReason).toBe("gh is not logged in");
    expect(buildTasks(one(workspace([], { tasks: [] })), "project", NOW).empty).toBe(false);
    const folder = buildTasks(one(workspace([checkout("f", { worktree: false, panes: ["x"] })], { git: false, tasks: null }), [agent("x")]), "project", NOW);
    expect(folder.empty).toBe(false);
    expect(folder.columns).toBe(false);
    expect(folder.adHoc.map((card) => card.id)).toEqual(["checkout:f"]);
  });

  it("mixes every project's tasks on All projects and gathers a project with agents and no source below (B15)", () => {
    const herdr = workspace([checkout("feat", { task: "github:acme/project#170", panes: ["h"] })], { id: "herdr", tasks: [task(170), task(172)] });
    const quiet = workspace([checkout("main", { worktree: false, panes: ["m"] })], { id: "modakbul", tasks: null });
    const board = buildTasks(
      [
        { workspace: herdr, agents: [agent("h")], device: null },
        { workspace: quiet, agents: [agent("m")], device: null },
      ],
      "all",
      NOW,
    );
    expect(board.cards.map((card) => card.place.projectId)).toEqual(["herdr", "herdr"]);
    expect(board.cards[0]?.idHelp).toBe("GitHub · herdr · feat");
    expect(board.unconnected).toEqual([{ place: { projectId: "modakbul", projectLabel: "modakbul" }, agents: 1, reason: "gh is not logged in" }]);
    expect(board.adHoc).toEqual([]);
  });
});

describe("the Agents board", () => {
  it("draws every agent, a delegated one right under its parent in the parent's column, with its task and its PR in the chip's tooltip (B11)", () => {
    const project = workspace([checkout("feat/waiting-band", { panes: ["a", "a2"], task: "github:acme/project#170", pr: pr("open") }), checkout("quick", { panes: ["b"] })], { tasks: [task(170)] });
    const board = buildAgents(one(project, [agent("a", "needs_you", { lineage_child_pane_ids: ["a2"] }), agent("a2", "working", { lineage_parent_pane_id: "a", delegated: true }), agent("b", "seen"), agent("elsewhere", "working")]), "project");
    expect(board.cards.map((card) => [card.agent.pane_id, card.depth])).toEqual([
      ["a", 0],
      ["a2", 1],
      ["b", 0],
    ]);
    expect(agentColumnCards(board, "active").map((card) => card.agent.pane_id)).toEqual(["a", "a2"]);
    expect(agentColumnCards(board, "seen").map((card) => card.agent.pane_id)).toEqual(["b"]);
    expect(agentColumnCards(board, "done")).toEqual([]);
    const [first] = board.cards;
    expect(first?.task?.id).toBe("#170");
    expect(first?.taskHelp).toBe("Task 170 · feat/waiting-band · PR #7");
    expect(first?.where).toBe("feat/waiting-band");
    expect(board.cards[2]?.task).toBeNull();
  });

  it("names the project before the checkout on All projects and the device of a remote agent (B12)", () => {
    const board = buildAgents([{ workspace: workspace([checkout("main", { worktree: false, panes: ["r"] })], { id: "sasu" }), agents: [agent("r")], device: "mini" }], "all");
    expect(board.cards[0]?.where).toBe("sasu · main");
    expect(board.cards[0]?.device).toBe("mini");
  });
});

describe("the facts line", () => {
  it("counts worktrees, open pull requests once GitHub answered, main behind origin only above zero, and merged worktrees", () => {
    const main = { ...checkout("main", { worktree: false }), worktree: { is_main: true, behind_upstream: 3 } as Checkout["worktree"] };
    const answered = { failure_category: null, available: true, loading: false, stale: false, last_success_at_unix_ms: NOW, unavailable_reason: null };
    const checkouts = [main, checkout("a", { pr: pr("open") }), checkout("b", { pr: pr("merged") }), checkout("c", { pr: pr("review") })];
    expect(projectStats(workspace(checkouts))).toEqual({ worktrees: 3, openPullRequests: null, behind: { branch: "main", count: 3 }, merged: 1, disk: null });
    const read = checkouts.map((row) => ({ ...row, github: answered }));
    expect(projectStats(workspace(read)).openPullRequests).toBe(2);
    const even = { ...main, worktree: { is_main: true, behind_upstream: 0 } as Checkout["worktree"] };
    expect(projectStats(workspace([even])).behind).toBeNull();
  });

  it("totals every Project's open pull requests and merged worktrees only when every Project can give its part", () => {
    const answered = { failure_category: null, available: true, loading: false, stale: false, last_success_at_unix_ms: NOW, unavailable_reason: null };
    const noRemote = { ...answered, available: false, last_success_at_unix_ms: null, failure_category: "no GitHub remote" };
    const read = workspace([checkout("a", { pr: pr("open") }), checkout("b", { pr: pr("merged") })].map((row) => ({ ...row, github: answered })));
    const local = workspace([{ ...checkout("c"), github: noRemote }]);
    const folder = workspace([checkout("f", { worktree: false })], { git: false });
    expect(allProjectsStats([read, local, folder])).toEqual({ projects: 3, openPullRequests: 1, merged: 1 });
    const unanswered = workspace([checkout("d", { pr: pr("merged") })]);
    expect(allProjectsStats([read, unanswered])).toEqual({ projects: 2, openPullRequests: null, merged: 2 });
    expect(allProjectsStats([read, null])).toEqual({ projects: 2, openPullRequests: null, merged: null });
  });

  it("states the disk as a size once measured, pending while measuring, and nothing when a part could not be read", () => {
    const sized = (disk: Workspace["disk"]) => projectStats({ ...workspace([]), disk }).disk;
    expect(sized({ total_bytes: null, unavailable_reason: null, measuring: true })).toBe("measuring");
    expect(sized({ total_bytes: 1_503_238_553, unavailable_reason: null, measuring: false })).toBe(1_503_238_553);
    expect(sized({ total_bytes: null, unavailable_reason: "Permission denied", measuring: false })).toBeNull();
    expect(formatBytes(512)).toBe("512 B");
    expect(formatBytes(812 * 1024 * 1024)).toBe("812 MB");
    expect(formatBytes(1_503_238_553)).toBe("1.4 GB");
  });
});
