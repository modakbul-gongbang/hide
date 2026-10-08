// Production components over fixed, invented wire values. These examples
// compare typography and geometry; runtime policy is tested in herdr-core.
import { useLayoutEffect, useMemo } from "react";
import { createActions } from "../actions";
import { AgentSessions } from "../AgentSessions";
import { TooltipProvider } from "../components/ui/tooltip";
import { PaneView } from "../PaneView";
import type { AgentRow, PaneHeader, SessionGroup, SessionTag } from "../snapshot";
import type { StatusTone, RequestVerb } from "../snapshot";
import { useShellStore } from "../store";
import { REFERENCE_FOLDS, sidebarScene } from "./sceneData";
import type { SceneParams } from "./SidebarScene";

const EXAMPLES: { title: string; group: SessionGroup; tag: SessionTag; line: string }[] = [
  { title: "입력과 세션 복귀 흐름 검토", group: "my_turn", tag: "approval", line: "검증 명령 실행 권한이 필요합니다" },
  { title: "한국어 입력 경계 검토", group: "my_turn", tag: "answer", line: "기존 stdin 종료 경로도 남길까요?" },
  { title: "자식 검토에서 답이 필요함", group: "my_turn", tag: "approval", line: "↰ 입력 경계 검토" },
  { title: "세션 상태 투영 정리", group: "review_merge", tag: "merge", line: "CI 통과 · 승인됨" },
  { title: "오래된 PR의 검토", group: "review_merge", tag: "review", line: "검토가 필요합니다" },
  { title: "세션 패널 구현", group: "in_progress", tag: "working", line: "프로젝트 범위를 연결하는 중" },
  { title: "회귀 테스트", group: "in_progress", tag: "ci_wait", line: "검증 결과를 기다리는 중" },
  { title: "쉬는 세션", group: "resting", tag: "idle", line: "" },
  { title: "완료한 검토", group: "resolved_today", tag: "result", line: "오늘 해결했습니다" },
];
const BANDS: [string, PaneHeader["band"]][] = [
  ["sleeping", { kind: "sleeping", tone: "muted", reason: "12분 동안 휴면 중", since_unix_ms: null, action: null, more: 0, exit_code: null, child_tag: null }],
  ["failed", { kind: "failed", tone: "error", reason: "세션을 찾을 수 없음", since_unix_ms: null, action: null, more: 0, exit_code: null, child_tag: null }],
  ["exit", { kind: "exit", tone: "error", reason: null, since_unix_ms: null, action: null, more: 0, exit_code: 1, child_tag: null }],
  ["device", { kind: "device_offline", tone: "muted", reason: "mini", since_unix_ms: null, action: null, more: 0, exit_code: null, child_tag: null }],
  ["approval", { kind: "approval", tone: "warning", reason: "검증 명령 실행 권한이 필요합니다", since_unix_ms: null, action: null, more: 0, exit_code: null, child_tag: null }],
  ["answer", { kind: "answer", tone: "warning", reason: "기존 stdin 종료 경로도 남길까요?", since_unix_ms: null, action: null, more: 0, exit_code: null, child_tag: null }],
  ["fix", { kind: "fix", tone: "error", reason: "verify 실패", since_unix_ms: null, action: { kind: "pr", workspace_id: "herdr-ide", number: 221, checks: "failed", tone: "error" }, more: 0, exit_code: null, child_tag: null }],
  ["merge", { kind: "merge", tone: "pr", reason: "CI 통과 · 승인됨", since_unix_ms: null, action: { kind: "pr", workspace_id: "herdr-ide", number: 221, checks: "passing", tone: "pr" }, more: 0, exit_code: null, child_tag: null }],
  ["raised", { kind: "raised_child", tone: "warning", reason: "한국어 입력 경계 검토", since_unix_ms: null, action: { kind: "child", pane_id: "a1c1", label: "한국어 입력 경계 검토" }, more: 1, exit_code: null, child_tag: "approval" }],
  ["result", { kind: "result", tone: "success", reason: "보고서가 준비됐습니다", since_unix_ms: null, action: null, more: 0, exit_code: null, child_tag: null }],
  ["working", null], ["idle", null],
];

// Frozen visual examples, not a second implementation of core judgments.
const MARKS: [string, StatusTone["kind"], RequestVerb][] = [
  ["!", "warning", "answer"], ["?", "warning", "answer"], ["!", "warning", "answer"],
  ["✓", "success", "review"], ["✓", "success", "review"], ["●", "working", "working"],
  ["○", "working", "waiting"], ["○", "subtle", "idle"], ["✓", "subtle", "result"],
];

export function SessionWorkflowScene({ scene: kind, theme, content, scale }: SceneParams & { scene: "session-panel" | "pane-header" }) {
  const actions = useMemo(() => createActions(() => true), []);
  const fixture = useMemo(() => {
    const base = sidebarScene(content, REFERENCE_FOLDS, Date.now());
    const project = base.rest.navigator!.workspaces!.find((row) => row.id === "herdr-ide")!;
    const checkout = project.checkouts[0]!;
    const pane = checkout.tabs[0]!.panes[0]!;
    const seed = base.agents.find((row) => row.pane_id === "a1")!;
    const agents: AgentRow[] = EXAMPLES.map((example, index) => {
      const [symbol, tone, verb] = MARKS[index]!;
      return {
        ...seed, id: `s${index}`, pane_id: `s${index}`, symbol, agent_kind: index % 2 ? "codex" : "claude",
        identity_label: content === "long" ? `${example.title} - 긴 한국어 제목과 변경 사항을 좁은 화면에서 확인하는 작업` : example.title,
        lineage_child_pane_ids: index === 5 ? ["a1c1", "a1c2"] : [],
        state: { ...seed.state, verb, mark_tone: { kind: tone, read: false }, chip_tone: { kind: tone, read: false }, session: { group: example.group, tag: example.tag }, request_since: Date.now() - 180_000, line: { text: example.line, mode: "request", tone: { kind: "subtle", read: false } } },
        request: { verb, verb_since_unix_ms: Date.now(), line: example.line, request: null, later_by: null, reply: null, pull_requests: [3, 4, 6].includes(index) ? [{ number: index === 4 ? 222 : 221, title: example.title, url: "https://example.invalid/pull", badge: "open", checks: index === 6 ? "pending" : "passing", review: index === 3 ? "approved" : "review_required", head_branch: "prd/session-ui", closing_issues: [], live: true, duty: true, created: true, settled_at_unix_ms: null }] : [] },
        resolved: index === 8 ? { at_unix_ms: Date.now(), source: "operator", local_date: "2026-10-08" } : null,
      };
    });
    const feature = { ...checkout, id: "session-ui", label: "prd/session-ui", branch: "prd/session-ui" };
    const scope = { ...project.agent_scope, members: agents.map((agent, index) => ({ pane_id: agent.pane_id, project_id: project.id, checkout_id: index === 5 ? checkout.id : feature.id })), work: { s3: { pull: 0, more: 0, issues: [], issue_chips: [] }, s4: { pull: 0, more: 0, issues: [], issue_chips: [] }, s5: { pull: null, more: 0, issues: ["issue-219"], issue_chips: ["issue-219"] }, s6: { pull: 0, more: 0, issues: [], issue_chips: [] } }, sessions: { closed_prs: [], counts: { my_turn: 3, review_merge: 2, in_progress: 2, resting: 1, resolved_today: 1 }, groups: [{ group: "my_turn" as const, members: [0, 1, 2] }, { group: "review_merge" as const, members: [3, 4] }, { group: "in_progress" as const, members: [5, 6] }, { group: "resting" as const, members: [7] }, { group: "resolved_today" as const, members: [8] }] } };
    project.agent_scope = scope;
    project.tasks = { source: null, overflow: false, tasks: [{ key: "issue-219", source: "github", id: `#${219}`, url: "https://example.invalid/issue/219", title: "세션 패널 구현", open: true }] };
    checkout.agent_scope = { ...scope, members: [scope.members[5]!], sessions: { closed_prs: [], counts: { my_turn: 0, review_merge: 0, in_progress: 1, resting: 0, resolved_today: 0 }, groups: [{ group: "in_progress", members: [0] }] } };
    const panes = agents.map((agent) => ({ ...pane, id: agent.pane_id, identity_label: agent.identity_label }));
    checkout.tabs = [{ ...checkout.tabs[0]!, panes: [panes[5]!] }];
    feature.tabs = [{ ...checkout.tabs[0]!, id: "session-ui-tab", panes: panes.filter((_, index) => index !== 5) }];
    project.checkouts = [checkout, feature];
    base.rest.navigator!.workspaces = [project];
    base.rest.navigator!.focused_checkout_id = checkout.id;
    base.rest.navigator!.focused_workspace_id = project.id;
    const children = base.agents.filter((row) => ["a1c1", "a1c2"].includes(row.pane_id));
    const headers = Object.fromEntries(BANDS.map(([id, band]) => [id, { working: id === "working", pull: null, band }]));
    const headerAgents = BANDS.map(([id]) => ({ ...seed, id, pane_id: id, lineage_child_pane_ids: ["a1c1", "a1c2"] }));
    base.rest.terminal = { headers };
    return { ...base, agents: [...agents, ...children, ...headerAgents], pane };
  }, [content]);
  useLayoutEffect(() => {
    useShellStore.setState({ rest: fixture.rest, agents: fixture.agents, connection: "gone" });
    document.documentElement.classList.toggle("dark", theme === "dark");
    document.documentElement.classList.toggle("light", theme === "light");
    document.documentElement.style.setProperty("--interface-scale", String(scale));
  }, [fixture, theme, scale]);
  return <TooltipProvider>{kind === "session-panel" ? <div className="flex h-full bg-background text-foreground" data-gallery-scene={kind}><AgentSessions actions={actions} /></div> : <div className="grid grid-cols-2 gap-lg bg-background p-xl text-foreground" data-gallery-scene={kind}>{BANDS.map(([id]) => <div key={id} className="h-[var(--size-pane-state-example)] min-w-0 border border-border"><PaneView pane={{ ...fixture.pane, id, identity_label: content === "long" ? "긴 한국어 제목을 좁은 pane에서 확인하는 상태 검토" : "한국어 입력 경계 검토" }} transport={undefined} focused={false} scale={1} actions={actions} agentKind="codex" markSymbol="○" markTone="text-muted-foreground" paneCount={2} zoomed={false} /></div>)}</div>}</TooltipProvider>;
}
