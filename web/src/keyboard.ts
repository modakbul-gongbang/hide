// Window-level interception of registry chords, ahead of xterm.
//
// The listener runs in the capture phase on `window`, so a chord is answered
// before xterm's textarea sees it and before Chrome's default (bookmark,
// find, zoom) runs (PRD S2 B12). A keydown during IME composition is never a
// chord: Korean input composes through the same keys.
//
// In the desktop app the same table runs its Electron column, and the app
// menu's commands arrive through the host bridge into the same `run`: a
// chord the listener answers is consumed here, so the menu's own
// accelerator for it never fires as well.

import type { Actions } from "./actions";
import { hostBridge, hostKind } from "./host";
import { availableSurfaces, currentSurface, observeProject, observeSurfaces, panelItem, placeLabel, projectItem, reconcileCycle, recentProjectOrder, recentSurfaces, type CycleItem } from "./recent";
import { contextWorkspaces, remoteContext, remoteView } from "./remote";
import { hostRegistry, matchHost, REGISTRY, storedBindings, type CommandId } from "./shortcuts";
import { editorFor, type SnapshotRest } from "./snapshot";
import { useShellStore } from "./store";
import { useUiStore, type Cycle } from "./ui";
import { drawnViews, viewAreaInUse } from "./viewFocus";

/**
 * Recent Panels: every surface this machine holds, most recent first, so
 * index 0 is the one in use and one chord lands on the one before it.
 */
export function panelCycle(rest: SnapshotRest | null): Cycle | null {
  if (remoteContext(rest)) return deviceTabCycle(rest);
  const items = recentSurfaces()
    .map((surface) => panelItem(rest, surface))
    .filter((item): item is CycleItem => item !== null);
  return items.length > 1 ? { kind: "panels", items, index: 0 } : null;
}

/**
 * While a device is in front, its visible checkout's Herdr tabs, the one it
 * shows first: the web shell cannot bring a device's surface forward from
 * anywhere else yet, so Recent Panels there stays within that checkout.
 */
function deviceTabCycle(rest: SnapshotRest | null): Cycle | null {
  const view = remoteView(remoteContext(rest)?.session ?? null);
  const checkout = view?.checkout;
  if (!checkout) return null;
  const workspace = contextWorkspaces(rest).find((row) => row.id === checkout.workspace_id);
  const tabs = checkout.tabs.filter((tab) => tab.id);
  const shown = view.tab?.id ?? checkout.active_tab_id;
  const ordered = [...tabs.filter((tab) => tab.id === shown), ...tabs.filter((tab) => tab.id !== shown)];
  const detail = `${workspace ? placeLabel(workspace, checkout) : checkout.label} · Terminal`;
  const items: CycleItem[] = ordered.map((tab) => ({
    key: tab.id!,
    title: tab.label ?? tab.id!,
    detail,
    kind: "herdr",
    agent: null,
    surface: null,
    deviceTabId: tab.id!,
    workspaceId: checkout.workspace_id,
    checkoutId: checkout.id,
  }));
  return items.length > 1 ? { kind: "panels", items, index: 0 } : null;
}

/** Recent Projects on the device in front, each row naming the surface it restores. */
export function projectCycle(rest: SnapshotRest | null): Cycle | null {
  const workspaces = contextWorkspaces(rest);
  const local = remoteContext(rest) === null;
  const order = recentProjectOrder(workspaces.map((workspace) => workspace.id));
  const items = order
    .map((id) => workspaces.find((workspace) => workspace.id === id))
    .map((workspace) => (workspace ? projectItem(workspace, rest, local) : null))
    .filter((item): item is CycleItem => item !== null);
  return items.length > 1 ? { kind: "projects", items, index: 0 } : null;
}

/**
 * Brings the recent order up to date with the session, and records what the
 * operator is using now when `moved` says the surface in use may have
 * changed. While another device is in front nothing of this machine's is in
 * use, so the order only drops what is gone.
 */
export function observeRecent(rest: SnapshotRest | null, moved: boolean) {
  const local = remoteContext(rest) === null;
  observeSurfaces(rest, local && moved ? currentSurface(rest, viewAreaInUse(rest)) : null);
  if (local && moved) observeProject(rest?.navigator?.focused_workspace_id);
}

/** The held cycle once the session changed under it: see `reconcileCycle`. */
export function reconcileHeldCycle(cycle: Cycle, rest: SnapshotRest | null): Cycle | null {
  const deviceTabs = remoteView(remoteContext(rest)?.session ?? null)?.checkout?.tabs;
  const alive =
    cycle.kind === "projects"
      ? new Set(contextWorkspaces(rest).map((workspace) => workspace.id))
      : remoteContext(rest)
        ? new Set((deviceTabs ?? []).map((tab) => tab.id ?? ""))
        : new Set(availableSurfaces(rest, recentSurfaces()).map((surface) => surface.key));
  const kept = reconcileCycle(cycle.items, cycle.index, (item) => alive.has(item.key));
  return kept && kept.items.length > 1 ? { ...cycle, ...kept } : null;
}

function advance(cycle: Cycle, backward: boolean): Cycle {
  const count = cycle.items.length;
  return { ...cycle, index: (cycle.index + (backward ? -1 : 1) + count) % count };
}

export function installKeyboard(actions: Actions): () => void {
  const ui = () => useUiStore.getState();
  const host = hostKind();
  // The modifier whose release commits the running cycle: ⌥ for the ⌥ family,
  // ⌃ for the desktop app's ⌃Tab.
  let cycleRelease = "Alt";

  const run = (id: CommandId, event: KeyboardEvent | null) => {
    switch (id) {
      case "new_tab":
        return actions.createTab();
      case "close_tab":
        return actions.closeTab();
      case "reopen_closed_tab":
        return actions.reopenClosed();
      case "new_workspace":
        return actions.openNewWorkspace();
      case "recent_panel":
      case "previous_recent_panel":
      case "recent_project":
      case "previous_recent_project": {
        if (!event) return;
        const backward = id.startsWith("previous");
        cycleRelease = event.ctrlKey ? "Control" : "Alt";
        const kind = id.endsWith("panel") ? "panels" : "projects";
        const current = ui().cycle;
        const cycle = current?.kind === kind ? current : kind === "panels" ? panelCycle(useShellStore.getState().rest) : projectCycle(useShellStore.getState().rest);
        if (!cycle) return;
        ui().setCycle(advance(cycle, backward));
        return;
      }
      case "search":
        return actions.openSearch();
      case "open_file":
        return actions.openFilePalette();
      case "project_home":
        return actions.openProjectOverview();
      case "toggle_right_panel":
        return actions.toggleRightPanel();
      case "toggle_left_sidebar":
        return actions.toggleLeftSidebar();
      case "toggle_sidebar_view":
        return actions.toggleSidebarView();
      case "find_in_pane": {
        // One chord, two surfaces, chosen by where the operator works: the
        // View area they are in finds in its document, anywhere else the
        // focused terminal pane's find bar opens, even with an editor open
        // beside it.
        const { editor, rest } = useShellStore.getState();
        if (editorFor(editor) && drawnViews() && viewAreaInUse(rest)) return actions.requestEditorFind();
        return actions.openFind();
      }
      case "save_file":
        return actions.saveFile();
      case "keep_open":
        // The same chord answers a pending close and the active preview
        // display: the confirmation is showing first, so it wins when it is.
        if (ui().pendingClose) return actions.keepOpen();
        return actions.keepViewOpen();
      case "split_right":
        return actions.split("right");
      case "split_down":
        return actions.split("down");
      case "toggle_zoom":
        return actions.toggleZoom();
      case "close_pane":
        return actions.closePane();
      case "text_larger":
        return actions.textScale("in");
      case "text_smaller":
        return actions.textScale("out");
      case "text_reset":
        return actions.textScale("reset");
      case "shortcuts":
        return actions.openShortcuts();
      case "settings":
        return actions.openSettings();
      case "move_to_trash":
        return;
      default: {
        const never: never = id;
        useShellStore.getState().noteDiagnostic(`shortcut without an action: ${String(never)} (${event?.code ?? "menu"})`);
      }
    }
  };

  const onKeyDown = (event: KeyboardEvent) => {
    if (event.isComposing || event.keyCode === 229) return;
    // A Shortcuts row that is recording owns the next chord, Escape included:
    // no command runs while the operator is showing the recorder a key.
    if (ui().recordingShortcut) return;
    if (event.key === "Escape") {
      // An Escape the shell answers is consumed here: the pane's textarea is
      // still the focused element under the sheet or a cycle, and xterm
      // would send the same press to the program as an ESC byte.
      const consume = () => {
        event.preventDefault();
        event.stopPropagation();
      };
      if (ui().cycle) {
        ui().setCycle(null);
        consume();
        return;
      }
      const innermost = ui().escapeLayers.at(-1);
      if (innermost) {
        innermost();
        consume();
        return;
      }
      if (ui().pendingClose) {
        actions.keepOpen();
        consume();
        return;
      }
      if (ui().overlay !== "none") {
        ui().closeOverlay();
        consume();
        return;
      }
      // With no layer open, Escape leaves a Project's Overview for the
      // Workspace in front, or All projects when there is none
      // (web-project-overview B1). Every dialog, menu and popover is an escape
      // layer, answered above; a text field holding text answers it itself, so
      // the Sessions search clears before a second Escape leaves.
      const field = event.target instanceof HTMLInputElement || event.target instanceof HTMLTextAreaElement ? event.target : null;
      if (ui().screen?.kind === "overview" && !field?.value) {
        actions.leaveProjectOverview();
        consume();
      }
      return;
    }
    const { registry } = hostRegistry(useShellStore.getState().rest?.ui_state, host);
    const command = matchHost(event, registry, host);
    if (!command) return;
    event.preventDefault();
    event.stopPropagation();
    run(command.id, event);
  };

  // Releasing the held modifier commits the cycle: one focus event for the
  // row the operator stopped on, none when they stopped where they started.
  // A project with no surface to restore comes forward as its checkout does
  // from the sidebar, which is also how a device's project comes forward.
  const onKeyUp = (event: KeyboardEvent) => {
    if (event.key !== cycleRelease) return;
    const cycle = ui().cycle;
    if (!cycle) return;
    ui().setCycle(null);
    const chosen = cycle.items[cycle.index];
    if (!chosen || cycle.index === 0) return;
    if (chosen.surface) actions.openSurface(chosen.surface);
    else if (chosen.deviceTabId) actions.focusTab(chosen.deviceTabId);
    else actions.focusCheckout(chosen.workspaceId, chosen.checkoutId);
  };

  // Losing the window mid-cycle (⌥-Tab switching apps) cancels it; nothing
  // is committed for a chord the operator did not finish here.
  const onBlur = () => {
    if (ui().cycle) ui().setCycle(null);
  };

  // A menu item names a command id; one this registry does not know is a
  // host/shell version mismatch, recorded rather than guessed at.
  const bridge = hostBridge();
  const unsubscribeMenu = bridge?.onCommand((id) => {
    const command = REGISTRY.find((row) => row.id === id);
    if (command) run(command.id, null);
    else useShellStore.getState().noteDiagnostic(`menu command the shell does not know: ${id}`);
  });
  // The app menu's accelerators follow the stored macOS set: the host gets
  // it once now and again whenever its contents change, and resolves it with
  // the same rules this listener runs.
  // The store notifies per snapshot; an unchanged set is the same object, so
  // most notifications end at the reference check.
  let seen: Record<string, string> | undefined | null = null;
  let reported: string | null = null;
  const report = () => {
    const stored = storedBindings(useShellStore.getState().rest?.ui_state, "electron");
    if (stored === seen) return;
    seen = stored;
    const text = JSON.stringify(stored ?? {});
    if (text === reported) return;
    reported = text;
    bridge?.reportBindings(stored ?? {});
  };
  const unsubscribeBindings = bridge ? useShellStore.subscribe(report) : null;
  if (bridge) report();

  window.addEventListener("keydown", onKeyDown, true);
  window.addEventListener("keyup", onKeyUp, true);
  window.addEventListener("blur", onBlur);
  return () => {
    unsubscribeMenu?.();
    unsubscribeBindings?.();
    window.removeEventListener("keydown", onKeyDown, true);
    window.removeEventListener("keyup", onKeyUp, true);
    window.removeEventListener("blur", onBlur);
  };
}
