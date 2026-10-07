// The parts of the core's `rest` section the web shell reads, typed as the
// core serializes them (herdr-core/src/model.rs), plus the pure selectors that
// resolve the operator's focused checkout, its visible tab and that tab's
// layout. Every selector is a lookup: the snapshot carries every tab's layout,
// so a tab switch never draws a waiting state (PRD S2 B3).

import type { ProviderUsage } from "./generated/hided-ws";

/** The status codes the core sends in `status_code`; the catalogs name one word for each (agents.status.*). */
export type AgentStatusCode = "error" | "question" | "approval" | "working" | "done" | "idle" | "unknown" | "waiting" | "attached" | "sleeping" | "waking" | "sleep_failed";

export type AgentRow = {
  id: string;
  pane_id: string;
  identity_label: string;
  agent_kind: string;
  /** The checkout that physically owns this pane. */
  checkout_label?: string | null;
  /** Presentation-only device facts attached by `allAgents`. */
  device_id?: string;
  device_label?: string;
  symbol: string;
  group: string;
  status_code: AgentStatusCode;
  detail?: string | null;
  /** Everything the agent last said through its hooks, uncut: the request, then the progress, one per line; absent when it said nothing. */
  message?: string | null;
  /** When the core last saw this agent change state (epoch ms); the shell counts the elapsed time from it. `null` before the core observed it. */
  changed_at_unix_ms: number | null;
  /** The core's ordering key: `changed_at_unix_ms` as thirteen digits, else Herdr's state sequence zero-padded, so it sorts as text. */
  last_activity?: string;
  emphasized: boolean;
  unread: boolean;
  requires_close_confirmation?: boolean;
  requires_close_status_check?: boolean;
  unknown?: boolean;
  demand?: string;
  activity?: string;
  /** Work another agent delegated; it is only ever Working or Seen (docs/status-model.md). */
  delegated?: boolean;
  lineage_parent_pane_id?: string | null;
  lineage_child_pane_ids?: string[];
  /** Every live descendant pane in the order closing this row takes them, deepest first; absent when none (PRD close-agent-subtree D-20). */
  close_descendant_pane_ids?: string[];
  /** What every live descendant is doing, counted by state; unknown activity is in none. */
  descendant_counts?: DescendantCounts;
  /** A quiet root whose live descendant is still working or asking: drawn as a ring in Working (docs/status-model.md). */
  waiting_on_descendants?: boolean;
  /** How deep under its lineage root; a root is 0. */
  lineage_depth?: number;
  /** Whether the operator has this row's descendants folded away; folded is the default. */
  lineage_collapsed?: boolean;
  /** The checkout this row runs in, set only when it differs from its parent's. */
  lineage_worktree_badge?: string | null;
  /** Present only while Hide holds this agent asleep (PRD agent-sleep); absent when awake. */
  sleep?: AgentSleep;
  /** The conversation id Herdr recorded for this agent; absent when it recorded none. */
  session_id?: string | null;
  /** The agent searches its own conversation, so ⌘F asks the core where the search goes (`pane_find_open`). */
  own_find?: boolean;
  /** The request view's block (`AgentRequestSnapshot`); absent on a row the core has not laid it on. */
  request?: AgentRequest;
};

/** How the label read a turn's end (`LabelEnd`). */
export type LabelEnd = "working" | "question" | "done" | "waiting" | "unfinished";

/** What a row asks of the operator now (`RequestVerb`), in the order the request view draws its groups. */
export type RequestVerb = "answer" | "fix" | "review" | "stopped" | "result" | "working" | "waiting" | "idle";

/** Who sent the request a row shows (`RequestSender`). */
export type RequestSender = { kind: "operator" } | { kind: "named"; name: string } | { kind: "agent" };

/** The request view's part of an agent row (`AgentRequestSnapshot`, PRD overview-request-view). */
export type AgentRequest = {
  verb: RequestVerb;
  /** When the row took this verb; kept across a restart (D-40). */
  verb_since_unix_ms: number;
  /** The label's line for the turn (B18, B47); absent with summaries off or no analysis, when the reply stands in (D-12). */
  line?: string;
  /** How the label read the turn's end (`contracts/snapshot-wire-enums.json`: `label_end`). */
  end?: LabelEnd;
  /** The operator's last request, else the last one another agent sent. */
  request: { text: string; cut: boolean; images: number; at_unix_ms: number; sender: RequestSender } | null;
  /** Who sent a request after the operator's last one (B4). */
  later_by: RequestSender | null;
  /** The agent's last words. */
  reply: { text: string; cut: boolean; at_unix_ms: number } | null;
  /** The chip first (D-46), then the other live ones, then settled ones. */
  pull_requests: AgentPullRequest[];
};

/** One of a row's pull requests (`AgentPullRequestSnapshot`). */
export type AgentPullRequest = {
  number: number;
  title: string;
  url: string;
  badge: PullRequest["badge"];
  checks: NonNullable<PullRequest["checks"]>;
  head_branch: string;
  closing_issues: IssueReference[];
  /** Drawn as the row's chip or counted in its `+N` (D-43). */
  live: boolean;
  /** This row holds the pull request's duty (D-31). */
  duty: boolean;
  /** The row's session made it, as opposed to its branch having it. */
  created: boolean;
  settled_at_unix_ms: number | null;
};

/** A sleeping agent's state (`AgentSleepSnapshot`): the row and the pane draw it. */
export type AgentSleep = {
  state: "sleeping" | "waking" | "failed";
  /** Why the last wake failed, in plain words; set only when `failed`. */
  reason?: string | null;
  /** When it fell asleep, in Unix milliseconds. */
  since_unix_ms: number;
  /** The last progress line it reported before it slept. */
  progress?: string | null;
};

/** The pane menu's Sleep agent item (`AgentSleepActionSnapshot`); absent where it does not apply. */
export type AgentSleepAction = { available: boolean; reason?: string | null };

export type DescendantCounts = { error: number; approval: number; question: number; working: number; done: number };

/** One agent in a line of them: a pane header chip or a lineage step's sibling (`AgentChipSnapshot`). */
export type AgentChip = {
  pane_id: string;
  label: string;
  checkout_label?: string | null;
  detail: string | null;
  status_word_visible: boolean;
  agent_kind: string;
  demand: string;
  activity: string;
  emphasized: boolean;
  symbol: string;
  status_code: AgentStatusCode;
  delegated: boolean;
};

/** What a pane's agent delegated (`PaneChildrenSnapshot`); absent on a pane with no agent. */
export type PaneChildren = {
  instrumented: boolean;
  uninstrumented_reason: string | null;
  uninstrumented_label: string | null;
  chips: AgentChip[];
  /** Whether Hide hears this pane's session; absent for an agent with no connection to judge. */
  connection?: PaneConnection | null;
};

/** Why a session is not connected: `codex_shared_server`, `started_before_hide`, or `setup_needed` (fix it in Settings, no Reopen). */
export type PaneConnectionReason = "codex_shared_server" | "started_before_hide" | "setup_needed";

/** `PaneConnectionSnapshot`: `reason` is present exactly when `connected` is false; `reopen` is the last Reopen of the pane. */
export type PaneConnection = {
  connected: boolean;
  /** Whether the popover offers Reopen: false for `setup_needed` and for a pane on another device. */
  can_reopen: boolean;
  reason: PaneConnectionReason | null;
  reopen: { state: "pending" } | { state: "failed"; reason: PaneReopenFailure } | null;
};

export type PaneReopenFailure = "start_refused" | "session_gone" | "codex_unread" | "agent_busy" | "end_refused";

/** One step of a pane's lineage, root first and ending at the pane itself. */
export type LineageStep = { pane_id: string; label: string; siblings: AgentChip[] };

/** The core's outcome of the latest pane focus that carried a request id. */
export type PaneFocusRequest = {
  request_id: string;
  target_pane_id: string;
  phase: "pending" | "succeeded" | "failed" | string;
  message: string | null;
  retryable: boolean;
};

export type PullRequest = {
  number: number;
  title: string;
  url: string;
  badge: "merged" | "closed" | "review" | "open";
  review: "review_required" | "changes_requested" | "approved" | null;
  is_draft: boolean;
  /** The CI rollup; unknown and absent checks never read as a pass (`PullRequestChecks`). */
  checks?: "unknown" | "none" | "pending" | "failed" | "passing";
  merged_at_unix_ms?: number | null;
  updated_at_unix_ms?: number | null;
  /** The branch the pull request comes from. */
  head_branch?: string;
  /** The issues its body closes when it merges, as GitHub reads the body. */
  closing_issues?: IssueReference[];
};

/** Why a `gh` lookup failed, as the core names it; the catalogs word each (issueSettings.gh*). */
export type GithubFailureCategory = "not_installed" | "not_logged_in" | "no_github_remote" | "network_or_rate_limit";

/** How a repository's `gh` lookup is doing, apart from what it found (`GithubStatusSnapshot`). */
export type GithubStatus = {
  failure_category: GithubFailureCategory | null;
  available: boolean;
  loading: boolean;
  stale: boolean;
  last_success_at_unix_ms: number | null;
  unavailable_reason: string | null;
};

export type IssueReference = { repository: string; number: number };

/** One GitHub issue as `gh` reported it (`IssueSnapshot`); `state` is `OPEN` or `CLOSED`. */
export type Issue = {
  reference: IssueReference;
  title: string;
  url: string;
  state: string;
  project_status: string | null;
  updated_at_unix_ms: number | null;
  created_at_unix_ms: number | null;
  /** Absent when the issue has none; the web reads a task's labels, not these. */
  labels?: IssueLabel[];
};

/** The issue a checkout is linked to and where the link came from (`IssueLinkSnapshot`). */
export type IssueLink = { issue: Issue; source: string };

/** A project's open issues, capped by the core (`ProjectIssuesSnapshot`). */
export type ProjectIssues = { repository: string | null; issues: Issue[]; overflow: boolean };

/**
 * One task of a project's source, in the core's source-neutral shape
 * (`TaskSnapshot`, PRD task-agents-views D-14). The web reads no source's own
 * shape: `source` is `github` or `local`, and a later source is drawn the same way.
 */
export type Task = {
  key: string;
  source: string;
  /** The id the source shows (`#N`, `owner/repo#N` for another repository, `L-N` for a local issue), or null when it has none. */
  id: string | null;
  url: string | null;
  title: string;
  open: boolean;
  /** When the source last changed it, for the backlog's order and age. */
  updated_at_unix_ms?: number | null;
  created_at_unix_ms?: number | null;
  /** Actual closure time; editing a closed issue does not reset it. */
  closed_at_unix_ms?: number | null;
  /** The open tasks this one waits on, possibly of another project (`TaskRefSnapshot`). */
  blocked_by?: TaskRef[];
  /** GitHub's sub-issues of this task with its own progress count; absent when it has none (`TaskSubIssuesSnapshot`). */
  sub_issues?: TaskSubIssues | null;
  /** The source's labels in its order, read with the list; absent when it has none, as every Local issue (`TaskLabel`). */
  labels?: IssueLabel[];
};

/** GitHub's `completed` of `total` sub-issues, and the sub-issues themselves. */
export type TaskSubIssues = { total: number; completed: number; items: TaskSubIssue[] };

/** One sub-issue, by the key a pull request's closing reference names it with. */
export type TaskSubIssue = { key: string; id: string | null; title: string; open: boolean };

/** Another task, by its key and the id this task's source shows for it. */
export type TaskRef = { key: string; id: string | null };

/** Where a project's tasks come from and how the last read went (`TaskSourceSnapshot`). */
export type TaskSource = {
  kind: string;
  label: string;
  name: string | null;
  /** No read has answered yet. */
  reading: boolean;
  /** The last read failed; the tasks are the answer before it. */
  failure: string | null;
  last_read_at_unix_ms: number | null;
  /** The operator chose this source from the project's Issue source menu rather than the default. */
  chosen?: boolean;
};

/** A project's tasks (`ProjectTasksSnapshot`); `source` is null only for a device's project. */
export type ProjectTasks = {
  source: TaskSource | null;
  tasks: Task[];
  overflow: boolean;
};

/** How starting work from an issue behaves (`IssueSettingsSnapshot`, Settings › Issues). */
export type IssueSettings = {
  ai_worktree_name: boolean;
  closes_instruction: boolean;
};

/**
 * The agent kind and each kind's model the last start chose (`ui_state.agent_start`,
 * PRD home-device-rail D-18): every start surface preselects them. Null kind and no
 * model mean the CLI's own default.
 */
export type AgentStartChoice = {
  kind: "claude" | "codex" | null;
  models: Partial<Record<"claude" | "codex", string>>;
};

/** A label as the issue's source colours it (`TaskLabel`); `color` is six hex digits. */
export type IssueLabel = { name: string; color: string | null };

export type IssueComment = { author: string | null; created_at_unix_ms: number | null; body: string };

/**
 * One issue as its panel and the Start dialog read it (`IssueDetailSnapshot`):
 * the body, and for a GitHub issue its labels, author, assignees and latest
 * comments. The fields past `message` are empty until `ready`.
 */
export type IssueDetail = {
  task_key: string;
  phase: "reading" | "ready" | "failed";
  body: string | null;
  message: string | null;
  labels: IssueLabel[];
  author: string | null;
  created_at_unix_ms: number | null;
  assignees: string[];
  /** Absent for a source with no comments (Local). */
  comment_count: number | null;
  comments: IssueComment[];
};

/** The Overview's issue work in flight (`IssueWorkSnapshot`), one slot each. */
export type IssueWork = {
  create: { id: number; workspace_id: string; phase: "working" | "ready" | "failed"; task_key: string | null; message: string | null } | null;
  detail: IssueDetail | null;
  name: { request_id: string; phase: "working" | "ready" | "failed"; name: string | null; message: string | null } | null;
  /** The answer to a Local issue's edit, by the web's request id. */
  update?: { request_id: string; task_key: string; phase: "ready" | "failed"; message: string | null } | null;
  /** ⌘K's explicit GitHub search, by the web's request id (`github_search`). */
  search?: GithubSearch | null;
};

/** One pull request or issue the core found on GitHub for ⌘K's search (`GithubSearchResultSnapshot`). */
export type GithubSearchResult = {
  kind: "pr" | "issue";
  repository: string;
  number: number;
  title: string;
  state: string;
  url: string;
  is_draft?: boolean;
};

/** The answer to the latest `github_search`: working while it runs, then ready with its results or failed. */
export type GithubSearch = {
  request_id: string;
  query: string;
  phase: "working" | "ready" | "failed";
  results: GithubSearchResult[];
  message: string | null;
};

/** A pull request linked to an issue (`PrLinkSnapshot`): the issue made, Hide's link, then `Closes #N` in the body. */
export type PrLink = {
  request_id: string;
  workspace_id: string;
  pr_number: number;
  /** The step working now, or the one that failed. */
  step: "create" | "link" | "body";
  phase: "working" | "ready" | "failed";
  issue_key: string | null;
  /** The id the source shows for the issue (`#12`, `L-3`). */
  issue_id: string | null;
  /** This request made the issue. */
  created: boolean;
  message: string | null;
};

/** A pull request's body, failed checks and standing change requests, read for a new issue made from it or an agent's first prompt (`PrFeedbackSnapshot`). */
export type PrFeedback = {
  request_id: string;
  pr_number: number;
  phase: "reading" | "ready" | "failed";
  /** The body once read. */
  body: string | null;
  failed_checks: { name: string; url: string | null }[];
  change_requests: { author: string | null; body: string }[];
  message: string | null;
};

/** The Overview's pull-request work in flight (`PrWorkSnapshot`), one slot each. */
export type PrWork = { link: PrLink | null; feedback: PrFeedback | null };

export type Purpose = { text: string; origin: string };

/** A checkout's agents by state; `unknown` is a subset of `seen`, not a fifth group. */
export type CheckoutAgentSummary = {
  representative_pane_id: string | null;
  needs_you: number;
  done: number;
  working: number;
  seen: number;
  unknown: number;
  /** How many agents here draw each mark on their own row. */
  marks: MarkCounts;
};

/** How many rows draw each status mark (`MarkCountsSnapshot`); a row Herdr cannot classify is in none. */
export type MarkCounts = { error: number; approval: number; question: number; working: number; done: number; idle: number };

export type PaneRow = {
  ports?: number[];
  servers?: { host: string; port: number }[];
  id: string;
  herdr_label: string | null;
  terminal_title: string | null;
  cwd: string;
  status_code: AgentStatusCode;
  requires_close_confirmation: boolean;
  requires_close_status_check: boolean;
  identity_label: string | null;
  children?: PaneChildren | null;
  lineage_path?: LineageStep[];
  sleep?: AgentSleep;
  sleep_action?: AgentSleepAction;
};

export type TabAgent = Pick<AgentRow, "agent_kind" | "symbol" | "demand" | "activity" | "emphasized" | "waiting_on_descendants" | "status_code">;

export type Tab = {
  agent?: TabAgent | null;
  id: string | null;
  workspace_id: string | null;
  checkout_id: string | null;
  label: string | null;
  empty: boolean;
  delegated: boolean;
  panes: PaneRow[];
};

export type StripTab = {
  id: string;
  kind: "herdr" | "file" | "diff" | "session" | "memory";
  source_id: string;
  label: string;
  preview: boolean;
};

/** The removal gate the core computed for a linked worktree (`WorktreeDeletionGateSnapshot`). */
export type DeletionGate = {
  /** Main, locked or unavailable ignored-repository measurement. */
  blocked_reason: string | null;
  warnings: string[];
  button_label: string;
  can_delete_branch: boolean;
  /** What deleting the branch loses; present means the core deletes it with `git branch -D`. */
  branch_warning: string | null;
  /** The checkbox that accepts losing the folder's changes; present means deletion waits for it. */
  discard_label: string | null;
};

/** The parts of the core's worktree row the removal confirmation reads. */
export type WorktreeRow = {
  lock_reason?: string | null;
  ignored_repositories?: string[];
  ignored_scan_unavailable?: string | null;
  path: string;
  branch: string | null;
  head_sha: string | null;
  last_commit_unix_seconds?: number | null;
  is_main: boolean;
  missing: boolean;
  dirty: boolean;
  changed_file_count: number;
  /** Whether the branch is merged into its base; null when it could not be told. */
  merged?: boolean | null;
  /** Commits the upstream has that this branch does not, as of the last fetch; null when unread or no upstream. */
  behind_upstream?: number | null;
  pane_count: number;
  deletion_gate: DeletionGate;
  /** What the core measured of this checkout's folder; its `layers` split the size by what a build tool remakes. */
  disk?: WorktreeDisk;
};

/** One layer's share of a checkout: allocated bytes, how many folders, the biggest one's name (never a path). */
export type DiskCell = { bytes: number; folders: number; largest_name: string | null };

/** A checkout's allocated bytes split into the layers cleanup names; Git-visible files are `source_bytes`. */
export type DiskLayers = { build_cache: DiskCell; dependencies: DiskCell; other: DiskCell; source_bytes: number };

/** The two layers a cleanup may empty; `other` is size only (PRD disk-layers D-10). */
export type CacheLayer = "build_cache" | "dependencies";

export type WorktreeDisk = {
  path?: string | null;
  /** Null until measured and when a part could not be read. */
  total_bytes: number | null;
  unavailable_reason: string | null;
  measured_at_unix_ms?: number | null;
  largest_child_name?: string | null;
  largest_child_bytes?: number | null;
  layers?: DiskLayers | null;
  volume_free_bytes?: number | null;
};

export type Checkout = {
  id: string;
  workspace_id: string;
  label: string;
  path: string;
  branch: string | null;
  purpose: Purpose | null;
  is_worktree: boolean;
  /** Core-owned home choice; absent in snapshots from older daemons. */
  is_primary?: boolean;
  exists: boolean;
  /** A checkout Hide opened outside every registered project. */
  temporary?: boolean;
  has_panes: boolean;
  /** Who works here, counted by state, and the one agent that speaks for them (`CheckoutAgentSummary`). */
  agent_summary?: CheckoutAgentSummary;
  /** The worktree row behind a Git checkout, or null for a plain folder. */
  worktree?: WorktreeRow | null;
  pull_request: PullRequest | null;
  /** The issue this checkout's work is linked to. */
  issue?: IssueLink | null;
  /** The key of the task in its project's `tasks` this checkout works on. */
  task_key?: string | null;
  /** The other tasks this checkout's pull request closes; each shows the same pull request. */
  closes_task_keys?: string[];
  github?: GithubStatus;
  changed_file_count?: number;
  /** Commits on this branch since its base. */
  ahead?: number;
  tabs: Tab[];
  active_tab_id: string | null;
  strip: StripTab[];
  next_tab_label: string;
};

export type Workspace = {
  id: string;
  label: string;
  path: string;
  device_id: string;
  /** Set for a project on a registered SSH device; local projects carry null. */
  remote_target_id?: string | null;
  /** False while the operator has this project's checkouts folded (`ui_state.collapsed_workspace_ids`). */
  expanded?: boolean;
  is_git?: boolean;
  default_branch?: string | null;
  branches?: string[];
  registered: boolean;
  temporary: boolean;
  pinned: boolean;
  /** The device's Hide Home (`~/hide`, PRD home-device-rail D-01): drawn as the Home row, never in the Projects list. */
  is_home?: boolean;
  last_activity_unix_ms?: number | null;
  /** What `Remove project…` would close, counted by the core (D-10). */
  removal?: { pane_count: number; running_agent_count: number };
  checkouts: Checkout[];
  inactive_checkouts: { expanded: boolean; checkout_ids: string[] };
  /** The repository's open issues, in the shape older readers read. */
  home_issues?: ProjectIssues;
  /** The project's tasks, the Overview's Tasks and Agents views read these. */
  tasks?: ProjectTasks;
  /**
   * The project's pull requests for the Overview's PRs tab, one per branch:
   * every open one, and a merged one while its worktree is recorded here or
   * for 14 days after it merged (PRD overview-lenses-prs D-52). A local Git
   * project's only; absent while it has none.
   */
  pull_requests?: PullRequest[];
  /**
   * A local Git project's allocated disk, every worktree and the shared Git
   * directory counted once. Present once an Overview named the project for
   * measuring (`card_measure_disk` with its `workspace_id`).
   */
  disk?: ProjectDisk;
  /** The project's disk cleanup: its review, its progress and its result, present on the project the last `cleanup_review` named. */
  cleanup?: DiskCleanup | null;
};

export type DiskCleanupPhase = "loading" | "review" | "removing" | "complete" | "failed";

/** Why a worktree cannot be removed by the sheet; the shell words each code. */
export type CleanupExclusionCode =
  | "main"
  | "locked"
  | "unavailable"
  | "missing"
  | "alias"
  | "current"
  | "contains_worktree"
  | "in_use"
  | "pane_open"
  | "main_unavailable"
  | "detached"
  | "dirty"
  | "not_merged"
  | "merge_unverified"
  | "nested_repository"
  | "unverified";

/** `unverified` is a checkout the core has no facts about, which is never read as idle. */
export type CleanupInUse = { code: "agent_working" | "agent_waiting" | "descendant_busy" | "process" | "port" | "unverified"; name: string | null; port: number | null };

export type CleanupRow = {
  path: string;
  branch: string | null;
  head: string | null;
  is_main: boolean;
  exclusion_code: CleanupExclusionCode | null;
  exclusion_count: number | null;
  in_use: CleanupInUse | null;
  /** How many live panes of the checkout close with it: finished agents and shells nothing here is using. */
  pane_count: number;
  /** The worktree's own outcome once confirmed: `skipped` when a recheck found it changed or busy, `failed` when Git refused. */
  result: "removed" | "skipped" | "failed" | null;
  /** Why a skipped or failed worktree stayed: an exclusion code, `changed`, `not_found`, `unverified`, `close_refused` (a pane would not close) or `remove_refused`. */
  result_code: CleanupResultCode | null;
  /** The allocated size of the worktree this run removed. */
  bytes: number | null;
};

export type CleanupResultCode = CleanupExclusionCode | "changed" | "not_found" | "unverified" | "close_refused" | "remove_refused";

export type CleanupCellSkip = "in_use" | "changed" | "tracked_files" | "nested_repository" | "symlink" | "not_found" | "unverified";

export type CleanupCellResult = {
  path: string;
  layer: CacheLayer;
  outcome: "removed" | "skipped" | "failed";
  bytes: number;
  folders: number;
  /** What kept a folder of the cell (or all of it): a recheck code, or `io` when a move failed. */
  reason_code: CleanupCellSkip | "io" | null;
};

export type DiskCleanup = {
  id: number;
  workspace_id: string;
  repository_root: string;
  phase: DiskCleanupPhase;
  main_head: string | null;
  message: string | null;
  /** Set when in-use could not be read (no Herdr connection, a failed process read): nothing is selectable. */
  usage_error: string | null;
  /** The in-use reads are in: cache cells may be ticked while worktree checks still run (phase `loading`). */
  usage_ready: boolean;
  free_bytes: number | null;
  progress: { done: number; total: number } | null;
  rows: CleanupRow[];
  cell_results: CleanupCellResult[];
  free_before: number | null;
  free_after: number | null;
};

export type ProjectDisk = {
  /** Known only once every part is measured. */
  total_bytes: number | null;
  unavailable_reason: string | null;
  /** The named measurement has not come back yet. */
  measuring: boolean;
  /** Free space on the volume the project sits on, as of the last measurement. */
  free_bytes?: number | null;
  /** What the checkouts that could be measured add up to; the entrance shows it as a subtotal while `total_bytes` is null. */
  confirmed_bytes?: number | null;
  /** The layers of the checkouts that could be measured, summed; absent only while none was. */
  layers?: ProjectDiskLayers | null;
};

export type ProjectDiskLayers = { build_cache: number; dependencies: number; source: number; other: number; shared_git: number };

export type InactiveProjectGroup = { device_id: string; expanded: boolean; project_ids: string[] };

export type LayoutNode =
  | { type: "pane"; pane_id: string }
  | { type: "split"; direction: "right" | "down"; ratio: number; first: LayoutNode; second: LayoutNode };

export type PaneLayout = {
  workspace_id: string;
  tab_id: string;
  focused_pane_id: string;
  zoomed: boolean;
  root: LayoutNode;
};

export type TerminalPane = {
  pane_id: string;
  closed: boolean;
  exit_code: number | null;
  transport_state: string;
  transport_message: string | null;
  /** This client only observes the pane and Herdr did not move it for the last wheel; absent while false. */
  scroll_held_elsewhere?: boolean;
};

export type AsyncOperation = {
  id: string;
  kind: string;
  target_id: string;
  scope_id: string;
  phase: string;
  stage: string;
  message: string | null;
  retryable: boolean;
};

export type RecentClosedPending = {
  key: string;
  target_id: string;
  label: string;
  phase: string;
  checking: boolean;
  message: string | null;
  retryable: boolean;
};

export type RecentClosed = {
  count: number;
  top_label: string | null;
  restoring: boolean;
  pending: RecentClosedPending[];
  can_reopen: boolean;
  reopen_blocked_reason: string | null;
};

/**
 * One entry of ⌘K's Recent list: the ids that open the checkout and the names
 * its row is drawn from while its device has no catalog to read them from.
 */
export type RecentCheckout = { device_id: string; checkout_id: string; project_name: string; branch: string; device_name: string };

/** The explorer's most recent filesystem change and how far it got. */
export type ExplorerOperation = {
  id: number;
  kind: string;
  phase: string;
  path: string;
  destination: string;
  message: string | null;
};

export type WorkspaceRegistration = {
  primary_checkout_id?: string | null;
  id: string;
  label: string;
  path: string;
  device_id: string;
  pinned: boolean;
  /** This registration is the device's Home (`~/hide`). */
  home?: boolean;
};

export type PaneFind = {
  pane_id: string | null;
  term: string;
  index: number;
  total: number;
  truncated: boolean;
  unavailable_reason: string | null;
  /** Where the last `pane_find_open` sent the search, named by the request that asked. */
  opened?: { request_id: string; route: PaneFindRoute };
};

/** `agent`: the agent's own search took the pane; `bar`: Hide's find bar opens (`contracts/snapshot-wire-enums.json`). */
export type PaneFindRoute = "agent" | "bar";

/** What kind of document the core decided an open file is (`files::open`). */
export type DocumentKind = "text" | "markdown" | "image" | "pdf" | "binary";

export type EditorTabKind = "file" | "diff" | "session" | "memory";

/** One editor tab the core holds. Its id is the strip entry's `source_id`. */
export type EditorTabSnapshot = {
  id: string;
  workspace_id: string;
  checkout_id: string;
  path: string;
  label: string;
  kind: EditorTabKind;
  /** Which Changes group a diff tab shows; present only for diff tabs. */
  diff_committed: boolean | null;
  markdown_live: boolean;
  wrap: boolean;
  dirty: boolean;
  /** The checkout's one replaceable tab: a single click opens it, a double click promotes it. */
  preview: boolean;
  /** Why a View tab restored after a restart has no document (file gone, device unreachable); it only offers Close (S6 B20). */
  unavailable_reason?: string | null;
};

/** The disk state that makes a save a choice rather than a write: the draft's base revision and what the file holds now (null when it was removed). */
export type EditorConflictSnapshot = {
  opened_revision: string;
  disk_revision: string | null;
};

/** A save without a landed result: `saving`, `waiting` (for the device's helper), `unknown` (the answer was lost), `checking` (being read back), `refused` (refused or never sent) or `not_applied` (read back unchanged: not reached the file yet). */
export type EditorSaveSnapshot = {
  state: "saving" | "waiting" | "unknown" | "checking" | "refused" | "not_applied";
  message: string | null;
};

export type EditorDocumentSnapshot = {
  path: string;
  language: string | null;
  document_kind: DocumentKind;
  contents_utf8: string | null;
  /** The content revision (`sha256:<hex>`) the draft is based on; present for an editable document. */
  revision: string | null;
  dirty: boolean;
  /** Why an editable document takes no edits: its size or its permissions. */
  readonly_reason: string | null;
  conflict: EditorConflictSnapshot | null;
  save: EditorSaveSnapshot | null;
};

/** A file a device is still reading; its tab shows when the answer arrives. */
export type EditorOpeningSnapshot = { workspace_id: string; checkout_id: string; path: string };

/**
 * The core's editor section: every open document (one buffer per device,
 * checkout and document, S5.5) and the document of the active View area's
 * active display. The web shell reads documents from the `documents` section
 * instead (`DocumentsSection`); the wire's `document` field serves a
 * client without View areas and is always null here.
 */
export type EditorSnapshot = {
  tabs: EditorTabSnapshot[];
  active_tab_id: string | null;
  opening?: EditorOpeningSnapshot[];
};

/**
 * The documents the front Workspace's visible displays show (S7 contract
 * 3.1), a delta section of its own beside `editor`: `visible` names every
 * document on screen, `changed` carries only those past this reader's cursor.
 */
export type DocumentsSection = {
  visible: string[];
  changed: { tab_id: string; document: EditorDocumentSnapshot }[];
};

/** What a display shows now (S7 contract 3): its document, a read in flight, a root not readable yet, or a read that failed. */
export type ViewDisplayState = "open" | "opening" | "waiting" | "unavailable";

/** One place a file or diff is shown in a View area; several displays may show one document. */
export type ViewDisplaySnapshot = {
  id: string;
  /** The editor tab (document buffer) it shows; null while it is opening or waiting. */
  tab_id: string | null;
  /** The file or diff it shows; empty for a browser display. */
  path: string;
  /** The document's name, or a browser page's title (its host until it has one). */
  label: string;
  kind: "file" | "diff" | "browser";
  /** Which History group a diff shows; null for a file. */
  committed: boolean | null;
  preview: boolean;
  state: ViewDisplayState;
  /** Why it is waiting or unavailable. */
  reason: string | null;
  /** A browser display's address as the core last recorded it; absent for a file or diff. */
  url?: string | null;
  /** A browser display's page title as the core last recorded it. */
  title?: string | null;
  /** A browser display's load stamp: it moves when the core asks the page to load `url` (an open or a navigate). */
  load?: number | null;
};

export type ViewAreaSnapshot = import("./areaLayout").Area<ViewDisplaySnapshot>;
export type ViewSplitSnapshot = import("./areaLayout").AreaSplit<ViewDisplaySnapshot>;
export type ViewNode = import("./areaLayout").AreaNode<ViewDisplaySnapshot>;
export type ViewLayoutSnapshot = import("./areaLayout").AreaLayout<ViewDisplaySnapshot>;

export type DiffSnapshot = { path: string; committed: boolean; text: string; notice: string | null };

/** The six working-tree states the core presents (ChangedFileStatus). */
export type ChangedFileStatus = "modified" | "added" | "deleted" | "untracked" | "renamed" | "conflict";

export type ChangedFileSnapshot = {
  /** Absolute, so a row needs no second join against the root. */
  path: string;
  /** Relative to the checkout root, which is what the row shows. */
  relative_path: string;
  previous_relative_path: string | null;
  status: ChangedFileStatus;
  added_lines: number | null;
  removed_lines: number | null;
};

/**
 * One checkout's Git state. The core computes this only while the Changes or
 * Explorer surface is visible, so the web asks for it by sending that ui state
 * rather than by opening a second read path (PRD B1).
 */
export type ChangesSnapshot = {
  root_path: string | null;
  entries: ChangedFileSnapshot[];
  committed: ChangedFileSnapshot[];
  base_branch: string | null;
  selected_path: string | null;
  selected_committed: boolean;
  /** The selected diff of a client without View areas; the web shell reads `diffs`. */
  diff: { path: string; text: string; notice: string | null } | null;
  /** One bounded patch per visible diff display of the front Workspace (S7 contract 3.2); absent when none shows. */
  diffs?: DiffSnapshot[];
  unavailable_reason: string | null;
  /** The reason is only that Git finds no repository here: nothing to repair, so the Explorer draws no mark (issue 570). Absent when false. */
  not_a_repository?: boolean;
  /** Why the latest read failed while these entries, from the last good read, are still shown (S5.5 B22). */
  stale_reason?: string | null;
};

export type DeviceTestStage = { stage: string; state: string; detail: string };

/**
 * Where a device's file and Git work runs (`DeviceHostSnapshot`). consent is
 * `this_machine`, `none`, `granted` or `outdated`; state is `ready`,
 * `connecting`, `not_allowed`, `identity_changed`, `unsupported` or
 * `unavailable`, with message saying why and what to do.
 */
export type DeviceHost = {
  consent: "this_machine" | "none" | "granted" | "outdated";
  helper_root: string | null;
  /** The folder the device's `hide` command is linked in, named like `helper_root`. */
  cli_dir: string | null;
  contract: number;
  bound_identity: string | null;
  granted_at_unix_ms: number | null;
  state: "ready" | "connecting" | "not_allowed" | "identity_changed" | "unsupported" | "unavailable";
  message: string | null;
  platform: string | null;
  helper_path: string | null;
};

export type Device = {
  id: string;
  label: string;
  /** `local` for the daemon's own machine, `remote` for an SSH device. */
  kind: string;
  /** `local`, or for an SSH device `ready`, `unavailable` or `disabled`. */
  state: string;
  message: string | null;
  /**
   * Which trust or sign-in step refused the connection (S5.5 B38):
   * `host_key_changed`, `host_key_unknown` or `authentication`; null otherwise.
   */
  problem?: string | null;
  ssh_alias: string | null;
  /** The Herdr socket the registration names on the device; null reads its default server. */
  herdr_socket_path?: string | null;
  agent_count: number;
  test: { state: string; checked_at_unix_ms: number | null; stages: DeviceTestStage[] } | null;
  host?: DeviceHost;
  /** What Hide's install kit has put on this machine (`KitSnapshot`). */
  kit?: Kit;
};

/** One part of the install kit (`contracts/snapshot-wire-enums.json`: `kit_component_id`). */
export type KitComponentId = "cli" | "claude_code_hook" | "codex_hook" | "coordination_retirement";

/** What a part is on its machine (`contracts/snapshot-wire-enums.json`: `kit_component_state`). */
export type KitComponentState = "installed" | "outdated" | "not_installed" | "removed" | "failed" | "absent" | "off";

/** Whether an agent can be switched on where a machine runs (`contracts/snapshot-wire-enums.json`: `kit_agent_availability`). */
export type KitAgentAvailability = "available" | "not_installed" | "unsupported_system";

/** One piece of an agent (its skill or its hook) on its machine. */
export type KitPiece = {
  state: KitComponentState;
  reason: string | null;
  location: string | null;
};

/**
 * One agent on one machine (`KitAgentSnapshot`): its switch, and what Hide put
 * there for it. `hook` is null for an agent that gets the skill only.
 */
export type KitAgent = {
  id: string;
  label: string;
  availability: KitAgentAvailability;
  enabled: boolean;
  /**
   * The machine's record holds the operator's own choice for this agent
   * (`KitAgentSnapshot.chosen`). An agent that is on only by default (Claude
   * Code, Codex) has none, so it is not kept under Installed once its
   * program is gone (B9).
   */
  chosen: boolean;
  skill: KitPiece;
  hook: KitPiece | null;
  /** Herdr's own integration for the agent; null for an agent the pinned Herdr has none for (Gemini CLI). */
  herdr?: KitPiece | null;
  /** Hide does only some of what it does for Claude Code with this agent: the row wears the Partial chip, on or off. */
  partial?: boolean;
  /** Every feature of the Partial popover in the table's order (`hide_kit::Feature` ids); `supported` is what this build does. */
  features?: { id: KitFeatureId; supported: boolean }[];
  /** The agent's open sessions on this machine; null for an agent with no connection to judge (Partial agents) and before the machine's sessions are read. */
  sessions?: KitAgentSessions | null;
  doc_url: string;
};

export type KitFeatureId =
  | "skill"
  | "guidance"
  | "letters"
  | "bell"
  | "memory"
  | "subagents"
  | "spawn_guard"
  | "herdr_integration"
  | "sleep"
  | "fork"
  | "start"
  | "titles";

/** `KitAgentSessionsSnapshot`: `not_connected` lists at most 32 sessions, and `not_connected_hidden` counts the rest. */
export type KitAgentSessions = {
  connected: number;
  not_connected: { pane_id: string; title: string; project: string; reason: PaneConnectionReason }[];
  not_connected_hidden: number;
};

/** How the last turn-off of Codex's shared server on a machine ended (`CodexDaemonOffSnapshot`). */
export type CodexDaemonOff =
  | { state: "pending" }
  | { state: "done" }
  | { state: "failed"; reason: "codex_missing" | "codex_refused" | "timed_out" | "unreachable" | "stop_failed" };

export type KitComponent = {
  id: KitComponentId;
  label: string;
  state: KitComponentState;
  reason: string | null;
  location: string | null;
};

/**
 * A machine's install kit (PRD device-parity B7): the same parts on This Mac
 * and every device. `unavailable` says why the kit does not run there at all;
 * `offers_reinstall` is true only while a part needs it; `shares_account_with`
 * names another registered device that reaches the same account there.
 */
export type Kit = {
  unavailable: string | null;
  busy: boolean;
  components: KitComponent[];
  /** Every agent Hide has an adapter for; empty until the machine was checked. */
  agents: KitAgent[];
  /** A kit read asked with Check again is under way on this machine (B10). */
  checking?: boolean;
  /** Why the machine's last read failed, a code; null when it did not (B10). */
  check_failed?: string | null;
  offers_reinstall: boolean;
  shares_account_with: string | null;
  /** What the machine's Codex answered about its shared daemon: true has it, false is older, null no answer. */
  codex_daemon?: boolean | null;
  /** Whether that Codex starts the shared server on its own now: true is the reason a pane reads not connected; null no answer or an older Codex. */
  codex_daemon_on?: boolean | null;
  /** The last turn-off of the shared server on this machine; absent until one was asked. */
  codex_daemon_off?: CodexDaemonOff | null;
};

/** One pane's rectangle in a remote tab, as fractions of the tab's area (`RemotePaneLayoutFrame`). */
export type RemotePaneFrame = { pane_id: string; x: number; y: number; width: number; height: number };

/** A remote tab's geometry: Herdr reports rectangles, not the local split tree (`RemotePaneLayoutSnapshot`). */
export type RemotePaneLayout = {
  workspace_id: string;
  tab_id: string;
  focused_pane_id: string;
  zoomed: boolean;
  frames: RemotePaneFrame[];
};

/**
 * What the core projected from one SSH device's Herdr (`RemoteSessionSnapshot`).
 * Every id is scoped to the target (`remote:<target>:workspace:…`, `…:tab:…`,
 * `…:pane:…`), so a remote id can never name a local pane, and the focus
 * fields are that Herdr's own.
 */
export type RemoteSession = {
  workspaces: Workspace[];
  agents: AgentRow[];
  active_tab_ids: Record<string, string>;
  focused_workspace_id: string | null;
  focused_checkout_id: string | null;
  focused_tab_id: string | null;
  focused_pane_id: string | null;
  pane_layouts: RemotePaneLayout[];
};

export type RemoteStatus = {
  target_id: string;
  /** `connected`, `not_connected`, `stale`, `disabled`, `socket_missing`, or a failure word. */
  state: string;
  message: string | null;
  herdr_version: string | null;
  /** The last session read from that host; kept while `stale`. */
  session?: RemoteSession | null;
  /** How far the device's helper has confirmed its projects (`device_catalog`). */
  catalog?: DeviceCatalog;
};

/**
 * `resolving`: the helper is being asked; `ready`: every directory is
 * confirmed; `unavailable`: the helper cannot be asked and `message` says why.
 * An unconfirmed directory is listed as its Herdr workspace alone.
 */
export type DeviceCatalog = {
  state: "resolving" | "ready" | "unavailable";
  message: string | null;
  refused: { path: string; message: string }[];
};

export type HerdrStatus = {
  state?: string;
  socket_path?: string | null;
  message?: string | null;
  expected_protocol?: number | null;
  received_protocol?: number | null;
  received_version?: string | null;
};

export type EnvironmentStatus = {
  key: string;
  required: boolean;
  format: string;
  state: string;
  absent_behavior: string;
  message: string;
};

export type CoreDiagnostic = { kind: string; message: string; occurred_at: number };

export type AiProvider = {
  id: string;
  label: string;
  /** The install kit's adapter id for the same agent (`claude-code`, `codex`, `gemini-cli`, `grok`, `opencode`, `pi`, `cursor`). */
  agent: string;
  /** `ready`, `needs_login`, `usage_limited`, `not_installed`, `unavailable`, `unsupported`, or `unread`. */
  state: string;
  headline: string;
  /** The provider layer's own reason code (`cannot_guarantee_read_only`, `model_not_offered:<model>`), never prose. */
  message: string | null;
  /** The agent's program was found. */
  installed: boolean;
  /** Runs on and Add agent offer it: installed, signed in, a backend, and a call that cannot change files. */
  selectable: boolean;
  /** When a usage limit ends (unix ms), when the agent said. */
  retry_at_ms: number | null;
  /** False when the agent has no sign-in check (Gemini CLI): `ready` then only means its program was found; absent from an older daemon. */
  login_checked?: boolean;
  /** The model it is asked for; empty is the CLI's own default. */
  model: string;
  models: string[];
  /** The list is the CLI's documented one and cannot be asked for (Gemini CLI). */
  models_fixed: boolean;
  /** "CLI default" can be chosen: the agent is asked with no `--model`. */
  cli_default: boolean;
  /** Why the list is empty: a code (`not_asked`, `not_observed`, or the provider's own). */
  models_unavailable_reason: string | null;
};

/** The agents tried, in this order, when Runs on cannot answer. */
export type AiFallback = { provider: string; model: string };

/** Why Runs on is not answering, and who answers instead; absent while it answers. */
export type AiRefusal = {
  provider: string;
  /** `usage_limited`, `needs_login`, `not_installed`, `unavailable` or `unsupported`. */
  reason: string;
  retry_at_ms: number | null;
  /** The listed agent answering instead; null when none can. */
  using: string | null;
};

/** One concrete Host of `~/.ssh/config`: where `ssh -G` says it leads and the device that already uses it. */
export type SshHost = {
  alias: string;
  /** `user@host:port`; null when it could not be resolved (`problem` says why). */
  address: string | null;
  /** The name of the registered device using this alias or reaching the same address. */
  added_as: string | null;
  problem: "ssh_missing" | "ssh_failed" | "timed_out" | null;
};

/** `idle` before the first `ssh_hosts_list`, `loading` while one runs (the last answer stays), `ready` once it is in. */
export type SshHosts = {
  state: "idle" | "loading" | "ready";
  hosts: SshHost[];
  /** The config names more concrete aliases than are listed. */
  truncated: boolean;
};

export type BackgroundAi = {
  /** Use Hide AI; off, no model is asked anything. Absent from an older daemon, which is on. */
  enabled?: boolean;
  /** Runs on; null when nobody chose and none can be chosen yet. */
  provider: string | null;
  chosen: boolean;
  /** The `에이전트 요약` switch; absent from an older daemon, which always summarizes. */
  agent_summary?: boolean;
  providers: AiProvider[];
  fallback?: AiFallback[];
  refusal?: AiRefusal | null;
  unavailable_reason: string | null;
};

/** Each machine's hook parts are its kit rows (`Device.kit`); this section keeps what only the panes say. */
export type AgentHooks = {
  last_report_failure: string | null;
};

/** The core's one task slot: a worktree creation, an agent start, a purpose write. */
export type TaskOperation = {
  id: number;
  /** The device the task runs on; null is the daemon's own machine. */
  device_id?: string | null;
  kind: string;
  /** `working`, `ready` or `failed`. */
  phase: string;
  repository_root: string | null;
  branch: string | null;
  base_branch: string | null;
  path: string | null;
  pane_id: string | null;
  agent_kind: string | null;
  message: string | null;
  /** `starting`, `started`, `failed` or `unknown`; null when no agent was chosen. */
  agent_phase: string | null;
  agent_message: string | null;
  /** The id the start surface sent (`agent_start_in_checkout.request_id`), so each surface reads only its own answer. */
  request_id?: string | null;
};

/**
 * The core's one repository clone, from Add a project's Clone from URL. The
 * URL is never here (it can carry credentials); `host` is what is shown.
 */
export type RepositoryClone = {
  id: number;
  host: string;
  /** The folder the clone creates, canonical; the registration carries the same path. */
  path: string;
  /** `cloning`, `cancelling`, then `finished`, `failed` or `cancelled`. */
  phase: string;
  /** Git's own stage, `Receiving objects` and the like. */
  stage: string | null;
  percent: number | null;
  message: string | null;
};

/** One worktree deletion: `checking` before any close, `closing` panes, `removing` on the core's worker, then `finished` or `failed`. */
export type WorktreeRemoval = {
  id: number;
  /** The device the worktree is on; null is the daemon's own machine. */
  device_id?: string | null;
  repository_root: string;
  checkout_path: string;
  branch: string | null;
  delete_branch: boolean;
  force_delete_branch?: boolean;
  discard_changes?: boolean;
  phase: string;
  message: string | null;
};

/** One session of a Project's history (`project_sessions.rows`), newest first. */
export type SessionRow = {
  id: string;
  /** `claude` or `codex`. */
  provider: string;
  provider_label: string;
  /** The provider file the session was read from; Copy source location copies it. */
  locator: string;
  checkout_path: string;
  first_human_request: string | null;
  started_at_unix_ms: number | null;
  /** 0 when neither the session nor its file carries a time. */
  updated_at_unix_ms: number;
  title: string | null;
  /** Why the session's file cannot be read, in the core's words; null when it can. */
  unavailable_reason: string | null;
};

/** One recorded turn of a session: `role` is `user` or `assistant`, `kind` `human`, `assistant`, `interrupted` or `injected`. */
export type ArchiveEvent = {
  source_offset?: number | null;
  role: string;
  kind: string;
  at_unix_ms: number;
  text: string;
};

export type ArchiveDetail = {
  id: string;
  kind: string;
  title: string;
  provider: string | null;
  unavailable_reason: string | null;
  events: ArchiveEvent[];
};

/** The session opened beside a Project's Sessions (`archive_open` with a `workspace_id`). */
export type ProjectSessionDetail = {
  session_id: string;
  /** Empty when the history no longer lists the session. */
  locator: string;
  loading: boolean;
  failure: string | null;
  archive: ArchiveDetail | null;
};

/**
 * The Sessions of the Project a screen named with `sessions_refresh` (PRD S8
 * D-03): its whole history, the one session open beside it, and why either
 * cannot be read. It rides its own section of the wire and is absent until a
 * Project is named; the core holds one named Project at a time.
 */
export type ProjectSessions = {
  device_id: string;
  workspace_id: string;
  /** Why this Project's sessions are not read here at all (another device, gone from the catalog); no Retry. */
  unavailable_reason: string | null;
  loading: boolean;
  /** Why reading the history failed; Retry reads it again. */
  failure: string | null;
  rows: SessionRow[];
  detail: ProjectSessionDetail | null;
};

/** Where a pull request's issue link comes from: GitHub's closing reference or Hide's branch link. */
export type LinkIssueSource = "closes" | "hide";
/** `created`: the session printed the pull request's address when GitHub made it. */
export type LinkSessionRole = "created" | "worked";
/** Whether the session's own file is still there; `unknown` for a device's file. */
export type LinkFileState = "present" | "missing" | "unknown";
export type LinkTarget = { kind: "pr"; number: number } | { kind: "issue"; key: string };

/** One session line of a link panel: a session, or several joined by continuation (`ids`, oldest first). */
export type LinkedSession = {
  agent: string;
  id: string;
  ids: string[];
  device_id: string;
  role: LinkSessionRole;
  /** The pull request the line belongs to. */
  pr: number;
  request: string | null;
  started_at_unix_ms: number | null;
  ended_at_unix_ms: number | null;
  path: string | null;
  cwd: string | null;
  file: LinkFileState;
  parent: { name: string; agent: string; session_id: string; available: boolean } | null;
};

/**
 * The link record for the one pull request or issue a panel shows (PRD
 * link-graph D-45), on its own section of the wire. Failures are codes the
 * shell words.
 */
export type LinkPanel = {
  workspace_id: string;
  target: LinkTarget;
  loading: boolean;
  failure: string | null;
  pr: {
    number: number;
    branch: string;
    title: string;
    url: string;
    created_at_unix_ms: number | null;
    closed_at_unix_ms: number | null;
    merged_at_unix_ms: number | null;
    issues: { key: string; source: LinkIssueSource }[];
    /** Recorded worktree paths for the branch, newest first. */
    worktrees: string[];
  } | null;
  /** The pull requests an issue target is linked to, newest first. */
  prs: number[];
  sessions: LinkedSession[];
  /** Every line the record holds for the target; `sessions` carries at most 200. */
  total: number;
};

/** Each Project's link counts and session chips by workspace id. */
export type LinkSummaries = {
  projects: Record<string, {
    prs?: Record<string, number>;
    issues?: Record<string, number>;
    sessions?: Record<string, { number: number; created: boolean }[]>;
  }>;
  /** Session files are still being read for the first time. */
  filling: boolean;
};

export type SnapshotRest = {
  git_worktrees_loading?: boolean;
  navigator?: {
    focused_workspace_id?: string | null;
    focused_checkout_id?: string | null;
    /** The focused checkout's root, which the Explorer reveals under. */
    root_path?: string | null;
    /** History's registered-folder scope; it may be narrower than root_path. */
    changes_root_path?: string | null;
    workspaces?: Workspace[];
    inactive_projects?: InactiveProjectGroup[];
    agents?: AgentRow[];
    devices?: Device[];
    focused_device_id?: string | null;
    /** Each provider's weekly window, typed by the contract (`providerUsage`). */
    provider_usage?: ProviderUsage[];
  };
  connection?: { kind: string; state: string; target_id: string | null };
  task_operation?: TaskOperation | null;
  issue_work?: IssueWork;
  pr_work?: PrWork;
  repository_clone?: RepositoryClone | null;
  worktree_removal?: WorktreeRemoval | null;
  tab?: Tab;
  zoomed?: string | null;
  focused?: {
    pane_id?: string | null;
    /** The last operator `focus_pane` number the core applied, per page (`operatorFocus.ts`). */
    operator_focus?: { client_id: string; sequence: number }[];
  };
  pane_layouts?: PaneLayout[];
  terminal?: { pane_id?: string | null; panes?: TerminalPane[] };
  ui_state?: {
    left_sidebar_visible?: boolean;
    device_rail_visible?: boolean;
    right_panel_visible?: boolean;
    right_panel_section?: string;
    workspace_registrations?: WorkspaceRegistration[];
    pane_text_scales?: Record<string, number>;
    editor_text_scale?: number;
    expanded_paths?: string[];
    /** Each SSH device's expanded Explorer folders; this machine's are `expanded_paths`. */
    device_expanded_paths?: Record<string, string[]>;
    /** Projects whose checkouts the sidebar folds. */
    collapsed_workspace_ids?: string[];
    /** Checkouts whose agent rows the Projects list opened; absence is closed, where line two names the agents. */
    expanded_checkout_ids?: string[];
    selected_path?: string | null;
    selected_pane_id?: string | null;
    accent_hex?: string;
    /** `system`, `light` or `dark`; the page reads anything else as Dark. */
    theme?: string;
    /** Core-confirmed explicit choice; null follows this client's system language. */
    interface_language?: "en" | "ko" | "zh-CN" | "ja" | null;
    /** The first-run agent choice (`contracts/snapshot-wire-enums.json`: `agent_onboarding`): absent until this Mac's kit answered, `pending` while its kit record waits for the operator's answer. */
    agent_onboarding?: "pending" | "done" | null;
    /** The agents left on in that choice. */
    agent_onboarding_agents?: string[];
    font_size?: number;
    /** The sidebar's width in CSS pixels, 220 to 440; the core refuses anything else (PRD sidebar-typography D-09). */
    sidebar_width?: number;
    /** The desktop app's macOS pane chords, in the removed native app's format. */
    shortcut_bindings?: Record<string, string>;
    browser_shortcut_bindings?: Record<string, string>;
    /** Sleep idle agents after this many hours; null or absent is Never (PRD agent-sleep). */
    agent_sleep_after_hours?: number | null;
    /** Each local project's chosen issue source (`github` or `local`) by path; absent is the default. */
    project_issue_sources?: Record<string, string>;
    issue_settings?: IssueSettings;
    agent_start?: AgentStartChoice;
    /** The checkouts last brought to the front, newest first, up to ten (PRD cmdk-recent). */
    recent_checkouts?: RecentCheckout[];
    /** The terminal panes the keyboard has been in, newest first, up to fifty: the Agent cycle's order (issue 301). */
    recent_pane_ids?: string[];
    [key: string]: unknown;
  };
  status?: {
    server_discovery?: { loading: boolean; failure: string | null };
    herdr?: HerdrStatus;
    remote?: RemoteStatus[];
    environment?: EnvironmentStatus[];
    agent_hooks?: AgentHooks;
    background_ai?: BackgroundAi;
    /** The Host entries Add device lists, read on an `ssh_hosts_list` event (PRD settings-cleanup D-19). */
    ssh_hosts?: SshHosts;
    diagnostics?: CoreDiagnostic[];
    async_operations?: AsyncOperation[];
    tab_rename?: { request_id: string; tab_id: string; label: string; phase: "pending" | "succeeded" | "failed" } | null;
    pane_focus_request?: PaneFocusRequest | null;
    /** The core's most recent failure; the shell logs its detail (B5). */
    last_error?: { kind: string; message: string; retryable: boolean; occurred_at: number; request_id?: string | null } | null;
  };
  recent_closed?: RecentClosed;
  explorer_operation?: ExplorerOperation | null;
  /** The front Workspace's layout and tools (S6 D-10); absent when no Workspace is in front. */
  workspace_view?: import("./workspace").WorkspaceView;
  /** All retained Browser Views, including those outside the front Workspace. */
  browser_views?: { device_id: string; path: string; view_id: string; area_id: string }[];
  /** Positive connected Workspace area authority from the core, including empty areas; incarnation changes on revoke/regrant. */
  browser_scopes?: { device_id: string; path: string; area_id: string; incarnation: number }[];
};

/**
 * The SSH device the operator selected, or null while this machine is the
 * context. With one selected, the local tabs are not what the operator is
 * looking at, so the shell neither draws them nor sends them pane commands
 * (PRD S5 B19), the way the native shell's remote context works.
 */
export function focusedRemoteDevice(rest: SnapshotRest | null): Device | null {
  const id = rest?.navigator?.focused_device_id;
  if (!id || id === "local") return null;
  return rest?.navigator?.devices?.find((device) => device.id === id && device.kind === "remote") ?? null;
}

export function focusedCheckout(rest: SnapshotRest | null): Checkout | null {
  const id = rest?.navigator?.focused_checkout_id;
  if (!id) return null;
  for (const workspace of rest?.navigator?.workspaces ?? []) {
    const checkout = workspace.checkouts.find((c) => c.id === id);
    if (checkout) return checkout;
  }
  return null;
}

/**
 * Every project the core's catalog carries: this machine's and registered ones
 * in the navigator, then each device's Herdr session. The core looks a
 * checkout up in the same two places (`catalog_checkout`); device ids are
 * scoped, so one id never names checkouts on two machines.
 */
export function catalogWorkspaces(rest: SnapshotRest | null): Workspace[] {
  return [
    ...(rest?.navigator?.workspaces ?? []),
    ...(rest?.status?.remote ?? []).flatMap((remote) => remote.session?.workspaces ?? []),
  ];
}

export function checkoutById(rest: SnapshotRest | null, id: string): Checkout | null {
  for (const workspace of catalogWorkspaces(rest)) {
    const checkout = workspace.checkouts.find((c) => c.id === id);
    if (checkout) return checkout;
  }
  return null;
}

/** The device a catalog checkout lives on; `local` for this machine's. */
export function deviceOfCheckout(rest: SnapshotRest | null, id: string): string {
  return catalogWorkspaces(rest).find((workspace) => workspace.checkouts.some((c) => c.id === id))?.device_id ?? "local";
}

/**
 * The checkout the operator is looking at: this machine's focus while it is
 * the selected device, otherwise that device's own Herdr focus, which the core
 * follows (`front_checkout`).
 */
export function frontCheckout(rest: SnapshotRest | null): Checkout | null {
  const device = focusedRemoteDevice(rest);
  if (!device) return focusedCheckout(rest);
  const session = rest?.status?.remote?.find((remote) => remote.target_id === device.id)?.session;
  const id = session?.focused_checkout_id;
  if (!id) return null;
  return session?.workspaces.flatMap((workspace) => workspace.checkouts).find((c) => c.id === id) ?? null;
}

export function visibleTab(checkout: Checkout | null): Tab | null {
  if (!checkout?.active_tab_id) return null;
  return checkout.tabs.find((tab) => tab.id === checkout.active_tab_id) ?? null;
}

export function layoutForTab(rest: SnapshotRest | null, tabId: string | null): PaneLayout | null {
  if (!tabId) return null;
  return rest?.pane_layouts?.find((layout) => layout.tab_id === tabId) ?? null;
}

/**
 * The core's editor section when the active View area shows an open
 * document, else null: a chord that could mean the document or the pane
 * (⌘F, text size) reads null as "the terminal owns it".
 */
export function editorFor(editor: EditorSnapshot | null): EditorSnapshot | null {
  return editor?.active_tab_id ? editor : null;
}

/** The editor tab (document) with this id, or null once the core closed it. */
export function editorTabFor(editor: EditorSnapshot | null, tabId: string | null): EditorTabSnapshot | null {
  if (!tabId) return null;
  return editor?.tabs.find((tab) => tab.id === tabId) ?? null;
}

const NO_PATHS: string[] = [];

/**
 * What the Explorer draws: the selected device, the checkout in front on it,
 * and that device's expanded folders. A path is only ever read against its
 * own device, so the same path on two machines never shares a row (S5.5 B2).
 */
export function explorerContext(rest: SnapshotRest | null): { device: string; checkout: Checkout | null; expanded: string[] } {
  const device = focusedRemoteDevice(rest)?.id ?? "local";
  const state = rest?.ui_state;
  return {
    device,
    checkout: frontCheckout(rest),
    expanded: (device === "local" ? state?.expanded_paths : state?.device_expanded_paths?.[device]) ?? NO_PATHS,
  };
}

/** The changes of the checkout a listing belongs to, or null when they are another checkout's. */
export function changesFor(changes: ChangesSnapshot | null, rootPath: string | null): ChangesSnapshot | null {
  if (!changes || !rootPath || changes.root_path !== rootPath) return null;
  return changes;
}

export function paneIds(node: LayoutNode): string[] {
  if (node.type === "pane") return [node.pane_id];
  return [...paneIds(node.first), ...paneIds(node.second)];
}

/** The pane a divider between `first` and `second` names in `resize_pane`: the last pane of the first subtree. */
export function dividerPaneId(first: LayoutNode): string {
  const ids = paneIds(first);
  return ids[ids.length - 1] ?? "";
}

export type SessionSearchHit = { session_id: string; source_offset: number; role: string; at_unix_ms: number; snippet: string };
export type SessionSearch = { workspace_id: string; device_id: string; provider: string; query: string; loading: boolean; indexing: boolean; indexed: number; total: number; days: number; policy_loaded: boolean; control_failure: string | null; failure: string | null; page: { hits: SessionSearchHit[]; limited: boolean; stale: boolean } };
