// The Overview's lenses (PRD overview-lenses-tiles-agents): the tiles'
// values, the checkout lanes' order, columns and folds, the lineage rows,
// and where every way in lands. The expected answers are the PRD's
// Behaviors, read against small fixtures.

import { describe, expect, it } from "vitest";
import { agentsTile, bucketOf, buildLanes as lanesOf, buildLineages as lineagesOf, childSummary, entryLane, issuesTile, lineageAgentCount, scopeAgents, sessionsTile, startOfDay } from "./overviewLens";
import { buildTasks, type BoardProject } from "./projectBoard";
import type { AgentRow, Checkout, ProjectSessions, PullRequest, SessionRow, Task, Workspace } from "./snapshot";

const buildLanes = (projects: BoardProject[], scope: "project" | "all") => lanesOf(projects, scopeAgents(projects), scope);
const buildLineages = (projects: BoardProject[]) => lineagesOf(scopeAgents(projects));

const NOW = new Date(2026, 8, 28, 15, 0, 0).getTime();

function checkout(id: string, options: { primary?: boolean; panes?: string[]; merged?: boolean; missing?: boolean; pr?: PullRequest; task?: string; changed?: number; ahead?: number } = {}): Checkout {
  const primary = options.primary ?? false;
  return {
    id,
    workspace_id: "project",
    label: id,
    path: `/fixture/${id}`,
    branch: id,
    purpose: null,
    is_worktree: true,
    is_primary: primary,
    exists: !options.missing,
    has_panes: (options.panes ?? []).length > 0,
    worktree: { merged: options.merged ?? null, is_main: primary, missing: options.missing ?? false, changed_file_count: options.changed ?? 0, dirty: (options.changed ?? 0) > 0 } as Checkout["worktree"],
    pull_request: options.pr ?? null,
    task_key: options.task ?? null,
    changed_file_count: options.changed ?? 0,
    ahead: options.ahead ?? 0,
    tabs: [{ id: `${id}:tab`, workspace_id: "project", checkout_id: id, label: "Tab 1", empty: false, delegated: false, panes: (options.panes ?? []).map((pane) => ({ id: pane })) as never }],
    active_tab_id: `${id}:tab`,
    strip: [],
    next_tab_label: "Tab 2",
  };
}

function workspace(checkouts: Checkout[], options: { id?: string; tasks?: Task[]; reading?: boolean; failure?: string } = {}): Workspace {
  return {
    id: options.id ?? "project",
    label: options.id ?? "Project",
    path: "/fixture",
    device_id: "local",
    is_git: true,
    default_branch: "main",
    registered: true,
    temporary: false,
    pinned: false,
    checkouts,
    inactive_checkouts: { expanded: false, checkout_ids: [] },
    tasks: {
      source: { kind: "github", label: "GitHub", name: "acme/project", reading: options.reading ?? false, failure: options.failure ?? null, last_read_at_unix_ms: NOW - 3 * 60_000 },
      tasks: options.tasks ?? [],
      overflow: false,
    },
  };
}

function agent(pane: string, group: string, extra: Partial<AgentRow> = {}): AgentRow {
  return { id: pane, pane_id: pane, identity_label: pane, agent_kind: "claude", symbol: "●", group, status_label: group, elapsed: "1m", emphasized: false, unread: false, demand: "none", activity: "working", last_activity: "0000000000001", ...extra };
}

function one(project: Workspace, agents: AgentRow[]): BoardProject[] {
  return [{ workspace: project, agents, device: null }];
}

function task(number: number, open = true): Task {
  return { key: `github:acme/project#${number}`, source: "github", id: `#${number}`, url: `https://github.com/acme/project/issues/${number}`, title: `Task ${number}`, open, updated_at_unix_ms: NOW - 60_000 };
}

function pr(badge: PullRequest["badge"]): PullRequest {
  return { number: 9, title: "PR", url: "https://github.com/acme/project/pull/9", badge, review: null, is_draft: false, checks: "passing" };
}

describe("the buckets", () => {
  it("reads asking and unseen finished agents as the operator's turn, a quiet root waiting on children apart from working, and the rest resting", () => {
    expect(bucketOf(agent("a", "needs_you", { demand: "question" }))).toBe("turn");
    expect(bucketOf(agent("a", "done"))).toBe("turn");
    expect(bucketOf(agent("a", "working", { waiting_on_descendants: true }))).toBe("delegating");
    expect(bucketOf(agent("a", "working"))).toBe("working");
    expect(bucketOf(agent("a", "seen"))).toBe("resting");
  });
});

describe("the tiles", () => {
  it("counts the scope's agents, badges the operator's turn with its breakdown, and bars the four buckets (B2, B3)", () => {
    const project = workspace([checkout("main", { primary: true, panes: ["q", "d", "w", "r", "x"] })]);
    const agents = [
      agent("q", "needs_you", { demand: "question" }),
      agent("d", "done"),
      agent("w", "working"),
      agent("r", "seen"),
      agent("x", "working", { waiting_on_descendants: true }),
      agent("elsewhere", "needs_you"),
    ];
    const tile = agentsTile(buildLanes(one(project, agents), "project").lanes.flatMap((lane) => lane.nodes), { state: "ready" });
    expect(tile.value).toBe(5);
    expect(tile.badge).toEqual({ count: 2, parts: [{ key: "question", label: "질문", count: 1 }, { key: "done", label: "끝남", count: 1 }] });
    expect(tile.bar?.map((segment) => [segment.key, segment.count])).toEqual([["turn", 2], ["working", 1], ["delegating", 1], ["resting", 1]]);
  });

  it("draws zero as zero with no badge, nothing for a device that has not answered, and ⚠ with the reason for one that cannot (B6)", () => {
    expect(agentsTile([], { state: "ready" })).toMatchObject({ value: 0, badge: null, bar: [{ count: 0 }, { count: 0 }, { count: 0 }, { count: 0 }], failure: null });
    expect(agentsTile([], { state: "loading", text: "connecting" })).toMatchObject({ value: null, badge: null, bar: null, failure: null });
    expect(agentsTile([], { state: "unavailable", text: "mini is unreachable", retry: "connect" })).toMatchObject({ value: null, bar: null, failure: "에이전트를 읽지 못함 · mini is unreachable" });
  });

  it("counts open issues once the source answered, bars backlog, in progress and review, and keeps the last value with its age when a read fails (B3, B6)", () => {
    const project = workspace([checkout("main", { primary: true }), checkout("wip", { task: task(2).key }), checkout("rev", { task: task(3).key, pr: pr("open") })], { tasks: [task(1), task(2), task(3), task(4, false)] });
    const tile = issuesTile(buildTasks(one(project, []), "project", NOW), NOW, NOW - 3 * 60_000);
    expect(tile).toMatchObject({ value: 3, unit: "열림", failure: null });
    expect(tile.bar?.map((segment) => [segment.key, segment.count])).toEqual([["backlog", 1], ["working", 1], ["review", 1]]);
    const reading = issuesTile(buildTasks(one(workspace([], { tasks: [task(1)], reading: true }), []), "project", NOW), NOW, null);
    expect(reading).toMatchObject({ value: null, bar: null });
    const failed = issuesTile(buildTasks(one(workspace([], { tasks: [task(1)], failure: "gh auth" }), []), "project", NOW), NOW, NOW - 3 * 60_000);
    expect(failed.value).toBe(1);
    expect(failed.failure).toContain("3분 전");
  });

  it("counts today's sessions by this machine's date, split Claude and Codex, and stays empty until this Project's history answered (B5, D-16)", () => {
    const row = (id: string, provider: string, at: number): SessionRow => ({ id, provider, provider_label: provider, locator: id, checkout_path: "/fixture", first_human_request: null, started_at_unix_ms: at, updated_at_unix_ms: at, title: null, unavailable_reason: null });
    const history = (rows: SessionRow[], extra: Partial<ProjectSessions> = {}): ProjectSessions => ({ device_id: "local", workspace_id: "project", unavailable_reason: null, loading: false, failure: null, rows, detail: null, ...extra });
    const today = startOfDay(NOW);
    const rows = [row("a", "claude", today + 1), row("b", "codex", NOW - 1), row("c", "claude", today - 1)];
    const tile = sessionsTile(history(rows), "project", NOW);
    expect(tile).toMatchObject({ value: 2, unit: "오늘" });
    expect(tile.bar).toEqual([{ key: "claude", label: "Claude", count: 1 }, { key: "codex", label: "Codex", count: 1 }]);
    expect(sessionsTile(history(rows), "another", NOW).value).toBeNull();
    expect(sessionsTile(history([], { loading: true }), "project", NOW).value).toBeNull();
    expect(sessionsTile(null, "project", NOW).value).toBeNull();
    expect(sessionsTile(history(rows, { failure: "unreadable" }), "project", NOW)).toMatchObject({ value: 2, failure: expect.stringContaining("마지막으로 읽은 값") });
  });
});

describe("the checkout lanes", () => {
  it("puts main first, then lanes with the operator's turn, working, resting, most recent first inside each, and orders nodes turn, working, waiting, resting (B13)", () => {
    const project = workspace([
      checkout("main", { primary: true, panes: ["m-rest", "m-work", "m-turn", "m-wait"] }),
      checkout("resting", { panes: ["r1"] }),
      checkout("working-old", { panes: ["w-old"] }),
      checkout("working-new", { panes: ["w-new"] }),
      checkout("turn", { panes: ["t1"] }),
    ]);
    const agents = [
      agent("m-rest", "seen"),
      agent("m-work", "working"),
      agent("m-turn", "done"),
      agent("m-wait", "working", { waiting_on_descendants: true }),
      agent("r1", "seen"),
      agent("w-old", "working", { last_activity: "0000000000002" }),
      agent("w-new", "working", { last_activity: "0000000000009" }),
      agent("t1", "needs_you", { demand: "question" }),
    ];
    const board = buildLanes(one(project, agents), "project");
    expect(board.lanes.map((lane) => lane.id)).toEqual(["main", "turn", "working-new", "working-old", "resting"]);
    expect(board.lanes[0]!.nodes.map((node) => node.agent.pane_id)).toEqual(["m-turn", "m-work", "m-wait", "m-rest"]);
  });

  it("keeps a delegated child under its parent's column across lanes, and marks a delegation inside one lane as within it (B14)", () => {
    const project = workspace([checkout("main", { primary: true, panes: ["obs-a", "obs-b"] }), checkout("impl", { panes: ["impl", "review"] })]);
    const agents = [
      agent("obs-a", "working"),
      agent("obs-b", "working", { last_activity: "0000000000000" }),
      agent("impl", "needs_you", { demand: "question", lineage_parent_pane_id: "obs-b" }),
      agent("review", "working", { lineage_parent_pane_id: "impl" }),
    ];
    const board = buildLanes(one(project, agents), "project");
    const column = (pane: string) => board.lanes.flatMap((lane) => lane.nodes).find((node) => node.agent.pane_id === pane)?.column;
    expect([column("obs-a"), column("obs-b"), column("impl"), column("review")]).toEqual([0, 1, 1, 2]);
    expect(board.delegations).toEqual([
      { from: "obs-b", to: "impl", within: false },
      { from: "impl", to: "review", within: true },
    ]);
    expect(board.columns).toBe(3);
  });

  it("keeps a line's column free in the lanes it crosses, so it never runs through another agent's node (B14)", () => {
    const project = workspace([checkout("main", { primary: true, panes: ["observer"] }), checkout("asking", { panes: ["asking"] }), checkout("impl", { panes: ["impl"] })]);
    const agents = [agent("observer", "working", { waiting_on_descendants: true }), agent("asking", "needs_you", { demand: "question" }), agent("impl", "working", { lineage_parent_pane_id: "observer" })];
    const board = buildLanes(one(project, agents), "project");
    expect(board.lanes.map((lane) => lane.id)).toEqual(["main", "asking", "impl"]);
    expect(board.lanes.map((lane) => lane.nodes.map((node) => node.column))).toEqual([[0], [1], [0]]);
    expect(board.columns).toBe(2);
  });

  it("folds worktrees with no agent, and merged or folder-less ones whose agents rest, into their own lines; a merged one with working agents stays a lane (B18, B20)", () => {
    const project = workspace([
      checkout("main", { primary: true }),
      checkout("idle"),
      checkout("merged-rest", { merged: true, panes: ["mr"] }),
      checkout("gone", { missing: true }),
      checkout("merged-busy", { pr: pr("merged"), panes: ["mb"] }),
    ]);
    const board = buildLanes(one(project, [agent("mr", "seen"), agent("mb", "working")]), "project");
    expect(board.lanes.map((lane) => [lane.id, lane.cleanup])).toEqual([["main", null], ["merged-busy", "merged"]]);
    expect(board.empty.map((lane) => lane.id)).toEqual(["idle"]);
    expect(board.cleanup.map((lane) => [lane.id, lane.cleanup])).toEqual([["merged-rest", "merged"], ["gone", "missing"]]);
  });

  it("on All projects ranks each project's main by its agents, main first among equals, and folds an idle main with the idle worktrees (B30)", () => {
    const a = workspace([checkout("a-main", { primary: true, panes: ["a1"] })], { id: "a" });
    const b = workspace([{ ...checkout("b-main", { primary: true }), workspace_id: "b" }, { ...checkout("b-wt", { panes: ["b1"] }), workspace_id: "b" }], { id: "b" });
    const c = workspace([{ ...checkout("c-wt", { panes: ["c1"] }), workspace_id: "c" }, { ...checkout("c-main", { primary: true, panes: ["c2"] }), workspace_id: "c" }], { id: "c" });
    const board = buildLanes(
      [
        { workspace: a, agents: [agent("a1", "seen")], device: null },
        { workspace: b, agents: [agent("b1", "needs_you")], device: null },
        { workspace: c, agents: [agent("c1", "working", { last_activity: "0000000000009" }), agent("c2", "working")], device: null },
      ],
      "all",
    );
    expect(board.lanes.map((lane) => [lane.project.id, lane.id, lane.rank])).toEqual([
      ["b", "b-wt", "turn"],
      ["c", "c-main", "working"],
      ["c", "c-wt", "working"],
      ["a", "a-main", "resting"],
    ]);
    expect(board.empty.map((lane) => lane.id)).toEqual(["b-main"]);
  });
});

describe("the lineage mode", () => {
  it("lays a lineage left to right by depth, a second child on the next row, asking lineages first, and folds resting and cleanup lineages (B23, B25)", () => {
    const project = workspace([
      checkout("main", { primary: true, panes: ["obs", "solo", "idle"] }),
      checkout("impl", { panes: ["impl", "sub-a", "sub-b"] }),
      checkout("old", { merged: true, panes: ["old"] }),
    ]);
    const agents = [
      agent("solo", "working", { last_activity: "0000000000009" }),
      agent("obs", "working", { waiting_on_descendants: true, lineage_child_pane_ids: ["impl"] }),
      agent("impl", "needs_you", { demand: "question", lineage_parent_pane_id: "obs", lineage_child_pane_ids: ["sub-a", "sub-b"] }),
      agent("sub-a", "working", { lineage_parent_pane_id: "impl" }),
      agent("sub-b", "seen", { lineage_parent_pane_id: "impl" }),
      agent("idle", "seen"),
      agent("old", "seen"),
    ];
    const board = buildLineages(one(project, agents));
    expect(board.lineages.map((lineage) => lineage.rootPaneId)).toEqual(["obs", "solo"]);
    const first = board.lineages[0]!;
    expect(first.nodes.map((node) => [node.agent.pane_id, node.depth, node.row])).toEqual([
      ["obs", 0, 0],
      ["impl", 1, 0],
      ["sub-a", 2, 0],
      ["sub-b", 2, 1],
    ]);
    expect(board.columns).toBe(3);
    expect(board.resting.map((lineage) => lineage.rootPaneId)).toEqual(["idle"]);
    expect(board.cleanup.map((lineage) => lineage.rootPaneId)).toEqual(["old"]);
    expect(lineageAgentCount(board.resting)).toBe(1);
  });

  it("summarises a waiting parent's children as `일하는 중 N · 물음 N` (B21)", () => {
    expect(childSummary(agent("p", "working", { descendant_counts: { error: 0, approval: 1, question: 1, working: 1, done: 0 } }))).toBe("일하는 중 1 · 물음 2");
    expect(childSummary(agent("p", "working"))).toBeNull();
  });
});

describe("the way in", () => {
  it("selects the lane of the checkout in front, else main (B12, D-17)", () => {
    const project = workspace([checkout("main", { primary: true }), checkout("wt")]);
    expect(entryLane(project, "wt")).toBe("wt");
    expect(entryLane(project, "elsewhere")).toBe("main");
    expect(entryLane(project, null)).toBe("main");
  });
});
