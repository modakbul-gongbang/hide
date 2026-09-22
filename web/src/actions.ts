// Every shell command in one place, so a shortcut, a button and a menu run
// the same code against the same snapshot. Each action is one core event
// (dispatch is fire-and-forget; a sequence would arrive as several frames).

import { closeDecision, statusUnknownNotice } from "./close";
import { lastCheckoutOf } from "./recent";
import { activeEditorTab, focusedCheckout, visibleTab, type Checkout, type Tab } from "./snapshot";
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

  return {
    dispatch,

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

    textScale(direction: "in" | "out" | "reset") {
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
      dispatch({
        schema_version: 2,
        kind: "file_open",
        payload: { path, workspace_id: here.checkout.workspace_id, checkout_id: here.checkout.id, preview },
      });
    },

    /** Expands the tree to a path and highlights it; a folder is not opened. */
    revealPath(path: string, isDirectory: boolean) {
      const here = current();
      if (!here) return diagnostic("reveal_path: no focused checkout");
      dispatch({
        schema_version: 2,
        kind: "reveal_path",
        payload: { path, workspace_id: here.checkout.workspace_id, checkout_id: here.checkout.id, is_directory: isDirectory },
      });
    },

    /** ⌘⇧K: the showing preview tab becomes an ordinary tab. */
    keepOpenFile() {
      const tab = activeEditorTab(useShellStore.getState().editor);
      if (!tab || !tab.preview) return;
      dispatch({ schema_version: 2, kind: "file_keep_open", payload: { tab_id: tab.id } });
    },

    focusFileTab(tabId: string) {
      dispatch({ schema_version: 2, kind: "file_focus", payload: { tab_id: tabId } });
    },

    closeFileTab(tabId: string) {
      dispatch({ schema_version: 2, kind: "file_close", payload: { tab_id: tabId, pending_save: null } });
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
