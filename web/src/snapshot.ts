// The parts of the core's `rest` section the web shell reads, typed as the
// core serializes them (herdr-core/src/model.rs), plus the pure selectors that
// resolve the operator's focused checkout, its visible tab and that tab's
// layout. Every selector is a lookup: the snapshot carries every tab's layout,
// so a tab switch never draws a waiting state (PRD S2 B3).

import type { ProviderUsage } from "./generated/hided-ws";

export type AgentRow = {
  id: string;
  pane_id: string;
  identity_label: string;
  agent_kind: string;
  symbol: string;
  group: string;
  status_label: string;
  detail?: string | null;
  elapsed: string;
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
};

export type DescendantCounts = { error: number; approval: number; question: number; working: number; done: number };

/** One agent in a line of them: a pane header chip or a lineage step's sibling (`AgentChipSnapshot`). */
export type AgentChip = {
  pane_id: string;
  label: string;
  detail: string | null;
  status_word_visible: boolean;
  agent_kind: string;
  demand: string;
  activity: string;
  emphasized: boolean;
  symbol: string;
  status_label: string;
  delegated: boolean;
};

/** What a pane's agent delegated (`PaneChildrenSnapshot`); absent on a pane with no agent. */
export type PaneChildren = {
  instrumented: boolean;
  uninstrumented_reason: string | null;
  uninstrumented_label: string | null;
  chips: AgentChip[];
};

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
};

/** How a repository's `gh` lookup is doing, apart from what it found (`GithubStatusSnapshot`). */
export type GithubStatus = {
  failure_category: string | null;
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
};

/** The issue a checkout is linked to and where the link came from (`IssueLinkSnapshot`). */
export type IssueLink = { issue: Issue; source: string };

/** A project's open issues, capped by the core (`ProjectIssuesSnapshot`). */
export type ProjectIssues = { repository: string | null; issues: Issue[]; overflow: boolean };

export type Purpose = { text: string; origin: string };

/** A checkout's agents by state; `unknown` is a subset of `seen`, not a fifth group. */
export type CheckoutAgentSummary = {
  representative_pane_id: string | null;
  needs_you: number;
  done: number;
  working: number;
  seen: number;
  unknown: number;
};

export type PaneRow = {
  id: string;
  herdr_label: string | null;
  terminal_title: string | null;
  workspace_label: string | null;
  cwd: string;
  status_label: string;
  requires_close_confirmation: boolean;
  requires_close_status_check: boolean;
  identity_label: string | null;
  children?: PaneChildren | null;
  lineage_path?: LineageStep[];
};

export type Tab = {
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
  blocked_reason: string | null;
  warnings: string[];
  button_label: string;
  can_delete_branch: boolean;
};

/** The parts of the core's worktree row the removal confirmation reads. */
export type WorktreeRow = {
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
  running_agent_count: number;
  deletion_gate: DeletionGate;
};

export type Checkout = {
  id: string;
  workspace_id: string;
  label: string;
  path: string;
  branch: string | null;
  purpose: Purpose | null;
  is_worktree: boolean;
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
  last_activity_unix_ms?: number | null;
  /** What `Remove project…` would close, counted by the core (D-10). */
  removal?: { pane_count: number; running_agent_count: number };
  checkouts: Checkout[];
  inactive_checkouts: { expanded: boolean; checkout_ids: string[] };
  /** The repository's open issues, for the Overview's backlog cards. */
  home_issues?: ProjectIssues;
};

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
  id: string;
  label: string;
  path: string;
  device_id: string;
  pinned: boolean;
};

export type PaneFind = {
  pane_id: string | null;
  term: string;
  index: number;
  total: number;
  truncated: boolean;
  unavailable_reason: string | null;
};

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
 * instead (`DocumentsSection`); the wire's `document` field is the Swift
 * shell's and is always null here.
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

export type ViewAreaSnapshot = { id: string; active: string | null; displays: ViewDisplaySnapshot[] };

/** `row`: first left, second right; `column`: first top, second bottom. `ratio` is the first child's share. */
export type ViewSplitSnapshot = { id: string; axis: "row" | "column"; ratio: number; first: ViewNode; second: ViewNode };

export type ViewNode = { area: ViewAreaSnapshot } | { split: ViewSplitSnapshot };

/** The front Workspace's View areas as the core owns them (S7 D-11). */
export type ViewLayoutSnapshot = {
  root: ViewNode;
  active_area: string;
  limits: { areas: number; depth: number; displays: number };
  display_count: number;
};

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
  /** The Swift shell's selected diff; the web shell reads `diffs`. */
  diff: { path: string; text: string; notice: string | null } | null;
  /** One bounded patch per visible diff display of the front Workspace (S7 contract 3.2); absent when none shows. */
  diffs?: DiffSnapshot[];
  unavailable_reason: string | null;
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
  /** `ready`, `needs_login`, `not_installed`, `unavailable`, `unsupported`, or `unread`. */
  state: string;
  headline: string;
  message: string | null;
  model: string;
  models: string[];
  models_unavailable_reason: string | null;
};

export type BackgroundAi = {
  provider: string;
  chosen: boolean;
  providers: AiProvider[];
  unavailable_reason: string | null;
};

export type AgentHookRuntime = {
  id: string;
  label: string;
  path: string;
  headline: string;
  installed: boolean;
  offers_install: boolean;
};

export type AgentHooks = {
  runtimes: AgentHookRuntime[];
  sessions_predating_install: { pane_id: string; label: string; message: string }[];
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
};

/** One worktree deletion: `closing` panes, `removing` on the core's worker, then `finished` or `failed`. */
export type WorktreeRemoval = {
  id: number;
  /** The device the worktree is on; null is the daemon's own machine. */
  device_id?: string | null;
  repository_root: string;
  checkout_path: string;
  branch: string | null;
  delete_branch: boolean;
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

export type SnapshotRest = {
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
  worktree_removal?: WorktreeRemoval | null;
  tab?: Tab;
  zoomed?: string | null;
  focused?: { pane_id?: string | null };
  pane_layouts?: PaneLayout[];
  terminal?: { pane_id?: string | null; panes?: TerminalPane[] };
  ui_state?: {
    left_sidebar_visible?: boolean;
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
    font_size?: number;
    /** The macOS pane chords the Swift app and the desktop app share, in the Swift app's format. */
    shortcut_bindings?: Record<string, string>;
    browser_shortcut_bindings?: Record<string, string>;
    [key: string]: unknown;
  };
  status?: {
    herdr?: HerdrStatus;
    remote?: RemoteStatus[];
    environment?: EnvironmentStatus[];
    agent_hooks?: AgentHooks;
    background_ai?: BackgroundAi;
    diagnostics?: CoreDiagnostic[];
    async_operations?: AsyncOperation[];
    pane_focus_request?: PaneFocusRequest | null;
    /** The core's most recent failure; the shell logs its detail (B5). */
    last_error?: { kind: string; message: string; retryable: boolean; occurred_at: number } | null;
  };
  recent_closed?: RecentClosed;
  explorer_operation?: ExplorerOperation | null;
  /** The front Workspace's layout and tools (S6 D-10); absent when no Workspace is in front. */
  workspace_view?: import("./workspace").WorkspaceView;
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
