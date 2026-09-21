// Window-level interception of registry chords, ahead of xterm.
//
// The listener runs in the capture phase on `window`, so a chord is answered
// before xterm's textarea sees it and before Chrome's default (bookmark,
// find, zoom) runs (PRD S2 B12). A keydown during IME composition is never a
// chord: Korean input composes through the same keys.

import type { Actions } from "./actions";
import { recentCheckoutOrder, recentTabOrder } from "./recent";
import { matchBrowser, type CommandId } from "./shortcuts";
import { focusedCheckout, type SnapshotRest } from "./snapshot";
import { useShellStore } from "./store";
import { useUiStore, type Cycle } from "./ui";

function tabCycle(rest: SnapshotRest | null): Cycle | null {
  const checkout = focusedCheckout(rest);
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
  const rows = (rest?.navigator?.workspaces ?? []).flatMap((workspace) =>
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

  const run = (id: CommandId, event: KeyboardEvent) => {
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
        const backward = id.startsWith("previous");
        const kind = id.endsWith("tab") ? "tabs" : "projects";
        const current = ui().cycle;
        const cycle = current?.kind === kind ? current : kind === "tabs" ? tabCycle(useShellStore.getState().rest) : projectCycle(useShellStore.getState().rest);
        if (!cycle) return;
        ui().setCycle(advance(cycle, backward));
        return;
      }
      case "search":
        return actions.notReady("Search (⌘K)");
      case "open_file":
        return actions.notReady("Open file (⌘P)");
      case "project_home":
        return actions.notReady("Project home (⌘⇧H)");
      case "toggle_right_panel":
        return actions.notReady("Right panel (⌘⇧B)");
      case "toggle_left_sidebar":
        return actions.toggleLeftSidebar();
      case "toggle_sidebar_view":
        return actions.toggleSidebarView();
      case "find_in_pane":
        return actions.openFind();
      case "keep_open":
        return actions.keepOpen();
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
      case "move_to_trash":
        return;
      default: {
        const never: never = id;
        useShellStore.getState().noteDiagnostic(`shortcut without an action: ${String(never)} (${event.code})`);
      }
    }
  };

  const onKeyDown = (event: KeyboardEvent) => {
    if (event.isComposing || event.keyCode === 229) return;
    if (event.key === "Escape") {
      if (ui().cycle) {
        ui().setCycle(null);
        event.preventDefault();
        return;
      }
      if (ui().pendingClose) {
        actions.keepOpen();
        event.preventDefault();
        return;
      }
      if (ui().overlay !== "none") {
        ui().closeOverlay();
        event.preventDefault();
        return;
      }
      return;
    }
    const command = matchBrowser(event);
    if (!command) return;
    event.preventDefault();
    event.stopPropagation();
    run(command.id, event);
  };

  // Releasing ⌥ commits the cycle: one focus event for the row the operator
  // stopped on, none when they stopped where they started.
  const onKeyUp = (event: KeyboardEvent) => {
    if (event.key !== "Alt") return;
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

  window.addEventListener("keydown", onKeyDown, true);
  window.addEventListener("keyup", onKeyUp, true);
  window.addEventListener("blur", onBlur);
  return () => {
    window.removeEventListener("keydown", onKeyDown, true);
    window.removeEventListener("keyup", onKeyUp, true);
    window.removeEventListener("blur", onBlur);
  };
}
