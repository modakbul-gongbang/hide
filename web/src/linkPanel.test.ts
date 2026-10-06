import { describe, expect, it } from "vitest";
import { foldLines, requestTurn, resumeBlock, resumeCheckout, sameIssue, sessionLines, spanText, viewBlock } from "./linkPanel";
import type { AgentRow, Checkout, Device, LinkedSession, Workspace } from "./snapshot";

function line(id: string, patch: Partial<LinkedSession> = {}): LinkedSession {
  return { agent: "claude", id, ids: [id], device_id: "local", role: "worked", pr: 7, request: null, started_at_unix_ms: null, ended_at_unix_ms: null, path: `/Users/example/.claude/${id}.jsonl`, cwd: "/repo/task", file: "present", parent: null, ...patch };
}

function agent(session: string, patch: Partial<AgentRow>): AgentRow {
  return { pane_id: `pane-${session}`, session_id: session, group: "idle", demand: "none", ...patch } as AgentRow;
}

function checkout(path: string, patch: Partial<Checkout> = {}): Checkout {
  return { id: path, workspace_id: "w", path, branch: "4-task", exists: true, ...patch } as Checkout;
}

const MINI = { id: "mini", label: "mini", kind: "remote", state: "ready" } as Device;

describe("the sessions section", () => {
  it("puts a session asking the operator, then one working, above the ended lines in the record's order", () => {
    const lines = sessionLines([line("a"), line("b"), line("c"), line("d", { ids: ["d0", "d"] })], [agent("c", { group: "working" }), agent("d0", { demand: "question" }), agent("a", {})]);
    expect(lines.map((entry) => [entry.line.id, entry.live?.kind ?? null])).toEqual([
      ["d", "question"],
      ["c", "working"],
      ["a", "idle"],
      ["b", null],
    ]);
  });

  it("folds six lines or more to the newest five and says how many are behind the fold", () => {
    expect(foldLines([1, 2, 3, 4, 5], false)).toEqual({ shown: [1, 2, 3, 4, 5], earlier: 0 });
    expect(foldLines([1, 2, 3, 4, 5, 6, 7], false)).toEqual({ shown: [1, 2, 3, 4, 5], earlier: 2 });
    expect(foldLines([1, 2, 3, 4, 5, 6, 7], true).shown).toHaveLength(7);
  });

  it("says why a line cannot resume, the agent first and the worktree last", () => {
    const here = checkout("/repo/task");
    expect(resumeBlock(line("a", { agent: "opencode" }), here, [])).toEqual({ key: "links.why.opencode" });
    expect(resumeBlock(line("a", { file: "missing" }), here, [])).toEqual({ key: "links.why.file" });
    expect(resumeBlock(line("a", { device_id: "mini" }), here, [{ ...MINI, state: "unavailable" }])).toEqual({ key: "links.why.deviceOffline", device: "mini" });
    expect(resumeBlock(line("a"), null, [])).toEqual({ key: "links.why.worktree" });
    expect(resumeBlock(line("a", { device_id: "mini", file: "unknown" }), here, [MINI])).toBeNull();
    expect(resumeBlock(line("a", { agent: "codex" }), here, [])).toBeNull();
  });

  it("opens this Mac's conversations only, while its file is there", () => {
    expect(viewBlock(line("a"), [])).toBeNull();
    expect(viewBlock(line("a", { file: "missing" }), [])).toEqual({ key: "links.why.file" });
    expect(viewBlock(line("a", { device_id: "mini", file: "unknown" }), [MINI])).toEqual({ key: "links.why.deviceView", device: "mini" });
  });

  it("resumes in the deepest checkout holding the line's folder on its device, else on the PR's branch", () => {
    const workspaces = [
      { id: "w", device_id: "local", checkouts: [checkout("/repo", { branch: "main" }), checkout("/repo/task"), checkout("/repo/gone", { exists: false, branch: "gone" })] },
      { id: "m", device_id: "mini", checkouts: [checkout("/srv/repo", { branch: "4-task" })] },
    ] as Workspace[];
    expect(resumeCheckout(line("a", { cwd: "/repo/task/web" }), workspaces, null)?.path).toBe("/repo/task");
    expect(resumeCheckout(line("a", { cwd: "/repo/taskforce" }), workspaces, null)?.path).toBe("/repo");
    expect(resumeCheckout(line("a", { cwd: "/elsewhere" }), workspaces, "4-task")?.path).toBe("/repo/task");
    expect(resumeCheckout(line("a", { cwd: "/elsewhere" }), workspaces, "gone")).toBeNull();
    expect(resumeCheckout(line("a", { device_id: "mini", cwd: "/repo/task" }), workspaces, "4-task")?.path).toBe("/srv/repo");
  });

  it("writes a line's time as its day and clock, the day again when it ran past midnight", () => {
    const at = (day: number, hour: number, minute: number) => new Date(2026, 9, day, hour, minute).getTime();
    expect(spanText(at(6, 13, 10), at(6, 13, 52))).toBe("10/6 13:10 - 13:52");
    expect(spanText(at(5, 23, 10), at(6, 0, 31))).toBe("10/5 23:10 - 10/6 00:31");
    expect(spanText(at(6, 9, 5), null)).toBe("10/6 09:05");
    expect(spanText(null, at(6, 9, 5))).toBeNull();
  });

  it("scrolls to the last person's turn that starts with the line's request", () => {
    const turns = [
      { role: "user", text: "PR 올려 줘" },
      { role: "assistant", text: "PR 올려 줘" },
      { role: "user", text: "PR  올려 줘\n그리고 CI도 봐 줘" },
      { role: "user", text: "고마워" },
    ];
    expect(requestTurn(turns, "PR 올려 줘")).toBe(turns[2]);
    expect(requestTurn(turns, "없는 요청")).toBeNull();
    expect(requestTurn(turns, null)).toBeNull();
  });

  it("matches a GitHub issue key whatever its case and a local key exactly", () => {
    expect(sameIssue("github:Acme/Project#3", "github:acme/project#3")).toBe(true);
    expect(sameIssue("local:/Repo#3", "local:/repo#3")).toBe(false);
  });
});
