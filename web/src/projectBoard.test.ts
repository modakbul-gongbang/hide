// The Tasks and Agents views (PRD task-agents-views, reworked issue-first on
// 2026-09-28): four stages from Git, a card per issue and per worktree, the
// backlog that starts work, the pull request as the result, at most two
// agents per card, and both scopes.

import { describe, expect, it } from "vitest";
import { allProjectsStats, buildDependencies, buildTasks, filterActive, filterBoard, formatBytes, NO_FILTER, projectStats, shownAgents, stageCards, stageOf, type BoardProject } from "./projectBoard";
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

const key = (number: number) => `github:acme/project#${number}`;

describe("the Issues board", () => {
  it("puts an open issue no checkout works on in the backlog, never a closed one, and heads a linked card with its issue and checkout chip (B1, B2)", () => {
    const board = buildTasks(one(workspace([checkout("feat", { task: key(170), changed: 4 }), checkout("main", { worktree: false })], { tasks: [task(170), task(172), task(8, false)] })), "project", NOW);
    expect(stageCards(board, "backlog").map((card) => card.task.id)).toEqual(["#172"]);
    const [working] = stageCards(board, "working");
    expect(working?.title).toBe("Task 170");
    expect(working?.chip).toEqual({ branch: "feat", primary: false, ahead: null, files: 4 });
    expect(working?.first).toBe("workspace");
    expect(stageCards(board, "backlog")[0]?.chip).toBeNull();
    // The primary checkout with no issue and no agent is not a card.
    expect(board.cards.some((card) => card.checkout?.id === "main")).toBe(false);
  });

  it("orders the backlog by when each issue last changed, most recent first", () => {
    const at = (value: Task, updated: number): Task => ({ ...value, updated_at_unix_ms: updated });
    const board = buildTasks(one(workspace([], { tasks: [at(task(1), 10), at(task(2), 30), task(3), at(task(4), 20)] })), "project", NOW);
    expect(stageCards(board, "backlog").map((card) => card.task.id)).toEqual(["#2", "#4", "#1", "#3"]);
    expect(board.source).toEqual({ reading: false, failure: null, openIssues: 4, label: "GitHub" });
  });

  it("makes no card of work with no issue: a worktree in progress and an open pull request each fold into their column's line (B1, B4)", () => {
    const board = buildTasks(
      one(workspace([checkout("loose", { changed: 2, panes: ["a"] }), checkout("fix/tab-crash", { pr: pr("open", "passing") }), checkout("merged-work", { merged: true })], { tasks: [] })),
      "project",
      NOW,
    );
    expect(board.cards).toEqual([]);
    expect(board.loose.worktrees.map((value) => value.branch)).toEqual(["loose"]);
    expect(board.loose.pullRequests.map((value) => [value.number, value.title, value.tone])).toEqual([[7, "PR", "open"]]);
  });

  it("puts an issue in review when its open pull request reaches it by the branch's link or by a closing reference, and shows that pull request on each (B3)", () => {
    const board = buildTasks(
      one(
        workspace(
          [
            checkout("191-desktop", { task: key(191), pr: pr("review", "passing") }),
            // A pull request whose branch names no issue but whose body closes one.
            { ...checkout("closer", { pr: { ...pr("open"), number: 9 }, closes: [key(193), key(194)] }) },
          ],
          { tasks: [task(191), task(193), task(194)] },
        ),
      ),
      "project",
      NOW,
    );
    const review = stageCards(board, "review");
    expect(review.map((card) => [card.task.id, card.pr?.number])).toEqual([
      ["#191", 7],
      ["#193", 9],
      ["#194", 9],
    ]);
    expect(review.every((card) => card.first === "pull_request")).toBe(true);
    expect(stageCards(board, "backlog")).toEqual([]);
    expect(board.loose.pullRequests).toEqual([]);
  });

  it("lists a done issue with the pull request that closed it and when that merged (B3)", () => {
    const merged: PullRequest = { ...pr("merged"), number: 180, merged_at_unix_ms: NOW - 86_400_000 };
    const board = buildTasks(one(workspace([checkout("both", { pr: merged, task: key(174), closes: [key(175)] })], { tasks: [task(174, false), task(175)] })), "project", NOW);
    const done = stageCards(board, "done");
    expect(done.map((card) => [card.task.id, card.pr?.number, card.pr?.tone, card.mergedAt])).toEqual([
      ["#174", 180, "merged", NOW - 86_400_000],
      ["#175", 180, "merged", NOW - 86_400_000],
    ]);
    expect(done.every((card) => card.chip === null && card.first === null)).toBe(true);
  });

  it("colours the pull request by its lifecycle and reads CI only once it was read (D-06)", () => {
    const chip = (value: PullRequest) => buildTasks(one(workspace([checkout("a", { pr: value, task: key(1) })], { tasks: [task(1)] })), "project", NOW).cards[0]?.pr;
    expect(chip(pr("open", "passing"))).toMatchObject({ tone: "open", checks: "passing" });
    expect(chip(pr("review", "failed"))).toMatchObject({ tone: "open", checks: "failed" });
    expect(chip(pr("merged", "pending"))).toMatchObject({ tone: "merged", checks: "pending" });
    expect(chip(pr("closed", "none"))).toMatchObject({ tone: "closed", checks: null });
    expect(chip(pr("open", "unknown", true))).toMatchObject({ tone: "draft", checks: null });
  });

  it("names at most two agents, the ones that need the operator first, and counts the rest (B2)", () => {
    const rows = ["w", "q", "d"].map((pane) => ({ agent: agent(pane, pane === "q" ? "needs_you" : pane === "d" ? "done" : "working", pane === "q" ? { demand: "question" } : {}), depth: 0 }));
    const { shown, more } = shownAgents(rows);
    expect(shown.map((row) => row.pane_id)).toEqual(["q", "d"]);
    expect(more).toBe(1);
  });

  it("offers 시작 on an open backlog issue of a project on this Mac and editing on a Local one only (B6, D-41)", () => {
    const project = workspace([checkout("linked", { task: key(1), changed: 1 })], { tasks: [task(1), task(2)] });
    const board = buildTasks(one(project), "project", NOW);
    expect(board.cards.filter((card) => card.canStart).map((card) => card.id)).toEqual(["task:project:github:acme/project#2"]);
    expect(board.cards.find((card) => card.canStart)?.first).toBe("start");
    expect(board.cards.some((card) => card.editable)).toBe(false);
    const local: Task = { key: "local:/fixture#3", source: "local", id: "L-3", url: null, title: "Local 3", open: true };
    expect(buildTasks(one(workspace([], { tasks: [local] })), "project", NOW).cards[0]?.editable).toBe(true);
    const remote = buildTasks(one(workspace([checkout("w")], { tasks: [task(3), local], remote: "mini" })), "project", NOW);
    expect(remote.cards.some((card) => card.canStart || card.editable)).toBe(false);
  });

  it("raises a card whose agent waits on the operator to the top of its own column and keeps the rest in order (B5)", () => {
    const board = buildTasks(
      one(workspace([checkout("a", { task: key(1), panes: ["a"] }), checkout("b", { task: key(2), panes: ["b"] }), checkout("c", { task: key(3), panes: ["c"] })], { tasks: [task(1), task(2), task(3)] }), [
        agent("a"),
        agent("b"),
        agent("c", "needs_you", { demand: "question" }),
      ]),
      "project",
      NOW,
    );
    expect(stageCards(board, "working").map((card) => [card.id, card.needsYou])).toEqual([
      ["checkout:c", true],
      ["checkout:a", false],
      ["checkout:b", false],
    ]);
  });

  it("keeps a cross-checkout descendant under its root", () => {
    const parent = agent("p", "needs_you", { demand: "question", lineage_child_pane_ids: ["child"], lineage_collapsed: true });
    const child = agent("child", "working", { lineage_parent_pane_id: "p", delegated: true });
    const board = buildTasks(
      one(workspace([checkout("parent-branch", { task: key(1), panes: ["p"] }), checkout("child-branch", { task: key(2), panes: ["child"] })], { tasks: [task(1), task(2)] }), [child, parent]),
      "project",
      NOW,
    );
    const parentCard = board.cards.find((card) => card.id === "checkout:parent-branch");
    expect(parentCard?.rows.map((row) => [row.agent.pane_id, row.depth])).toEqual([
      ["p", 0],
      ["child", 1],
    ]);
    expect(parentCard?.needsYou).toBe(true);
    expect(board.cards.find((card) => card.id === "checkout:child-branch")?.rows.map((row) => row.agent.pane_id)).toEqual(["child"]);
  });

  it("keeps the last issues when the source could not be read and says so on every card with the value's age (B17)", () => {
    const board = buildTasks(one(workspace([checkout("feat", { task: key(170) }), checkout("loose")], { tasks: [task(170), task(171)], failure: "rate limited" })), "project", NOW);
    expect(board.cards).toHaveLength(2);
    expect(board.cards.map((card) => card.sourceFailure)).toEqual(["GitHub 읽기 실패 · 5분 전 값 · 이유는 로그에", "GitHub 읽기 실패 · 5분 전 값 · 이유는 로그에"]);
    expect(board.source.failure).toBe("GitHub 읽기 실패 · 이유는 로그에");
  });

  it("puts a folder or the primary checkout on the board only while an agent there works on an issue", () => {
    const local = (number: number): Task => ({ key: `local:/fixture#${number}`, source: "local", id: `L-${number}`, url: null, title: `Local ${number}`, open: true });
    const folder = (task: string | undefined, panes: string[]) => workspace([checkout("f", { worktree: false, panes, task })], { git: false, tasks: [local(1)] });
    const working = stageCards(buildTasks(one(folder("local:/fixture#1", ["x"]), [agent("x")]), "project", NOW), "working");
    expect(working.map((card) => [card.task.id, card.chip?.primary])).toEqual([["L-1", true]]);
    expect(buildTasks(one(folder(undefined, ["x"]), [agent("x")]), "project", NOW).cards.map((card) => card.id)).toEqual(["task:project:local:/fixture#1"]);
    expect(stageCards(buildTasks(one(folder("local:/fixture#1", [])), "project", NOW), "working")).toEqual([]);
    // A project still reading its source has no count to state.
    const reading = workspace([], { tasks: [] });
    reading.tasks = { ...reading.tasks!, source: { ...reading.tasks!.source!, reading: true } };
    expect(buildTasks(one(reading), "project", NOW).source.openIssues).toBeNull();
  });

  it("mixes every project's issues on All projects, each card naming its project", () => {
    const herdr = workspace([checkout("feat", { task: key(170), panes: ["h"] })], { id: "herdr", tasks: [task(170), task(172)] });
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

describe("the Issues filter", () => {
  const board = buildTasks(
    one(workspace([checkout("sig", { task: key(192), panes: ["q"] })], { tasks: [task(192, true, "hided has no SIGTERM handler"), task(214, true, "Stale HERDR_BIN_PATH")] }), [agent("q", "needs_you", { demand: "question" })]),
    "project",
    NOW,
  );

  it("keeps the issues whose id or title holds every word, and the operator's turn only when asked (B21)", () => {
    expect(filterBoard(board, NO_FILTER)).toBe(board);
    expect(filterBoard(board, { query: "sigterm HIDED", turn: false }).cards.map((card) => card.task.id)).toEqual(["#192"]);
    expect(filterBoard(board, { query: "#214", turn: false }).cards.map((card) => card.task.id)).toEqual(["#214"]);
    expect(filterBoard(board, { query: "", turn: true }).cards.map((card) => card.task.id)).toEqual(["#192"]);
    expect(filterBoard(board, { query: "nothing", turn: false }).cards).toEqual([]);
    expect(filterActive({ query: "  ", turn: false })).toBe(false);
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
