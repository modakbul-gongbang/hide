// The PRs lens's join of the core's board with the pull requests and what it
// says of GitHub reading (PRD work-tab-pr-lens B39, B41, B42, B43): the core
// decided the groups; this only gathers them and words how reading stands.

import { describe, expect, it } from "vitest";
import { hasPullRequests, prListModel, prReading } from "./prWork";
import type { PrBoard } from "./agentScope";
import type { GithubStatus, Workspace } from "./snapshot";

const status = (over: Partial<GithubStatus>): GithubStatus => ({ available: true, ...over }) as GithubStatus;

function project(id: string, github: Partial<GithubStatus>, board: PrBoard, numbers: number[]): Workspace {
  return {
    id,
    is_git: true,
    checkouts: [{ github: status(github) }],
    pull_requests: numbers.map((number) => ({ number, title: `PR ${number}` })),
    agent_scope: { prs: board },
  } as unknown as Workspace;
}

const row = (number: number, group: PrBoard["rows"][number]["group"], over: Partial<PrBoard["rows"][number]> = {}): PrBoard["rows"][number] => ({
  number,
  group,
  fold: null,
  reason: { kind: "merge" },
  agents: [],
  agent_pane: null,
  checkout_id: null,
  checkout_ids: [],
  issue: null,
  ...over,
});

describe("prReading", () => {
  const NOW = 10 * 60_000;

  it("blocks on a refusal of gh itself before anything was read", () => {
    expect(prReading([project("a", { failure_category: "not_logged_in" }, { rows: [], groups: [], needs_you: 0 }, [])], NOW)).toEqual({ state: "blocked", category: "not_logged_in" });
  });

  it("is reading until some project answered, then ready", () => {
    const board = { rows: [], groups: [], needs_you: 0 };
    expect(prReading([project("a", {}, board, [])], NOW)).toEqual({ state: "reading" });
    expect(prReading([project("a", { last_success_at_unix_ms: 1000 }, board, [])], NOW)).toEqual({ state: "ready" });
  });

  it("keeps the last values and says how old they are when the last read failed", () => {
    const board = { rows: [], groups: [], needs_you: 0 };
    expect(prReading([project("a", { last_success_at_unix_ms: 4 * 60_000, stale: true }, board, [])], NOW)).toEqual({ state: "stale", minutes: 6 });
  });
});

describe("hasPullRequests", () => {
  it("is false for a folder that is not Git or has no GitHub remote", () => {
    expect(hasPullRequests({ is_git: false, checkouts: [] } as unknown as Workspace)).toBe(false);
    expect(hasPullRequests({ is_git: true, checkouts: [{ github: { failure_category: "no_github_remote" } }] } as unknown as Workspace)).toBe(false);
  });
});

describe("prListModel", () => {
  const board: PrBoard = {
    rows: [row(1, "needs_you", { checkout_ids: ["c1"] }), row(2, "needs_you", { fold: "team", checkout_ids: ["c1"] }), row(3, "waiting", { checkout_ids: ["c2"] }), row(4, "merged")],
    groups: [{ group: "needs_you", numbers: [1, 2] }, { group: "waiting", numbers: [3] }, { group: "merged", numbers: [4] }],
    needs_you: 1,
  };
  const scope = [{ workspace: project("p", { last_success_at_unix_ms: 1 }, board, [1, 2, 3, 4]), agents: [], device: null }];

  it("keeps the core's groups in the screen order and folds team asks out of the rows and the count", () => {
    const model = prListModel(scope, null, 0);
    expect(model.groups.map((group) => group.group)).toEqual(["needs_you", "waiting", "merged"]);
    expect(model.groups[0]!.entries.map((entry) => entry.pr.number)).toEqual([1]);
    expect(model.groups[0]!.team.map((entry) => entry.pr.number)).toEqual([2]);
    expect(model.needsYou.map((entry) => entry.pr.number)).toEqual([1]);
    expect(model.total).toBe(4);
  });

  it("narrows to one checkout by the ids the core named", () => {
    const model = prListModel(scope, "c2", 0);
    expect(model.groups.flatMap((group) => group.entries.map((entry) => entry.pr.number))).toEqual([3]);
    expect(model.needsYou).toEqual([]);
  });

  it("throws on a core row without its pull request instead of drawing a hole", () => {
    const broken = [{ workspace: project("p", {}, board, [1]), agents: [], device: null }];
    expect(() => prListModel(broken, null, 0)).toThrow(/without its pull request/);
  });
});
