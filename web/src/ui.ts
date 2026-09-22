// Shell-local UI state the core does not own: which overlay is open, the
// pending close confirmation, and the one-line notice. Everything the core
// owns (focus, tabs, layout, sidebar visibility) stays in `store.ts`.

import { create } from "zustand";

export type SidebarMode = "agents" | "projects" | "explorer";

/** The order ⌘E walks; the Explorer is the third mode, not a right panel. */
export const SIDEBAR_MODES: readonly SidebarMode[] = ["agents", "projects", "explorer"];

export type PendingClose = {
  kind: "pane" | "tab";
  id: string;
  title: string;
  consequence: string;
  affected: string[];
};

export type Overlay = "none" | "shortcuts" | "find" | "new_workspace";

/** A held-modifier cycle over recent tabs or projects; committed when ⌥ is released. */
export type Cycle = {
  kind: "tabs" | "projects";
  items: { id: string; label: string; detail: string; workspaceId: string }[];
  index: number;
};

type UiStore = {
  sidebarMode: SidebarMode;
  /** The Explorer row the operator last touched; the core owns the opened
   * document's `selected_path`, and a reveal syncs that into here. */
  explorerSelection: string | null;
  overlay: Overlay;
  pendingClose: PendingClose | null;
  cycle: Cycle | null;
  /** A notice the operator can act on; `refreshable` offers `refresh_status` (activity unknown). */
  notice: { text: string; refreshable: boolean } | null;
  setSidebarMode: (mode: SidebarMode) => void;
  toggleSidebarMode: () => void;
  setExplorerSelection: (path: string | null) => void;
  openOverlay: (overlay: Overlay) => void;
  closeOverlay: (overlay?: Overlay) => void;
  setPendingClose: (pending: PendingClose | null) => void;
  setCycle: (cycle: Cycle | null) => void;
  setNotice: (notice: { text: string; refreshable: boolean } | null) => void;
};

export const useUiStore = create<UiStore>((set, get) => ({
  sidebarMode: "agents",
  explorerSelection: null,
  overlay: "none",
  pendingClose: null,
  cycle: null,
  notice: null,
  setSidebarMode: (sidebarMode) => set({ sidebarMode }),
  toggleSidebarMode: () => {
    const index = SIDEBAR_MODES.indexOf(get().sidebarMode);
    set({ sidebarMode: SIDEBAR_MODES[(index + 1) % SIDEBAR_MODES.length] });
  },
  setExplorerSelection: (explorerSelection) => set({ explorerSelection }),
  openOverlay: (overlay) => set({ overlay }),
  closeOverlay: (overlay) => {
    if (!overlay || get().overlay === overlay) set({ overlay: "none" });
  },
  setPendingClose: (pendingClose) => set({ pendingClose }),
  setCycle: (cycle) => set({ cycle }),
  setNotice: (notice) => set({ notice }),
}));
