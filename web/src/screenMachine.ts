import type { TFunction } from "i18next";
import type { MoveMachines, MoveView } from "./coreMove";
import { localDeviceId, type Device, type SnapshotRest } from "./snapshot";

/** The longest node id a hash may name. */
const NODE_MAX = 128;

/**
 * The machine named by a page's hash as `node`: `hide connect` adds it when
 * this machine's hided runs in the node role, its core on another machine
 * (docs/ARCHITECTURE.md, A core on another machine). A value that cannot be
 * a node id is not one.
 */
export function screenNodeFromHash(hash: string): string | null {
  const node = new URLSearchParams(hash.replace(/^#/, "")).get("node");
  return node && node.length <= NODE_MAX && /^[A-Za-z0-9._-]+$/.test(node) ? node : null;
}

function hashNode(): string | null {
  const hash = typeof window === "undefined" ? undefined : window.location?.hash;
  return hash ? screenNodeFromHash(hash) : null;
}

/** The core's own row: the machine the core runs on, wherever the window is. */
function coreRowId(devices: readonly Device[] | undefined): string {
  return devices?.find((device) => device.kind !== "remote")?.id ?? "";
}

/**
 * The device this screen runs on: what only this machine can do (reveal or
 * open with an app, a terminal path, a page on its loopback) follows its
 * panes and checkouts. Without a node in the hash it is the core's own.
 */
export function screenDeviceId(rest: SnapshotRest | null): string {
  return hashNode() ?? localDeviceId(rest);
}

/** The same, read from the device rows the core lists. */
export function windowDeviceId(devices: readonly Device[] | undefined): string {
  return hashNode() ?? coreRowId(devices);
}

/**
 * What this window calls a machine (PRD core-host-node-move B2, N1): its own
 * machine is This Mac wherever the core runs; the core's machine, seen from
 * a node, goes by the name that machine gives itself; any other device by
 * the name it was registered under.
 */
export function machineName(devices: readonly Device[] | undefined, deviceId: string, t: TFunction<"translation">): string {
  if (deviceId === windowDeviceId(devices)) return t("common.thisMac");
  const device = devices?.find((row) => row.id === deviceId);
  if (!device) return deviceId;
  return device.kind !== "remote" ? (device.machine_name ?? device.id) : device.label;
}

/** The core's machine as this window names it. */
export function coreMachineName(devices: readonly Device[] | undefined, t: TFunction<"translation">): string {
  return machineName(devices, coreRowId(devices), t);
}

/** Whether `deviceId` is the machine the core runs on. */
export function isCoreMachine(devices: readonly Device[] | undefined, deviceId: string): boolean {
  return deviceId === coreRowId(devices);
}

/** The device rows in this window's order: its own machine first, then the rest as the core lists them (N1). */
export function windowFirst(devices: readonly Device[] | undefined): Device[] {
  const rows = devices ?? [];
  const own = windowDeviceId(rows);
  return [...rows.filter((device) => device.id === own), ...rows.filter((device) => device.id !== own)];
}

/**
 * Whether the core works with another machine (Q11): this window is on a
 * node, or a node dials in to the core. Only then does the rail mark the
 * core's machine; a core alone on its Mac has nothing to tell apart (B1).
 */
export function coreShared(devices: readonly Device[] | undefined): boolean {
  return windowDeviceId(devices) !== coreRowId(devices) || (devices ?? []).some((device) => device.dials_in === true);
}

/**
 * The machines a move names, as this window calls them: toward a device,
 * from this Mac to that device; back, from the core's machine to this Mac.
 */
export function moveMachines(devices: readonly Device[] | undefined, view: Pick<MoveView, "direction" | "device">, t: TFunction<"translation">): MoveMachines {
  const here = t("common.thisMac");
  if (view.direction === "back") return { from: coreMachineName(devices, t), to: here };
  return { from: here, to: view.device ? machineName(devices, view.device, t) : here };
}
