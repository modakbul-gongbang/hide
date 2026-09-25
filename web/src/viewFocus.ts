// What the page itself knows about the View areas, apart from the pure rules:
// which area a chord that could mean either one acts on (⌥W, text size), what
// the View areas last drew (a split's room is a pixel question, and a palette
// command asks it too), and whether a focus came from the keyboard. Each reads
// the page, so none of it lives in the rules or the store.

import type { SnapshotRest } from "./snapshot";
import type { Geometry, LayoutSizes, ViewFrame } from "./viewLayout";
import { workspaceViewOf } from "./workspace";

/** The View area when only Views show or the keyboard is inside it, else the Agent area. */
export function viewAreaInUse(rest: SnapshotRest | null): boolean {
  const view = workspaceViewOf(rest);
  if (!view) return false;
  if (view.mode === "views") return true;
  if (view.mode === "agents") return false;
  if (typeof document === "undefined") return false;
  // A narrow Together draws one working region (B13); while it is the View
  // areas, they are all that shows, wherever the keyboard is.
  if (drawnViews() !== null && document.querySelector("[data-agent-area]") === null) return true;
  const active = document.activeElement;
  return active instanceof Element && active.closest("[data-view-area]") !== null;
}

/**
 * The View areas as they are drawn now: the frame they show (its Workspace
 * and layout, which every View action names and acts on, contract 4.1), and
 * the geometry and token sizes they were measured with.
 */
export type DrawnViews = ViewFrame & { geometry: Geometry; sizes: LayoutSizes };

let drawn: DrawnViews | null = null;

/**
 * The View areas report every draw here as it is committed, and null when
 * they leave the screen, so an action reads the frame the operator sees.
 */
export function noteDrawnViews(views: DrawnViews | null): void {
  drawn = views;
}

/** What the View areas last drew, or null while none show. */
export function drawnViews(): DrawnViews | null {
  return drawn;
}

// A focus that lands while Tab is held is the operator moving the keyboard;
// any other focus without a pointer press is the page's own (a menu handing
// the keyboard back, a restored caret) and asks nothing of the core (S7 B20).
let tabbing = false;

/** Watches the Tab key for `focusFromKeyboard`; returns the removal. */
export function installFocusModality(): () => void {
  const down = (event: KeyboardEvent) => {
    if (event.key === "Tab") tabbing = true;
  };
  const up = (event: KeyboardEvent) => {
    if (event.key === "Tab") tabbing = false;
  };
  const reset = () => {
    tabbing = false;
  };
  window.addEventListener("keydown", down, true);
  window.addEventListener("keyup", up, true);
  window.addEventListener("blur", reset);
  return () => {
    window.removeEventListener("keydown", down, true);
    window.removeEventListener("keyup", up, true);
    window.removeEventListener("blur", reset);
    tabbing = false;
  };
}

/** Whether the focus landing now was moved by the keyboard. */
export function focusFromKeyboard(): boolean {
  return tabbing;
}
