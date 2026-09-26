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
import { recentCheckoutOrder, recentTabOrder } from "./recent";
import { contextWorkspaces, remoteContext, remoteView } from "./remote";
import { hostRegistry, matchHost, REGISTRY, storedBindings, type CommandId } from "./shortcuts";
import { editorFor, focusedCheckout, type SnapshotRest } from "./snapshot";
import { useShellStore } from "./store";
import { useUiStore, type Cycle } from "./ui";
import { drawnViews, viewAreaInUse } from "./viewFocus";

/** The checkout whose tabs ⌥` walks: this machine's focused one, or the selected device's visible one. */
function cycleCheckout(rest: SnapshotRest | null) {
  return remoteContext(rest) ? (remoteView(remoteContext(rest)?.session ?? null)?.checkout ?? null) : focusedCheckout(rest);
}

function tabCycle(rest: SnapshotRest | null): Cycle | null {
  const checkout = cycleCheckout(rest);
  if (!checkout) return null;
  const tabs = checkout.tabs.filter((tab) => tab.id);
  const order = recentTabOrder(checkout.id, tabs.map((tab) => tab.id as string));
  const items = order.map((id) => {
    const tab = tabs.find((row) => row.id === id);
    return { id, label: tab?.label ?? id, detail: tab?.panes.length === 1 ? "1 pane" : `${tab?.panes.length ?? 0} panes`, workspaceId: checkout.workspace_id };
  });
  return items.length > 1 ? { kind: "tabs", items, index: 0 } : null;
}

function projectCycle(rest: SnapshotRest | null): Cycle | null {
  const rows = contextWorkspaces(rest).flatMap((workspace) =>
    workspace.checkouts.map((checkout) => ({
      id: checkout.id,
      label: checkout.label,
      detail: workspace.label === checkout.label ? (checkout.branch ?? "") : `${workspace.label}${checkout.branch ? ` · ${checkout.branch}` : ""}`,
      workspaceId: workspace.id,
    })),
  );
  const order = recentCheckoutOrder(rows.map((row) => row.id));
  const items = order.map((id) => rows.find((row) => row.id === id)!).filter(Boolean);
  return items.length > 1 ? { kind: "projects", items, index: 0 } : null;
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
      case "recent_tab":
      case "previous_recent_tab":
      case "recent_project":
      case "previous_recent_project": {
        if (!event) return;
        const backward = id.startsWith("previous");
        cycleRelease = event.ctrlKey ? "Control" : "Alt";
        const kind = id.endsWith("tab") ? "tabs" : "projects";
        const current = ui().cycle;
        const cycle = current?.kind === kind ? current : kind === "tabs" ? tabCycle(useShellStore.getState().rest) : projectCycle(useShellStore.getState().rest);
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
  const onKeyUp = (event: KeyboardEvent) => {
    if (event.key !== cycleRelease) return;
    const cycle = ui().cycle;
    if (!cycle) return;
    ui().setCycle(null);
    const chosen = cycle.items[cycle.index];
    if (!chosen || cycle.index === 0) return;
    if (cycle.kind === "tabs") actions.focusTab(chosen.id);
    else actions.focusCheckout(chosen.workspaceId, chosen.id);
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
