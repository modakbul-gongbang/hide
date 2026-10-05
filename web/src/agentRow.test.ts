import { describe, expect, it } from "vitest";
import { badgeLabel, badgeParts, badgeWords, branchChip, directChildren, rowAccessibleName, lineShownAtRest, lineTone, markTone, rowLine, sectionCount, sectionTree, sidebarLine, unfoldedRows } from "./agentRow";
import { createInterfaceI18n } from "./i18n/instance";
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
    changed_at_unix_ms: null,
    emphasized: false,
    unread: false,
    demand: "none",
    activity: "working",
    ...patch,
  };
}

describe("the second line (sidebar-agent-status D-05, B7, B8)", () => {
  it("keeps a request on screen in the warning colour until it is resolved, dimmed like its mark once read", () => {
    const asking = row("q", { demand: "question", activity: "stopped", detail: "PR 병합 전 검증을 다시 돌려도 될까요?", group: "seen", unread: false });
    const line = rowLine(asking)!;
    expect(line.mode).toBe("request");
    expect(lineShownAtRest(line, false)).toBe(true);
    expect(lineTone(line, asking)).toBe("text-warning opacity-(--opacity-read-status)");
    const unread: AgentRow = { ...asking, group: "needs_you", emphasized: true, unread: true };
    expect(lineTone(line, unread)).toBe("text-warning");
    const failed = row("e", { demand: "error", detail: "빌드 실패", group: "needs_you", emphasized: true });
    expect(lineTone(rowLine(failed)!, failed)).toBe("text-destructive");
  });

  it("shows a changed row's sentence bright until the operator reads it, then only on selection", () => {
    const changed = row("w", { detail: "main 브랜치에 커밋하고 서버 시작", unread: true });
    expect(rowLine(changed)).toEqual({ text: "main 브랜치에 커밋하고 서버 시작", mode: "news" });
    expect(lineTone(rowLine(changed)!, changed)).toBe("text-foreground");
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
    expect(markTone(row("ask", { demand: "question", activity: "stopped", group: "needs_you", emphasized: true, unread: true }))).toBe("text-warning");
  });

  it("keeps a read demand's hue at reduced emphasis, so an unread one stands out", () => {
    expect(markTone(row("asked", { demand: "question", activity: "stopped", group: "seen" }))).toBe("text-warning opacity-(--opacity-read-status)");
    expect(markTone(row("approved", { demand: "approval", activity: "stopped", group: "seen" }))).toBe("text-warning opacity-(--opacity-read-status)");
    expect(markTone(row("failed", { demand: "error", activity: "stopped", group: "seen" }))).toBe("text-destructive opacity-(--opacity-read-status)");
    expect(markTone(row("blocked", { demand: "approval", activity: "stopped", group: "needs_you", emphasized: true }))).toBe("text-warning");
  });

  it("lists the badge's states worst first and leaves the zero ones out", async () => {
    const { t } = await createInterfaceI18n("en");
    const counts = { error: 0, approval: 0, question: 1, working: 1, done: 0 };
    expect(badgeParts(counts).map((part) => `${part.symbol}${part.count}`)).toEqual(["?1", "●1"]);
    expect(badgeLabel(counts, 2, t)).toBe("2 live descendants: 1 question, 1 working");
    expect(badgeLabel(undefined, 1, t)).toBe("1 live descendant");
    expect(badgeParts(undefined)).toEqual([]);
  });

  it("reads the badge in the selected language", async () => {
    const { t } = await createInterfaceI18n("ko");
    expect(badgeWords({ error: 2, approval: 0, question: 1, working: 3, done: 0, idle: 1 }, t)).toBe("오류 2개, 질문 1개, 작업 중인 에이전트 3개, 쉬는 에이전트 1개");
    expect(badgeLabel({ question: 1 }, 2, t)).toBe("활성 하위 에이전트 2개: 질문 1개");
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

describe("the sidebar row's line (sidebar-readability D-4, B4, B6)", () => {
  it("draws a request and news from the start and never a quiet sentence, read or selected", () => {
    const asking = row("q", { demand: "approval", detail: "프로덕션 배포를 승인해 주세요", unread: false });
    expect(sidebarLine(asking)).toEqual({ text: "프로덕션 배포를 승인해 주세요", mode: "request" });
    expect(sidebarLine(row("n", { detail: "hide 앱을 다시 열었고 정상 실행 중", unread: true }))?.mode).toBe("news");
    expect(sidebarLine(row("s", { detail: "hide 앱을 다시 열었고 정상 실행 중", unread: false }))).toBeNull();
    expect(sidebarLine(row("e", { detail: "   " }))).toBeNull();
  });
});

describe("the Projects lineage fold (sidebar-readability D-6, B12)", () => {
  const walk = (rows: [AgentRow, number][]) => unfoldedRows(rows.map(([agent, depth]) => ({ agent, depth }))).map((drawn) => drawn.agent.pane_id);

  it("leaves out every descendant of a folded parent, the default, and draws the next root", () => {
    expect(walk([[row("a"), 0], [row("b"), 1], [row("c"), 2], [row("d"), 0]])).toEqual(["a", "d"]);
    expect(walk([[row("a", { lineage_collapsed: true }), 0], [row("b"), 1], [row("d"), 0]])).toEqual(["a", "d"]);
  });

  it("opens one level at a time: an unfolded parent shows its children and a folded child keeps its own", () => {
    const open = { lineage_collapsed: false };
    expect(walk([[row("a", open), 0], [row("b"), 1], [row("c"), 2], [row("e", open), 1], [row("f"), 2], [row("d"), 0]])).toEqual(["a", "b", "e", "f", "d"]);
  });
});
