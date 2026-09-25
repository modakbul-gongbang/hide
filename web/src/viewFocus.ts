// Which area a chord that could mean either one acts on (⌥W, text size): the
// View area when only Views show or the keyboard is inside it, else the Agent
// area. This reads the page's focus, so it lives apart from the pure rules.

import type { SnapshotRest } from "./snapshot";
import { workspaceViewOf } from "./workspace";

export function viewAreaInUse(rest: SnapshotRest | null): boolean {
  const view = workspaceViewOf(rest);
  if (!view) return false;
  if (view.mode === "views") return true;
  if (view.mode === "agents") return false;
  const active = typeof document === "undefined" ? null : document.activeElement;
  return active instanceof Element && active.closest("[data-view-area]") !== null;
}
