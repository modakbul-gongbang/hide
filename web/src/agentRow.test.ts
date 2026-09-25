import { describe, expect, it } from "vitest";
import { badgeLabel, badgeParts, branchChip, directChildren, rowAccessibleName, lineShownAtRest, lineTone, markTone, rowLine, sectionCount, sectionTree } from "./agentRow";
import type { AgentRow } from "./snapshot";

function row(pane: string, patch: Partial<AgentRow> = {}): AgentRow {
  return {
    id: pane,
    pane_id: pane,
    identity_label: pane,
    agent_kind: "claude",
    symbol: "●",
    group: "working",
    status_label: "Working",
    detail: null,
    elapsed: "1m",
    emphasized: false,
    unread: false,
    demand: "none",
    activity: "working",
    ...patch,
  };
}

describe("the second line (sidebar-agent-status D-05, B7, B8)", () => {
  it("keeps a request on screen in the warning colour until it is resolved, read or not", () => {
    const asking = row("q", { demand: "question", activity: "stopped", detail: "PR 병합 전 검증을 다시 돌려도 될까요?", unread: false });
    const line = rowLine(asking)!;
    expect(line.mode).toBe("request");
    expect(lineShownAtRest(line, false)).toBe(true);
    expect(lineTone(line, "question")).toBe("text-warning");
    expect(lineTone(rowLine(row("e", { demand: "error", detail: "빌드 실패" }))!, "error")).toBe("text-destructive");
  });

  it("shows a changed row's sentence bright until the operator reads it, then only on selection", () => {
    const changed = row("w", { detail: "main 브랜치에 커밋하고 서버 시작", unread: true });
    expect(rowLine(changed)).toEqual({ text: "main 브랜치에 커밋하고 서버 시작", mode: "news" });
    expect(lineTone(rowLine(changed)!, "none")).toBe("text-foreground");
    const read = rowLine({ ...changed, unread: false })!;
    expect(read.mode).toBe("quiet");
    expect(lineShownAtRest(read, false)).toBe(false);
    expect(lineShownAtRest(read, true)).toBe(true);
  });

  it("draws no second line for a row the core gave no sentence, and never a progress number", () => {
    expect(rowLine(row("s", { group: "seen", activity: "stopped", detail: null, unread: true }))).toBeNull();
    expect(rowLine(row("blank", { detail: "   " }))).toBeNull();
  });
});

describe("the mark and the badge (D-01, D-02)", () => {
  it("draws a waiting root's ring in the working colour and every other row from its own axes", () => {
    expect(markTone(row("root", { activity: "stopped", symbol: "○", waiting_on_descendants: true }))).toBe("text-agent-working");
    expect(markTone(row("idle", { activity: "stopped", symbol: "○" }))).toBe("text-subtle-foreground");
    expect(markTone(row("ask", { demand: "question", activity: "stopped" }))).toBe("text-warning");
  });

  it("lists the badge's states worst first and leaves the zero ones out", () => {
    const counts = { error: 0, approval: 0, question: 1, working: 1, done: 0 };
    expect(badgeParts(counts).map((part) => `${part.symbol}${part.count}`)).toEqual(["?1", "●1"]);
    expect(badgeLabel(counts, 2)).toBe("2 live descendants: 1 question, 1 working");
    expect(badgeParts(undefined)).toEqual([]);
  });
});

describe("the branch chip (B9)", () => {
  it("names a delegated row's checkout only when the core says it differs from its parent's", () => {
    expect(branchChip(row("child", { delegated: true, lineage_worktree_badge: "web-design-system-reset" }))).toBe("web-design-system-reset");
    expect(branchChip(row("same", { delegated: true, lineage_worktree_badge: null }))).toBeNull();
    expect(branchChip(row("root", { delegated: false, lineage_worktree_badge: "main" }))).toBeNull();
  });
});

describe("the row's accessible name", () => {
  it("reads out everything the row shows, since its visible text is hidden from assistive technology", () => {
    const child = row("child", { identity_label: "웹 디자인 시스템 리셋 구현", delegated: true, lineage_worktree_badge: "web-design-system-reset", detail: "토큰 이관 중" });
    expect(rowAccessibleName(child, "mini")).toBe("웹 디자인 시스템 리셋 구현, mini, web-design-system-reset, claude, Working, 토큰 이관 중");
  });
});

describe("the tree a group section draws (D-03, B6)", () => {
  const parent = row("p", { activity: "stopped", waiting_on_descendants: true, lineage_child_pane_ids: ["c1", "c2"], lineage_collapsed: true });
  const c1 = row("c1", { delegated: true, lineage_depth: 1, lineage_child_pane_ids: ["g"], lineage_collapsed: true });
  const c2 = row("c2", { delegated: true, lineage_depth: 1, demand: "question" });
  const g = row("g", { delegated: true, lineage_depth: 2 });
  const all = new Map([parent, c1, c2, g].map((agent) => [agent.pane_id, agent]));
  const lookup = (_device: string | null, id: string) => all.get(id);
  const none = () => 0;

  it("hides a folded parent's descendants and never lists a delegated row on its own", () => {
    const rows = sectionTree([{ agent: parent, device: null }, { agent: c1, device: null }], lookup, none);
    expect(rows.map((entry) => entry.agent.pane_id)).toEqual(["p"]);
  });

  it("draws an unfolded parent's descendants below it, deeper each level, in lineage order", () => {
    const open = new Map(all);
    open.set("p", { ...parent, lineage_collapsed: false });
    open.set("c1", { ...c1, lineage_collapsed: false });
    const rows = sectionTree([{ agent: open.get("p")!, device: null }], (_d, id) => open.get(id), none);
    expect(rows.map((entry) => [entry.agent.pane_id, entry.depth])).toEqual([
      ["p", 0],
      ["c1", 1],
      ["g", 2],
      ["c2", 1],
    ]);
  });

  it("counts every agent a heading speaks for, folded descendants included, each once", () => {
    const counts = new Map([["p", 3], ["c1", 1]]);
    const count = (_device: string | null, id: string) => counts.get(id) ?? 0;
    const other = row("o");
    const folded = sectionTree([{ agent: parent, device: null }, { agent: other, device: null }], lookup, count);
    expect(sectionCount(folded)).toBe(5);
    const open = new Map(all);
    open.set("p", { ...parent, lineage_collapsed: false });
    const unfolded = sectionTree([{ agent: open.get("p")!, device: null }, { agent: other, device: null }], (_d, id) => open.get(id), count);
    expect(sectionCount(unfolded)).toBe(5);
  });

  it("lists only the direct children still present for the popover", () => {
    const withGone = { ...parent, lineage_child_pane_ids: ["c1", "gone", "c2"] };
    expect(directChildren(withGone, (id) => all.get(id)).map((child) => child.pane_id)).toEqual(["c1", "c2"]);
  });
});
