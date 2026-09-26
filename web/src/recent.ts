// Recent navigation (docs/UI_BEHAVIOR.md, Recent navigation), kept for the
// session.
//
// One recent-use order over every surface this machine holds, across every
// project and checkout: its Herdr tabs and the View-area displays (file, diff
// and browser) of each checkout's Workspace. Recent Panels walks it; Recent
// Projects walks the projects in the order they were used and restores each
// one's last surface, which is this same order narrowed to the project. The
// core reports what is in front, not what was before, so the order is the
// shell's convenience: nothing here is authority, and a reload rebuilds it
// from use. Swift owner: `AgentMRU.swift`.

import type { AgentRow, Checkout, SnapshotRest, ViewDisplaySnapshot, Workspace } from "./snapshot";
import { activeDisplay, areasOf } from "./viewLayout";
import { workspaceViewOf } from "./workspace";

/** A Herdr tab, or what a View-area display shows. */
export type SurfaceKind = "herdr" | ViewDisplaySnapshot["kind"];

const AGENT_SURFACE = "herdr";

/** One surface a Recent Panels row stands for, with what committing it names. */
export type Surface = {
  key: string;
  kind: SurfaceKind;
  workspaceId: string;
  checkoutId: string;
  /** The Herdr tab id, or the display id within its checkout's Workspace. */
  id: string;
  /** A display's name as its View area last drew it; a tab's comes from the snapshot. */
  label: string;
};

export function tabSurface(checkout: Checkout, tabId: string): Surface {
  return { key: `tab\u0000${checkout.id}\u0000${tabId}`, kind: AGENT_SURFACE, workspaceId: checkout.workspace_id, checkoutId: checkout.id, id: tabId, label: "" };
}

export function displaySurface(checkout: Checkout, display: ViewDisplaySnapshot): Surface {
  return { key: `display\u0000${checkout.id}\u0000${display.id}`, kind: display.kind, workspaceId: checkout.workspace_id, checkoutId: checkout.id, id: display.id, label: display.label };
}

/** This machine's checkouts, in the navigator's order. */
function localCheckouts(rest: SnapshotRest | null): Checkout[] {
  return (rest?.navigator?.workspaces ?? []).filter((workspace) => workspace.device_id === "local").flatMap((workspace) => workspace.checkouts);
}

/** The Workspace in front when it is this checkout's, which is the only one whose displays the snapshot carries. */
function frontLayoutOf(rest: SnapshotRest | null, checkout: Checkout) {
  const view = workspaceViewOf(rest);
  return view && view.device_id === "local" && view.path === checkout.path ? (view.layout ?? null) : null;
}

/**
 * The surface the operator is using now: the focused checkout's active
 * display while the View area is the one in use, else its visible Herdr
 * tab. `viewInUse` is the page's answer (`viewAreaInUse`), since only the
 * page knows where the keyboard is.
 */
export function currentSurface(rest: SnapshotRest | null, viewInUse: boolean): Surface | null {
  const id = rest?.navigator?.focused_checkout_id;
  const checkout = localCheckouts(rest).find((row) => row.id === id);
  if (!checkout) return null;
  const layout = frontLayoutOf(rest, checkout);
  const display = viewInUse && layout ? activeDisplay(layout) : null;
  if (display) return displaySurface(checkout, display.display);
  return checkout.active_tab_id ? tabSurface(checkout, checkout.active_tab_id) : null;
}

/**
 * What the core has in front: the device, checkout, visible tab, and the
 * front Workspace's mode and active display. Only a change here, or the
 * keyboard moving between a checkout's areas, is a visit; an agent's status
 * or a sidebar click that has not landed yet moves none of it.
 */
export function focusSignature(rest: SnapshotRest | null): string {
  const navigator = rest?.navigator;
  const checkout = localCheckouts(rest).find((row) => row.id === navigator?.focused_checkout_id);
  const layout = checkout ? frontLayoutOf(rest, checkout) : null;
  const display = layout ? activeDisplay(layout)?.display.id : null;
  return [navigator?.focused_device_id, checkout?.id, checkout?.active_tab_id, workspaceViewOf(rest)?.mode, display].join("\u0000");
}

/**
 * Every surface the session still holds, in the navigator's order: each
 * checkout's Herdr tabs, the front Workspace's displays as the snapshot
 * carries them, and the displays remembered for a checkout not in front,
 * which the snapshot does not carry and which the core checks on commit.
 */
export function availableSurfaces(rest: SnapshotRest | null, remembered: readonly Surface[]): Surface[] {
  const surfaces: Surface[] = [];
  for (const checkout of localCheckouts(rest)) {
    for (const tab of checkout.tabs) if (tab.id) surfaces.push(tabSurface(checkout, tab.id));
    const layout = frontLayoutOf(rest, checkout);
    if (layout) {
      for (const area of areasOf(layout.root)) for (const display of area.displays) surfaces.push(displaySurface(checkout, display));
    } else {
      surfaces.push(...remembered.filter((surface) => surface.kind !== AGENT_SURFACE && surface.checkoutId === checkout.id));
    }
  }
  return surfaces;
}

let surfaces: Surface[] = [];
let projects: string[] = [];
/** The surface a Recent Panels or Recent Projects commit asked for, until the page shows it (see `expectSurface`). */
let expected: string | null = null;

/**
 * Brings the order up to date with what the session holds and moves the
 * surface in use to its front. Surfaces that are gone leave, and ones never
 * used join at the end in the navigator's order, so the order is bounded by
 * what exists.
 */
export function observeSurfaces(rest: SnapshotRest | null, current: Surface | null) {
  const available = availableSurfaces(rest, surfaces);
  const byKey = new Map(available.map((surface) => [surface.key, surface]));
  const known = new Set<string>();
  const next: Surface[] = [];
  for (const surface of surfaces) {
    const fresh = byKey.get(surface.key);
    if (fresh && !known.has(surface.key)) {
      known.add(surface.key);
      next.push(fresh);
    }
  }
  for (const surface of available) {
    if (known.has(surface.key)) continue;
    known.add(surface.key);
    next.push(surface);
  }
  surfaces = next;
  // A commit's own frames pass through the target's other surface (the
  // checkout arrives before the keyboard does); none of them is a visit.
  if (expected && current?.key !== expected) return;
  expected = null;
  if (!current || !byKey.has(current.key)) return;
  surfaces = [byKey.get(current.key)!, ...surfaces.filter((surface) => surface.key !== current.key)];
}

/** The project in front, first in the Recent Projects order. */
export function observeProject(workspaceId: string | null | undefined) {
  if (!workspaceId || projects[0] === workspaceId) return;
  projects = [workspaceId, ...projects.filter((id) => id !== workspaceId)];
}

/** Marks the surface a commit is bringing forward: visits are not recorded until the page shows it or the operator acts. */
export function expectSurface(key: string | null) {
  expected = key;
}

export function recentSurfaces(): readonly Surface[] {
  return surfaces;
}

/** The project's surface used last, which Recent Projects restores. */
export function lastSurfaceOf(workspaceId: string): Surface | null {
  return surfaces.find((surface) => surface.workspaceId === workspaceId) ?? null;
}

/** `existing` projects in recent order, unused ones after in their own order. */
export function recentProjectOrder(existing: readonly string[]): string[] {
  const seen = projects.filter((id) => existing.includes(id));
  return [...seen, ...existing.filter((id) => !seen.includes(id))];
}

/** Test seam. */
export function resetRecent() {
  surfaces = [];
  projects = [];
  expected = null;
}

// --- rows -----------------------------------------------------------------

/** What a switcher row draws and what committing it names. */
export type CycleItem = {
  key: string;
  title: string;
  detail: string;
  kind: SurfaceKind | "project";
  /** The one agent a tab holds, drawn with its status mark. */
  agent: Pick<AgentRow, "symbol" | "status_label" | "demand" | "activity" | "emphasized" | "waiting_on_descendants"> | null;
  /** The surface a commit brings forward, or null for a project with none to restore and for a device's tab. */
  surface: Surface | null;
  /** A tab of the device in front, which that device's Herdr focuses. */
  deviceTabId?: string;
  workspaceId: string;
  checkoutId: string;
};

const KIND_LABEL: Record<SurfaceKind, string> = { herdr: "Terminal", file: "File", diff: "Diff", browser: "Browser" };

/** "project · checkout", collapsed to the checkout when both share a name. */
export function placeLabel(workspace: Pick<Workspace, "label">, checkout: Pick<Checkout, "label">): string {
  return workspace.label === checkout.label ? checkout.label : `${workspace.label} · ${checkout.label}`;
}

function findCheckout(rest: SnapshotRest | null, checkoutId: string) {
  for (const workspace of rest?.navigator?.workspaces ?? []) {
    const checkout = workspace.checkouts.find((row) => row.id === checkoutId);
    if (checkout) return { workspace, checkout };
  }
  return null;
}

/**
 * A Recent Panels row: a tab holding exactly one agent pane is called by
 * that agent and carries its status mark; any other tab keeps its Herdr
 * label, and a display its own name.
 */
export function panelItem(rest: SnapshotRest | null, surface: Surface): CycleItem | null {
  const place = findCheckout(rest, surface.checkoutId);
  if (!place) return null;
  const detail = `${placeLabel(place.workspace, place.checkout)} · ${KIND_LABEL[surface.kind]}`;
  const base = { key: surface.key, kind: surface.kind, surface, workspaceId: surface.workspaceId, checkoutId: surface.checkoutId, detail };
  if (surface.kind !== AGENT_SURFACE) return { ...base, title: surface.label, agent: null };
  const tab = place.checkout.tabs.find((row) => row.id === surface.id);
  if (!tab) return null;
  const paneIds = new Set(tab.panes.map((pane) => pane.id));
  const agents = (rest?.navigator?.agents ?? []).filter((agent) => paneIds.has(agent.pane_id));
  const agent = agents.length === 1 ? agents[0]! : null;
  return { ...base, title: agent?.identity_label ?? tab.label ?? surface.id, agent };
}

/** A Recent Projects row: the project, with the surface and checkout it would come back on. */
export function projectItem(workspace: Workspace, rest: SnapshotRest | null, local: boolean): CycleItem | null {
  const last = local ? lastSurfaceOf(workspace.id) : null;
  const lastItem = last ? panelItem(rest, last) : null;
  const checkout = (lastItem && workspace.checkouts.find((row) => row.id === lastItem.checkoutId)) ?? workspace.checkouts[0];
  if (!checkout) return null;
  const detail = lastItem ? `${lastItem.title} · ${checkout.label}` : checkout.label;
  return { key: workspace.id, title: workspace.label, detail, kind: "project", agent: null, surface: lastItem ? last : null, workspaceId: workspace.id, checkoutId: checkout.id };
}

// --- the held cycle ---------------------------------------------------------

/** How many rows either switcher shows around its highlight. */
export const CYCLE_WINDOW = 9;

/** The rows around the highlight, at most `CYCLE_WINDOW`, the highlight among them. */
export function visibleWindow<T>(items: readonly T[], index: number): { start: number; rows: T[] } {
  const count = Math.min(CYCLE_WINDOW, items.length);
  const start = Math.min(Math.max(0, index - Math.floor(count / 2)), items.length - count);
  return { start, rows: items.slice(start, start + count) };
}

/**
 * The held cycle after the session changed under it: rows that are gone
 * leave without reordering the rest, a highlight that went moves to the next
 * surviving row, and with none surviving the cycle ends.
 */
export function reconcileCycle<T extends { key: string }>(items: readonly T[], index: number, alive: (item: T) => boolean): { items: T[]; index: number } | null {
  if (items.length === 0) return null;
  const successor = Array.from({ length: items.length }, (_, step) => items[(index + step) % items.length]!).find(alive);
  if (!successor) return null;
  const kept = items.filter(alive);
  return { items: kept, index: kept.indexOf(successor) };
}
