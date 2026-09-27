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

/** The fold choices the core would keep in its ui state. */
export type SceneFolds = {
  expandedCheckouts: string[];
  collapsedWorkspaces: string[];
  /** Parents whose descendants are unfolded; every other parent is folded, the core's default. */
  expandedAgents: string[];
  inactiveCheckoutsOpen: string[];
  inactiveProjectsOpen: boolean;
};

/** The folds `Screen / Projects Sidebar` draws: main's agents open, a1 unfolded, sasu folded. */
export const REFERENCE_FOLDS: SceneFolds = {
  expandedCheckouts: ["herdr-ide:main"],
  collapsedWorkspaces: ["sasu"],
  expandedAgents: ["a1"],
  inactiveCheckoutsOpen: [],
  inactiveProjectsOpen: false,
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

type AgentSpec = Partial<AgentRow> & Pick<AgentRow, "pane_id" | "group" | "symbol" | "status_label" | "elapsed">;

function agent(spec: AgentSpec): AgentRow {
  return {
    id: spec.pane_id,
    identity_label: spec.pane_id,
    agent_kind: "claude",
    emphasized: false,
    unread: false,
    demand: "none",
    activity: "idle",
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
          running_agent_count: panes.length,
          deletion_gate: { blocked_reason: null, warnings: [], button_label: "Delete worktree", can_delete_branch: true },
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
    workspace_label: null,
    cwd: ROOT,
    status_label: "idle",
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
export function sidebarScene(content: SceneContent, folds: SceneFolds, nowMs: number): { rest: SnapshotRest; agents: AgentRow[] } {
  const now = Math.floor(nowMs / 1000);
  const title = TITLES[content];
  const unfolded = (id: string) => folds.expandedAgents.includes(id);
  const agents: AgentRow[] = [
    agent({ pane_id: "tn1", identity_label: "회의록 요약", group: "seen", symbol: "○", status_label: "Idle", elapsed: "3h" }),
    agent({
      pane_id: "a1",
      identity_label: title.a1,
      group: "working",
      symbol: "●",
      status_label: "Working",
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
      status_label: "Working",
      activity: "working",
      elapsed: "42s",
      delegated: true,
      lineage_parent_pane_id: "a1",
      lineage_depth: 1,
      lineage_worktree_badge: "feat/ui",
    }),
    agent({ pane_id: "a1c2", identity_label: title.a1c2, group: "seen", symbol: "○", status_label: "Idle", elapsed: "38s", delegated: true, lineage_parent_pane_id: "a1", lineage_depth: 1 }),
    agent({
      pane_id: "a2",
      identity_label: title.a2,
      group: "working",
      symbol: "○",
      status_label: "Done",
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
      status_label: "Question",
      demand: "question",
      elapsed: "1m",
      delegated: true,
      detail: "질문 목록을 이대로 보내도 될까요?",
      lineage_parent_pane_id: "a2",
      lineage_depth: 1,
    }),
    agent({ pane_id: "a2c2", identity_label: "사례 조사", group: "working", symbol: "●", status_label: "Working", activity: "working", elapsed: "50s", delegated: true, lineage_parent_pane_id: "a2", lineage_depth: 1 }),
    agent({
      pane_id: "a3",
      identity_label: title.a3,
      group: "needs_you",
      symbol: "?",
      status_label: "Question",
      demand: "question",
      elapsed: "30s",
      unread: true,
      detail: "프로덕션 배포 전에 변경 내용을 확인해 주세요",
    }),
    agent({ pane_id: "q1", identity_label: "브라우저 표시 확인", group: "needs_you", symbol: "?", status_label: "Question", demand: "question", elapsed: "40m", detail: "주소 경계를 어디에 둘까요?" }),
    agent({ pane_id: "q2", identity_label: "검색 팔레트", group: "done", symbol: "✓", status_label: "Done", activity: "stopped", emphasized: true, elapsed: "1h" }),
    agent({ pane_id: "e1", identity_label: "단축키 연결", group: "working", symbol: "●", status_label: "Working", activity: "working", elapsed: "2h" }),
  ];

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

  const rest: SnapshotRest = {
    navigator: {
      workspaces,
      inactive_projects: [{ device_id: "local", expanded: folds.inactiveProjectsOpen, project_ids: ["old-prototype", "dotfiles", "research-notes"] }],
      agents,
      devices: [{ id: "local", label: "This Mac", kind: "local", state: "local", message: null, ssh_alias: null, agent_count: agents.length, test: null }],
      focused_device_id: "local",
      focused_checkout_id: null,
    },
    ui_state: {
      left_sidebar_visible: true,
      collapsed_workspace_ids: folds.collapsedWorkspaces,
      expanded_checkout_ids: folds.expandedCheckouts,
      workspace_registrations: [],
    },
  };
  return { rest, agents };
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
  if (kind === "ui_state_update") {
    const next = { ...folds };
    if (Array.isArray(payload.expanded_checkout_ids)) next.expandedCheckouts = payload.expanded_checkout_ids as string[];
    if (Array.isArray(payload.collapsed_workspace_ids)) next.collapsedWorkspaces = payload.collapsed_workspace_ids as string[];
    return next;
  }
  if (kind === "agent_tree_toggle") return { ...folds, expandedAgents: toggled(folds.expandedAgents, String(payload.pane_id)) };
  if (kind === "inactive_checkouts_toggle") return { ...folds, inactiveCheckoutsOpen: toggled(folds.inactiveCheckoutsOpen, String(payload.project_path)) };
  if (kind === "inactive_projects_toggle") return { ...folds, inactiveProjectsOpen: !folds.inactiveProjectsOpen };
  return null;
}
