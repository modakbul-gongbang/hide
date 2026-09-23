// Every shell command in one place, so a shortcut, a button and a menu run
// the same code against the same snapshot. Each action is one core event
// (dispatch is fire-and-forget; a sequence would arrive as several frames).

import { deleteBuffer } from "./buffers";
import { closeDecision, statusUnknownNotice } from "./close";
import { latestDraft } from "./editor/draft";
import { lastCheckoutOf } from "./recent";
import { activeEditorTab, checkoutById, editorFor, focusedCheckout, visibleTab, type Checkout, type Tab } from "./snapshot";
import { useShellStore } from "./store";
import { useUiStore, type SidebarMode } from "./ui";
import type { DispatchFn } from "./ws";

export type Actions = ReturnType<typeof createActions>;

export function createActions(dispatch: DispatchFn) {
  const rest = () => useShellStore.getState().rest;
  const diagnostic = (message: string) => useShellStore.getState().noteDiagnostic(message);
  const ui = () => useUiStore.getState();

  const current = (): { checkout: Checkout; tab: Tab | null } | null => {
    const checkout = focusedCheckout(rest());
    return checkout ? { checkout, tab: visibleTab(checkout) } : null;
  };

  const setLeftSidebarVisible = (visible: boolean) => {
    const state = rest()?.ui_state;
    if (!state || state.left_sidebar_visible === visible) return;
    updateUiState({ left_sidebar_visible: visible });
  };

  /**
   * The core's ui state with one patch applied. The event replaces the whole
   * state rather than merging it, so every field the snapshot carries rides
   * along; a field the web omits would fall back to the core's default and
   * erase a value another surface owns.
   */
  const updateUiState = (patch: Record<string, unknown>) => {
    const state = rest()?.ui_state;
    if (!state) return;
    dispatch({ schema_version: 2, kind: "ui_state_update", payload: { ...state, ...patch } });
  };

  const requestClose = (kind: "pane" | "tab", id: string, panes: Tab["panes"]) => {
    const decision = closeDecision(kind, panes, useShellStore.getState().agents);
    if (decision.action === "status_unknown") {
      ui().setNotice({ text: statusUnknownNotice(decision.label), refreshable: true });
      return;
    }
    if (decision.action === "confirm") {
      ui().setPendingClose({ kind, id, title: decision.title, consequence: decision.consequence, affected: decision.affected });
      return;
    }
    dispatch({
      schema_version: 2,
      kind: kind === "pane" ? "close_pane" : "close_tab",
      payload: kind === "pane" ? { pane_id: id, confirmed: false } : { tab_id: id, confirmed: false },
    });
  };

  const focusCheckout = (workspaceId: string, checkoutId: string) =>
    dispatch({ schema_version: 2, kind: "focus_checkout", payload: { workspace_id: workspaceId, checkout_id: checkoutId } });

  /**
   * Switches the sidebar's mode. Explorer mode also tells the core, because
   * the core computes a checkout's changed files only while its Explorer or
   * Changes surface is visible (`Runtime::changes_request`); without that ui
   * state the tree's rows carry no Git decoration.
   */
  const showSidebarMode = (mode: SidebarMode) => {
    ui().setSidebarMode(mode);
    if (mode === "explorer") {
      updateUiState({ right_panel_visible: true, right_panel_section: "explorer" });
    }
  };

  /**
   * The folder chain to an opened file under the focused checkout, so the
   * Explorer shows and highlights the row without the core's `reveal_path`,
   * which would open a pinned tab (B3).
   */
  const revealAncestors = (path: string) => {
    const state = rest()?.ui_state;
    const root = rest()?.navigator?.root_path;
    if (!state || !root || !path.startsWith(`${root}/`)) return;
    const parts = path.slice(root.length + 1).split("/");
    parts.pop();
    const expanded = new Set(state.expanded_paths ?? []);
    let changed = false;
    for (let depth = 1; depth <= parts.length; depth += 1) {
      const ancestor = `${root}/${parts.slice(0, depth).join("/")}`;
      if (!expanded.has(ancestor)) {
        expanded.add(ancestor);
        changed = true;
      }
    }
    if (changed) updateUiState({ expanded_paths: [...expanded] });
  };

  return {
    dispatch,
    revealAncestors,

    createTab() {
      const here = current();
      if (!here) return diagnostic("create_tab: no focused checkout");
      dispatch({
        schema_version: 2,
        kind: "create_tab",
        payload: { workspace_id: here.checkout.workspace_id, checkout_id: here.checkout.id, label: here.checkout.next_tab_label },
      });
    },

    focusTab(tabId: string) {
      const here = current();
      if (!here) return;
      dispatch({
        schema_version: 2,
        kind: "focus_tab",
        payload: { workspace_id: here.checkout.workspace_id, checkout_id: here.checkout.id, tab_id: tabId },
      });
    },

    reorderTab(stripId: string, toIndex: number) {
      const here = current();
      if (!here) return;
      dispatch({
        schema_version: 2,
        kind: "reorder_tab",
        payload: { workspace_id: here.checkout.workspace_id, checkout_id: here.checkout.id, tab_id: stripId, to_index: toIndex },
      });
    },

    closeTab(tabId?: string) {
      const here = current();
      const id = tabId ?? here?.tab?.id;
      if (!here || !id) return diagnostic("close_tab: no visible tab");
      const tab = here.checkout.tabs.find((row) => row.id === id);
      requestClose("tab", id, tab?.panes ?? []);
    },

    closePane(paneId?: string) {
      const here = current();
      const id = paneId ?? useShellStore.getState().focusedPaneId;
      if (!here?.tab || !id) return diagnostic("close_pane: no focused pane");
      const pane = here.tab.panes.find((row) => row.id === id);
      requestClose("pane", id, pane ? [pane] : []);
    },

    /** The operator chose "Stop work and close" on the confirmation. */
    confirmClose() {
      const pending = ui().pendingClose;
      if (!pending) return;
      ui().setPendingClose(null);
      dispatch({
        schema_version: 2,
        kind: pending.kind === "pane" ? "close_pane" : "close_tab",
        payload: pending.kind === "pane" ? { pane_id: pending.id, confirmed: true } : { tab_id: pending.id, confirmed: true },
      });
    },

    keepOpen() {
      ui().setPendingClose(null);
    },

    refreshStatus() {
      ui().setNotice(null);
      dispatch({ schema_version: 2, kind: "refresh_status", payload: {} });
    },

    /** A close whose outcome the core could not read; asks it to check (`check_close_status`). */
    checkCloseStatus(key: string) {
      dispatch({ schema_version: 2, kind: "check_close_status", payload: { key } });
    },

    reopenClosed() {
      const recent = rest()?.recent_closed;
      if (!recent?.can_reopen) {
        diagnostic(`reopen_closed: nothing to reopen${recent?.reopen_blocked_reason ? ` (${recent.reopen_blocked_reason})` : ""}`);
        return;
      }
      dispatch({ schema_version: 2, kind: "reopen_closed", payload: {} });
    },

    split(direction: "right" | "down") {
      const here = current();
      const paneId = useShellStore.getState().focusedPaneId;
      const pane = here?.tab?.panes.find((row) => row.id === paneId);
      if (!here?.tab?.id || !pane) return diagnostic("create_pane: no focused pane");
      dispatch({
        schema_version: 2,
        kind: "create_pane",
        payload: { tab_id: here.tab.id, cwd: pane.cwd, command: null, direction },
      });
    },

    toggleZoom() {
      const paneId = useShellStore.getState().focusedPaneId;
      if (!paneId) return diagnostic("toggle_zoom: no focused pane");
      dispatch({ schema_version: 2, kind: "toggle_zoom", payload: { pane_id: paneId } });
    },

    /** ⌘= / ⌘- / ⌘0 scale whichever surface is showing: the document when an
     * editor tab owns the canvas, else the focused terminal pane. */
    textScale(direction: "in" | "out" | "reset") {
      if (editorFor(useShellStore.getState().editor)) {
        dispatch({ schema_version: 2, kind: "editor_text_scale", payload: { direction } });
        return;
      }
      const paneId = useShellStore.getState().focusedPaneId;
      if (!paneId) return diagnostic("pane_text_scale: no focused pane");
      dispatch({ schema_version: 2, kind: "pane_text_scale", payload: { pane_id: paneId, direction } });
    },

    focusCheckout,

    /** A project row: the checkout the operator was last in, else the first row (PRD S2 D-07). */
    focusProject(workspaceId: string) {
      const workspace = rest()?.navigator?.workspaces?.find((row) => row.id === workspaceId);
      if (!workspace) return;
      const ids = workspace.checkouts.map((row) => row.id);
      const checkoutId = lastCheckoutOf(ids) ?? ids[0];
      if (!checkoutId) return diagnostic(`focus_checkout: project ${workspace.label} has no checkout`);
      focusCheckout(workspaceId, checkoutId);
    },

    toggleInactiveCheckouts(projectPath: string) {
      dispatch({ schema_version: 2, kind: "inactive_checkouts_toggle", payload: { project_path: projectPath } });
    },

    toggleInactiveProjects(deviceId: string) {
      dispatch({ schema_version: 2, kind: "inactive_projects_toggle", payload: { device_id: deviceId } });
    },

    toggleLeftSidebar() {
      const state = rest()?.ui_state;
      if (state) setLeftSidebarVisible(!state.left_sidebar_visible);
    },

    toggleSidebarView() {
      const order: SidebarMode[] = ["agents", "projects", "explorer"];
      const index = order.indexOf(ui().sidebarMode);
      showSidebarMode(order[(index + 1) % order.length] ?? "agents");
    },

    showSidebarMode,

    openShortcuts() {
      ui().openOverlay(ui().overlay === "shortcuts" ? "none" : "shortcuts");
    },

    openFind() {
      ui().openOverlay("find");
    },

    /** The registration field lives in the sidebar, so a hidden sidebar is shown first. */
    openNewWorkspace() {
      setLeftSidebarVisible(true);
      ui().openOverlay("new_workspace");
    },

    /** ⌘K, ⌘P and the right-panel chords are claimed now and answered in S3. */
    notReady(label: string) {
      ui().setNotice({ text: `${label}: 준비 중 (S3)`, refreshable: false });
    },

    /** ⌘P: the file palette over hided's index of the focused checkout. */
    openFilePalette() {
      ui().openOverlay(ui().overlay === "file_palette" ? "none" : "file_palette");
    },

    /** ⌘K: the palette that searches the snapshot's agents, projects and checkouts. */
    openSearch() {
      ui().openOverlay(ui().overlay === "search" ? "none" : "search");
    },

    requestFileIndex(root: string, query: string) {
      dispatch({ schema_version: 2, kind: "file_index", payload: { root, query } });
    },

    /** Hands a checkout file to the daemon's host OS handler (D-12). */
    openExternal(path: string) {
      dispatch({ schema_version: 2, kind: "open_external", payload: { path } });
    },

    /** A palette pick opens in the checkout's preview tab (B12) and closes the palette. */
    openIndexEntry(path: string) {
      ui().closeOverlay();
      const here = current();
      if (!here) return diagnostic("file_open: no focused checkout");
      revealAncestors(path);
      // The core's selected_path may already be this file from an earlier
      // open, so the row is highlighted from the pick itself (B3).
      ui().setExplorerSelection(path);
      dispatch({
        schema_version: 2,
        kind: "file_open",
        payload: { path, workspace_id: here.checkout.workspace_id, checkout_id: here.checkout.id, preview: true },
      });
    },

    createWorkspace(path: string, label: string) {
      dispatch({ schema_version: 2, kind: "create_workspace", payload: { path, label, initialize_git: false } });
    },

    listDirectory(path: string) {
      dispatch({ schema_version: 2, kind: "remote_file_list", payload: { target_id: "local", root_path: path } });
    },

    /** One checkout folder's children, answered by hided as a `directory_list`. */
    listChildren(root: string, path: string) {
      dispatch({ schema_version: 2, kind: "file_list", payload: { root, path } });
    },

    /** The core owns which folders the tree has expanded; this replaces the set. */
    setExpandedPaths(paths: string[]) {
      updateUiState({ expanded_paths: paths });
    },

    /** A single click opens the checkout's preview slot; a double click pins it. */
    openFile(path: string, preview: boolean) {
      const here = current();
      if (!here) return diagnostic("file_open: no focused checkout");
      revealAncestors(path);
      dispatch({
        schema_version: 2,
        kind: "file_open",
        payload: { path, workspace_id: here.checkout.workspace_id, checkout_id: here.checkout.id, preview },
      });
    },

    /** ⌘⇧K: the showing preview tab becomes an ordinary tab. */
    keepOpenFile() {
      const tab = activeEditorTab(useShellStore.getState().editor);
      if (!tab || !tab.preview) return;
      dispatch({ schema_version: 2, kind: "file_keep_open", payload: { tab_id: tab.id } });
    },

    /** A new file or folder in `parent`; the core opens a created file (B9). */
    createEntry(parent: string, name: string, isDirectory: boolean) {
      const here = current();
      if (!here) return diagnostic("explorer create: no focused checkout");
      dispatch({
        schema_version: 2,
        kind: isDirectory ? "dir_create" : "file_create",
        payload: { root: here.checkout.path, parent, name },
      });
    },

    renameEntry(path: string, name: string) {
      const here = current();
      if (!here) return diagnostic("path_rename: no focused checkout");
      dispatch({ schema_version: 2, kind: "path_rename", payload: { root: here.checkout.path, path, name } });
    },

    /** A drag that landed: one `path_move` into the folder it was dropped on. */
    moveEntry(path: string, destination: string) {
      const here = current();
      if (!here) return diagnostic("path_move: no focused checkout");
      dispatch({ schema_version: 2, kind: "path_move", payload: { root: here.checkout.path, path, destination } });
    },

    /** Opens the trash confirmation; nothing is dispatched until it is confirmed. */
    requestTrash(path: string, name: string, isDirectory: boolean, selectAfter: string) {
      ui().setPendingTrash({ path, name, isDirectory, selectAfter });
    },

    confirmTrash() {
      const pending = ui().pendingTrash;
      const here = current();
      ui().setPendingTrash(null);
      if (!pending) return;
      if (!here) return diagnostic("path_trash: no focused checkout");
      dispatch({
        schema_version: 2,
        kind: "path_trash",
        payload: {
          root: here.checkout.path,
          path: pending.path,
          select_after: pending.selectAfter,
          inode: null,
        },
      });
    },

    cancelTrash() {
      ui().setPendingTrash(null);
    },

    focusFileTab(tabId: string) {
      dispatch({ schema_version: 2, kind: "file_focus", payload: { tab_id: tabId } });
    },

    closeFileTab(tabId: string) {
      const state = useShellStore.getState();
      const tab = state.editor?.tabs.find((row) => row.id === tabId);
      if (tab) void deleteBuffer(checkoutById(state.rest, tab.checkout_id)?.path ?? "", tab.path);
      dispatch({ schema_version: 2, kind: "file_close", payload: { tab_id: tabId, pending_save: null } });
    },

    /** One keystroke's contents; the core keeps the draft and the dirty flag. */
    updateDraft(contents: string) {
      dispatch({ schema_version: 2, kind: "file_draft", payload: { contents_utf8: contents } });
    },

    /**
     * A save of the showing document. The expected modification time is the
     * one the core read when it opened the file, so a disk change since then
     * makes the save a conflict rather than a silent overwrite (B5).
     */
    saveFile() {
      const state = useShellStore.getState();
      const tab = activeEditorTab(state.editor);
      const document = state.editor?.document;
      if (!tab || !document || tab.kind !== "file") return diagnostic("file_save: no showing document");
      const contents = latestDraft(tab.id) ?? document.contents_utf8 ?? "";
      dispatch({
        schema_version: 2,
        kind: "file_save",
        payload: {
          tab_id: tab.id,
          path: tab.path,
          contents_utf8: contents,
          expected_modified_at_unix_ms: document.opened_modified_at_unix_ms,
        },
      });
    },

    /** The tab's Markdown mode and wrap choice; the core persists both. */
    setFileView(live: boolean, wrap: boolean) {
      const tab = activeEditorTab(useShellStore.getState().editor);
      if (!tab) return;
      dispatch({ schema_version: 2, kind: "file_view", payload: { tab_id: tab.id, markdown_live: live, wrap } });
    },

    /** "reload" reads the disk contents; "keep_editing" accepts the disk
     * timestamp so the next save overwrites. */
    resolveConflict(action: "reload" | "keep_editing") {
      dispatch({ schema_version: 2, kind: "file_conflict", payload: { action } });
    },

    requestEditorFind() {
      ui().requestEditorFind();
    },

    /** `step` 0 searches and keeps the current match; +1 and -1 move (core `PaneFindPayload`). */
    find(paneId: string, term: string, step: -1 | 0 | 1) {
      dispatch({
        schema_version: 2,
        kind: "pane_find",
        payload: { pane_id: paneId, term, case_sensitive: false, whole_word: false, regex: false, step },
      });
    },
  };
}
