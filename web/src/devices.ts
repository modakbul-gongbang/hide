// The device rail's facts (PRD home-device-rail D-09, D-10, D-27, superseded
// by quick device-rail-badges and quick device-rail-slack): which devices the
// rail lists, whether each is connected, the Needs You and unseen Done counts
// each tile carries, and the Home row's project count. Everything is read from the snapshot the core
// already publishes; nothing is counted that it does not carry, and a device
// that is not connected has no count, since what it last reported is not current.

import type { TFunction } from "i18next";
import { groupCounts } from "./navigation";
import { localDeviceId, type AgentRow, type Device, type SnapshotRest, type Workspace } from "./snapshot";

/** The rail is shown unless the operator hid it; the choice is the core's `ui_state.device_rail_visible`. */
export function railShown(rest: SnapshotRest | null): boolean {
  return rest?.ui_state?.device_rail_visible ?? true;
}

export { localDeviceId } from "./snapshot";

/** The device in front: the one the core focuses, this machine by default. */
export function frontDeviceId(rest: SnapshotRest | null): string {
  return rest?.navigator?.focused_device_id ?? localDeviceId(rest);
}

/** Whether the core reads the device now: this machine always, a remote while its connection row says connected. */
export function deviceConnected(rest: SnapshotRest | null, deviceId: string): boolean {
  if (deviceId === localDeviceId(rest)) return true;
  return rest?.status?.remote?.find((row) => row.target_id === deviceId)?.state === "connected";
}

/** The agents a device reports now: this machine's, or a connected device's own; none while it is not connected. */
export function deviceAgents(rest: SnapshotRest | null, localAgents: AgentRow[], deviceId: string): AgentRow[] {
  if (deviceId === localDeviceId(rest)) return localAgents;
  const status = rest?.status?.remote?.find((row) => row.target_id === deviceId);
  return status?.state === "connected" ? (status.session?.agents ?? []) : [];
}

/** The states the Agents tab counts above its list, in its order: Needs You, unseen Done, Working. */
export const BADGE_STATES = [{ state: "needs_you" }, { state: "done" }, { state: "working" }] as const;

/** The text color of a state's count in the Agents tab's header: one color per state, the fill the rail's marks wear. */
export const BADGE_TEXT: Record<BadgeState, string> = { needs_you: "text-warning", done: "text-success", working: "text-agent-working" };

export type BadgeState = (typeof BADGE_STATES)[number]["state"];

/**
 * What a device's tile counts now (quick device-rail-slack): its Needs You and
 * unseen Done agents, both zero while it is not connected. Working is not on
 * the rail, since it is no reason to switch device; the Agents tab counts it.
 */
export type TileCounts = { needs_you: number; done: number };

export function tileCounts(rest: SnapshotRest | null, localAgents: AgentRow[], deviceId: string): TileCounts {
  const counts = groupCounts(deviceAgents(rest, localAgents, deviceId));
  return { needs_you: counts.needs_you, done: counts.done };
}

/** The number the Needs You pill draws: the count, or `9+` from ten. */
export function badgeText(count: number): string {
  return count >= 10 ? "9+" : String(count);
}

/**
 * The letters a remote device's tile draws in place of a name: the first
 * letter of each of the first two words (`Mac mini` → `Mm`, `build-box` →
 * `Bb`), or the first letter alone for a one-word name (`mini` → `M`), the
 * first one capitalized. The name itself is the tile's hint.
 */
export function tileMonogram(label: string): string {
  const [first = "", second = ""] = label.normalize("NFC").split(/[\s._-]+/).filter(Boolean);
  const initial = (word: string) => Array.from(word)[0] ?? "";
  return initial(first).toLocaleUpperCase() + initial(second);
}

/** What a tile says besides its name: that it is not connected, or each non-zero count it marks, in full where the pill reads `9+`. */
function tileFacts(connected: boolean, counts: TileCounts, t: TFunction<"translation">): string[] {
  if (!connected) return [t("devices.rail.notConnected")];
  return [counts.needs_you > 0 ? t("devices.rail.needsYou", { count: counts.needs_you }) : null, counts.done > 0 ? t("devices.rail.done", { count: counts.done }) : null].filter((fact) => fact !== null);
}

/** What a tile is called for assistive technology: the name, then its connection or its counts (B4). */
export function tileName(label: string, connected: boolean, counts: TileCounts, t: TFunction<"translation">): string {
  return [label, ...tileFacts(connected, counts, t)].join(", ");
}

/** A tile's hint, the only place its name is written: the name, then its connection or its counts (`mini · Needs You 12 · Done 1`). */
export function tileHint(label: string, connected: boolean, counts: TileCounts, t: TFunction<"translation">): string {
  return [label, ...tileFacts(connected, counts, t)].join(" · ");
}

/** How many projects a device has registered, without its Home (D-04, B16): a count from the registrations, not from any folder. */
export function homeProjectCount(rest: SnapshotRest | null, deviceId: string): number {
  return (rest?.ui_state?.workspace_registrations ?? []).filter((row) => row.device_id === deviceId && !row.home).length;
}

/** The device's Home once its first start created it, from the projection it reports. */
export function homeOf(rest: SnapshotRest | null, deviceId: string): Workspace | null {
  const workspaces = deviceId === localDeviceId(rest) ? rest?.navigator?.workspaces : rest?.status?.remote?.find((row) => row.target_id === deviceId)?.session?.workspaces;
  return workspaces?.find((row) => row.is_home) ?? null;
}

/** The name on the sidebar's top line and the smaller word after it: the device in front. */
export function frontTitle(devices: readonly Device[] | undefined, frontId: string | null | undefined, t: TFunction<"translation">): { name: string; note: string | null } {
  const device = frontId ? devices?.find((row) => row.id === frontId) : devices?.find((row) => row.kind !== "remote");
  return { name: device?.label ?? t("common.thisMac"), note: device?.kind === "remote" ? t("devices.remote") : null };
}

/** What the sidebar's list shows: the front device's Projects or Agents tab, or the way to reconnect a device that cannot be read. */
export type SidebarBody = "disconnected" | "projects" | "agents";

/** Which list fills the sidebar (B8): the device's name and the way to reconnect while it is not connected, else the tab the operator chose. */
export function sidebarBody(rest: SnapshotRest | null, mode: "projects" | "agents"): SidebarBody {
  return deviceConnected(rest, frontDeviceId(rest)) ? mode : "disconnected";
}
