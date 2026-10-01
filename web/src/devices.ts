// The device rail's facts (PRD home-device-rail D-09, D-10, D-27, superseded
// by quick device-rail-badges): which devices the rail lists, whether each is
// connected, the Needs You, Done and Working counts each tile carries, and the
// Home row's project count. Everything is read from the snapshot the core
// already publishes; nothing is counted that it does not carry, and a device
// that is not connected has no count, since what it last reported is not current.

import { groupCounts } from "./navigation";
import type { AgentRow, Device, SnapshotRest, Workspace } from "./snapshot";

/** The rail is shown unless the operator hid it; the choice is the core's `ui_state.device_rail_visible`. */
export function railShown(rest: SnapshotRest | null): boolean {
  return rest?.ui_state?.device_rail_visible ?? true;
}

/** This machine's device id: the row of kind `local`, `local` before the snapshot names it. */
export function localDeviceId(rest: SnapshotRest | null): string {
  return rest?.navigator?.devices?.find((device) => device.kind !== "remote")?.id ?? "local";
}

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

/** The states a tile counts, in the order its circles stack from the top: Needs You, unseen Done, Working. */
export const BADGE_STATES = [
  { state: "needs_you", label: "Needs You" },
  { state: "done", label: "Done" },
  { state: "working", label: "Working" },
] as const;

/** The text color of a state's count, the circle's color read as text: the tile and the Agents tab show one color per state. */
export const BADGE_TEXT: Record<BadgeState, string> = { needs_you: "text-warning", done: "text-success", working: "text-agent-working" };

export type BadgeState = (typeof BADGE_STATES)[number]["state"];
export type TileBadge = { state: BadgeState; label: string; count: number };

export type StateCounts = Record<BadgeState, number>;

/** What a device's tile counts now: its agents in each of the three states, all zero while it is not connected. */
export function deviceStateCounts(rest: SnapshotRest | null, localAgents: AgentRow[], deviceId: string): StateCounts {
  const counts = groupCounts(deviceAgents(rest, localAgents, deviceId));
  return { needs_you: counts.needs_you, done: counts.done, working: counts.working };
}

/** A tile's circles: one per state with agents, in the fixed order, none for a zero (B4). */
export function tileBadges(counts: StateCounts): TileBadge[] {
  return BADGE_STATES.filter(({ state }) => counts[state] > 0).map(({ state, label }) => ({ state, label, count: counts[state] }));
}

/** The number a circle draws: the count, or `9+` from ten. */
export function badgeText(count: number): string {
  return count >= 10 ? "9+" : String(count);
}

/** What a tile is called for assistive technology: the name, the connection state, then each non-zero count (B4). */
export function tileName(label: string, connected: boolean, badges: readonly TileBadge[]): string {
  return [label, connected ? null : "연결 안 됨", ...badges.map((badge) => `${badge.label} ${badge.count}`)].filter(Boolean).join(", ");
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
export function frontTitle(devices: readonly Device[] | undefined, frontId: string | null | undefined): { name: string; note: string | null } {
  const device = devices?.find((row) => row.id === (frontId ?? "local"));
  return { name: device?.label ?? "This Mac", note: device?.kind === "remote" ? "Remote" : null };
}

/** What the sidebar's list shows: the front device's Projects or Agents tab, or the way to reconnect a device that cannot be read. */
export type SidebarBody = "disconnected" | "projects" | "agents";

/** Which list fills the sidebar (B8): the device's name and the way to reconnect while it is not connected, else the tab the operator chose. */
export function sidebarBody(rest: SnapshotRest | null, mode: "projects" | "agents"): SidebarBody {
  return deviceConnected(rest, frontDeviceId(rest)) ? mode : "disconnected";
}
