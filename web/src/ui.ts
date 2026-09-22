// Shell-local UI state the core does not own: which overlay is open, the
// pending close confirmation, and the one-line notice. Everything the core
// owns (focus, tabs, layout, sidebar visibility) stays in `store.ts`.

import { create } from "zustand";

export type SidebarMode = "agents" | "projects";

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
  overlay: Overlay;
  pendingClose: PendingClose | null;
  cycle: Cycle | null;
  /** A notice the operator can act on; `refreshable` offers `refresh_status` (activity unknown). */
  notice: { text: string; refreshable: boolean } | null;
  setSidebarMode: (mode: SidebarMode) => void;
  toggleSidebarMode: () => void;
  openOverlay: (overlay: Overlay) => void;
  closeOverlay: (overlay?: Overlay) => void;
  setPendingClose: (pending: PendingClose | null) => void;
  setCycle: (cycle: Cycle | null) => void;
  setNotice: (notice: { text: string; refreshable: boolean } | null) => void;
};

export const useUiStore = create<UiStore>((set, get) => ({
  sidebarMode: "agents",
  overlay: "none",
  pendingClose: null,
  cycle: null,
  notice: null,
  setSidebarMode: (sidebarMode) => set({ sidebarMode }),
  toggleSidebarMode: () => set({ sidebarMode: get().sidebarMode === "agents" ? "projects" : "agents" }),
  openOverlay: (overlay) => set({ overlay }),
  closeOverlay: (overlay) => {
    if (!overlay || get().overlay === overlay) set({ overlay: "none" });
  },
  setPendingClose: (pendingClose) => set({ pendingClose }),
  setCycle: (cycle) => set({ cycle }),
  setNotice: (notice) => set({ notice }),
}));
