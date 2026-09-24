// The parts of the core's `rest` section the web shell reads, typed as the
// core serializes them (herdr-core/src/model.rs), plus the pure selectors that
// resolve the operator's focused checkout, its visible tab and that tab's
// layout. Every selector is a lookup: the snapshot carries every tab's layout,
// so a tab switch never draws a waiting state (PRD S2 B3).

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
};

export type PullRequest = {
  number: number;
  title: string;
  url: string;
  badge: "merged" | "closed" | "review" | "open";
  review: "review_required" | "changes_requested" | "approved" | null;
  is_draft: boolean;
};

export type Purpose = { text: string; origin: string };

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
  is_main: boolean;
  missing: boolean;
  dirty: boolean;
  changed_file_count: number;
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
  has_panes: boolean;
  /** The worktree row behind a Git checkout, or null for a plain folder. */
  worktree?: WorktreeRow | null;
  pull_request: PullRequest | null;
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
};

/** The disk state that makes a save a choice rather than a write: the draft's base revision and what the file holds now (null when it was removed). */
export type EditorConflictSnapshot = {
  opened_revision: string;
  disk_revision: string | null;
};

/** A save without a known result yet: `saving`, `unknown` (the answer was lost) or `checking` (being read back). */
export type EditorSaveSnapshot = {
  state: "saving" | "waiting" | "unknown" | "checking" | "refused";
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

/** The core's editor section: its tabs, which one shows, and that tab's document. */
/** A file a device is still reading; its tab shows when the answer arrives. */
export type EditorOpeningSnapshot = { workspace_id: string; checkout_id: string; path: string };

export type EditorSnapshot = {
  tabs: EditorTabSnapshot[];
  active_tab_id: string | null;
  document: EditorDocumentSnapshot | null;
  opening?: EditorOpeningSnapshot[];
};

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
  diff: { path: string; text: string; notice: string | null } | null;
  unavailable_reason: string | null;
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
    selected_path?: string | null;
    selected_pane_id?: string | null;
    accent_hex?: string;
    font_size?: number;
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
    /** The core's most recent failure; the shell logs its detail (B5). */
    last_error?: { kind: string; message: string; retryable: boolean; occurred_at: number } | null;
  };
  recent_closed?: RecentClosed;
  explorer_operation?: ExplorerOperation | null;
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
 * The core's editor section when it holds a showing tab, else null. The canvas
 * reads null as "the terminal owns the surface": the core keeps the editor's
 * tabs while a terminal tab shows, so the tabs alone do not say what is drawn.
 */
export function editorFor(editor: EditorSnapshot | null): EditorSnapshot | null {
  return editor?.active_tab_id ? editor : null;
}

/**
 * The device file or diff the editor shows in `checkout`, or null when the host's
 * terminal tab does. The core keeps one active editor tab across devices, so
 * a tab of another checkout is not this surface's.
 */
export function remoteEditorTab(editor: EditorSnapshot | null, checkout: Checkout): EditorTabSnapshot | null {
  const tab = activeEditorTab(editor);
  return tab && (tab.kind === "file" || tab.kind === "diff") && tab.workspace_id === checkout.workspace_id && tab.checkout_id === checkout.id ? tab : null;
}

/** The editor tab the core says is showing, or null when the terminal does. */
export function activeEditorTab(editor: EditorSnapshot | null): EditorTabSnapshot | null {
  const active = editorFor(editor);
  if (!active) return null;
  return active.tabs.find((tab) => tab.id === active.active_tab_id) ?? null;
}

/** The editor tab a strip entry stands behind, or null for a Herdr entry. */
export function editorTabFor(editor: EditorSnapshot | null, tabId: string): EditorTabSnapshot | null {
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
