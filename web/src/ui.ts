// Shell-local UI state the core does not own: which overlay is open, the
// pending close confirmation, and the one-line notice. Everything the core
// owns (focus, tabs, layout, sidebar visibility) stays in `store.ts`.

import { create } from "zustand";
import type { Relation } from "./lineage";
import type { Opening } from "./navigation";

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
 * Where the center stands (PRD S6 D-02): Main lists every Project, Overview
 * one Project's Workspaces and agents, Workspace the front checkout. It is
 * this page's own navigation, like the sidebar's mode: every value it shows
 * is the core's, and the Workspace it shows is the core's front checkout.
 * `null` until the first snapshot decides where the page starts (D-11).
 */
export type Screen = { kind: "main" } | { kind: "overview"; projectId: string } | { kind: "workspace" };

/** `file_palette_beside` is ⌘P's list for "Open file to the side" (S7 B4): its pick opens beside the active View area. */
export type Overlay = "none" | "shortcuts" | "find" | "new_workspace" | "file_palette" | "file_palette_beside" | "search" | "settings";

/**
 * Where the keyboard goes once the core has moved there (S7 B20): a display
 * that a menu, the palette or a drop moved or split, or an area a focus
 * command chose. The View areas focus it when the snapshot shows it active.
 */
export type ViewFocusRequest = { displayId: string } | { areaId: string };

/** The two working regions of a Workspace, for a Together window too narrow for both (S7 B13). */
export type WorkingRegion = "agents" | "views";

/** A project or checkout management dialog, named by the row that opened it. */
export type WorkspaceDialog =
  | { kind: "new_worktree"; workspaceId: string }
  | { kind: "purpose"; workspaceId: string; checkoutId: string }
  | { kind: "delete_worktree"; workspaceId: string; checkoutId: string }
  | { kind: "remove_project"; workspaceId: string };

/** A held-modifier cycle over recent tabs or projects; committed when ⌥ is released. */
export type Cycle = {
  kind: "tabs" | "projects";
  items: { id: string; label: string; detail: string; workspaceId: string }[];
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
   * The working region the operator last worked in, and the one a Together
   * window too narrow for both shows (S7 B13). This page's presentation
   * only: it is never sent or stored, so widening shows both again.
   */
  workingRegion: WorkingRegion;
  /**
   * Whether the operator dismissed the Workspace tools drawn over a narrow
   * window (S7 B12). Only that overlay reads it, and it resets once the
   * window is wide enough for the tool column; the stored tools stay as they
   * are, so widening brings the column back.
   */
  toolsDismissed: boolean;
  /** The Explorer's inline name field, or null. */
  explorerDraft: ExplorerDraft | null;
  /** The trash confirmation the Explorer is showing, or null. */
  pendingTrash: PendingTrash | null;
  overlay: Overlay;
  pendingClose: PendingClose | null;
  cycle: Cycle | null;
  /** A notice the operator can act on; `refreshable` offers `refresh_status` (activity unknown). */
  notice: { text: string; refreshable: boolean } | null;
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
  /** A Workspace or agent asked for from Main, an Overview or the Agents list, until it is in front or refused (S6 B2, B21). */
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
  setRelation: (relation: Relation | null) => void;
  setSidebarMode: (mode: SidebarMode) => void;
  toggleSidebarMode: () => void;
  setExplorerSelection: (path: string | null) => void;
  requestEditorFind: (displayId: string | null) => void;
  setViewFocusRequest: (request: ViewFocusRequest | null) => void;
  setWorkingRegion: (region: WorkingRegion) => void;
  setToolsDismissed: (dismissed: boolean) => void;
  setExplorerDraft: (draft: ExplorerDraft | null) => void;
  setPendingTrash: (trash: PendingTrash | null) => void;
  openOverlay: (overlay: Overlay) => void;
  closeOverlay: (overlay?: Overlay) => void;
  setPendingClose: (pending: PendingClose | null) => void;
  setCycle: (cycle: Cycle | null) => void;
  setNotice: (notice: { text: string; refreshable: boolean } | null) => void;
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
  relation: null,
  sidebarMode: "agents",
  explorerSelection: null,
  editorFindRequest: 0,
  editorFindDisplay: null,
  viewFocusRequest: null,
  workingRegion: "agents",
  toolsDismissed: false,
  explorerDraft: null,
  pendingTrash: null,
  overlay: "none",
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
  setRelation: (relation) => set({ relation }),
  setSidebarMode: (sidebarMode) => set({ sidebarMode }),
  toggleSidebarMode: () => {
    const index = SIDEBAR_MODES.indexOf(get().sidebarMode);
    set({ sidebarMode: SIDEBAR_MODES[(index + 1) % SIDEBAR_MODES.length] });
  },
  setExplorerSelection: (explorerSelection) => set({ explorerSelection }),
  requestEditorFind: (displayId) => set({ editorFindRequest: get().editorFindRequest + 1, editorFindDisplay: displayId }),
  setViewFocusRequest: (viewFocusRequest) => set({ viewFocusRequest }),
  setWorkingRegion: (workingRegion) => {
    if (get().workingRegion !== workingRegion) set({ workingRegion });
  },
  setToolsDismissed: (toolsDismissed) => {
    if (get().toolsDismissed !== toolsDismissed) set({ toolsDismissed });
  },
  setExplorerDraft: (explorerDraft) => set({ explorerDraft }),
  setPendingTrash: (pendingTrash) => set({ pendingTrash }),
  openOverlay: (overlay) => set({ overlay }),
  closeOverlay: (overlay) => {
    if (!overlay || get().overlay === overlay) set({ overlay: "none" });
  },
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
