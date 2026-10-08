// The Factory screens' gallery scene data (PRD software-factory-ui B24, D-18):
// a synthetic `FactorySummary` the real `FactoryScreen` renders, with the
// content `Screen / Factory` in design/hide-screens.pen draws. Everything here
// is invented example data: no project, title or number comes from a machine.
// An issue number is written through `issue()` because a bare hash and digits
// in web source read as a hex color to the design contract.

import type { AgentRow } from "../snapshot";
import { galleryAgentState } from "./agentStates";
import type { FactoryConfig } from "../factory/FactorySettings";
import type { CardView, Column, FactorySummary, FactoryView, InboxItem, TaskDetail, TaskState, WorkerCandidate } from "../factory/model";
import type { FactoryTab } from "../ui";
import type { SceneContent } from "./sceneData";

const MINUTE = 60_000;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;

const issue = (number: number) => `#${number}`;
const taskId = (number: number) => `t-${number}`;

type CardSpec = {
  number: number;
  title: string;
  summary?: string;
  pr?: number;
  runtime?: string;
  resume?: boolean;
  state: TaskState;
  column: Column;
  /** How long ago the Task last changed. */
  ago: number;
  needsPerson?: boolean;
  /** The display id of the predecessor a waiting Task waits on. */
  waitingFor?: string;
  unread?: boolean;
  folded?: boolean;
  failures?: number;
  priority?: number;
  external?: string[];
  /** Days a cancelled Task can still be revived. */
  reviveDays?: number;
  worker?: string;
  /** A Task before Ready shows `T-n`. */
  draft?: number;
};

function card(spec: CardSpec, now: number): CardView {
  return {
    task: spec.draft ? `t-draft-${spec.draft}` : taskId(spec.number),
    display_id: spec.draft ? `T-${spec.draft}` : issue(spec.number),
    column: spec.state === "cancelled" ? null : spec.column,
    title: spec.title,
    summary: spec.summary ?? `${spec.title}을 끝낸다.`,
    issue: spec.draft ? null : issue(spec.number),
    issue_url: spec.draft ? null : `https://example.invalid/issues/${spec.number}`,
    pr: spec.pr ? { number: spec.pr, url: `https://example.invalid/pull/${spec.pr}`, head: "example", by_factory: spec.state !== "outside", open: spec.state !== "done" } : null,
    worker_runtime: spec.runtime ?? (spec.worker ? "codex" : null),
    resume_at: spec.resume ? now + 50 * MINUTE : null,
    waiting_group: spec.column === "stuck" ? spec.needsPerson ? "person" : "other" : null,
    stage: spec.state === "done" ? 4 : ["outside", "merge_waiting", "landed"].includes(spec.state) ? 3 : ["verifying", "stopped"].includes(spec.state) ? 2 : spec.state === "drafting" || (spec.state === "waiting" && !spec.resume) ? 0 : 1,
    state: spec.state,
    state_label: spec.state,
    needs_person: spec.needsPerson ?? false,
    waiting_for: spec.waitingFor ?? null,
    waiting_code: spec.waitingFor ? "predecessors" : spec.state === "waiting" ? "slot" : null,
    waiting_on: spec.waitingFor ? [spec.waitingFor] : [],
    env_hold: null,
    stop: spec.state === "stopped" ? "verify_failed" : null,
    priority: spec.priority ?? 0,
    since: now - spec.ago,
    unread: spec.unread ?? false,
    folded: spec.folded ?? false,
    archived: false,
    failures: spec.failures ?? 0,
    external: spec.external ?? [],
    revive_until: spec.reviveDays === undefined ? null : now + spec.reviveDays * DAY,
    worker_pane: spec.worker ?? null,
    worker_label: spec.worker ? (spec.runtime === "claude" ? "Claude Code" : "Codex") : null,
    pause_reason: null,
  };
}

/** The long set: a long Korean title and a long issue number, to try the cards' and rows' wrapping. */
const LONG_TITLE = "Task 상세 API 응답 형식을 정하고 기존 snapshot 필드와 겹치는 이름을 정리하면서 한글과 English가 섞인 아주 긴 제목이 카드 안에서 두 줄로 줄어드는지 확인하는 작업";

function herdrSpecs(content: SceneContent): CardSpec[] {
  const long = content === "long";
  return [
    { draft: 7, number: 0, summary: "설정 페이지에서 알림 항목을 한 곳에 모은다", title: "알림 설정 화면 정리", state: "drafting", column: "before", ago: 12 * MINUTE },
    { number: 421, summary: "Task 상세 패널에 목표와 진행 상황을 보인다", title: "Task 상세 화면", state: "waiting", column: "before", ago: 3 * HOUR, waitingFor: issue(420) },
    { number: 422, summary: "보드 카드를 누르면 Task 상세를 연다", title: "보드에서 Task 상세 패널 열기", state: "waiting", column: "before", ago: 3 * HOUR, waitingFor: issue(421) },
    { number: 431, summary: "docs/ 안의 깨진 링크를 고친다", title: "문서 깨진 링크 정리", state: "waiting", column: "before", ago: 5 * HOUR, priority: 1 },
    { number: long ? 123456 : 420, summary: "Task 상세를 읽는 API 형식을 정한다", title: long ? LONG_TITLE : "Task 상세 API 응답 형식", state: "blocked", column: "stuck", worker: "worker-420", runtime: "claude", ago: 3 * DAY, needsPerson: true },
    { number: 412, summary: "Issues 보드에서 정렬 기준을 고르게 한다", title: long ? "FactorySnapshotDependencyGraphProjectionWithAnUnbrokenIdentifierThatMustWrapWithoutOverflowAtEveryCardWidth" : "Issues 보드에 정렬 추가", state: "running", column: "moving", ago: 2 * MINUTE, worker: "worker-412", pr: 563, failures: 1 },
    { number: 415, summary: "마지막 정렬 기준을 다음 실행에도 유지한다", title: "보드 정렬 상태 기억", state: "running", column: "moving", ago: MINUTE, worker: "worker-415", pr: 564, runtime: "claude" },
    { number: 417, summary: "디스크 정리 표를 크기순으로 다시 그린다", title: "디스크 정리 표 다시 그리기", state: "stopped", column: "stuck", worker: "worker-417", runtime: "claude", pr: 560, ago: 40 * MINUTE, needsPerson: true, failures: 3 },
    { number: 405, summary: "hide-ai가 하루에 호출하는 횟수를 제한한다", title: "hide-ai 호출 상한 조정", state: "merge_waiting", column: "stuck", pr: 561, worker: "worker-405", runtime: "claude", ago: HOUR, needsPerson: true },
    { number: 426, title: "단축키 도움말 시트", summary: "⌘/로 여는 단축키 목록 시트를 만든다", state: "waiting", column: "stuck", ago: 50 * MINUTE, resume: true },
    { number: 398, summary: "Sessions 검색의 첫 응답 시간을 줄인다", title: "Sessions 검색 속도 개선", state: "verifying", column: "moving", ago: 20 * MINUTE, pr: 552, worker: "worker-398", runtime: "claude" },
    { number: 430, summary: "pane-focus e2e가 가끔 실패하는 원인을 찾는다", title: "flaky: pane-focus 테스트", state: "outside", column: "stuck", pr: 566, ago: 6 * MINUTE, external: ["sasu/gate-cache"] },
    { number: 410, pr: 558, summary: "tasks.rs가 updated_at으로 정렬할 수 있게 한다", title: "정렬 API: tasks.rs에 updated_at", state: "done", column: "done", ago: 2 * HOUR, unread: true },
    { number: 409, pr: 557, summary: "정렬 키 세 개를 docs/factory.md에 적는다", title: "Task 목록 정렬 키 문서화", state: "done", column: "done", ago: 5 * HOUR },
    { number: 401, title: "Task 저장소를 별도 crate로", state: "done", column: "done", ago: 4 * DAY, folded: true },
    { number: 399, title: "Factory 설정 기본값 정리", state: "done", column: "done", ago: 6 * DAY, folded: true },
  ];
}

function sasuSpecs(): CardSpec[] {
  return [
    { number: 88, title: "gate 결과 요약 보기", state: "running", column: "moving", ago: 8 * MINUTE, needsPerson: false, worker: "worker-88" },
    { number: 91, title: "implement 단계 로그 정리", state: "waiting", column: "before", ago: 2 * HOUR },
    { number: 86, title: "verify 리포트 한 줄 요약", state: "done", column: "done", ago: 50 * MINUTE },
  ];
}

const COLUMN_LABEL: Record<Column, string> = { before: "시작 전", moving: "진행 중", stuck: "멈춤", done: "완료" };
const COLUMN_ORDER: Column[] = ["before", "moving", "stuck", "done"];

function view(options: {
  id: string;
  project: string;
  name: string;
  source: string;
  verification: string;
  cards: CardView[];
  cancelled: CardView[];
  dependencies: [string, string][];
  reduced: [string, string][];
  now: number;
  readAgo: number | null;
}): FactoryView {
  const { cards, now } = options;
  const columns = COLUMN_ORDER.map((column) => ({
    column,
    label: COLUMN_LABEL[column],
    // The person's cards first, longest waiting first; then the engine's priority order.
    cards: cards
      .filter((value) => value.column === column)
      .sort((a, b) => Number(b.needs_person) - Number(a.needs_person) || Number(b.state === "stopped") - Number(a.state === "stopped") || (a.needs_person ? a.since - b.since : b.priority - a.priority || a.since - b.since)),
  }));
  const count = (column: Column) => columns.find((value) => value.column === column)!.cards.filter((value) => !value.folded).length;
  const linked = new Set(options.dependencies.flat());
  return {
    id: options.id,
    project: options.project,
    project_name: options.name,
    source: options.source,
    verification: options.verification,
    closed: false,
    flow: {
      before: count("before"),
      stuck: count("stuck"),
      moving: count("moving"),
      done_today: cards.filter((value) => value.column === "done" && now - value.since < DAY).length,
    },
    my_turn: cards.filter((value) => value.needs_person).length,
    columns,
    cancelled: options.cancelled,
    graph: { nodes: cards.filter((value) => linked.has(value.task)).map((value) => value.task), edges: options.reduced, unrelated: cards.filter((value) => !linked.has(value.task)).map((value) => value.task) },
    dependencies: options.dependencies,
    outside_read_at: options.readAgo === null ? null : now - options.readAgo,
    stale: false,
    main_broken: false,
    auto_merge_available: options.verification !== "none",
    merge_mode: options.verification !== "none" ? "auto" : "manual",
    paused: false,
    notices: 0,
    observer_mode: "assist",
    observer_today: 37,
    observer_limit: 100,
    factory_ai: null,
    workers: [{ agent: "codex", model: "gpt-6.1-sol", effort: "high", description: "대부분의 Task" }],
    macos_notifications: false,
  };
}

function inboxItem(item: Partial<InboxItem> & Pick<InboxItem, "group" | "kind" | "factory" | "task" | "display_id" | "title" | "project" | "text">, now: number, ago: number): InboxItem {
  return {
    rank: 0,
    question: null,
    suggestion: "",
    result: "",
    default_action: null,
    choices: [],
    deadline: null,
    remaining: null,
    remaining_hours: null,
    waiting_since: now - ago,
    waiting_days: Math.floor(ago / DAY),
    result_code: "acknowledge",
    unblocks: [],
    gates: [],
    stop: null,
    notice: null,
    refers_to: null,
    decision_kind: null,
    observer_reason: null,
    overridable: false,
    ...item,
  };
}

export type FactorySceneFixture = {
  summary: FactorySummary;
  workers: AgentRow[];
  /** A Task page's detail by Task id, for the page the scene opens. */
  detail: (task: string) => TaskDetail | null;
  config: { config: FactoryConfig; machine: { max_workers: number } };
};

/**
 * The Observer set (PRD factory-observer): herdr-ide answers with 함께 and
 * sasu, paused, with 맡김; 내 차례 holds the requests Factory AI left to the
 * person, the stops it diagnosed or could not, and its notices, as the
 * `fx-obs-*` frames of `Screen / Factory` draw them.
 */
function observerScene(base: FactorySceneFixture, now: number, variant: ObserverVariant | null): FactorySceneFixture {
  const [herdr0, sasu0] = base.summary.factories as [FactoryView, FactoryView];
  const stuck = (spec: CardSpec, patch: Partial<CardView> = {}) => ({ ...card(spec, now), ...patch });
  const t436 = stuck({ number: 436, title: "보드 빈 열 문구", summary: "빈 열에 보일 한 줄 문구를 정한다", state: "blocked", column: "stuck", worker: "worker-436", runtime: "claude", ago: 6 * MINUTE, needsPerson: true });
  const t437 = stuck({ number: 437, title: "정렬 상태 기억", summary: "고른 정렬을 다시 열어도 그대로 둔다", state: "blocked", column: "stuck", worker: "worker-437", ago: 30 * MINUTE, needsPerson: true });
  const t435 = stuck({ number: 435, title: "Sessions 칩 정렬", summary: "Sessions 칩을 최근 활동순으로 놓는다", state: "stopped", column: "stuck", worker: "worker-435", ago: 9 * MINUTE, needsPerson: true }, { stop: "no_report", worker_runtime: "codex" });
  const t433 = stuck({ number: 433, title: "설정 검색 결과 강조", summary: "설정 검색에서 맞은 글자를 굵게 보인다", state: "stopped", column: "stuck", ago: 18 * MINUTE, needsPerson: true, runtime: "claude" }, { stop: "worker_gone", worker_runtime: "claude" });
  const t434 = stuck({ number: 434, title: "빈 Factory 안내 문구", summary: "Task가 없을 때 넣는 방법을 한 줄로 안내한다", state: "paused", column: "stuck", ago: 25 * MINUTE }, { pause_reason: "pane_closed", worker_runtime: "codex" });
  const added = [t436, t437, t435, t433, t434];
  const columns = herdr0.columns.map((column) => (column.column === "stuck" ? { ...column, cards: [...added, ...column.cards] } : column));
  // 직접 keeps one candidate; 맡김 fills all five and has used the whole day.
  const three = base.config.config.workers;
  const workers = variant === "direct" ? three.slice(0, 1) : variant === "auto" ? [...three, ...MORE_WORKERS] : three;
  const observer_mode: FactoryConfig["observer_mode"] = variant === "direct" ? "manual" : variant === "auto" ? "autonomous" : "assist";
  const herdr: FactoryView = { ...herdr0, columns, flow: { ...herdr0.flow, stuck: herdr0.flow.stuck + added.length }, observer_mode, observer_today: variant === "auto" ? 100 : 37, workers, macos_notifications: true };
  const sasu: FactoryView = { ...sasu0, paused: true, observer_mode: "autonomous", observer_today: 100, workers: workers.slice(0, 1) };
  const item = (patch: Partial<InboxItem> & Pick<InboxItem, "group" | "kind" | "factory" | "task" | "display_id" | "title" | "project" | "text">, ago: number) => inboxItem(patch, now, ago);
  const answers: InboxItem[] = [
    item({ group: "answer", kind: "blocking", factory: herdr.id, task: t436.task, display_id: t436.display_id, title: t436.title, project: "herdr-ide", question: "q-436", text: "빈 열에 무엇을 보일까요?", suggestion: "아무것도 보이지 않기", choices: ["\"없음\" 한 단어", "열마다 다른 안내"], result_code: "wake_worker", decision_kind: "C", observer_reason: "작업자는 첫 안을 추천합니다." }, 6 * MINUTE),
    item({ group: "answer", kind: "blocking", factory: herdr.id, task: t437.task, display_id: t437.display_id, title: t437.title, project: "herdr-ide", question: "q-437", text: `카드가 틀림: 완료 조건이 ${issue(412)}와 겹칩니다`, suggestion: `AI 제안: ${issue(412)}에 합치기`, choices: ["그대로 진행"], result_code: "wake_worker", decision_kind: "E" }, 30 * MINUTE),
    item({ group: "answer", kind: "default", factory: sasu.id, task: taskId(88), display_id: issue(88), title: "gate 결과 요약 보기", project: "sasu", question: "q-88", text: "gate 요약을 PR 댓글로도 올릴까요?", suggestion: "올리지 않기", choices: ["PR 댓글로 올리기"], default_action: "올리지 않기", deadline: now + 5 * HOUR, result_code: "wake_worker", decision_kind: "D", observer_reason: "밖에 글을 씁니다." }, 20 * MINUTE),
  ];
  const merge = base.summary.inbox.filter((row) => row.group === "merge").map((row) => ({ ...row, gates: ["risk_path" as const] }));
  const stops: InboxItem[] = [
    item({ group: "stopped", kind: "stopped", factory: herdr.id, task: t435.task, display_id: t435.display_id, title: t435.title, project: "herdr-ide", text: "멈춤: 보고 없음", suggestion: "retry", choices: ["cancel"], result_code: "restart_worker", stop: "no_report", observer_reason: "테스트 실행을 기다리다 멈춘 것으로 보입니다" }, 9 * MINUTE),
    item({ group: "stopped", kind: "stopped", factory: herdr.id, task: t433.task, display_id: t433.display_id, title: t433.title, project: "herdr-ide", text: "멈춤: 작업자 사라짐", suggestion: "retry", choices: ["cancel"], result_code: "restart_worker", stop: "worker_gone" }, 18 * MINUTE),
    item({ group: "stopped", kind: "paused", factory: herdr.id, task: t434.task, display_id: t434.display_id, title: t434.title, project: "herdr-ide", text: "일시정지: 작업자 pane을 닫음", suggestion: "resume", choices: ["cancel"], result_code: "resume_worker" }, 25 * MINUTE),
  ];
  const notices: InboxItem[] = [
    item({ group: "notice", kind: "notice", factory: herdr.id, task: taskId(412), display_id: issue(412), title: "Issues 보드에 정렬 추가", project: "herdr-ide", question: "n-412", text: "정렬 키는? → updated_at", choices: ["ok"], notice: "ai_answered", refers_to: "q-412", decision_kind: "B", overridable: true }, 12 * MINUTE),
    item({ group: "notice", kind: "notice", factory: sasu.id, task: taskId(86), display_id: issue(86), title: "verify 리포트 한 줄 요약", project: "sasu", question: "n-86", text: `${issue(86)}: 검증 통과, 위험 경로만 걸림`, choices: ["ok"], notice: "ai_risk_merge" }, 50 * MINUTE),
    item({ group: "notice", kind: "notice", factory: sasu.id, task: taskId(91), display_id: issue(91), title: "implement 단계 로그 정리", project: "sasu", question: "n-91", text: `${issue(91)} → ${issue(92)}: 로그 형식 정하기`, choices: ["ok"], notice: "ai_new_task", decision_kind: "E" }, HOUR),
    item({ group: "notice", kind: "notice", factory: sasu.id, task: taskId(88), display_id: issue(88), title: "gate 결과 요약 보기", project: "sasu", question: "n-88", text: "오늘 AI 판단 100번을 다 써서 남은 결정은 나에게 옵니다.", choices: ["ok"], notice: "daily_limit" }, 20 * MINUTE),
  ];
  const inbox = [...answers, ...merge, ...stops, ...notices].map((row, rank) => ({ ...row, rank }));
  const turn = inbox.length - notices.length;
  const summary: FactorySummary = { my_turn: turn, notices: notices.length, factories: [herdr, sasu], inbox };
  const detail = (task: string): TaskDetail | null => {
    if (task === t435.task) {
      const at = now - 12 * MINUTE;
      return {
        ...base.detail(taskId(412))!,
        card: t435,
        goal: "Sessions 칩이 최근 활동순으로 놓인다.",
        criteria: ["칩이 마지막 활동 시각 내림차순으로 놓인다", "칩은 다섯 개까지 보이고 나머지는 +n으로 접힌다"],
        out_of_scope: [],
        pr: null,
        attempts: [],
        allowed: ["retry", "cancel"],
        stop_code: "no_report",
        verification: "0/3",
        resting_since: now - 2 * MINUTE,
        branch: "435-session-chips",
        worker_name: "t-435-worker",
        diagnosis: "테스트 실행을 기다리다 멈춘 것으로 보입니다",
        worker: { agent: "codex", label: "Codex", model: "gpt-6.1-luna", effort: "low", picked: "문구, 문서, 작은 UI", pick_reason: "칩 정렬만 바꾸는 작은 UI 변경" },
        woke_at: now - 6 * MINUTE,
        diagnosed_at: now - 4 * MINUTE,
        diagnosed_from: "screen",
        questions: [{ id: "q-sort", origin: "worker", kind: { kind: "default" }, text: "칩 정렬 키?", suggestion: "last_activity", default_action: null, deadline: null, asked_at: at - 5_000, choices: [], answer: { text: "last_activity", chose: null, relayed_by: "observer", at }, letter: null, routing: { kind: "B" } }],
        decisions: [
          { text: "정렬은 web 쪽에서 한다", by: "worker:t-435", at: now - 2 * HOUR },
          { text: "뒤집음: 칩 최대 개수 -> 5", by: "operator", at: now - HOUR },
          { text: "칩 정렬 키는 last_activity로 둔다", by: "observer", at, kind: "B" },
        ],
      };
    }
    if (task === taskId(431)) {
      return { ...base.detail(task)!, goal: "docs 안의 상대 링크가 모두 열린다.", criteria: ["깨진 상대 링크 23개가 맞는 문서를 가리킨다", "check-doc-links가 docs 전체에서 통과한다"], branch: "431-doc-links", ai_picked_worker: 3, ai_pick_reason: "문서 링크만 고치는 작은 변경" };
    }
    return base.detail(task);
  };
  // The values the 고급 설정 frame draws.
  const config = {
    config: { ...base.config.config, workers, observer_mode, macos_notifications: true, risk_paths: ["hided/", "herdr-core/"], no_report_ms: 2 * MINUTE, watch_interval_ms: 30 * MINUTE, watch_daily_limit: 5, recovery: [], worker_args: { claude: ["--permission-mode", "acceptEdits"] } },
    machine: { max_workers: 5 },
  };
  // The workers still in a pane, each with the line it last reported.
  const template = base.workers.find((row) => row.request)!;
  const panes: AgentRow[] = [[t436, "세 안을 화면에 그려 두고 답을 기다림"], [t437, "두 Task의 완료 조건을 비교하고 답을 기다림"], [t435, "테스트 실행을 기다리는 중"]].map(([task, line]) => {
    const view = task as CardView;
    const pane = view.worker_pane!;
    return { ...template, state: galleryAgentState(pane, null, view.since), id: pane, pane_id: pane, identity_label: pane, agent_kind: view.worker_runtime ?? "codex", activity: "idle", symbol: "○", status_code: "idle", lineage_child_pane_ids: [], close_descendant_pane_ids: [], request: { ...template.request!, line: line as string } };
  });
  return { ...base, summary, detail, config, workers: [...base.workers, ...panes] };
}

/** The two candidates 맡김's frame adds to the three: one more model, and one left on its CLI's defaults. */
const MORE_WORKERS: WorkerCandidate[] = [
  { agent: "claude", model: "sonnet", effort: "medium", description: "테스트만 고치는 Task" },
  { agent: "codex", model: null, effort: null, description: "실험, 버려도 되는 시도" },
];

export type ObserverVariant = "direct" | "auto";

/**
 * The `fx-obs-*` frames as scene states: which tab, Factory or Task each
 * opens, and what differs from the 함께 set. `cards` draws the five Observer
 * cards at the three sizes; `aiOff` turns Hide AI off.
 */
export const OBSERVER_STATES: Record<string, { tab: FactoryTab; factory?: string; task?: string; variant?: ObserverVariant; aiOff?: true; cards?: true }> = {
  "obs-turn": { tab: "turn" },
  "obs-set": { tab: "settings", factory: "f-herdr-ide" },
  "obs-direct": { tab: "settings", factory: "f-herdr-ide", variant: "direct" },
  "obs-auto": { tab: "settings", factory: "f-herdr-ide", variant: "auto" },
  "obs-off": { tab: "settings", factory: "f-herdr-ide", aiOff: true },
  "obs-all": { tab: "settings" },
  "obs-paused": { tab: "turn", factory: "f-sasu" },
  "obs-task": { tab: "turn", task: taskId(435) },
  "obs-pick": { tab: "turn", task: taskId(431) },
  "obs-cards": { tab: "turn", cards: true },
};

/** The scene's summary, the Task pages it can open and the settings it answers with. */
export function factoryScene(content: SceneContent, now: number, observer = false, variant: ObserverVariant | null = null): FactorySceneFixture {
  const fixture = referenceScene(content, now);
  return observer ? observerScene(fixture, now, variant) : fixture;
}

function referenceScene(content: SceneContent, now: number): FactorySceneFixture {
  const long = content === "long";
  const herdrCards = herdrSpecs(content).map((spec) => card(spec, now));
  const blocked = herdrCards.find((value) => value.state === "blocked")!;
  const cancelled = [
    card({ number: 433, title: "오래된 알림 제거", state: "cancelled", column: "done", ago: 2 * DAY, reviveDays: 5 }, now),
    card({ number: 434, title: "아이콘 세트 교체 시도", state: "cancelled", column: "done", ago: 6 * DAY, reviveDays: 1 }, now),
  ];
  // The chain 420 -> 421 -> 422 with 420 -> 422 as well: the graph draws no arrow for the longer way round.
  const A = blocked.task;
  const B = taskId(421);
  const C = taskId(422);
  const dependencies: [string, string][] = [
    [taskId(401), A],
    [A, B],
    [B, C],
    [A, C],
    [taskId(410), taskId(412)],
    [taskId(410), taskId(415)],
    [taskId(412), C],
  ];
  const reduced = dependencies.filter(([from, to]) => !(from === A && to === C));
  const herdr = view({
    id: "f-herdr-ide",
    project: "/work/herdr-ide",
    name: "herdr-ide",
    source: "github",
    verification: "ci",
    cards: herdrCards,
    cancelled,
    dependencies,
    reduced,
    now,
    readAgo: 3 * MINUTE,
  });
  const sasuCards = sasuSpecs().map((spec) => card(spec, now));
  const sasu = view({ id: "f-sasu", project: "/work/sasu", name: "sasu", source: "local", verification: "verify", cards: sasuCards, cancelled: [], dependencies: [], reduced: [], now, readAgo: null });

  const inbox: InboxItem[] = [
    inboxItem(
      {
        group: "answer",
        kind: "blocking",
        factory: herdr.id,
        task: blocked.task,
        display_id: blocked.display_id,
        title: long ? LONG_TITLE : "Task 상세를 새 REST 엔드포인트로 낼까요, 기존 WS snapshot에 합칠까요?",
        project: "herdr-ide",
        question: "q-blocking",
        text: "Task 상세를 새 REST 엔드포인트로 낼까요, 기존 WS snapshot에 합칠까요?",
        suggestion: "WS snapshot에 합치기",
        choices: ["REST 엔드포인트"],
        result_code: "wake_worker",
        unblocks: [issue(421)],
      },
      now,
      3 * DAY,
    ),
    inboxItem(
      {
        group: "answer",
        kind: "default",
        factory: herdr.id,
        task: taskId(412),
        display_id: issue(412),
        title: "보드 정렬 기본값은 무엇으로?",
        project: "herdr-ide",
        question: "q-default",
        text: "처음 열 때 어떤 정렬로 보일지 정해야 합니다.",
        suggestion: "최근 수정순",
        choices: ["번호순", "만든 순"],
        default_action: "최근 수정순",
        deadline: now + 21 * HOUR,
        result_code: "apply_or_merge",
      },
      now,
      3 * HOUR,
    ),
    inboxItem(
      {
        group: "answer",
        kind: "default",
        factory: sasu.id,
        task: taskId(88),
        display_id: issue(88),
        title: "요약은 gate마다 한 줄? 실패만?",
        project: "sasu",
        question: "q-sasu",
        text: "gate가 열두 개라 모두 적으면 긴 줄이 됩니다.",
        suggestion: "실패만",
        choices: ["gate마다 한 줄"],
        default_action: "실패만",
        deadline: now + 5 * HOUR,
        result_code: "apply_or_merge",
      },
      now,
      5 * HOUR,
    ),
    inboxItem(
      {
        group: "merge",
        kind: "merge",
        factory: herdr.id,
        task: taskId(405),
        display_id: issue(405),
        title: `PR ${issue(561)} 머지`,
        project: "herdr-ide",
        text: "검증을 통과했고 manual 머지 대기입니다.",
        suggestion: "merge",
        choices: ["request-changes", "cancel"],
        result_code: "merge",
        unblocks: [issue(405)],
        gates: ["manual_mode", "risk_path"],
      },
      now,
      HOUR,
    ),
    inboxItem(
      {
        group: "stopped",
        kind: "stopped",
        factory: herdr.id,
        task: taskId(417),
        display_id: issue(417),
        title: "검증 3회 실패로 멈춤",
        project: "herdr-ide",
        text: "web-e2e가 세 번 연속 실패했습니다.",
        suggestion: "retry",
        choices: ["cancel"],
        result_code: "restart_worker",
        stop: "verify_failed",
      },
      now,
      40 * MINUTE,
    ),
    inboxItem({ group: "notice", kind: "notice", factory: herdr.id, task: taskId(398), display_id: issue(398), title: "main 깨짐 → revert 됨", project: "herdr-ide", text: "main이 깨져 마지막 머지를 되돌렸습니다." }, now, 25 * MINUTE),
    inboxItem({ group: "notice", kind: "notice", factory: herdr.id, task: taskId(415), display_id: issue(415), title: `${issue(412)}와 같은 정렬을 다르게 푸는 중`, project: "herdr-ide", text: "두 Task가 같은 정렬 상태를 서로 다르게 바꾸고 있습니다." }, now, 10 * MINUTE),
  ].map((item, rank) => ({ ...item, rank }));
  const notices = inbox.filter((item) => item.group === "notice").length;
  const summary: FactorySummary = { my_turn: inbox.length - notices, notices, factories: [herdr, sasu], inbox };

  const detail = (task: string): TaskDetail | null => {
    const found = [...herdr.columns, ...sasu.columns].flatMap((column) => column.cards).find((value) => value.task === task) ?? [...herdr.cancelled, ...sasu.cancelled].find((value) => value.task === task);
    if (!found) return null;
    const factory = herdr.columns.some((column) => column.cards.includes(found)) || herdr.cancelled.includes(found) ? herdr : sasu;
    const running = found.task === taskId(412);
    return {
      card: found,
      factory: factory.id,
      project: factory.project,
      goal: running ? "Issues 보드에서 정렬을 고를 수 있다." : `${found.title}을 끝낸다.`,
      criteria: running ? ["정렬 메뉴에서 최근 수정순 · 번호순 · 만든 순을 고른다", "고른 정렬로 다섯 열의 카드 순서가 바뀐다", "web e2e가 세 정렬을 모두 확인한다"] : ["변경이 테스트로 확인된다"],
      out_of_scope: running ? ["List view 정렬", `정렬 상태 저장 (${issue(415)})`] : [],
      before: [],
      after: [],
      attachments: [],
      pr: running ? { number: 563, url: "https://example.invalid/pull/563", head: "412-board-sort", by_factory: true, open: true } : null,
      verification: running ? "1/3" : "0/3",
      attempts: running ? [{ number: 1, stage: "task", started_at: now - 30 * MINUTE, outcome: "failed", check: "web-e2e", link: "https://example.invalid/runs/1", log_tail: "1 failed: sort menu keeps the previous order" }] : [],
      decisions: running
        ? [
            { text: "정렬 값은 URL이 아니라 ui_state에 둔다", by: "worker", at: now - 2 * HOUR },
            { text: "보드 e2e fixture 정렬 고정", by: "worker", at: now - HOUR },
          ]
        : [],
      questions: found.needs_person && found.state === "blocked"
        ? [{ id: "q-blocking", origin: "worker", kind: { kind: "blocking" }, text: inbox[0]!.text, suggestion: "WS snapshot에 합치기", default_action: null, deadline: null, asked_at: now - 3 * DAY, choices: ["REST 엔드포인트"], answer: null, letter: null }]
        : running
          ? [{ id: "q-default", origin: "worker", kind: { kind: "default" }, text: "보드를 처음 열 때 정렬 기본값은?", suggestion: "최근 수정순", default_action: "최근 수정순으로 진행", deadline: now + 21 * HOUR, asked_at: now - 3 * HOUR, choices: ["번호순", "만든 순"], answer: null, letter: null }]
          : [],
      discoveries: [],
      gates: [],
      gate_codes: found.state === "merge_waiting" ? ["manual_mode", "risk_path"] : [],
      allowed: running ? ["pause", "cancel"] : found.state === "merge_waiting" ? ["merge", "request-changes", "cancel"] : found.state === "stopped" ? ["retry", "cancel"] : found.state === "waiting" ? ["priority", "cancel"] : [],
      stop: null,
      stop_code: found.state === "stopped" ? "verify_failed" : null,
      merge_sha: null,
      worker_name: found.worker_pane ? `${found.task}-worker` : null,
      worktree: found.worker_pane ? `/work/herdr-ide.worktrees/${found.task}` : null,
      branch: running ? "412-board-sort" : null,
      worker: null,
      diagnosis: null,
      auto_restarts: 0,
      resting_since: null,
      pinned_worker: null,
      ai_picked_worker: null,
      ai_pick_reason: null,
      woke_at: null,
      diagnosed_at: null,
      diagnosed_from: null,
    };
  };

  const config: FactoryConfig = {
    verification: { kind: "ci", checks: ["web-e2e", "rust-test"] },
    merge_mode: "auto",
    merge_method: "squash",
    quick_check: "scripts/verify-web.sh",
    question_deadline_ms: 24 * HOUR,
    stall_ms: 30 * MINUTE,
    no_report_ms: 20 * MINUTE,
    watch_interval_ms: 15 * MINUTE,
    watch_daily_limit: 8,
    outside_read_ms: 5 * MINUTE,
    cancel_keep_ms: 7 * DAY,
    done_fold_ms: 3 * DAY,
    archive_fold_ms: 90 * DAY,
    new_task_limit: 3,
    verify_failure_limit: 3,
    verify_timeout_ms: 30 * MINUTE,
    disk_floor_bytes: 5 * 1024 ** 3,
    default_runtime: "codex",
    workers: [
      { agent: "codex", model: "gpt-6.1-sol", effort: "high", description: "대부분의 Task" },
      { agent: "claude", model: "opus", effort: "max", description: "herdr-core, 동시성, 큰 리팩터" },
      { agent: "codex", model: "gpt-6.1-luna", effort: "low", description: "문구, 문서, 작은 UI" },
    ],
    observer_mode: "assist",
    observer_daily_limit: 100,
    factory_ai: { provider: "claude", model: "sonnet", effort: "low" },
    harness: null,
    autonomy: [{ id: "rename_branch", description: "branch 이름을 Task 번호에 맞춘다", enabled: true }],
    autonomy_diff_limit: 200,
    recovery: ["remove_finished_worktrees", "restart_worker"],
    risk_paths: ["herdr-core/src/runtime"],
    checks: [{ at: "after_done", instruction: "변경이 요구한 범위 밖으로 번지지 않았는지 본다" }],
    prd_in_issue: false,
    macos_notifications: false,
    worker_args: {},
  };
  const workers: AgentRow[] = [...herdrCards, ...sasuCards].flatMap((value) => {
    if (!value.worker_pane) return [];
    const pane = value.worker_pane;
    const delegated = value.task === taskId(415);
    const blocked = value.state === "blocked" || value.state === "stopped";
    const row: AgentRow = { state: galleryAgentState(pane, null, value.since), id: pane, pane_id: pane, identity_label: pane, agent_kind: value.worker_runtime ?? "codex", emphasized: false, unread: false, demand: "none", activity: blocked ? "idle" : "working", group: "seen", symbol: blocked ? "○" : "●", status_code: blocked ? "idle" : "working", changed_at_unix_ms: value.since,
      request: { verb: "working", verb_since_unix_ms: value.since, request: null, later_by: null, reply: null, pull_requests: [], line: value.state === "stopped" ? "web-e2e 세 번째 실패 뒤 멈춤" : value.state === "blocked" ? "Task 상세 API 선택을 기다리는 중" : "변경을 구현하고 검증하는 중" },
      lineage_child_pane_ids: delegated ? [`${pane}:child-1`, `${pane}:child-2`] : [],
      close_descendant_pane_ids: delegated ? [`${pane}:child-1`, `${pane}:child-2`] : [],
    };
    const children: AgentRow[] = delegated ? [1, 2].map((index) => ({ ...row, state: galleryAgentState(`${pane}:child-${index}`, null, value.since), id: `${pane}:child-${index}`, pane_id: `${pane}:child-${index}`, identity_label: `하위 작업 ${index}`, lineage_child_pane_ids: [], close_descendant_pane_ids: [], request: undefined })) : [];
    return [row, ...children];
  });
  return { summary, workers, detail, config: { config, machine: { max_workers: 4 } } };
}
