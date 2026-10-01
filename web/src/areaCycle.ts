// Focused-area cycling narrows the existing page MRU to one exact drawn area.
// No second history or committed selection lives here.
import { areaFrame } from "./areaFrames";
import { areasOf, findArea } from "./areaLayout";
import { displaySurface, panelItem, recentEntries, tabSurface, type CycleItem, type Surface } from "./recent";
import { frontCheckout, type SnapshotRest } from "./snapshot";
import { workspaceViewOf } from "./workspace";
import { useUiStore, type Cycle } from "./ui";
import { keyboardOwner, type KeyboardOwner } from "./viewFocus";

export type CycleScope = { deviceId: string; checkoutId: string; path: string; kind: "agent" | "view"; areaId: string };

/** The owner must belong to the front checkout and an area actually drawn. */
export function focusedCycleScope(rest: SnapshotRest | null, owner: KeyboardOwner = keyboardOwner()): CycleScope | null {
  const checkout = frontCheckout(rest);
  if (!checkout || owner.kind === "none" || owner.kind === "tool" || owner.workspace !== checkout.id || useUiStore.getState().screen?.kind !== "workspace") return null;
  const kind = owner.kind === "view" ? "view" : "agent";
  const frame = areaFrame(kind);
  if (!frame || frame.workspace.path !== checkout.path) return null;
  let areaId = owner.kind === "view" || owner.kind === "agent" ? owner.areaId : undefined;
  if (owner.kind === "pane") {
    areaId = areasOf(frame.layout.root).find((area) => {
      // A delegated canvas is not a member of the area's normal tab strip.
      const shown = kind === "agent" ? areaFrame("agent")?.layout.canvases[area.id] ?? area.active : area.active;
      return area.displays.some((item) => item.id === shown) && checkout.tabs.find((tab) => tab.id === shown)?.panes.some((pane) => pane.id === owner.paneId);
    })?.id;
  }
  if (!areaId || !frame.geometry.areas.some((area) => area.id === areaId)) return null;
  return { deviceId: frame.workspace.device_id, checkoutId: checkout.id, path: checkout.path, kind, areaId };
}

/** Null means the originating scope disappeared, never permission to widen it. */
export function scopedSurfaces(rest: SnapshotRest | null, scope: CycleScope): { surfaces: Surface[]; active: string | null } | null {
  const checkout = frontCheckout(rest);
  const frame = areaFrame(scope.kind);
  if (!checkout || checkout.id !== scope.checkoutId || checkout.path !== scope.path || !frame || frame.workspace.device_id !== scope.deviceId || frame.workspace.path !== scope.path || useUiStore.getState().screen?.kind !== "workspace") return null;
  const view = workspaceViewOf(rest);
  if (!view || view.device_id !== scope.deviceId || view.path !== scope.path || (scope.kind === "view" && view.panel === "closed")) return null;
  const layout = scope.kind === "view" ? view.layout : scope.deviceId === "local" ? view.agent_layout : frame.layout;
  if (!layout) return null;
  const area = findArea(layout.root, scope.areaId);
  if (!area || !frame.geometry.areas.some((box) => box.id === scope.areaId)) return null;
  const surfaces = scope.kind === "agent"
    ? area.displays.filter((item) => checkout.tabs.some((tab) => tab.id === item.id)).map((item) => tabSurface(checkout, item.id, scope.deviceId))
    : area.displays.map((item) => ({ ...displaySurface(checkout, item as import("./snapshot").ViewDisplaySnapshot), deviceId: scope.deviceId }));
  return { surfaces, active: area.active };
}

export function focusedSurface(rest: SnapshotRest | null, scope: CycleScope | null): Surface | null {
  const membership = scope && scopedSurfaces(rest, scope);
  return membership ? membership.surfaces.find((surface) => surface.id === membership.active) ?? null : null;
}

export function areaCycle(rest: SnapshotRest | null, owner?: KeyboardOwner): Cycle | null {
  const scope = focusedCycleScope(rest, owner);
  const membership = scope && scopedSurfaces(rest, scope);
  if (!scope || !membership || membership.surfaces.length < 2) return null;
  const current = membership.surfaces.find((surface) => surface.id === membership.active);
  if (!current) return null;
  const known = new Map(membership.surfaces.map((surface) => [surface.key, surface]));
  const order = [current, ...recentEntries().flatMap((entry) => {
    const surface = known.get(entry.key);
    return surface && surface.key !== current.key ? [surface] : [];
  })];
  const ordered = new Set(order.map((surface) => surface.key));
  order.push(...membership.surfaces.filter((surface) => !ordered.has(surface.key)));
  const items = order.map((surface) => panelItem(rest, surface)).filter((item): item is CycleItem => item !== null);
  return items.length > 1 ? { kind: "area", scope, originKey: current.key, items, index: 0 } : null;
}
