// The Factory screens' gallery scene data (PRD software-factory-ui B24, D-18):
// a synthetic `FactorySummary` the real `FactoryScreen` renders, with the
// content `Screen / Factory` in design/hide-screens.pen draws. Everything here
// is invented example data: no project, title or number comes from a machine.
// An issue number is written through `issue()` because a bare hash and digits
// in web source read as a hex color to the design contract.

import type { FactoryConfig } from "../factory/FactorySettings";
import type { CardView, Column, FactorySummary, FactoryView, InboxItem, TaskDetail, TaskState } from "../factory/model";
import type { SceneContent } from "./sceneData";

const MINUTE = 60_000;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;

const issue = (number: number) => `#${number}`;
const taskId = (number: number) => `t-${number}`;

type CardSpec = {
  number: number;
  title: string;
  state: TaskState;
  column: Column;
  /** How long ago the Task last changed. */
  ago: number;
  needsPerson?: boolean;
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
    state: spec.state,
    state_label: spec.state,
    needs_person: spec.needsPerson ?? false,
    waiting_for: spec.waitingFor ?? null,
    priority: spec.priority ?? 0,
    since: now - spec.ago,
    unread: spec.unread ?? false,
    folded: spec.folded ?? false,
    archived: false,
    failures: spec.failures ?? 0,
    external: spec.external ?? [],
    revive_until: spec.reviveDays === undefined ? null : now + spec.reviveDays * DAY,
    worker_pane: spec.worker ?? null,
  };
}

/** The long set: a long Korean title and a long issue number, to try the cards' and rows' wrapping. */
const LONG_TITLE = "Task 상세 API 응답 형식을 정하고 기존 snapshot 필드와 겹치는 이름을 정리하면서 한글과 English가 섞인 아주 긴 제목이 카드 안에서 두 줄로 줄어드는지 확인하는 작업";

function herdrSpecs(content: SceneContent): CardSpec[] {
  const long = content === "long";
  return [
    { draft: 7, number: 0, title: "알림 설정 화면 정리", state: "drafting", column: "drafting", ago: 12 * MINUTE },
    { number: 421, title: "Task 상세 화면", state: "waiting", column: "waiting", ago: 3 * HOUR, waitingFor: issue(420) },
    { number: 422, title: "보드에서 Task 상세 패널 열기", state: "waiting", column: "waiting", ago: 3 * HOUR, waitingFor: issue(421) },
    { number: 431, title: "문서 깨진 링크 정리", state: "waiting", column: "waiting", ago: 5 * HOUR, priority: 1 },
    { number: long ? 123456 : 420, title: long ? LONG_TITLE : "Task 상세 API 응답 형식", state: "blocked", column: "running", ago: 3 * DAY, needsPerson: true },
    { number: 412, title: "Issues 보드에 정렬 추가", state: "running", column: "running", ago: 2 * MINUTE, worker: "worker-412", failures: 1 },
    { number: 415, title: "보드 정렬 상태 기억", state: "running", column: "running", ago: MINUTE, worker: "worker-415" },
    { number: 417, title: "디스크 정리 표 다시 그리기", state: "stopped", column: "running", ago: 40 * MINUTE, needsPerson: true, failures: 3 },
    { number: 405, title: "hide-ai 호출 상한 조정", state: "merge_waiting", column: "running", ago: HOUR, needsPerson: true },
    { number: 398, title: "Sessions 검색 속도 개선", state: "verifying", column: "running", ago: 20 * MINUTE },
    { number: 430, title: "flaky: pane-focus 테스트", state: "outside", column: "running", ago: 6 * MINUTE, external: ["sasu/gate-cache"] },
    { number: 410, title: "정렬 API: tasks.rs에 updated_at", state: "done", column: "done", ago: 2 * HOUR, unread: true },
    { number: 409, title: "Task 목록 정렬 키 문서화", state: "landed", column: "done", ago: 5 * HOUR },
    { number: 401, title: "Task 저장소를 별도 crate로", state: "landed", column: "done", ago: 4 * DAY, folded: true },
    { number: 399, title: "Factory 설정 기본값 정리", state: "landed", column: "done", ago: 6 * DAY, folded: true },
  ];
}

function sasuSpecs(): CardSpec[] {
  return [
    { number: 88, title: "gate 결과 요약 보기", state: "running", column: "running", ago: 8 * MINUTE, needsPerson: false, worker: "worker-88" },
    { number: 91, title: "implement 단계 로그 정리", state: "waiting", column: "waiting", ago: 2 * HOUR },
    { number: 86, title: "verify 리포트 한 줄 요약", state: "done", column: "done", ago: 50 * MINUTE },
  ];
}

const COLUMN_LABEL: Record<Column, string> = { drafting: "정리 중", waiting: "대기", running: "실행 중", done: "완료" };
const COLUMN_ORDER: Column[] = ["drafting", "waiting", "running", "done"];

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
      .sort((a, b) => Number(b.needs_person) - Number(a.needs_person) || (a.needs_person ? a.since - b.since : b.priority - a.priority || a.since - b.since)),
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
      drafting: count("drafting"),
      waiting: count("waiting"),
      running: count("running"),
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
    waiting_since: now - ago,
    waiting_days: Math.floor(ago / DAY),
    ...item,
  };
}

export type FactorySceneFixture = {
  summary: FactorySummary;
  /** A Task page's detail by Task id, for the page the scene opens. */
  detail: (task: string) => TaskDetail | null;
  config: { config: FactoryConfig; machine: { max_workers: number } };
};

/** The scene's summary, the Task pages it can open and the settings it answers with. */
export function factoryScene(content: SceneContent, now: number): FactorySceneFixture {
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
        text: "WS에 합치면 Task마다 snapshot이 약 2 KB 커지고, REST는 hided에 route가 하나 생깁니다.",
        suggestion: "WS snapshot에 합치기",
        choices: ["REST 엔드포인트"],
        result: `worker를 깨워 이어갑니다 · 끝나면 ${issue(421)}이 풀립니다`,
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
        result: "최근 수정순으로 이어갑니다",
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
        result: "실패한 gate만 요약합니다",
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
        result: `머지하면 ${issue(405)}가 완료됩니다`,
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
        result: "worker를 깨워 다시 시도합니다",
      },
      now,
      40 * MINUTE,
    ),
    inboxItem({ group: "notice", kind: "notice", factory: herdr.id, task: taskId(398), display_id: issue(398), title: "main 깨짐 → revert 됨", project: "herdr-ide", text: "main이 깨져 마지막 머지를 되돌렸습니다." }, now, 25 * MINUTE),
    inboxItem({ group: "notice", kind: "notice", factory: herdr.id, task: taskId(415), display_id: issue(415), title: `${issue(412)}와 같은 정렬을 다르게 푸는 중`, project: "herdr-ide", text: "두 Task가 같은 정렬 상태를 서로 다르게 바꾸고 있습니다." }, now, 10 * MINUTE),
  ].map((item, rank) => ({ ...item, rank }));
  const summary: FactorySummary = { my_turn: inbox.length, factories: [herdr, sasu], inbox };

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
      gates: found.state === "merge_waiting" ? ["검증 통과", "리뷰 요청 없음"] : [],
      allowed: running ? ["pause", "cancel"] : found.state === "merge_waiting" ? ["merge", "request-changes", "cancel"] : found.state === "stopped" ? ["retry", "cancel"] : found.state === "waiting" ? ["priority", "cancel"] : [],
      stop: found.state === "stopped" ? "검증 3회 실패" : null,
      merge_sha: null,
      worker_name: found.worker_pane ? `${found.task}-worker` : null,
      worktree: found.worker_pane ? `/work/herdr-ide.worktrees/${found.task}` : null,
      branch: running ? "412-board-sort" : null,
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
    default_runtime: "claude",
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
  return { summary, detail, config: { config, machine: { max_workers: 4 } } };
}
