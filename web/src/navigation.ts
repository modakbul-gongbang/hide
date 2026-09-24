// Main and Project Overview (PRD S6 D-02, B1-B3, B21): every registered
// Project on every device, and one Project's Workspaces and agents, read from
// the snapshot the core already publishes - the navigator for this machine,
// each device's projected Herdr session, the registrations for a device that
// has no session to show. Nothing here is counted that the snapshot does not
// carry, and a device that cannot answer says so instead of showing zeros.

import type { AgentRow, Checkout, Device, RemoteStatus, SnapshotRest, Workspace, WorkspaceRegistration } from "./snapshot";

export type AgentGroup = "needs_you" | "done" | "working" | "seen";
export const AGENT_GROUPS: readonly { group: AgentGroup; label: string }[] = [
  { group: "needs_you", label: "Needs You" },
  { group: "done", label: "Done" },
  { group: "working", label: "Working" },
  { group: "seen", label: "Seen" },
];

export type GroupCounts = Record<AgentGroup, number>;

/** How far a device's projects can be trusted right now. */
export type DeviceAvailability =
  | { state: "ready" }
  | { state: "loading"; text: string }
  | { state: "unavailable"; text: string; retry: boolean };

export type ProjectEntry = {
  id: string;
  label: string;
  path: string;
  deviceId: string;
  pinned: boolean;
  /** The Project's catalog row, or null when its device has no session to show it from. */
  workspace: Workspace | null;
  workspaceCount: number | null;
  counts: GroupCounts | null;
};

export type DeviceSection = {
  device: Device;
  local: boolean;
  availability: DeviceAvailability;
  projects: ProjectEntry[];
};

function emptyCounts(): GroupCounts {
  return { needs_you: 0, done: 0, working: 0, seen: 0 };
}

/** The pane ids a Project's checkouts hold. */
export function projectPaneIds(workspace: Workspace): Set<string> {
  const ids = new Set<string>();
  for (const checkout of workspace.checkouts) {
    for (const tab of checkout.tabs) for (const pane of tab.panes) ids.add(pane.id);
  }
  return ids;
}

/** The agents running in a Project, in the core's order. */
export function projectAgents(workspace: Workspace, agents: AgentRow[]): AgentRow[] {
  const panes = projectPaneIds(workspace);
  return agents.filter((agent) => panes.has(agent.pane_id));
}

export function groupCounts(agents: AgentRow[]): GroupCounts {
  const counts = emptyCounts();
  for (const agent of agents) {
    if (agent.group in counts) counts[agent.group as AgentGroup] += 1;
  }
  return counts;
}

/** The agents of one checkout. */
export function checkoutAgents(checkout: Checkout, agents: AgentRow[]): AgentRow[] {
  const panes = new Set(checkout.tabs.flatMap((tab) => tab.panes.map((pane) => pane.id)));
  return agents.filter((agent) => panes.has(agent.pane_id));
}

function deviceAvailability(device: Device, status: RemoteStatus | undefined): DeviceAvailability {
  if (device.kind !== "remote") return { state: "ready" };
  if (!status || status.state === "not_connected") return { state: "loading", text: "Connecting…" };
  if (status.state !== "connected" && status.state !== "stale") {
    return { state: "unavailable", text: status.message ?? `${device.label} is ${status.state.replace(/_/g, " ")}`, retry: status.state !== "disabled" };
  }
  if (status.state === "stale") {
    return { state: "unavailable", text: status.message ?? `${device.label} is not connected; showing what it last reported`, retry: true };
  }
  const catalog = status.catalog;
  if (catalog?.state === "resolving") return { state: "loading", text: "Reading projects…" };
  if (catalog?.state === "unavailable") return { state: "unavailable", text: catalog.message ?? "Projects could not be read", retry: false };
  return { state: "ready" };
}

function entryOf(workspace: Workspace, agents: AgentRow[], trusted: boolean): ProjectEntry {
  return {
    id: workspace.id,
    label: workspace.label,
    path: workspace.path,
    deviceId: workspace.device_id,
    pinned: workspace.pinned,
    workspace,
    workspaceCount: trusted ? workspace.checkouts.length : null,
    counts: trusted ? groupCounts(projectAgents(workspace, agents)) : null,
  };
}

function registrationEntry(registration: WorkspaceRegistration): ProjectEntry {
  return {
    id: registration.id,
    label: registration.label,
    path: registration.path,
    deviceId: registration.device_id,
    pinned: registration.pinned,
    workspace: null,
    workspaceCount: null,
    counts: null,
  };
}

function byPinThenLabel(left: ProjectEntry, right: ProjectEntry): number {
  if (left.pinned !== right.pinned) return left.pinned ? -1 : 1;
  return 0;
}

/**
 * Main: one section per device, this machine first. A device with a session
 * lists its projected Projects; one without lists its registrations with no
 * counts, marked loading or unavailable, so nothing is guessed (B3, B21).
 */
export function mainSections(rest: SnapshotRest | null, localAgents: AgentRow[]): DeviceSection[] {
  const devices = rest?.navigator?.devices ?? [];
  const registrations = rest?.ui_state?.workspace_registrations ?? [];
  const sections: DeviceSection[] = [];
  const local = devices.find((device) => device.kind !== "remote");
  if (local) {
    const projects = (rest?.navigator?.workspaces ?? []).map((workspace) => entryOf(workspace, localAgents, true));
    sections.push({ device: local, local: true, availability: { state: "ready" }, projects: [...projects].sort(byPinThenLabel) });
  }
  for (const device of devices.filter((row) => row.kind === "remote")) {
    const status = rest?.status?.remote?.find((row) => row.target_id === device.id);
    const availability = deviceAvailability(device, status);
    const session = status?.session ?? null;
    const projects = session
      ? session.workspaces.map((workspace) => entryOf(workspace, session.agents, availability.state === "ready"))
      : registrations.filter((row) => row.device_id === device.id).map(registrationEntry);
    sections.push({ device, local: false, availability, projects: [...projects].sort(byPinThenLabel) });
  }
  return sections;
}

/** The Project an Overview shows, on whichever device it lives, with the agents of that device. */
export function overviewProject(rest: SnapshotRest | null, localAgents: AgentRow[], projectId: string): { workspace: Workspace; agents: AgentRow[]; device: Device | null } | null {
  const local = rest?.navigator?.workspaces?.find((row) => row.id === projectId);
  if (local) {
    return { workspace: local, agents: projectAgents(local, localAgents), device: rest?.navigator?.devices?.find((row) => row.kind !== "remote") ?? null };
  }
  for (const status of rest?.status?.remote ?? []) {
    const workspace = status.session?.workspaces.find((row) => row.id === projectId);
    if (workspace) {
      return {
        workspace,
        agents: projectAgents(workspace, status.session?.agents ?? []),
        device: rest?.navigator?.devices?.find((row) => row.id === status.target_id) ?? null,
      };
    }
  }
  return null;
}
