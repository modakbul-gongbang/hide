// The Overview's lenses (PRD overview-lenses-tiles-agents): the tiles'
// values, the checkout lanes' order, columns and folds, the lineage rows,
// and where every way in lands. The expected answers are the PRD's
// Behaviors, read against small fixtures.

import { describe, expect, it } from "vitest";
import { initializeInterfaceI18n } from "./i18n/instance";
import { ageWords, agentsTile, bucketOf, issuesTile, prsTile, scopeAgents, sessionsTile, startOfDay } from "./overviewLens";
import { buildTasks, type BoardProject, type PrBoard, type PrRow } from "./projectBoard";
import type { AgentRow, Checkout, ProjectSessions, PullRequest, SessionRow, Task, Workspace } from "./snapshot";


const NOW = new Date(2026, 8, 28, 15, 0, 0).getTime();

const t = initializeInterfaceI18n("ko").getFixedT(null, "translation");
const english = initializeInterfaceI18n("en").getFixedT(null, "translation");

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
  return { id: pane, pane_id: pane, identity_label: pane, agent_kind: "claude", symbol: "●", group, status_label: group, changed_at_unix_ms: null, emphasized: false, unread: false, demand: "none", activity: "working", last_activity: "0000000000001", ...extra };
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
    const tile = agentsTile(scopeAgents(one(project, agents)), { state: "ready" }, t);
    expect(tile.value).toBe(5);
    expect(tile.badge).toEqual({ count: 2, parts: [{ key: "question", label: "질문", count: 1 }, { key: "done", label: "끝남", count: 1 }] });
    expect(tile.bar?.map((segment) => [segment.key, segment.count])).toEqual([["turn", 2], ["working", 1], ["delegating", 1], ["resting", 1]]);
  });

  it("draws zero as zero with no badge, nothing for a device that has not answered, and ⚠ with the reason for one that cannot (B6)", () => {
    expect(agentsTile([], { state: "ready" }, t)).toMatchObject({ value: 0, badge: null, bar: [{ count: 0 }, { count: 0 }, { count: 0 }, { count: 0 }], failure: null });
    expect(agentsTile([], { state: "loading", text: "connecting" }, t)).toMatchObject({ value: null, badge: null, bar: null, failure: null });
    expect(agentsTile([], { state: "unavailable", text: "mini is unreachable", retry: "connect" }, t)).toMatchObject({ value: null, bar: null, failure: "에이전트를 읽지 못함 · mini is unreachable" });
  });

  it("counts open issues once the source answered, bars backlog, in progress and review, and keeps the last value with its age when a read fails (B3, B6)", () => {
    const project = workspace([checkout("main", { primary: true }), checkout("wip", { task: task(2).key }), checkout("rev", { task: task(3).key, pr: pr("open") })], { tasks: [task(1), task(2), task(3), task(4, false)] });
    const tile = issuesTile(buildTasks(one(project, []), "project", NOW), NOW, NOW - 3 * 60_000, "ko", t);
    expect(tile).toMatchObject({ value: 3, unit: "열림", failure: null });
    expect(tile.bar?.map((segment) => [segment.key, segment.count])).toEqual([["backlog", 1], ["working", 1], ["review", 1]]);
    const reading = issuesTile(buildTasks(one(workspace([], { tasks: [task(1)], reading: true }), []), "project", NOW), NOW, null, "ko", t);
    expect(reading).toMatchObject({ value: null, bar: null });
    const failed = issuesTile(buildTasks(one(workspace([], { tasks: [task(1)], failure: "gh auth" }), []), "project", NOW), NOW, NOW - 3 * 60_000, "ko", t);
    expect(failed.value).toBe(1);
    expect(failed.failure).toContain("3분 전");
  });

  it("counts open pull requests, badges the operator's turn with review, drafts and finished agents to look at, and bars turn, fixing and blocked (B1, B22)", () => {
    const row = (group: PrRow["group"], extra: Partial<PrRow> = {}) => ({ group, tone: "open", needsLook: false, ...extra }) as PrRow;
    const board = (rows: PrRow[], extra: Partial<PrBoard> = {}): PrBoard => ({
      groups: (["turn", "fixing", "blocked", "merged"] as const).map((group) => ({ group, rows: rows.filter((value) => value.group === group) })).filter((entry) => entry.rows.length > 0),
      open: rows.filter((value) => value.group !== "merged").length,
      reading: false,
      failure: null,
      ...extra,
    });
    const tile = prsTile(board([row("turn"), row("turn"), row("turn", { needsLook: true }), row("turn", { tone: "draft" }), row("fixing"), row("blocked"), row("merged")]), t);
    expect(tile).toMatchObject({ id: "prs", label: "PR", value: 6, unit: "열림", failure: null });
    expect(tile.badge).toEqual({
      count: 4,
      parts: [
        { key: "review", label: "리뷰", count: 2 },
        { key: "draft", label: "초안", count: 1 },
        { key: "look", label: "끝난 에이전트 확인", count: 1 },
      ],
    });
    expect(tile.bar?.map((segment) => [segment.key, segment.count])).toEqual([["turn", 4], ["fixing", 1], ["blocked", 1]]);
    expect(prsTile(board([], { open: null, reading: true }), t)).toMatchObject({ value: null, badge: null, bar: null });
    expect(prsTile(board([row("fixing")], { failure: { project: null, source: "GitHub", value: { minutes: 3 } } }), t)).toMatchObject({ value: 1, badge: null, failure: expect.stringContaining("3분 전") });
  });

  it("counts today's sessions by this machine's date, split Claude and Codex, and stays empty until this Project's history answered (B5, D-16)", () => {
    const row = (id: string, provider: string, at: number): SessionRow => ({ id, provider, provider_label: provider, locator: id, checkout_path: "/fixture", first_human_request: null, started_at_unix_ms: at, updated_at_unix_ms: at, title: null, unavailable_reason: null });
    const history = (rows: SessionRow[], extra: Partial<ProjectSessions> = {}): ProjectSessions => ({ device_id: "local", workspace_id: "project", unavailable_reason: null, loading: false, failure: null, rows, detail: null, ...extra });
    const today = startOfDay(NOW);
    const rows = [row("a", "claude", today + 1), row("b", "codex", NOW - 1), row("c", "claude", today - 1)];
    const tile = sessionsTile(history(rows), "project", NOW, t);
    expect(tile).toMatchObject({ value: 2, unit: "오늘" });
    expect(tile.bar).toEqual([{ key: "claude", label: "Claude", count: 1 }, { key: "codex", label: "Codex", count: 1 }]);
    expect(sessionsTile(history(rows), "another", NOW, t).value).toBeNull();
    expect(sessionsTile(history([], { loading: true }), "project", NOW, t).value).toBeNull();
    expect(sessionsTile(null, "project", NOW, t).value).toBeNull();
    expect(sessionsTile(history(rows, { failure: "unreadable" }), "project", NOW, t)).toMatchObject({ value: 2, failure: expect.stringContaining("마지막으로 읽은 값") });
  });
});

describe("the tiles and ages in English", () => {
  it("names the tiles, their units and the badge parts", () => {
    const project = workspace([checkout("main", { primary: true, panes: ["q", "d"] })]);
    const tile = agentsTile(scopeAgents(one(project, [agent("q", "needs_you", { demand: "question" }), agent("d", "done")])), { state: "ready" }, english);
    expect(tile.label).toBe("Agents");
    expect(tile.badge?.parts.map((part) => part.label)).toEqual(["Question", "Finished"]);
    expect(tile.bar?.map((segment) => segment.label)).toEqual(["My turn", "Working", "Waiting for children", "Resting"]);
    const issues = issuesTile(buildTasks(one(workspace([], { tasks: [task(1)] }), []), "project", NOW), NOW, null, "en", english);
    expect(issues).toMatchObject({ label: "Issues", unit: "open" });
    expect(issues.bar?.map((segment) => segment.label)).toEqual(["Backlog", "In progress", "Review"]);
    expect(sessionsTile(null, "project", NOW, english)).toMatchObject({ label: "Sessions", unit: "today" });
  });

  it("says how old a value is in the language's own relative form", () => {
    const ages = [0, 30_000, 60_000, 5 * 60_000, 60 * 60_000, 3 * 3_600_000, 86_400_000, 3 * 86_400_000];
    expect(ages.map((ms) => ageWords("en", ms, english))).toEqual(["Just now", "Just now", "1 min. ago", "5 min. ago", "1 hr. ago", "3 hr. ago", "yesterday", "3 days ago"]);
    expect(ages.map((ms) => ageWords("ko", ms, t))).toEqual(["방금", "방금", "1분 전", "5분 전", "1시간 전", "3시간 전", "어제", "3일 전"]);
  });
});
