// Shell-local UI state the core does not own: which overlay is open, the
// pending close confirmation, and the one-line notice. Everything the core
// owns (focus, tabs, layout, sidebar visibility) stays in `store.ts`.

import { create } from "zustand";
import type { Filter as DiskFilter } from "./diskCleanup";
import type { Relation } from "./lineage";
import type { Opening } from "./navigation";
import { NO_GRAPH_FILTER, type GraphFilter } from "./agentGraph";
import { NO_FILTER, type IssueFilter } from "./projectBoard";
import type { SettingsTab } from "./settings";
import type { CycleItem } from "./recent";
import type { NumberedFamily } from "./shortcuts";
import { placementForWidth, type ToolsPlacement, type ViewFocusRequest, type ViewWorkspace } from "./viewLayout";

export type HomeStart = { requestId: string; deviceId: string; refusal: string | null };

export type { ToolsPlacement, ViewFocusRequest } from "./viewLayout";

export type SidebarMode = "agents" | "projects";

/** The sidebar's tabs, left to right, and the order the switch walks; Projects is first and the
 * default (PRD sidebar-shell D-03). The Explorer is not one of them: it lives in the right
 * panel where the core's `right_panel_section` says it does (D-13). */
export const SIDEBAR_MODES: readonly SidebarMode[] = ["projects", "agents"];

/**
 * A close the operator is being asked about. The sheet holds only what is
 * being closed: every snapshot re-derives whether it is the Stop-work sheet
 * or the subtree sheet, who is listed and in what state, and the sheet
 * closes by itself only when the target is gone (PRD close-agent-subtree
 * B13, B28, D-20, D-39, D-40).
 */
export type PendingClose = {
  kind: "pane" | "tab";
  id: string;
  /** The SSH device the pane or tab is on, or null for this machine. */
  targetId: string | null;
};

/**
 * The scope the center shows, picked in the sidebar (PRD S6 D-02): All
 * projects (`main`) lists every Project, `overview` is one Project, and
 * Workspace is the front checkout. It is this page's own navigation, like
 * the sidebar's mode: every value it shows is the core's, and the Workspace
 * it shows is the core's front checkout.
 * `null` until the first snapshot decides where the page starts (D-11).
 * `main` is one device's all-projects Overview, the screen its Home row opens
 * (PRD home-device-rail D-13): `deviceId` names the device, and without one it
 * is the device in front.
 */
export type Screen = { kind: "main"; deviceId?: string } | { kind: "overview"; projectId: string; lens: OverviewLens } | { kind: "workspace" };

/**
 * How All projects is looked at: every Project's tasks, every agent, or the
 * list of Projects (PRD task-agents-views D-01). A Project's Overview has its
 * own tiles instead (`OverviewLens`).
 */
export type MainView = "tasks" | "agents" | "projects";

/** A Project Overview's tiles (PRD overview-lenses-tiles-agents D-02, D-36; overview-lenses-prs D-14): its agents, its issues, its pull requests, its sessions. */
export type OverviewTab = "agents" | "issues" | "prs" | "sessions";

/**
 * The PRs tab's own state (PRD overview-lenses-prs B5, B19, B21): the rows
 * unfolded, the row the keyboard or a chip asked for, and whether `최근 머지`
 * is open.
 */
export type PrLens = { open: readonly number[]; focus: number | null; merged: boolean };

export const NO_PR_LENS: PrLens = { open: [], focus: null, merged: false };

/**
 * How a Project's Overview is looked at, the screen's own page state (D-04,
 * D-17, agents-graph-view D-22): the tile, the Issues mode, the selected box,
 * the graph's folds opened (by `foldId`) and its filter, and the issue card an
 * issue chip asked for. It rides on the
 * screen, so Recent Panels brings a Project's Overview back exactly as it was
 * left (B11); every other way in starts from `entryLens`. Nothing here is
 * stored.
 */
export type OverviewLens = {
  tab: OverviewTab;
  tasksMode: TasksMode;
  /** The checkout whose box is selected in the Agents graph, or null. */
  box: string | null;
  /** The Agents graph's opened fold lines, by `foldId`. */
  folds: readonly string[];
  /** The Agents graph's status chips, search and device (B29). */
  graph: GraphFilter;
  /** The issue card to bring into view on the Issues tab. */
  focusTask: string | null;
  /** The issue whose panel is open beside the Issues board, by task key (PRD overview-lenses-issues D-08). */
  panel: string | null;
  /** The Issues filter (B21). */
  filter: IssueFilter;
  /** The PRs tab's rows and folds (PRD overview-lenses-prs). */
  prs: PrLens;
};

/** `folds` with `fold` opened, or closed again when it was open. */
export function toggledFold(folds: readonly string[], fold: string): string[] {
  return folds.includes(fold) ? folds.filter((open) => open !== fold) : [...folds, fold];
}

/**
 * Where every way into a Project's Overview lands (D-04, D-17, agents-graph-view
 * D-22): the Agents graph with the given box selected and no filter. The
 * Issues mode is the page's, the one All projects' Tasks shows too
 * (task-agents-views D-10).
 */
export function entryLens(box: string | null, tasksMode: TasksMode): OverviewLens {
  return { tab: "agents", tasksMode, box, folds: [], graph: NO_GRAPH_FILTER, focusTask: null, panel: null, filter: NO_FILTER, prs: NO_PR_LENS };
}

/**
 * How the Tasks view draws its tasks: the stage columns, one row per task
 * grouped by stage, or the tasks that wait on one another laid out left to
 * right (PRD task-agents-views D-02). Like the view it belongs to the page,
 * so another scope keeps it (B9).
 */
export type TasksMode = "board" | "list" | "dependencies";

/** `file_palette_beside` is ⌘P's list for "Open file to the side" (S7 B4): its pick opens beside the active View area. */
export type Overlay = "none" | "shortcuts" | "find" | "add_project" | "file_palette" | "file_palette_beside" | "diff_palette" | "search" | "settings";

/**
 * A notice the operator can act on: `refreshable` offers `refresh_status`
 * (activity unknown), and `dontSave` offers closing a view whose document
 * holds unsaved work this page cannot save, without saving it (S7 B5).
 */
export type Notice = {
  text: string;
  refreshable: boolean;
  dontSave?: { workspace: ViewWorkspace; displayId: string };
};

/** A project or checkout management dialog, named by the row that opened it. */
export type WorkspaceDialog =
  | { kind: "new_worktree"; workspaceId: string }
  /** A new issue, in the named project's source to begin with; the dialog can move it to another project. */
  | { kind: "new_issue"; workspaceId: string }
  /** Work started from an issue: a worktree named for it, its agent and first prompt. */
  | { kind: "start_issue"; workspaceId: string; taskKey: string }
  /** A pull request linked to an issue of its project's source (PRD overview-lenses-prs B10, B11). */
  | { kind: "pr_link"; workspaceId: string; prNumber: number; issueKey: string }
  /** A new issue made from a pull request's title and body, then linked (B12, B13). */
  | { kind: "pr_new_issue"; workspaceId: string; prNumber: number }
  /** A pull request handed to an agent on its branch (B15, B16, B18). */
  | { kind: "pr_delegate"; workspaceId: string; prNumber: number }
  | { kind: "purpose"; workspaceId: string; checkoutId: string }
  | { kind: "delete_worktree"; workspaceId: string; checkoutId: string }
  /** The disk cleanup sheet of a local Git project, opened on a filter (PRD disk-layers B1-B3). */
  | { kind: "disk_cleanup"; workspaceId: string; filter: DiskFilter }
  | { kind: "remove_project"; workspaceId: string };

/** A held-modifier cycle over Recent Panels or Recent Projects; committed when the modifier is released. */
export type Cycle = {
  kind: "panels" | "projects" | "area";
  scope?: import("./areaCycle").CycleScope;
  originKey?: string;
  items: CycleItem[];
  index: number;
};

/** The Explorer's inline name field: a new entry in `parent`, or a rename of
 * `path`. `parent` is where a create lands; a rename ignores it. */
export type ExplorerDraft = {
  kind: "file" | "folder" | "rename";
  parent: string;
  path: string;
  initial: string;
};

/** The trash confirmation: what is about to move, and the row the tree selects
 * once it is gone. */
export type PendingTrash = {
  path: string;
  name: string;
  isDirectory: boolean;
  selectAfter: string;
  /** The row's inode when the prompt opened: the host refuses an item that replaced it. */
  inode: number | null;
  /**
   * The checkout and device the prompt was opened for. A device's front
   * checkout follows its own Herdr focus and can move while the prompt is
   * open, so the confirmation goes to this target, not the one in front then (B34).
   */
  target: { root: string; device_id?: string };
};

type UiStore = {
  screen: Screen | null;
  /** All projects' view; the page keeps it while the operator visits a Project. */
  mainView: MainView;
  /** All projects' Tasks mode. */
  tasksMode: TasksMode;
  /** The focus asked for by a chip, a Return or a relationship Open, until another replaces it (S6 B15, B16). */
  relation: Relation | null;
  sidebarMode: SidebarMode;
  /**
   * The Home row's new-tab start this page sent: its `request_id` until its
   * pane is opened, then, when it was refused, the reason that device's Home
   * row shows until the next start or a click on the row (PRD home-device-rail B21).
   */
  homeStart: HomeStart | null;
  /** The Explorer row the operator last touched; the core owns the opened
   * document's `selected_path`, and a reveal syncs that into here. */
  explorerSelection: string | null;
  /** Bumped by ⌘F while a document shows; the editor of `editorFindDisplay` opens its find panel. */
  editorFindRequest: number;
  /** The display the latest ⌘F is for; one display answers it, whichever else shows the document. */
  editorFindDisplay: string | null;
  /** The `pane_find_open` whose answer ⌘F waits for, until the find bar or the agent's own search takes it. */
  agentFindRequest: string | null;
  viewFocusRequest: ViewFocusRequest | null;
  /**
   * How the Workspace tools stand (S7 B12, D-08): the column while the side
   * panel has room for it, else an overlay that stays closed until the
   * operator asks for a tool. The Workspace screen keeps it in step with the
   * panel's width; the tools the core stores are never changed by it.
   */
  toolsPlacement: ToolsPlacement;
  /** A tool was asked for while the side panel was closed, so the overlay opens with the panel if it turns out narrow. */
  toolsAsked: boolean;
  /** The Explorer's inline name field, or null. */
  explorerDraft: ExplorerDraft | null;
  /** The trash confirmation the Explorer is showing, or null. */
  pendingTrash: PendingTrash | null;
  overlay: Overlay;
  /** The tab the Settings sheet opens on. */
  settingsTab: SettingsTab;
  pendingClose: PendingClose | null;
  cycle: Cycle | null;
  commandRequest: { id: import("./shortcuts").CommandId } | null;
  setCommandRequest: (commandRequest: { id: import("./shortcuts").CommandId } | null) => void;
  /** A notice the operator can act on. */
  notice: Notice | null;
  /** The management dialog a sidebar menu opened, or null. */
  workspaceDialog: WorkspaceDialog | null;
  /**
   * The task and removal this page started, so their later answers (an agent
   * start after the creation dialog closed, a removal that finished after its
   * dialog was hidden) are reported here and another page's are not.
   */
  watchedTask: number | null;
  /** The pane a creation made, to be focused once the snapshot lists it (B13). */
  focusWhenListed: string | null;
  /** A Workspace or agent asked for from All projects, an Overview or the Agents list, until it is in front or refused (S6 B2, B21). */
  opening: Opening | null;
  watchedRemoval: { deviceId: string; path: string; afterId: number } | null;
  /** True while a Shortcuts row is recording: the window listener then runs no command. */
  recordingShortcut: boolean;
  /**
   * The numbered family a modifier hold has revealed (`hints.ts`): the tabs
   * of the strip in front or the Agents list's rows carry their number
   * while it is set. Published by the window listener only on a change, so
   * an unrevealed hold renders nothing.
   */
  hint: NumberedFamily | null;
  /**
   * Escape handlers of the layers open inside an overlay, innermost last. The
   * window listener answers Escape with the innermost one first, so a nested
   * confirmation closes before the sheet behind it.
   */
  escapeLayers: (() => void)[];
  /**
   * The closers of the tooltips open on hover or focus (`Hint` and the
   * checkout card). Not Escape layers: a tooltip must not take the Escape a
   * focused terminal is about to receive, so its own dismiss closes it. An
   * Escape the shell consumes never reaches that dismiss, so the shell
   * closes these itself as it consumes.
   */
  tooltips: (() => void)[];
  setScreen: (screen: Screen) => void;
  setMainView: (view: MainView) => void;
  setTasksMode: (mode: TasksMode) => void;
  /** Changes the Project Overview's lens in place; a no-op on any other screen. */
  setLens: (patch: Partial<OverviewLens>) => void;
  setRelation: (relation: Relation | null) => void;
  setSidebarMode: (mode: SidebarMode) => void;
  setHomeStart: (homeStart: HomeStart | null) => void;
  toggleSidebarMode: () => void;
  setExplorerSelection: (path: string | null) => void;
  requestEditorFind: (displayId: string | null) => void;
  setAgentFindRequest: (requestId: string | null) => void;
  setViewFocusRequest: (request: ViewFocusRequest | null) => void;
  /** Follows the window: `column` when wide, a closed overlay when it turns narrow, unless a tool was asked for with the panel. */
  setToolsNarrow: (narrow: boolean) => void;
  /** The operator asked for a tool: a narrow window's overlay opens. */
  openTools: () => void;
  /** The operator asked for a tool while the side panel was closed. */
  askTools: () => void;
  /** Escape or a click outside closes a narrow window's overlay. */
  closeTools: () => void;
  setExplorerDraft: (draft: ExplorerDraft | null) => void;
  setPendingTrash: (trash: PendingTrash | null) => void;
  openOverlay: (overlay: Overlay) => void;
  closeOverlay: (overlay?: Overlay) => void;
  openSettings: (tab: SettingsTab) => void;
  setPendingClose: (pending: PendingClose | null) => void;
  setCycle: (cycle: Cycle | null) => void;
  setNotice: (notice: Notice | null) => void;
  setWorkspaceDialog: (dialog: WorkspaceDialog | null) => void;
  setWatchedTask: (id: number | null) => void;
  setFocusWhenListed: (paneId: string | null) => void;
  setOpening: (opening: Opening | null) => void;
  setWatchedRemoval: (removal: { deviceId: string; path: string; afterId: number } | null) => void;
  setRecordingShortcut: (recording: boolean) => void;
  setHint: (hint: NumberedFamily | null) => void;
  /** Registers an Escape layer and returns its removal. */
  pushEscape: (handler: () => void) => () => void;
  /** Registers an open tooltip's close and returns its removal. */
  pushTooltip: (close: () => void) => () => void;
};

export const useUiStore = create<UiStore>((set, get) => ({
  screen: null,
  mainView: "tasks",
  tasksMode: "board",
  relation: null,
  sidebarMode: "projects",
  homeStart: null,
  explorerSelection: null,
  editorFindRequest: 0,
  editorFindDisplay: null,
  agentFindRequest: null,
  viewFocusRequest: null,
  toolsPlacement: "column",
  toolsAsked: false,
  explorerDraft: null,
  pendingTrash: null,
  overlay: "none",
  settingsTab: "general",
  pendingClose: null,
  cycle: null,
  commandRequest: null,
  setCommandRequest: (commandRequest) => set({ commandRequest }),
  notice: null,
  workspaceDialog: null,
  watchedTask: null,
  focusWhenListed: null,
  opening: null,
  watchedRemoval: null,
  recordingShortcut: false,
  hint: null,
  escapeLayers: [],
  tooltips: [],
  // Moving by hand drops an open still waiting for its Workspace, so a late
  // answer does not pull the screen away from where the operator went.
  setScreen: (screen) => set({ screen, opening: null }),
  setMainView: (mainView) => set({ mainView }),
  setTasksMode: (tasksMode) => set({ tasksMode }),
  // A lens change is a new screen value, so Recent Panels records the
  // Overview as it now is; the open request stays, since nothing moved away.
  // An Issues mode chosen here is the page's as well.
  setLens: (patch) => {
    const screen = get().screen;
    if (screen?.kind !== "overview") return;
    set({ screen: { ...screen, lens: { ...screen.lens, ...patch } }, ...(patch.tasksMode ? { tasksMode: patch.tasksMode } : {}) });
  },
  setRelation: (relation) => set({ relation }),
  setSidebarMode: (sidebarMode) => set({ sidebarMode }),
  setHomeStart: (homeStart) => set({ homeStart }),
  toggleSidebarMode: () => {
    const index = SIDEBAR_MODES.indexOf(get().sidebarMode);
    set({ sidebarMode: SIDEBAR_MODES[(index + 1) % SIDEBAR_MODES.length] });
  },
  setExplorerSelection: (explorerSelection) => set({ explorerSelection }),
  requestEditorFind: (displayId) => set({ editorFindRequest: get().editorFindRequest + 1, editorFindDisplay: displayId }),
  setAgentFindRequest: (agentFindRequest) => set({ agentFindRequest }),
  setViewFocusRequest: (viewFocusRequest) => set({ viewFocusRequest }),
  setToolsNarrow: (narrow) => {
    const { toolsPlacement: current, toolsAsked } = get();
    const next = narrow && toolsAsked ? "open" : placementForWidth(current, narrow);
    if (next !== current || toolsAsked) set({ toolsPlacement: next, toolsAsked: false });
  },
  askTools: () => set({ toolsAsked: true }),
  openTools: () => {
    if (get().toolsPlacement === "closed") set({ toolsPlacement: "open" });
  },
  // A tool asked for with a panel that never opened is forgotten here too,
  // so a later narrow panel never opens its overlay by itself.
  closeTools: () => {
    const { toolsPlacement, toolsAsked } = get();
    if (toolsPlacement === "open" || toolsAsked) set({ toolsPlacement: toolsPlacement === "open" ? "closed" : toolsPlacement, toolsAsked: false });
  },
  setExplorerDraft: (explorerDraft) => set({ explorerDraft }),
  setPendingTrash: (pendingTrash) => set({ pendingTrash }),
  openOverlay: (overlay) => set({ overlay }),
  closeOverlay: (overlay) => {
    if (!overlay || get().overlay === overlay) set({ overlay: "none" });
  },
  openSettings: (settingsTab) => set({ overlay: "settings", settingsTab }),
  setPendingClose: (pendingClose) => set({ pendingClose }),
  setCycle: (cycle) => set({ cycle }),
  setNotice: (notice) => set({ notice }),
  setWorkspaceDialog: (workspaceDialog) => set({ workspaceDialog }),
  setWatchedTask: (watchedTask) => set({ watchedTask }),
  setFocusWhenListed: (focusWhenListed) => set({ focusWhenListed }),
  setOpening: (opening) => set({ opening }),
  setWatchedRemoval: (watchedRemoval) => set({ watchedRemoval }),
  setRecordingShortcut: (recordingShortcut) => set({ recordingShortcut }),
  setHint: (hint) => {
    if (get().hint !== hint) set({ hint });
  },
  pushEscape: (handler) => {
    set({ escapeLayers: [...get().escapeLayers, handler] });
    return () => set({ escapeLayers: get().escapeLayers.filter((layer) => layer !== handler) });
  },
  pushTooltip: (close) => {
    set({ tooltips: [...get().tooltips, close] });
    return () => set({ tooltips: get().tooltips.filter((tooltip) => tooltip !== close) });
  },
}));
