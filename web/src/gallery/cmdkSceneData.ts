// A snapshot shaped like the one ⌘K's design scenario draws (PRD
// cmdk-navigation): issue 273, the checkout that works on it with pull
// request 275 and a child agent whose parent sits in `main`, and a second
// agent in `main` outside that lineage.

import type { AgentRow, GithubSearchResult, SnapshotRest } from "../snapshot";

/** When the fixture's projects were last read from GitHub; the gallery scene moves it to a few minutes before it opens. */
export const READ_AT = 1_000_000;

/** The hash of an issue number, kept out of the literals so the token check does not read a number after it as a color. */
const HASH = "#";

const agent = (pane: string, label: string, extra: Partial<AgentRow> = {}): AgentRow =>
  ({ id: `a-${pane}`, pane_id: pane, identity_label: label, agent_kind: "claude", symbol: "●", group: "working", status_label: "Working", changed_at_unix_ms: Date.now() - 60_000, emphasized: false, unread: false, activity: "working", ...extra }) as AgentRow;

export const PARENT = agent("p-parent", "codex workspace-write 원인 조사", { lineage_child_pane_ids: ["p-child"] });
export const CHILD = agent("p-child", "mailbox 쓰기 명령 sandbox 오류 해결", { group: "done", status_label: "Done", activity: "stopped", emphasized: true, lineage_parent_pane_id: "p-parent" });
export const OTHER = agent("p-dag", "그래프 다이어그램 DAG 시각화");

const pane = (id: string) => ({ id, herdr_label: null, terminal_title: null, cwd: "/repo", status_label: "", requires_close_confirmation: false, requires_close_status_check: false, identity_label: null });
const tab = (checkout: string, panes: string[]) => ({ id: `t-${checkout}`, workspace_id: "w1", checkout_id: checkout, label: "Tab", empty: false, delegated: false, panes: panes.map(pane) });

export const PR_275 = { number: 275, title: "Surface mailbox sandbox refusals", url: "https://github.com/acme/herdr-ide/pull/275", badge: "open", review: null, is_draft: false, checks: "pending", head_branch: "fix/mailbox-sandbox-letters", closing_issues: [{ repository: "acme/herdr-ide", number: 273 }] };
export const PR_260 = { number: 260, title: "Terminal links click path", url: "https://github.com/acme/herdr-ide/pull/260", badge: "merged", review: null, is_draft: false, checks: "passing", head_branch: "feat/terminal-links" };

export const RICH = {
  navigator: {
    focused_device_id: "local",
    agents: [PARENT, CHILD, OTHER],
    workspaces: [
      {
        id: "w1",
        label: "herdr-ide",
        path: "/repo",
        device_id: "local",
        registered: true,
        temporary: false,
        pinned: false,
        is_git: true,
        tasks: {
          source: { kind: "github", label: "GitHub", name: "acme/herdr-ide", reading: false, failure: null, last_read_at_unix_ms: READ_AT },
          tasks: [{ key: `github:acme/herdr-ide${HASH}273`, source: "github", id: `${HASH}273`, url: "https://github.com/acme/herdr-ide/issues/273", title: "mailbox 쓰기 명령이 sandbox 거부를 internal로 숨김", open: true }],
          overflow: false,
        },
        pull_requests: [PR_275, PR_260],
        inactive_checkouts: { expanded: false, checkout_ids: [] },
        checkouts: [
          { id: "c-main", workspace_id: "w1", label: "main", path: "/repo", branch: "main", purpose: null, is_worktree: false, exists: true, has_panes: true, pull_request: null, tabs: [tab("c-main", ["p-parent", "p-dag"])], active_tab_id: null, strip: [], next_tab_label: "Tab 2", github: { failure_category: null, available: true, loading: false, stale: false, last_success_at_unix_ms: READ_AT, unavailable_reason: null } },
          { id: "c-sand", workspace_id: "w1", label: "mailbox-sandbox-letters", path: "/repo.worktrees/sand", branch: "fix/mailbox-sandbox-letters", purpose: null, is_worktree: true, exists: true, has_panes: true, pull_request: PR_275, task_key: `github:acme/herdr-ide${HASH}273`, closes_task_keys: [`github:acme/herdr-ide${HASH}273`], changed_file_count: 14, ahead: 1, tabs: [tab("c-sand", ["p-child"])], active_tab_id: null, strip: [], next_tab_label: "Tab 2" },
        ],
      },
    ],
  },
} as unknown as SnapshotRest;

/** What the explicit GitHub search finds for the scene: a pull request and an issue this Mac does not hold. */
export const CMDK_RESULTS: GithubSearchResult[] = [
  { kind: "pr", repository: "acme/herdr-ide", number: 118, title: "Close stale sandbox watches", state: "merged", url: "https://github.com/acme/herdr-ide/pull/118" },
  { kind: "issue", repository: "acme/herdr-ide", number: 96, title: "mailbox sandbox 거부 로그가 비어 있음", state: "closed", url: "https://github.com/acme/herdr-ide/issues/96" },
];
