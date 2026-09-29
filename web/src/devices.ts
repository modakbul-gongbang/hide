// The device rail's facts (PRD home-device-rail D-09..D-11, D-27): which
// devices the rail lists, whether each is connected, the Needs You count each
// tile carries, and the Home row's project count. Everything is read from the
// snapshot the core already publishes; nothing is counted that it does not
// carry, and a device that is not connected has no count, since what it last
// reported is not current.

import { groupCounts } from "./navigation";
import type { AgentRow, Device, SnapshotRest, Workspace } from "./snapshot";

/** The Inbox's selection value in the rail, beside the device ids. */
export const INBOX = "inbox";

const NO_DEVICES: Device[] = [];

/** Every registered SSH device, connected or not, in the core's order. */
export function remoteDevices(rest: SnapshotRest | null): Device[] {
  const devices = rest?.navigator?.devices;
  return devices?.some((device) => device.kind === "remote") ? devices.filter((device) => device.kind === "remote") : NO_DEVICES;
}

/** The rail exists while at least one remote device is registered, connected or not (D-11). */
export function railVisible(rest: SnapshotRest | null): boolean {
  return remoteDevices(rest).length > 0;
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

/** A tile's badge: the device's Needs You agents, or null when it has none or is not connected (B3, B8). */
export function deviceBadge(rest: SnapshotRest | null, localAgents: AgentRow[], deviceId: string): number | null {
  const count = groupCounts(deviceAgents(rest, localAgents, deviceId)).needs_you;
  return count > 0 ? count : null;
}

/** The Inbox's badge: the sum over connected devices, this machine included; a disconnected device adds nothing (D-27). */
export function inboxBadge(rest: SnapshotRest | null, localAgents: AgentRow[]): number | null {
  const devices = rest?.navigator?.devices ?? [];
  const total = devices.reduce((sum, device) => sum + (deviceBadge(rest, localAgents, device.id) ?? 0), 0);
  return total > 0 ? total : null;
}

/** What a tile is called for assistive technology: the name, then the state (B41). */
export function tileName(label: string, connected: boolean, badge: number | null): string {
  return [label, connected ? null : "연결 안 됨", badge === null ? null : `Needs You ${badge}`].filter(Boolean).join(", ");
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

/** The name on the sidebar's top line and the smaller word after it (D-14): the device in front, or the Inbox. */
export function frontTitle(devices: readonly Device[] | undefined, frontId: string | null | undefined, inbox: boolean): { name: string; note: string | null } {
  if (inbox) return { name: "Inbox", note: "모든 기기" };
  const device = devices?.find((row) => row.id === (frontId ?? "local"));
  return { name: device?.label ?? "This Mac", note: device?.kind === "remote" ? "Remote" : null };
}

/** What the sidebar's lists show: the Inbox, a device that cannot be read, its projects, or with no rail the tab the operator chose. */
export type SidebarBody = "inbox" | "disconnected" | "projects" | "agents";

/**
 * Which list fills the sidebar (PRD home-device-rail B2, B4, B8, B11): with
 * the rail, the Inbox while it is selected, else the device in front, its
 * name and the way to reconnect while it is not connected; with no rail the
 * Projects | Agents tab.
 */
export function sidebarBody(rest: SnapshotRest | null, inbox: boolean, mode: "projects" | "agents"): SidebarBody {
  if (!railVisible(rest)) return mode;
  if (inbox) return "inbox";
  return deviceConnected(rest, frontDeviceId(rest)) ? "projects" : "disconnected";
}
