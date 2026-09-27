// The Tasks and Agents views (PRD task-agents-views, reworked issue-first on
// 2026-09-28): four stages from Git, a card per issue and per worktree, the
// backlog that starts work, the pull request as the result, at most two
// agents per card, the Agents inbox, and both scopes.

import { describe, expect, it } from "vitest";
import { agentGroupCards, allProjectsStats, buildAgents, buildDependencies, buildTasks, formatBytes, projectStats, shownAgents, stageCards, stageOf, waitingCount, type BoardProject } from "./projectBoard";
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

function workspace(checkouts: Checkout[], options: { git?: boolean; tasks?: Task[] | null; failure?: string; id?: string; remote?: string } = {}): Workspace {
  const connected = options.tasks !== null;
  return {
    ...(options.remote ? { remote_target_id: options.remote } : {}),
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
  it("is done, then review, else in progress; agents and the issue's state never move it", () => {
    expect(stageOf(checkout("a"))).toBe("working");
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
    // The primary checkout with no issue and no agent is not a task.
    expect(board.cards.some((card) => card.checkout?.id === "main")).toBe(false);
  });

  it("orders the backlog by when each issue last changed, most recent first", () => {
    const at = (value: Task, updated: number): Task => ({ ...value, updated_at_unix_ms: updated });
    const board = buildTasks(one(workspace([], { tasks: [at(task(1), 10), at(task(2), 30), task(3), at(task(4), 20)] })), "project", NOW);
    expect(stageCards(board, "backlog").map((card) => card.task?.id)).toEqual(["#2", "#4", "#1", "#3"]);
    expect(board.source).toEqual({ reading: false, failure: null, openIssues: 4, label: "GitHub" });
  });

  it("titles an untracked checkout by its branch and gives it only its pull request (D-07)", () => {
    const board = buildTasks(one(workspace([checkout("fix/tab-crash", { pr: pr("open", "unknown", true) })], { tasks: [] })), "project", NOW);
    const [card] = stageCards(board, "review");
    expect(card?.task).toBeNull();
    expect(card?.title).toBe("fix/tab-crash");
    expect(card?.facts.pr).toEqual({ number: 7, url: "https://github.com/acme/project/pull/7", tone: "draft", checks: null, review: null });
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

  it("offers 시작 on an open backlog issue of a project on this Mac, and 이슈 연결 on a worktree with none (B7)", () => {
    const project = workspace([checkout("idle"), checkout("busy", { panes: ["b"] }), checkout("linked", { task: "github:acme/project#1", changed: 1 })], { tasks: [task(1), task(2)] });
    const board = buildTasks(one(project, [agent("b")]), "project", NOW);
    expect(board.cards.filter((card) => card.canStart).map((card) => card.id)).toEqual(["task:project:github:acme/project#2"]);
    expect(board.cards.filter((card) => card.canLink).map((card) => card.id)).toEqual(["checkout:idle", "checkout:busy"]);
    // A worktree with no issue and no agent folds away; one an agent works in stays.
    expect(board.cards.filter((card) => card.idle).map((card) => card.id)).toEqual(["checkout:idle"]);
    expect(board.cards.find((card) => card.id === "checkout:busy")?.title).toBe("busy");
    const remote = buildTasks(one(workspace([checkout("w")], { tasks: [task(3)], remote: "mini" })), "project", NOW);
    expect(remote.cards.some((card) => card.canStart || card.canLink)).toBe(false);
  });

  it("titles a worktree with no issue by its purpose, else its branch", () => {
    const named = { ...checkout("prd/agent-tab-groups"), purpose: { text: "Agent tab groups", origin: "token" } as Checkout["purpose"] };
    const board = buildTasks(one(workspace([named])), "project", NOW);
    expect(board.cards[0]?.title).toBe("Agent tab groups");
    expect(board.cards[0]?.branch).toBe("prd/agent-tab-groups");
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
    expect(board.source.failure).toBe("GitHub 읽기 실패 · 자세한 오류는 진단 로그");
  });

  it("puts a folder or the primary checkout on the board only while an agent there works on an issue (B14)", () => {
    const local = (number: number): Task => ({ key: `local:/fixture#${number}`, source: "local", id: `L-${number}`, url: null, title: `Local ${number}`, open: true });
    const folder = (key: string | undefined, panes: string[]) => workspace([checkout("f", { worktree: false, panes, task: key })], { git: false, tasks: [local(1)] });
    expect(stageCards(buildTasks(one(folder("local:/fixture#1", ["x"]), [agent("x")]), "project", NOW), "working").map((card) => card.task?.id)).toEqual(["L-1"]);
    expect(buildTasks(one(folder(undefined, ["x"]), [agent("x")]), "project", NOW).cards.map((card) => card.id)).toEqual(["task:project:local:/fixture#1"]);
    expect(stageCards(buildTasks(one(folder("local:/fixture#1", [])), "project", NOW), "working")).toEqual([]);
    // A project still reading its source has no count to state.
    const reading = workspace([], { tasks: [] });
    reading.tasks = { ...reading.tasks!, source: { ...reading.tasks!.source!, reading: true } };
    expect(buildTasks(one(reading), "project", NOW).source.openIssues).toBeNull();
  });

  it("mixes every project's tasks on All projects, each card naming its project (B15)", () => {
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
    expect(board.cards.map((card) => card.project)).toEqual(["herdr", "herdr"]);
  });
});

describe("the Dependencies mode", () => {
  const key = (number: number, repository = "acme/project") => `github:${repository}#${number}`;
  const blocked = (value: Task, ...blockers: { key: string; id: string | null }[]): Task => ({ ...value, blocked_by: blockers });
  const ids = (cards: { task: Task | null }[]) => cards.map((card) => card.task?.id ?? card.task?.title);

  it("locks a task on the tasks it waits on and lays the chain out left to right, unrelated tasks apart and untracked checkouts left out (B8)", () => {
    const board = buildTasks(
      one(
        workspace([checkout("feat", { task: key(170), changed: 1 }), checkout("quick", { changed: 1 })], {
          tasks: [task(170), blocked(task(171), { key: key(170), id: "#170" }), blocked(task(172), { key: key(171), id: "#171" }), task(173)],
        }),
      ),
      "project",
      NOW,
    );
    expect(stageCards(board, "backlog").find((card) => card.task?.id === "#171")?.blockedBy).toEqual([{ key: key(170), label: "#170" }]);
    const graph = buildDependencies(board);
    expect(graph.layers.map(ids)).toEqual([["#170"], ["#171"], ["#172"]]);
    expect(graph.edges).toEqual([
      { from: "checkout:feat", to: `task:project:${key(171)}` },
      { from: `task:project:${key(171)}`, to: `task:project:${key(172)}` },
    ]);
    expect(ids(graph.unrelated)).toEqual(["#173"]);
  });

  it("draws a blocker in another project of the scope as an arrow and one outside the scope as the lock line only (D-10)", () => {
    const sasu = { ...task(5, true, "judge 백엔드 전환"), key: key(5, "acme/sasu"), id: null };
    const board = buildTasks(
      [
        { workspace: workspace([], { id: "herdr", tasks: [task(170), task(171)] }), agents: [], device: null },
        { workspace: workspace([], { id: "sasu", tasks: [blocked(sasu, { key: key(170), id: "acme/project#170" }, { key: key(9, "acme/elsewhere"), id: "acme/elsewhere#9" })] }), agents: [], device: null },
      ],
      "all",
      NOW,
    );
    const graph = buildDependencies(board);
    expect(graph.layers.map(ids)).toEqual([["#170"], ["judge 백엔드 전환"]]);
    expect(graph.edges).toEqual([{ from: `task:herdr:${key(170)}`, to: `task:sasu:${key(5, "acme/sasu")}` }]);
    expect(graph.layers[1]?.[0]?.blockedBy.map((blocker) => blocker.label)).toEqual(["acme/project#170", "acme/elsewhere#9"]);
    expect(ids(graph.unrelated)).toEqual(["#171"]);
  });

  it("names a blocker with no id by its title, orders a column by the rows it hangs from, and drops the arrow that closes a cycle", () => {
    const local = { ...task(1, true, "verify 슬롯 병렬화"), id: null };
    const board = buildTasks(
      one(
        workspace([], {
          tasks: [task(10), local, blocked(task(11), { key: key(1), id: null }), blocked(task(12), { key: key(10), id: "#10" }), blocked(task(20), { key: key(21), id: "#21" }), blocked(task(21), { key: key(20), id: "#20" })],
        }),
      ),
      "project",
      NOW,
    );
    expect(board.cards.find((card) => card.task?.id === "#11")?.blockedBy).toEqual([{ key: key(1), label: "verify 슬롯 병렬화" }]);
    const graph = buildDependencies(board);
    // #10 comes first in the source and #12 hangs from it, so #12 sits on #10's row.
    expect(graph.layers.map(ids)).toEqual([
      ["#10", "verify 슬롯 병렬화", "#21"],
      ["#12", "#11", "#20"],
    ]);
    expect(graph.edges.filter((edge) => edge.from.includes("#2") && edge.to.includes("#2"))).toHaveLength(1);
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
    expect(agentGroupCards(board, "needs").map((card) => card.agent.pane_id)).toEqual(["a", "a2"]);
    expect(agentGroupCards(board, "resting").map((card) => card.agent.pane_id)).toEqual(["b"]);
    expect(agentGroupCards(board, "working")).toEqual([]);
    const [first] = board.cards;
    expect(first?.task?.id).toBe("#170");
    expect(first?.taskHelp).toBe("Task 170 · feat/waiting-band · PR #7");
    expect(first?.where).toBe("feat/waiting-band");
    expect(board.cards[2]?.task).toBeNull();
  });

  it("puts the agents waiting on the operator first, an error before a question before a result, and counts each lineage once", () => {
    const project = workspace([checkout("feat", { panes: ["ask", "fail", "done", "busy", "child"] }), checkout("old", { panes: ["rest"], merged: true })]);
    const agents = [
      agent("done", "done", { unread: true }),
      agent("ask", "needs_you", { demand: "question", lineage_child_pane_ids: ["child"] }),
      agent("child", "working", { lineage_parent_pane_id: "ask", delegated: true }),
      agent("busy"),
      agent("fail", "needs_you", { demand: "error" }),
      agent("rest", "seen"),
    ];
    const board = buildAgents(one(project, agents), "project");
    expect(agentGroupCards(board, "needs").map((card) => card.agent.pane_id)).toEqual(["fail", "ask", "child", "done"]);
    expect(agentGroupCards(board, "working").map((card) => card.agent.pane_id)).toEqual(["busy"]);
    // A resting agent in a merged worktree is there only to be closed.
    expect(agentGroupCards(board, "resting")).toEqual([]);
    expect(agentGroupCards(board, "cleanup").map((card) => card.agent.pane_id)).toEqual(["rest"]);
    expect(waitingCount(board)).toBe(3);
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
