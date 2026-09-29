// Where the start panel starts an agent (PRD home-device-rail D-19, D-22):
// the default is what is in front, the menu lists the front device's Home and
// checkouts, then each other device's, and a device that is not connected is
// listed but cannot be chosen. A target is never remembered: the default is
// read from the page when the panel opens, so nothing here is state.

import { remoteContext } from "./remote";
import type { Checkout, Device, SnapshotRest, Workspace } from "./snapshot";
import { frontCheckout } from "./snapshot";
import type { Screen } from "./ui";

export const NOT_CONNECTED = "연결 안 됨";

type TargetBase = {
  /** Unique across devices; the menu's value. */
  key: string;
  deviceId: string;
  label: string;
  /** Why the target cannot be chosen now, or null. */
  disabled: string | null;
};

export type StartTarget =
  | (TargetBase & { kind: "home" })
  | (TargetBase & { kind: "checkout"; checkoutPath: string; workspaceId: string; checkoutId: string });

export type TargetGroup = { deviceId: string; deviceLabel: string; connected: boolean; items: StartTarget[] };

export type StartTargets = {
  /** The front device's group first, then each other device's, in the order the catalog lists devices. */
  groups: TargetGroup[];
  /** The key the panel opens on; null only while nothing can be chosen. */
  defaultKey: string | null;
};

export const homeKey = (deviceId: string) => `home:${deviceId}`;
export const checkoutKey = (deviceId: string, path: string) => `checkout:${deviceId}:${path}`;

const LOCAL: Device = { id: "local", label: "This Mac", kind: "local", state: "local", message: null, ssh_alias: null, agent_count: 0, test: null };

/** A device's workspaces as the catalog lists them, and whether the device answers commands now. */
function deviceWorkspaces(rest: SnapshotRest, device: Device): { workspaces: Workspace[]; connected: boolean } {
  if (device.kind !== "remote") return { workspaces: rest.navigator?.workspaces ?? [], connected: true };
  const status = rest.status?.remote?.find((row) => row.target_id === device.id) ?? null;
  const session = status?.session ?? null;
  return { workspaces: session?.workspaces ?? [], connected: status?.state === "connected" && session !== null };
}

function groupOf(rest: SnapshotRest, device: Device, front: boolean): TargetGroup {
  const { workspaces, connected } = deviceWorkspaces(rest, device);
  const disabled = connected ? null : NOT_CONNECTED;
  // Only the front device's items go unprefixed: the trigger already sits where that device is.
  const prefix = front ? "" : `${device.label} · `;
  const items: StartTarget[] = [{ kind: "home", key: homeKey(device.id), deviceId: device.id, label: `${prefix}Home`, disabled }];
  for (const workspace of workspaces) {
    if (workspace.is_home || workspace.temporary) continue;
    for (const checkout of workspace.checkouts) {
      if (!checkout.exists) continue;
      items.push({
        kind: "checkout",
        key: checkoutKey(device.id, checkout.path),
        deviceId: device.id,
        label: `${prefix}${workspace.label} · ${checkout.label}`,
        disabled,
        checkoutPath: checkout.path,
        workspaceId: workspace.id,
        checkoutId: checkout.id,
      });
    }
  }
  return { deviceId: device.id, deviceLabel: device.label, connected, items };
}

/** The checkout a project's start lands on: its main one, else the first that exists. */
function mainCheckout(workspace: Workspace): Checkout | null {
  return workspace.checkouts.find((row) => row.exists && row.is_primary) ?? workspace.checkouts.find((row) => row.exists) ?? null;
}

/** The key of the thing in front on the front device, before it is checked against the menu. */
function frontKey(rest: SnapshotRest, screen: Screen | null, inbox: boolean, deviceId: string, workspaces: Workspace[]): string {
  const home = homeKey(deviceId);
  // Inbox is selected in the rail while the center stays where it was, so what the center shows is not what is in front.
  if (inbox) return home;
  if (screen?.kind === "workspace") {
    const checkout = frontCheckout(rest);
    if (!checkout) return home;
    const workspace = workspaces.find((row) => row.checkouts.some((candidate) => candidate.id === checkout.id));
    return workspace && !workspace.is_home ? checkoutKey(deviceId, checkout.path) : home;
  }
  if (screen?.kind === "overview") {
    const workspace = workspaces.find((row) => row.id === screen.projectId);
    const checkout = workspace && !workspace.is_home ? mainCheckout(workspace) : null;
    return checkout ? checkoutKey(deviceId, checkout.path) : home;
  }
  // All projects, a device's Home, Inbox and Settings are all "no project in front".
  return home;
}

/**
 * `inbox` is the rail's Inbox selection (page state beside the screen); a
 * device's own Home screen names its device, which is then the front device.
 */
export function startTargets(rest: SnapshotRest | null, screen: Screen | null, inbox = false): StartTargets {
  if (!rest) return { groups: [], defaultKey: null };
  const devices = rest.navigator?.devices?.length ? rest.navigator.devices : [LOCAL];
  const named = !inbox && screen?.kind === "main" ? screen.deviceId : undefined;
  const frontId = named && devices.some((device) => device.id === named) ? named : (remoteContext(rest)?.device.id ?? "local");
  const ordered = [...devices.filter((device) => device.id === frontId), ...devices.filter((device) => device.id !== frontId)];
  const groups = ordered.map((device) => groupOf(rest, device, device.id === frontId));
  const items = groups.flatMap((group) => group.items);
  const usable = (key: string) => items.some((item) => item.key === key && item.disabled === null);
  const front = groups[0];
  const workspaces = front ? deviceWorkspaces(rest, ordered[0]!).workspaces : [];
  const wanted = front ? frontKey(rest, screen, inbox, front.deviceId, workspaces) : null;
  const defaultKey =
    (wanted && usable(wanted) ? wanted : null) ?? (front && usable(homeKey(front.deviceId)) ? homeKey(front.deviceId) : null) ?? items.find((item) => item.disabled === null)?.key ?? null;
  return { groups, defaultKey };
}

/** The target a start goes to: the operator's pick while it is still listed and usable, else the default. */
export function resolveTarget(targets: StartTargets, chosen: string | null): StartTarget | null {
  const items = targets.groups.flatMap((group) => group.items);
  const pick = chosen ? items.find((item) => item.key === chosen && item.disabled === null) : undefined;
  return pick ?? items.find((item) => item.key === targets.defaultKey) ?? null;
}
