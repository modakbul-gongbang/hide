// Recent navigation (docs/UI_BEHAVIOR.md, Recent navigation), kept for the
// session.
//
// One recent-use order over every surface this machine holds, across every
// project and checkout: its Herdr tabs and the View-area displays (file, diff
// and browser) of each checkout's Workspace, and the page's own screens, All
// projects and each Project's Overview, once the operator has been on them.
// Recent Panels walks it; Recent Projects walks the projects in the order
// they were used and restores each one's last Workspace surface, which is
// this same order narrowed to the project. The core reports what is in
// front, not what was before, and the screens are the page's own navigation,
// so the order is the shell's convenience: nothing here is authority, and a
// reload rebuilds it from use.

import { catalogWorkspaces, type AgentRow, type Checkout, type SnapshotRest, type ViewDisplaySnapshot, type Workspace } from "./snapshot";
import type { Screen } from "./ui";
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

/** A screen of the page's own: All projects, or one Project's Overview. */
export type PageScreen = Exclude<Screen, { kind: "workspace" }>;

/**
 * A page screen in the recent order. It joins the order when the page shows
 * it, since it is somewhere the operator goes rather than something the
 * session holds open, and an Overview leaves with its Project.
 */
export type ScreenVisit = { key: string; screen: PageScreen };

/** One entry of the recent order: a Workspace surface, or a screen of the page's own. */
export type RecentEntry = Surface | ScreenVisit;

function screenVisit(screen: PageScreen): ScreenVisit {
  return { key: screen.kind === "main" ? "main" : `overview:${screen.projectId}`, screen };
}

export function isScreenVisit(entry: RecentEntry): entry is ScreenVisit {
  return "screen" in entry;
}

/** All projects is always there; an Overview while its Project is in the catalog, on any device. */
function screenExists(rest: SnapshotRest | null, screen: PageScreen): boolean {
  return screen.kind === "main" || catalogWorkspaces(rest).some((workspace) => workspace.id === screen.projectId);
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
 * tab. `viewInUse` is the page's answer (`keyboardOwner`), since only the
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
 * What the operator is on now: All projects or an Overview while the page
 * shows one, whichever device is in front, else the Workspace surface in use
 * (`currentSurface`) while this machine is in front.
 */
export function currentEntry(screen: Screen | null, rest: SnapshotRest | null, local: boolean, viewInUse: boolean): RecentEntry | null {
  if (screen && screen.kind !== "workspace") return screenVisit(screen);
  return local ? currentSurface(rest, viewInUse) : null;
}

/**
 * What the core has in front: the device, checkout, visible tab, and the
 * front Workspace's panel state and active display. Only a change here, or the
 * keyboard moving between a checkout's areas, is a visit; an agent's status
 * or a sidebar click that has not landed yet moves none of it.
 */
export function focusSignature(rest: SnapshotRest | null): string {
  const navigator = rest?.navigator;
  const checkout = localCheckouts(rest).find((row) => row.id === navigator?.focused_checkout_id);
  const layout = checkout ? frontLayoutOf(rest, checkout) : null;
  const display = layout ? activeDisplay(layout)?.display.id : null;
  return [navigator?.focused_device_id, checkout?.id, checkout?.active_tab_id, workspaceViewOf(rest)?.panel, display].join("\u0000");
}

/**
 * Every entry the session still holds, in the navigator's order: each
 * checkout's Herdr tabs, the front Workspace's displays as the snapshot
 * carries them, and the displays remembered for a checkout not in front,
 * which the snapshot does not carry and which the core checks on commit;
 * then the remembered screens that still exist.
 */
export function availableEntries(rest: SnapshotRest | null, remembered: readonly RecentEntry[]): RecentEntry[] {
  const entries: RecentEntry[] = [];
  for (const checkout of localCheckouts(rest)) {
    for (const tab of checkout.tabs) if (tab.id) entries.push(tabSurface(checkout, tab.id));
    const layout = frontLayoutOf(rest, checkout);
    if (layout) {
      for (const area of areasOf(layout.root)) for (const display of area.displays) entries.push(displaySurface(checkout, display));
    } else {
      entries.push(...remembered.filter((entry) => !isScreenVisit(entry) && entry.kind !== AGENT_SURFACE && entry.checkoutId === checkout.id));
    }
  }
  entries.push(...remembered.filter((entry) => isScreenVisit(entry) && screenExists(rest, entry.screen)));
  return entries;
}

let entries: RecentEntry[] = [];
let projects: string[] = [];
/** The entry a Recent Panels or Recent Projects commit asked for, until the page shows it (see `expectSurface`). */
let expected: string | null = null;

/**
 * Brings the order up to date with what the session holds and moves the
 * entry in use to its front. Entries that are gone leave, surfaces never
 * used join at the end in the navigator's order, and a screen joins when it
 * is first shown, so the order is bounded by what exists: every surface, one
 * Overview per Project in the catalog, and All projects.
 */
export function observeEntries(rest: SnapshotRest | null, current: RecentEntry | null) {
  const available = availableEntries(rest, entries);
  const byKey = new Map(available.map((entry) => [entry.key, entry]));
  const known = new Set<string>();
  const next: RecentEntry[] = [];
  for (const entry of entries) {
    const fresh = byKey.get(entry.key);
    if (fresh && !known.has(entry.key)) {
      known.add(entry.key);
      next.push(fresh);
    }
  }
  for (const entry of available) {
    if (known.has(entry.key)) continue;
    known.add(entry.key);
    next.push(entry);
  }
  entries = next;
  // A commit's own frames pass through the target's other surface (the
  // checkout arrives before the keyboard does, an Overview stays on screen
  // until the Workspace is in front); none of them is a visit.
  if (expected && current?.key !== expected) return;
  expected = null;
  if (!current) return;
  const visited = isScreenVisit(current) ? (screenExists(rest, current.screen) ? current : null) : byKey.get(current.key);
  if (!visited) return;
  entries = [visited, ...entries.filter((entry) => entry.key !== visited.key)];
}

/** The project in front, first in the Recent Projects order. */
export function observeProject(workspaceId: string | null | undefined) {
  if (!workspaceId || projects[0] === workspaceId) return;
  projects = [workspaceId, ...projects.filter((id) => id !== workspaceId)];
}

/** Marks the entry a commit is bringing forward: visits are not recorded until the page shows it or the operator acts. */
export function expectSurface(key: string | null) {
  expected = key;
}

export function recentEntries(): readonly RecentEntry[] {
  return entries;
}

/** The project's Workspace surface used last, which Recent Projects restores. */
export function lastSurfaceOf(workspaceId: string): Surface | null {
  for (const entry of entries) if (!isScreenVisit(entry) && entry.workspaceId === workspaceId) return entry;
  return null;
}

/** `existing` projects in recent order, unused ones after in their own order. */
export function recentProjectOrder(existing: readonly string[]): string[] {
  const seen = projects.filter((id) => existing.includes(id));
  return [...seen, ...existing.filter((id) => !seen.includes(id))];
}

/** Test seam. */
export function resetRecent() {
  entries = [];
  projects = [];
  expected = null;
}

// --- rows -----------------------------------------------------------------

/** What committing a switcher row brings forward. */
export type CycleTarget =
  /** A Workspace surface, in its own checkout. */
  | { kind: "surface"; surface: Surface }
  /** All projects or a Project's Overview, which the page shows itself. */
  | { kind: "screen"; screen: PageScreen }
  /** A tab of the device in front, which that device's Herdr focuses. */
  | { kind: "device_tab"; tabId: string }
  /** A project with no surface to restore, on its checkout. */
  | { kind: "checkout"; workspaceId: string; checkoutId: string };

/** What a switcher row draws and what committing it names. */
export type CycleItem = {
  key: string;
  title: string;
  detail: string;
  kind: SurfaceKind | "project" | PageScreen["kind"];
  /** The one agent a tab holds, drawn with its status mark and its own mark. */
  agent: Pick<AgentRow, "agent_kind" | "symbol" | "status_label" | "demand" | "activity" | "emphasized" | "waiting_on_descendants"> | null;
  target: CycleTarget;
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
 * label, and a display its own name. A screen is called by its Project, or
 * All projects, and is gone with its Project.
 */
export function panelItem(rest: SnapshotRest | null, entry: RecentEntry): CycleItem | null {
  if (isScreenVisit(entry)) return screenItem(rest, entry);
  const surface = entry;
  const place = findCheckout(rest, surface.checkoutId);
  if (!place) return null;
  const detail = `${placeLabel(place.workspace, place.checkout)} · ${KIND_LABEL[surface.kind]}`;
  const base = { key: surface.key, kind: surface.kind, target: { kind: "surface", surface } as const, detail };
  if (surface.kind !== AGENT_SURFACE) return { ...base, title: surface.label, agent: null };
  const tab = place.checkout.tabs.find((row) => row.id === surface.id);
  if (!tab) return null;
  const paneIds = new Set(tab.panes.map((pane) => pane.id));
  const agents = (rest?.navigator?.agents ?? []).filter((agent) => paneIds.has(agent.pane_id));
  const agent = agents.length === 1 ? agents[0]! : null;
  return { ...base, title: agent?.identity_label ?? tab.label ?? surface.id, agent };
}

function screenItem(rest: SnapshotRest | null, visit: ScreenVisit): CycleItem | null {
  const { screen } = visit;
  const title = screen.kind === "main" ? "All projects" : catalogWorkspaces(rest).find((workspace) => workspace.id === screen.projectId)?.label;
  if (title === undefined) return null;
  return { key: visit.key, title, detail: "Overview", kind: screen.kind, agent: null, target: { kind: "screen", screen } };
}

/** A Recent Projects row: the project, with the surface and checkout it would come back on. */
export function projectItem(workspace: Workspace, rest: SnapshotRest | null, local: boolean): CycleItem | null {
  const last = local ? lastSurfaceOf(workspace.id) : null;
  const lastItem = last ? panelItem(rest, last) : null;
  const restored = lastItem ? last : null;
  const checkout = (restored && workspace.checkouts.find((row) => row.id === restored.checkoutId)) ?? workspace.checkouts[0];
  if (!checkout) return null;
  const detail = lastItem ? `${lastItem.title} · ${checkout.label}` : checkout.label;
  const target: CycleTarget = restored ? { kind: "surface", surface: restored } : { kind: "checkout", workspaceId: workspace.id, checkoutId: checkout.id };
  return { key: workspace.id, title: workspace.label, detail, kind: "project", agent: null, target };
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
