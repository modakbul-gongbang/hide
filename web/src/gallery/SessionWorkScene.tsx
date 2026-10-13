// Production screens over invented wire values for the approved record design.
// These examples prove layout; the isolated daemon tests own runtime policy.
import { useLayoutEffect, useMemo } from "react";
import { createActions } from "../actions";
import { AgentSessions } from "../AgentSessions";
import { NewIssueDialog } from "../IssueDialogs";
import { Palette } from "../Palette";
import { SessionRecord } from "../SessionRecord";
import { TooltipProvider } from "../components/ui/tooltip";
import { useFollowInterfaceLanguage } from "../i18n/translator";
import type { SessionRecord as RecordValue, WorkSession, WorkTask } from "../snapshot";
import { useShellStore } from "../store";
import { useUiStore } from "../ui";
import { REFERENCE_FOLDS, sidebarScene } from "./sceneData";

const AT = 1_790_000_000_000;
const SESSION: WorkSession = { device_id: "local", agent: "claude", session_id: "record-example" };
const task = (id: string, chapter: string, name: string, end: WorkTask["end"], done = false): WorkTask => ({ id, chapter_id: chapter, name, end, done, at_unix_ms: AT, last_at_unix_ms: AT + 1000 });
const BASE: RecordValue = {
  session: SESSION, title: "세션 작업 기록과 검색 구현", fallback_title: "Claude", native_title: "기록 화면 구현", manual_title: null, state: "idle", last_end: "unfinished", cwd: "/work/hide", path: null, started_at_unix_ms: AT, ended_at_unix_ms: null,
  chapters: [{ id: "chapter-1", goal: "세션 작업 기록 구현", at_unix_ms: AT }, { id: "chapter-2", goal: "세션 검색과 놓친 일 정리", at_unix_ms: AT + 60000 }],
  tasks: [task("task-1", "chapter-1", "Sessions 기록 화면", "done", true), task("task-2", "chapter-1", "자식 결과물 합치기", "done"), task("task-3", "chapter-2", "⌘K 세션 검색", "unfinished"), task("task-4", "chapter-2", "이슈 미리보기와 재시도", "blocked")],
  requests: [{ offset: 1, text: "기록에 결과물과 놓친 일을 보여줘", at_unix_ms: AT, author: "operator", task_ids: ["task-1", "task-2"], chapter_id: "chapter-1", judged: true }, { offset: 2, text: "세션 검색을 연결하고 좁은 칸에서도 읽기 좋게 해줘", at_unix_ms: AT + 60000, author: "operator", task_ids: ["task-3"], chapter_id: "chapter-2", judged: true }, { offset: 3, text: "검색 결과에서 어떤 기록을 여는 거야?", at_unix_ms: AT + 61000, author: "operator", task_ids: [], chapter_id: "chapter-2", judged: false }],
  missed: [
    { id: "missed-1", kind: "leftover", text: "좁은 Tools 칸에서 긴 한국어 요청의 줄바꿈 확인", status: "open", at_unix_ms: AT, turn_offset: 1, issue_repository: null, issue_number: null, creating: false },
    { id: "missed-2", kind: "problem", text: "이슈 생성 실패 후 재시도 검증", status: "open", at_unix_ms: AT, turn_offset: 2, issue_repository: null, issue_number: null, creating: false },
    { id: "missed-3", kind: "follow_up", text: "장 제목의 여백 정리", status: "resolved", at_unix_ms: AT, turn_offset: 1, issue_repository: null, issue_number: null, creating: false },
    { id: "missed-4", kind: "follow_up", text: "이번 범위 밖의 검색 필터", status: "dismissed", at_unix_ms: AT, turn_offset: 1, issue_repository: null, issue_number: null, creating: false },
  ],
  outputs: [{ kind: "pr", repository: "example/hide", number: 121, title: "Sessions 기록 화면", url: "https://example.invalid/pull/121", state: "merged", stale: false, last_read_at_unix_ms: AT, created_at_unix_ms: AT, task_id: "task-1", source: SESSION, source_name: "세션 작업 기록과 검색 구현" }, { kind: "pr", repository: "example/hide", number: 122, title: "자식 결과물 합치기", url: "https://example.invalid/pull/122", state: "ci_failed", stale: false, last_read_at_unix_ms: AT, created_at_unix_ms: AT, task_id: "task-2", source: { ...SESSION, agent: "codex", session_id: "child-example" }, source_name: "하위 작업 검증" }],
  children: [{ session: { ...SESSION, agent: "codex", session_id: "child-example" }, title: "하위 작업 검증", fallback_title: "Codex", native_title: "하위 작업 검증", manual_title: null, state: "idle", last_end: "done", cwd: "/work/hide/task", path: null, started_at_unix_ms: AT, ended_at_unix_ms: null, chapters: [{ id: "child-chapter", goal: "자식 결과물 합치기", at_unix_ms: AT }], tasks: [task("child-task", "child-chapter", "기록 저장소 회귀 검사", "done", true)], requests: [], missed: [], outputs: [], children: [] }],
};

export function SessionWorkScene({ params }: { params: URLSearchParams }) {
  useFollowInterfaceLanguage("ko");
  const state = params.get("state") ?? "rest";
  const theme = params.get("theme") === "light" ? "light" : "dark";
  const scale = Number(params.get("scale") ?? 1);
  const fixture = useMemo(() => {
    const base = sidebarScene("reference", REFERENCE_FOLDS, AT);
    const project = base.rest.navigator!.workspaces!.find((row) => row.id === "hide")!;
    const record = structuredClone(BASE);
    record.cwd = project.checkouts[0]!.path;
    if (params.get("content") === "long") record.title += " - 긴 한국어 제목과 요청의 줄바꿈 확인";
    base.rest.status = { ...base.rest.status, background_ai: { provider: "claude", providers: [], unavailable_reason: null, enabled: state !== "off", chosen: true, agent_summary: true } };
    base.rest.navigator!.focused_workspace_id = project.id;
    base.rest.navigator!.focused_checkout_id = project.checkouts[0]!.id;
    return { base, record, project };
  }, [params, state]);
  const actions = useMemo(() => createActions((event) => {
    if (event.kind === "session_work_search") useShellStore.setState({ sessionWorkSearch: { query: String(event.payload.query), loading: false, failure: null, results: [{ session: SESSION, title: fixture.record.title, matched: "세션 검색을 연결하고 좁은 칸에서도 읽기 좋게 해줘", at_unix_ms: AT, cwd: fixture.record.cwd }] } });
    return true;
  }), [fixture]);
  useLayoutEffect(() => {
    useShellStore.setState({ rest: fixture.base.rest, agents: fixture.base.agents, connection: "live", linkSummaries: { filling:false, projects:{[fixture.project.id]:{work:[{session:SESSION,title:fixture.record.title,fallback_title:fixture.record.fallback_title,native_title:fixture.record.native_title,manual_title:null,cwd:fixture.record.cwd,ended_at_unix_ms:AT,open_missed:2,subtree_open_missed:0,missed:fixture.record.missed}]}} }, sessionRecord: { session: SESSION, loading: state === "loading", failure: state === "failed" ? "fixture_read_failed" : null, record: state === "loading" || state === "failed" ? null : fixture.record } });
    useUiStore.setState({ screen: { kind: "workspace" }, overlay: state === "search" ? "search" : "none" });
    document.documentElement.classList.toggle("dark", theme === "dark");
    document.documentElement.classList.toggle("light", theme === "light");
    document.documentElement.style.setProperty("--interface-scale", String(scale));
  }, [fixture, state, theme, scale]);
  return <TooltipProvider><div className="flex h-full min-w-0 flex-col bg-background text-foreground" data-gallery-scene="session-record">
    {state === "ended" ? <AgentSessions actions={actions} /> : <SessionRecord actions={actions} target={SESSION} />}
    {state === "draft" ? <NewIssueDialog actions={actions} workspace={fixture.project} draft={{ source: { session: SESSION, id: "missed-1" }, phase: "ready", workspace_id: fixture.project.id, repository: "example/hide", title: "좁은 Tools 칸에서 한국어 요청 줄바꿈 확인", body: "## 확인할 일\n긴 한국어 요청이 좁은 Tools 칸에서도 잘리지 않는지 확인합니다.\n\n## 완료 조건\nLight와 Dark에서 가로 넘침 없이 읽을 수 있습니다.", message: null }} onClose={() => {}} /> : null}
    {state === "search" ? <Palette actions={actions} /> : null}
  </div></TooltipProvider>;
}
