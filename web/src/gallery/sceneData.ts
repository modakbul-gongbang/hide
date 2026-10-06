// The Projects sidebar's gallery scene (PRD design-review-workflow D-04,
// B3, B4): a synthetic snapshot the real sidebar renders, with the content
// `Screen / Projects Sidebar` in design/hide-screens.pen draws, so an actual
// capture and the Pen frame show the same projects, checkouts and agents.
// Everything here is invented example data: no path, name or count comes from
// a machine. The folds are this scene's own ui state, applied by `applyEvent`
// the way the core applies the same events, so a chevron really folds.

import type { AgentRow, Checkout, MarkCounts, PullRequest, SnapshotRest, Workspace } from "../snapshot";

/** Which titles the scene carries: the Pen frame's, or long Korean ones for truncation. */
export type SceneContent = "reference" | "long";

/**
 * How many devices the scene registers: This Mac alone (the rail still
 * shows, with one tile), This Mac, a connected `mini` and a disconnected one
 * with a long Korean name (quick device-rail-badges), or that fixture with
 * `mini` busy enough to draw the `9+` pill.
 */
export type SceneDevices = "one" | "two" | "busy";

/** The device the scene opens in front: a rail tile's id. */
export type SceneFront = "local" | "mini" | "offline";

/** The registered-but-unreachable device of the two-device scene. */
export const OFFLINE_DEVICE = "build-box";

/** The fold choices the core would keep in its ui state. */
export type SceneFolds = {
  expandedCheckouts: string[];
  collapsedWorkspaces: string[];
  /** Parents whose descendants are unfolded; every other parent is folded, the core's default. */
  expandedAgents: string[];
  inactiveCheckoutsOpen: string[];
  inactiveProjectsOpen: boolean;
  /** The device the core has in front (`focus_device`); a rail tile changes it. */
  frontDevice: string;
};

/** The folds `Screen / Projects Sidebar` draws: main's agents open, a1 unfolded, sasu folded. */
export const REFERENCE_FOLDS: SceneFolds = {
  expandedCheckouts: ["herdr-ide:main"],
  collapsedWorkspaces: ["sasu"],
  expandedAgents: ["a1"],
  inactiveCheckoutsOpen: [],
  inactiveProjectsOpen: false,
  frontDevice: "local",
};

const ROOT = "/work";
const NO_MARKS: MarkCounts = { error: 0, approval: 0, question: 0, working: 0, done: 0, idle: 0 };

const TITLES: Record<SceneContent, Record<string, string>> = {
  reference: {
    a1: "사이드바 가독성 개선",
    a1c1: "컴포넌트 구조 정리",
    a1c2: "한글 가독성 확인",
    a2: "후속 UX 계획 인터뷰",
    a3: "배포 전 확인",
  },
  long: {
    a1: "사이드바 자식 에이전트 들여쓰기와 긴 한글 제목의 말줄임을 함께 확인하는 작업",
    a1c1: "컴포넌트 구조 정리와 상태 열 정렬을 위한 하위 작업",
    a1c2: "한글 가독성 확인 및 좁은 폭에서 시간 열이 밀리지 않는지 검토",
    a2: "후속 UX 계획 인터뷰 결과를 정리해서 다음 단계 제안서로 만들기",
    a3: "배포 전 확인 요청에 대한 답변 대기",
  },
};

/** A scene names how long ago each agent changed (`42s`, `3m`, `2h`); the row counts on from there. */
type AgentSpec = Partial<AgentRow> & Pick<AgentRow, "pane_id" | "group" | "symbol" | "status_code"> & { elapsed: string };

const UNIT_MS: Record<string, number> = { s: 1_000, m: 60_000, h: 3_600_000, d: 86_400_000 };

function agent({ elapsed, ...spec }: AgentSpec): AgentRow {
  const unit = UNIT_MS[elapsed.slice(-1)] ?? 0;
  return {
    id: spec.pane_id,
    identity_label: spec.pane_id,
    agent_kind: "claude",
    // The core draws a row bright exactly while it is in Needs You or Done.
    emphasized: spec.group === "needs_you" || spec.group === "done",
    unread: false,
    demand: "none",
    activity: "idle",
    changed_at_unix_ms: Date.now() - Number(elapsed.slice(0, -1)) * unit,
    ...spec,
  };
}

function marks(partial: Partial<MarkCounts>): MarkCounts {
  return { ...NO_MARKS, ...partial };
}

type CheckoutSpec = {
  id: string;
  workspace: string;
  branch: string | null;
  primary?: boolean;
  folder?: boolean;
  /** Seconds since the last commit; absent on a plain folder. */
  age?: number;
  purpose?: string;
  panes?: string[];
  marks?: Partial<MarkCounts>;
  pr?: Pick<PullRequest, "badge" | "is_draft" | "number" | "title">;
  exists?: boolean;
};

function checkout(spec: CheckoutSpec, nowSeconds: number): Checkout {
  const base = `${ROOT}/${spec.workspace}`;
  const path = spec.primary || spec.folder ? base : `${base}.worktrees/${spec.branch}`;
  const panes = spec.panes ?? [];
  const counted = spec.marks ? marks(spec.marks) : null;
  return {
    id: spec.id,
    workspace_id: spec.workspace,
    label: spec.branch ?? spec.workspace,
    path,
    branch: spec.branch,
    purpose: spec.purpose ? { text: spec.purpose, origin: "operator" } : null,
    is_worktree: !spec.primary && !spec.folder,
    is_primary: spec.primary === true,
    exists: spec.exists ?? true,
    has_panes: panes.length > 0,
    agent_summary: counted
      ? {
          representative_pane_id: panes[0] ?? null,
          needs_you: counted.question + counted.approval + counted.error,
          done: counted.done,
          working: counted.working,
          seen: counted.idle,
          unknown: 0,
          marks: counted,
        }
      : undefined,
    worktree: spec.folder
      ? null
      : {
          path,
          branch: spec.branch,
          head_sha: "0000000",
          last_commit_unix_seconds: spec.age === undefined ? null : nowSeconds - spec.age,
          is_main: Boolean(spec.primary),
          missing: spec.exists === false,
          dirty: false,
          changed_file_count: 0,
          merged: false,
          behind_upstream: null,
          pane_count: panes.length,
          deletion_gate: { blocked_reason: null, warnings: [], button_label: "Delete worktree", can_delete_branch: true, branch_warning: null, discard_label: null },
        },
    pull_request: spec.pr ? { url: "", review: null, ...spec.pr } : null,
    github: { failure_category: null, available: true, loading: false, stale: false, last_success_at_unix_ms: nowSeconds * 1000, unavailable_reason: null },
    tabs: panes.length ? [{ id: `${spec.id}:t1`, workspace_id: spec.workspace, checkout_id: spec.id, label: "1", empty: false, delegated: false, panes: panes.map(pane) }] : [],
    active_tab_id: panes.length ? `${spec.id}:t1` : null,
    strip: [],
    next_tab_label: "2",
  };
}

function pane(id: string) {
  return {
    id,
    herdr_label: null,
    terminal_title: null,
    cwd: ROOT,
    status_code: "idle" as const,
    requires_close_confirmation: false,
    requires_close_status_check: false,
    identity_label: id,
  };
}

function workspace(id: string, extra: Partial<Workspace> & Pick<Workspace, "checkouts">, folds: SceneFolds): Workspace {
  return {
    label: id,
    path: `${ROOT}/${id}`,
    device_id: "local",
    registered: true,
    temporary: false,
    pinned: false,
    is_git: true,
    expanded: !folds.collapsedWorkspaces.includes(id),
    inactive_checkouts: { expanded: folds.inactiveCheckoutsOpen.includes(`${ROOT}/${id}`), checkout_ids: [] },
    ...extra,
    id,
  };
}

/** Fourteen settled worktrees behind herdr-ide's `Inactive 14`; `feat/ui` is where a1's first child works. */
const INACTIVE = ["feat/ui", ...Array.from({ length: 13 }, (_, index) => `chore/cleanup-${index + 1}`)];

/** The scene's snapshot and agents for one content set and one set of folds. */
export function sidebarScene(content: SceneContent, folds: SceneFolds, nowMs: number, devices: SceneDevices = "one"): { rest: SnapshotRest; agents: AgentRow[] } {
  const now = Math.floor(nowMs / 1000);
  const title = TITLES[content];
  const unfolded = (id: string) => folds.expandedAgents.includes(id);
  const agents: AgentRow[] = [
    agent({ pane_id: "tn1", identity_label: "회의록 요약", group: "seen", symbol: "○", status_code: "idle", elapsed: "3h" }),
    agent({
      pane_id: "a1",
      identity_label: title.a1,
      group: "working",
      symbol: "●",
      status_code: "working",
      activity: "working",
      elapsed: "1m",
      lineage_child_pane_ids: ["a1c1", "a1c2"],
      lineage_collapsed: !unfolded("a1"),
      descendant_counts: { error: 0, approval: 0, question: 0, working: 1, done: 0 },
    }),
    agent({
      pane_id: "a1c1",
      identity_label: title.a1c1,
      agent_kind: "codex",
      group: "working",
      symbol: "●",
      status_code: "working",
      activity: "working",
      elapsed: "42s",
      delegated: true,
      lineage_parent_pane_id: "a1",
      lineage_depth: 1,
      lineage_worktree_badge: "feat/ui",
    }),
    agent({ pane_id: "a1c2", identity_label: title.a1c2, group: "seen", symbol: "○", status_code: "idle", elapsed: "38s", delegated: true, lineage_parent_pane_id: "a1", lineage_depth: 1 }),
    agent({
      pane_id: "a2",
      identity_label: title.a2,
      group: "working",
      symbol: "○",
      status_code: "done",
      activity: "stopped",
      elapsed: "2m",
      waiting_on_descendants: true,
      lineage_child_pane_ids: ["a2c1", "a2c2"],
      lineage_collapsed: !unfolded("a2"),
      descendant_counts: { error: 0, approval: 0, question: 1, working: 1, done: 0 },
    }),
    agent({
      pane_id: "a2c1",
      identity_label: "인터뷰 질문 정리",
      group: "working",
      symbol: "?",
      status_code: "question",
      demand: "question",
      elapsed: "1m",
      delegated: true,
      detail: "질문 목록을 이대로 보내도 될까요?",
      lineage_parent_pane_id: "a2",
      lineage_depth: 1,
    }),
    agent({ pane_id: "a2c2", identity_label: "사례 조사", group: "working", symbol: "●", status_code: "working", activity: "working", elapsed: "50s", delegated: true, lineage_parent_pane_id: "a2", lineage_depth: 1 }),
    agent({
      pane_id: "a3",
      identity_label: title.a3,
      group: "needs_you",
      symbol: "?",
      status_code: "question",
      demand: "question",
      elapsed: "30s",
      unread: true,
      detail: "프로덕션 배포 전에 변경 내용을 확인해 주세요",
    }),
    agent({ pane_id: "q1", identity_label: "브라우저 표시 확인", group: "needs_you", symbol: "?", status_code: "question", demand: "question", elapsed: "40m", detail: "주소 경계를 어디에 둘까요?" }),
    agent({ pane_id: "q2", identity_label: "검색 팔레트", group: "done", symbol: "✓", status_code: "done", activity: "stopped", elapsed: "1h" }),
    agent({ pane_id: "e1", identity_label: "단축키 연결", group: "working", symbol: "●", status_code: "working", activity: "working", elapsed: "2h" }),
  ];

  if (devices !== "one") agents.push(...HOME_AGENTS[content]);

  const herdrCheckouts: Checkout[] = [
    checkout({ id: "herdr-ide:main", workspace: "herdr-ide", branch: "main", primary: true, age: 10, purpose: "사이드바 가독성 개선", panes: ["a1", "a1c2", "a2", "a2c1", "a2c2", "a3"], marks: { question: 2, working: 3, idle: 1 } }, now),
    checkout({ id: "herdr-ide:155", workspace: "herdr-ide", branch: "quick/155-browser-display", age: 40 * 60, purpose: issueTitle(155, "browser display (WebContentsView)"), panes: ["q1"], marks: { question: 1 } }, now),
    checkout({ id: "herdr-ide:154", workspace: "herdr-ide", branch: "quick/154-search-palette", age: 3600, purpose: issueTitle(154, "⌘K search palette UI"), panes: ["q2"], marks: { done: 1 }, pr: { number: 154, title: "Search palette", badge: "open", is_draft: true } }, now),
    checkout({ id: "herdr-ide:electron", workspace: "herdr-ide", branch: "electron-shortcut-bindings", age: 2 * 3600, purpose: "Electron desktop host for the web shell", panes: ["e1"], marks: { working: 1 }, pr: { number: 149, title: "Electron host", badge: "open", is_draft: false } }, now),
    checkout({ id: "herdr-ide:ux", workspace: "herdr-ide", branch: "design/workspace-ux-proposal", age: 5 * 3600, purpose: "Workspace UX 제안과 상태 소유 정리" }, now),
    checkout({ id: "herdr-ide:fix", workspace: "herdr-ide", branch: "fix/registered-projects-only", age: 86400 }, now),
    checkout({ id: "herdr-ide:legacy", workspace: "herdr-ide", branch: "legacy-shell", exists: false }, now),
    ...INACTIVE.map((branch, index) =>
      checkout({ id: `herdr-ide:inactive-${index}`, workspace: "herdr-ide", branch, age: (8 + index) * 86400, ...(index === 0 ? { panes: ["a1c1"], marks: { working: 1 } } : {}) }, now),
    ),
  ];

  const workspaces: Workspace[] = [
    workspace("team-notes", { is_git: false, pinned: true, checkouts: [checkout({ id: "team-notes:folder", workspace: "team-notes", branch: null, folder: true, purpose: "회의록 요약 정리", panes: ["tn1"], marks: { idle: 1 } }, now)] }, folds),
    workspace(
      "herdr-ide",
      {
        checkouts: herdrCheckouts,
        inactive_checkouts: { expanded: folds.inactiveCheckoutsOpen.includes(`${ROOT}/herdr-ide`), checkout_ids: INACTIVE.map((_, index) => `herdr-ide:inactive-${index}`) },
      },
      folds,
    ),
    workspace("sasu", { checkouts: [checkout({ id: "sasu:main", workspace: "sasu", branch: "main", primary: true, age: 3 * 3600, purpose: "파이프라인 정리", marks: { working: 1, done: 1, idle: 1 } }, now)] }, folds),
    ...["old-prototype", "dotfiles", "research-notes"].map((id, index) =>
      workspace(id, { checkouts: [checkout({ id: `${id}:main`, workspace: id, branch: "main", primary: true, age: (30 + index) * 86400 }, now)] }, folds),
    ),
  ];

  const world = devices !== "one" ? deviceWorld(content, workspaces, now, folds, devices === "busy") : null;
  const rest: SnapshotRest = {
    navigator: {
      workspaces: world ? [...workspaces, world.home] : workspaces,
      inactive_projects: [{ device_id: "local", expanded: folds.inactiveProjectsOpen, project_ids: ["old-prototype", "dotfiles", "research-notes"] }],
      agents,
      devices: [{ id: "local", label: "This Mac", kind: "local", state: "local", message: null, ssh_alias: null, agent_count: agents.length, test: null }, ...(world?.devices ?? [])],
      focused_device_id: folds.frontDevice,
      focused_checkout_id: null,
    },
    ui_state: {
      left_sidebar_visible: true,
      collapsed_workspace_ids: folds.collapsedWorkspaces,
      expanded_checkout_ids: folds.expandedCheckouts,
      workspace_registrations: world?.registrations ?? workspaces.map((row) => registrationOf(row, "local")),
    },
    ...(world ? { status: { remote: world.remote } } : {}),
  };
  return { rest, agents };
}

const HOME_AGENTS: Record<SceneContent, AgentRow[]> = {
  reference: [
    agent({ pane_id: "h1", identity_label: "블로그 초안 정리", group: "needs_you", symbol: "?", status_code: "question", demand: "question", elapsed: "2m", unread: true, detail: "톤을 이대로 갈까요?" }),
    agent({ pane_id: "h2", identity_label: "두 프로젝트 비교 조사", agent_kind: "codex", group: "seen", symbol: "○", status_code: "idle", elapsed: "14m" }),
  ],
  long: [
    agent({ pane_id: "h1", identity_label: "여러 프로젝트에 걸친 블로그 초안 정리와 어투 통일 작업을 이어서 진행하는 에이전트", group: "needs_you", symbol: "?", status_code: "question", demand: "question", elapsed: "2m", unread: true, detail: "톤을 이대로 갈까요, 아니면 조금 더 딱딱하게 다듬을까요?" }),
    agent({ pane_id: "h2", identity_label: "두 프로젝트의 구조와 의존성을 나란히 비교 조사하는 작업", agent_kind: "codex", group: "seen", symbol: "○", status_code: "idle", elapsed: "14m" }),
  ],
};

const REMOTE_AGENTS: Record<SceneContent, AgentRow[]> = {
  reference: [
    agent({ pane_id: "remote:mini:pane:1", identity_label: "배치 감시", agent_kind: "codex", group: "needs_you", symbol: "?", status_code: "question", demand: "question", elapsed: "5m", unread: true, detail: "풀 리퀘스트 머지할까요?" }),
    agent({ pane_id: "remote:mini:pane:2", identity_label: "릴리스 빌드 확인", agent_kind: "codex", group: "working", symbol: "●", status_code: "working", activity: "working", elapsed: "1m" }),
  ],
  long: [
    agent({ pane_id: "remote:mini:pane:1", identity_label: "밤새 도는 배치 작업의 실패 원인과 재시도 여부를 감시하는 에이전트", agent_kind: "codex", group: "needs_you", symbol: "?", status_code: "question", demand: "question", elapsed: "5m", unread: true, detail: "풀 리퀘스트 머지할까요?" }),
    agent({ pane_id: "remote:mini:pane:2", identity_label: "릴리스 빌드 확인", agent_kind: "codex", group: "working", symbol: "●", status_code: "working", activity: "working", elapsed: "1m" }),
  ],
};

/** Enough more agents on `mini` for ten or more Needs You, past the Projects list's cap, an unseen Done and a second Working. */
const BUSY_AGENTS: AgentRow[] = [
  ...Array.from({ length: 11 }, (_, index) =>
    agent({ pane_id: `remote:mini:busy:${index}`, identity_label: `배치 ${index + 1}`, agent_kind: "codex", group: "needs_you", symbol: "?", status_code: "question", demand: "question", elapsed: "3m", detail: "확인이 필요합니다" }),
  ),
  agent({ pane_id: "remote:mini:done:1", identity_label: "빌드 정리", agent_kind: "codex", group: "done", symbol: "✓", status_code: "done", activity: "stopped", elapsed: "9m" }),
  agent({ pane_id: "remote:mini:working:2", identity_label: "로그 수집", agent_kind: "codex", group: "working", symbol: "●", status_code: "working", activity: "working", elapsed: "2m" }),
];

/** The registration the core keeps for a project on `deviceId`; `home` marks the device's Home. */
function registrationOf(workspace: Workspace, deviceId: string, extra: { home?: boolean } = {}) {
  return { id: workspace.id, label: workspace.label, path: workspace.path, device_id: deviceId, pinned: workspace.pinned, ...extra };
}

/**
 * The devices of the two-device scene: a Home on this Mac (its agents are the
 * Home row's children), a connected `mini` with its own project, Home and
 * agents, and a registered device that is not connected, named at length to
 * try the rail's and the header's truncation.
 */
function deviceWorld(content: SceneContent, workspaces: Workspace[], now: number, folds: SceneFolds, busy: boolean) {
  const offlineLabel = content === "long" ? "연구실 빌드 서버 자동화 장비 (긴 이름 확인용)" : "build-box";
  const home = workspace(
    "home",
    { label: "hide", path: `${ROOT}/hide`, is_home: true, is_git: false, checkouts: [checkout({ id: "home:folder", workspace: "home", branch: null, folder: true, panes: ["h1", "h2"], marks: { question: 1, idle: 1 } }, now)] },
    folds,
  );
  const remoteWorkspace = (id: string, label: string, extra: Partial<Workspace>, panes: string[]): Workspace => ({
    ...workspace(id, { checkouts: [], ...extra }, folds),
    label,
    path: `/srv/${label}`,
    device_id: "mini",
    checkouts: [{ ...checkout({ id: `${id}:main`, workspace: id, branch: "main", primary: true, age: 300, panes }, now), workspace_id: id }],
  });
  const miniWorkspaces = [
    // Busy, the extra agents run in web too, so its Projects list raises them past the Needs You cap.
    remoteWorkspace("remote:mini:workspace:web", "web", {}, ["remote:mini:pane:1", "remote:mini:pane:2", ...(busy ? BUSY_AGENTS.map((row) => row.pane_id) : [])]),
    { ...remoteWorkspace("remote:mini:workspace:home", "hide", { is_home: true, is_git: false }, []), path: "/Users/example/hide" },
  ];
  return {
    home,
    devices: [
      { id: "mini", label: "mini", kind: "remote", state: "ready", message: null, ssh_alias: "mini", agent_count: 2, test: null },
      { id: OFFLINE_DEVICE, label: offlineLabel, kind: "remote", state: "unavailable", message: null, ssh_alias: OFFLINE_DEVICE, agent_count: 0, test: null },
    ],
    registrations: [
      ...workspaces.map((row) => registrationOf(row, "local")),
      registrationOf(home, "local", { home: true }),
      registrationOf(miniWorkspaces[0]!, "mini"),
      registrationOf(miniWorkspaces[1]!, "mini", { home: true }),
    ],
    remote: [
      {
        target_id: "mini",
        state: "connected",
        message: null,
        herdr_version: "0.9.1",
        session: {
          workspaces: miniWorkspaces,
          agents: busy ? [...REMOTE_AGENTS[content], ...BUSY_AGENTS] : REMOTE_AGENTS[content],
          active_tab_ids: {},
          focused_workspace_id: "remote:mini:workspace:web",
          focused_checkout_id: "remote:mini:workspace:web:main",
          focused_tab_id: "remote:mini:workspace:web:main:t1",
          focused_pane_id: null,
          pane_layouts: [],
        },
      },
      { target_id: OFFLINE_DEVICE, state: "not_connected", message: null, herdr_version: null, session: null },
    ],
  };
}

/** A purpose that names its issue the way an agent's title does, `#<number> <title>`. */
function issueTitle(issue: number, title: string): string {
  return `#${issue} ${title}`;
}

function toggled(list: string[], id: string): string[] {
  return list.includes(id) ? list.filter((item) => item !== id) : [...list, id].sort();
}

/**
 * The events the sidebar's folds dispatch, applied to the scene's folds the
 * way the core applies them. Anything else is not the scene's to model and
 * returns null, so the caller can say so instead of pretending it happened.
 */
export function applyEvent(folds: SceneFolds, event: { kind: string; payload: Record<string, unknown> }): SceneFolds | null {
  const { kind, payload } = event;
  if (kind === "checkout_agents_toggle") return { ...folds, expandedCheckouts: toggled(folds.expandedCheckouts, String(payload.checkout_id)) };
  if (kind === "project_checkouts_fold") {
    const id = String(payload.workspace_id);
    const expanded = typeof payload.expanded === "boolean" ? payload.expanded : folds.collapsedWorkspaces.includes(id);
    return { ...folds, collapsedWorkspaces: expanded ? folds.collapsedWorkspaces.filter((item) => item !== id) : [...new Set([...folds.collapsedWorkspaces, id])].sort() };
  }
  if (kind === "agent_tree_toggle") return { ...folds, expandedAgents: toggled(folds.expandedAgents, String(payload.pane_id)) };
  if (kind === "inactive_checkouts_toggle") return { ...folds, inactiveCheckoutsOpen: toggled(folds.inactiveCheckoutsOpen, String(payload.project_path)) };
  if (kind === "inactive_projects_toggle") return { ...folds, inactiveProjectsOpen: !folds.inactiveProjectsOpen };
  if (kind === "focus_device") return { ...folds, frontDevice: String(payload.device_id) };
  return null;
}
