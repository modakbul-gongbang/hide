// Shell-local UI state the core does not own: which overlay is open, the
// pending close confirmation, and the one-line notice. Everything the core
// owns (focus, tabs, layout, sidebar visibility) stays in `store.ts`.

import { create } from "zustand";
import type { Relation } from "./lineage";
import type { Opening } from "./navigation";
import type { SettingsTab } from "./settings";
import type { CycleItem } from "./recent";
import { placementForWidth, type ToolsPlacement, type ViewFocusRequest, type ViewWorkspace } from "./viewLayout";

export type { ToolsPlacement, ViewFocusRequest } from "./viewLayout";

export type SidebarMode = "agents" | "projects";

/** The order ⌘E walks. The Explorer is not one of them: it lives in the right
 * panel where the core's `right_panel_section` says it does (D-13). */
export const SIDEBAR_MODES: readonly SidebarMode[] = ["agents", "projects"];

export type PendingClose = {
  kind: "pane" | "tab";
  id: string;
  /** The SSH device the pane or tab is on, or null for this machine. */
  targetId: string | null;
  title: string;
  consequence: string;
  affected: string[];
};

/**
 * The scope the center shows, picked in the sidebar (PRD S6 D-02): All
 * projects (`main`) lists every Project, `overview` is one Project, and
 * Workspace is the front checkout. It is this page's own navigation, like
 * the sidebar's mode: every value it shows is the core's, and the Workspace
 * it shows is the core's front checkout.
 * `null` until the first snapshot decides where the page starts (D-11).
 */
export type Screen = { kind: "main" } | { kind: "overview"; projectId: string } | { kind: "workspace" };

/**
 * How a scope is looked at: its Tasks board, its Agents board, a Project's
 * session history (PRD S8), or All projects' list of Projects. It belongs to
 * the page, not to one scope, so choosing another Project keeps the view the
 * operator was using, and a scope without it shows its first view (PRD
 * task-agents-views D-01).
 */
export type ProjectView = "tasks" | "agents" | "sessions" | "projects";

/**
 * How the Tasks view draws its tasks: the stage columns, or the tasks that
 * wait on one another laid out left to right (PRD task-agents-views D-02).
 * Like the view it belongs to the page, so another scope keeps it (B9).
 */
export type TasksMode = "board" | "dependencies";

/** The view a scope draws: the page's own when the scope has it, else the scope's first. */
export function scopeView(view: ProjectView, views: readonly ProjectView[]): ProjectView {
  return views.includes(view) ? view : (views[0] ?? view);
}

/** `file_palette_beside` is ⌘P's list for "Open file to the side" (S7 B4): its pick opens beside the active View area. */
export type Overlay = "none" | "shortcuts" | "find" | "new_workspace" | "file_palette" | "file_palette_beside" | "search" | "settings";

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
  | { kind: "purpose"; workspaceId: string; checkoutId: string }
  | { kind: "delete_worktree"; workspaceId: string; checkoutId: string }
  | { kind: "remove_project"; workspaceId: string };

/** A held-modifier cycle over Recent Panels or Recent Projects; committed when the modifier is released. */
export type Cycle = {
  kind: "panels" | "projects";
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
  projectView: ProjectView;
  tasksMode: TasksMode;
  /** The focus asked for by a chip, a Return or a relationship Open, until another replaces it (S6 B15, B16). */
  relation: Relation | null;
  sidebarMode: SidebarMode;
  /** The Explorer row the operator last touched; the core owns the opened
   * document's `selected_path`, and a reveal syncs that into here. */
  explorerSelection: string | null;
  /** Bumped by ⌘F while a document shows; the editor of `editorFindDisplay` opens its find panel. */
  editorFindRequest: number;
  /** The display the latest ⌘F is for; one display answers it, whichever else shows the document. */
  editorFindDisplay: string | null;
  viewFocusRequest: ViewFocusRequest | null;
  /**
   * How the Workspace tools stand (S7 B12, D-08): the column while the side
   * panel has room for it, else an overlay that stays closed until the
   * operator asks for a tool. The Workspace screen keeps it in step with the
   * panel's width; the tools the core stores are never changed by it.
   */
  toolsPlacement: ToolsPlacement;
  /** The Explorer's inline name field, or null. */
  explorerDraft: ExplorerDraft | null;
  /** The trash confirmation the Explorer is showing, or null. */
  pendingTrash: PendingTrash | null;
  overlay: Overlay;
  /** The tab the Settings sheet opens on. */
  settingsTab: SettingsTab;
  pendingClose: PendingClose | null;
  cycle: Cycle | null;
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
   * Escape handlers of the layers open inside an overlay, innermost last. The
   * window listener answers Escape with the innermost one first, so a nested
   * confirmation closes before the sheet behind it.
   */
  escapeLayers: (() => void)[];
  setScreen: (screen: Screen) => void;
  setProjectView: (view: ProjectView) => void;
  setTasksMode: (mode: TasksMode) => void;
  setRelation: (relation: Relation | null) => void;
  setSidebarMode: (mode: SidebarMode) => void;
  toggleSidebarMode: () => void;
  setExplorerSelection: (path: string | null) => void;
  requestEditorFind: (displayId: string | null) => void;
  setViewFocusRequest: (request: ViewFocusRequest | null) => void;
  /** Follows the window: `column` when wide, a closed overlay when it turns narrow. */
  setToolsNarrow: (narrow: boolean) => void;
  /** The operator asked for a tool: a narrow window's overlay opens. */
  openTools: () => void;
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
  /** Registers an Escape layer and returns its removal. */
  pushEscape: (handler: () => void) => () => void;
};

export const useUiStore = create<UiStore>((set, get) => ({
  screen: null,
  projectView: "tasks",
  tasksMode: "board",
  relation: null,
  sidebarMode: "agents",
  explorerSelection: null,
  editorFindRequest: 0,
  editorFindDisplay: null,
  viewFocusRequest: null,
  toolsPlacement: "column",
  explorerDraft: null,
  pendingTrash: null,
  overlay: "none",
  settingsTab: "general",
  pendingClose: null,
  cycle: null,
  notice: null,
  workspaceDialog: null,
  watchedTask: null,
  focusWhenListed: null,
  opening: null,
  watchedRemoval: null,
  recordingShortcut: false,
  escapeLayers: [],
  // Moving by hand drops an open still waiting for its Workspace, so a late
  // answer does not pull the screen away from where the operator went.
  setScreen: (screen) => set({ screen, opening: null }),
  setProjectView: (projectView) => set({ projectView }),
  setTasksMode: (tasksMode) => set({ tasksMode }),
  setRelation: (relation) => set({ relation }),
  setSidebarMode: (sidebarMode) => set({ sidebarMode }),
  toggleSidebarMode: () => {
    const index = SIDEBAR_MODES.indexOf(get().sidebarMode);
    set({ sidebarMode: SIDEBAR_MODES[(index + 1) % SIDEBAR_MODES.length] });
  },
  setExplorerSelection: (explorerSelection) => set({ explorerSelection }),
  requestEditorFind: (displayId) => set({ editorFindRequest: get().editorFindRequest + 1, editorFindDisplay: displayId }),
  setViewFocusRequest: (viewFocusRequest) => set({ viewFocusRequest }),
  setToolsNarrow: (narrow) => {
    const current = get().toolsPlacement;
    const next = placementForWidth(current, narrow);
    if (next !== current) set({ toolsPlacement: next });
  },
  openTools: () => {
    if (get().toolsPlacement === "closed") set({ toolsPlacement: "open" });
  },
  closeTools: () => {
    if (get().toolsPlacement === "open") set({ toolsPlacement: "closed" });
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
  pushEscape: (handler) => {
    set({ escapeLayers: [...get().escapeLayers, handler] });
    return () => set({ escapeLayers: get().escapeLayers.filter((layer) => layer !== handler) });
  },
}));
