// Page-local keyboard ownership and measured View geometry. Focus is recorded
// when it changes, never inferred from document.activeElement at chord time.

import { locateDisplay, workspaceKey, type Geometry, type LayoutSizes, type ViewFrame } from "./viewLayout";
import { browserBridge } from "./host";
import { frontCheckout } from "./snapshot";
import { useShellStore } from "./store";
import { workspaceViewOf } from "./workspace";

export type KeyboardOwner =
  | { kind: "view"; workspace: string; areaId: string }
  | { kind: "pane"; workspace: string; paneId: string }
  | { kind: "tool"; workspace: string }
  | { kind: "none" };

let owner: KeyboardOwner = { kind: "none" };

export function noteKeyboardOwner(next: KeyboardOwner): void {
  owner = next;
}

export function keyboardOwner(): KeyboardOwner {
  return owner;
}

/** One bounded value per page; no core events or per-key DOM reads. */
export function installKeyboardOwner(): () => void {
  const record = (event: Event) => {
    const target = event.target instanceof Element ? event.target : null;
    // A menu temporarily borrows the keyboard from its invoker.
    if (target?.closest('[role="menu"]')) return;
    const workspace = target?.closest<HTMLElement>("[data-workspace-screen]")?.dataset.workspaceScreen;
    if (!target || !workspace) {
      owner = { kind: "none" };
      return;
    }
    const tool = target.closest("[data-workspace-tools]");
    const areaId = target.closest<HTMLElement>("[data-view-area-id]")?.dataset.viewAreaId;
    const paneId = target.closest<HTMLElement>("[data-pane-view]")?.dataset.paneView;
    owner = tool ? { kind: "tool", workspace }
      : areaId ? { kind: "view", workspace, areaId }
      : paneId ? { kind: "pane", workspace, paneId }
      : { kind: "none" };
  };
  // Native browser pages are outside the renderer DOM. Their host reports
  // the same ownership transition when the operator enters a page.
  const unsubscribeBrowser = browserBridge()?.onEvent((event) => {
    if (event.kind !== "focus") return;
    const rest = useShellStore.getState().rest;
    const view = workspaceViewOf(rest);
    const checkout = frontCheckout(rest);
    if (!view?.layout || !checkout || view.panel === "closed" || workspaceKey(view) !== event.workspace) return;
    const located = locateDisplay(view.layout.root, event.id);
    if (located) noteKeyboardOwner({ kind: "view", workspace: checkout.id, areaId: located.area.id });
  });
  window.addEventListener("focusin", record, true);
  window.addEventListener("pointerdown", record, true);
  return () => {
    unsubscribeBrowser?.();
    window.removeEventListener("focusin", record, true);
    window.removeEventListener("pointerdown", record, true);
    owner = { kind: "none" };
  };
}

export type CloseShortcutTarget =
  | { kind: "view"; id: string }
  | { kind: "pane"; id: string }
  | { kind: "nothing"; reason: string };

/** Close the keyboard's unit, never fall through to a larger unit. */
export function closeShortcutPolicy(input: {
  owner: KeyboardOwner;
  workspace: string | null;
  displayId: string | null;
  paneIds: readonly string[];
}): CloseShortcutTarget {
  const { owner, workspace, displayId, paneIds } = input;
  if (owner.kind === "none") return { kind: "nothing", reason: "no keyboard owner" };
  if (owner.workspace !== workspace) return { kind: "nothing", reason: "keyboard owner is outside the front Workspace" };
  if (owner.kind === "tool") return { kind: "nothing", reason: "the tool column owns the keyboard" };
  if (owner.kind === "view") return displayId
    ? { kind: "view", id: displayId }
    : { kind: "nothing", reason: "the focused View area has no visible display" };
  return paneIds.includes(owner.paneId)
    ? { kind: "pane", id: owner.paneId }
    : { kind: "nothing", reason: "the focused pane is no longer visible" };
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

export { focusFromKeyboard, installFocusModality } from "./areaFocus";
