// Main and Project Overview (PRD S6 D-02, B1-B3, B21): every registered
// Project on every device, and one Project's Workspaces and agents, read from
// the snapshot the core already publishes - the navigator for this machine,
// each device's projected Herdr session, the registrations for a device that
// has no session to show. Nothing here is counted that the snapshot does not
// carry, and a device that cannot answer says so instead of showing zeros.

import { focusedRemoteDevice, type AgentRow, type Checkout, type Device, type RemoteStatus, type SnapshotRest, type Workspace, type WorkspaceRegistration } from "./snapshot";

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

export type AgentSection = { group: string; label: string; agents: AgentRow[] };

/**
 * The Agents explorer's sections (S6 B13): every current agent under Needs
 * You, Done, Working and Seen, in the core's order within each, with an
 * empty group left out. A group the core names that this list does not know
 * is still shown, under its own name, rather than dropping its rows.
 */
export function agentSections(agents: AgentRow[]): AgentSection[] {
  const known = new Set<string>(AGENT_GROUPS.map((row) => row.group));
  const sections: AgentSection[] = AGENT_GROUPS.map(({ group, label }) => ({ group, label, agents: agents.filter((agent) => agent.group === group) }));
  for (const agent of agents) {
    if (known.has(agent.group)) continue;
    let section = sections.find((row) => row.group === agent.group);
    if (!section) {
      section = { group: agent.group, label: agent.group.replace(/_/g, " "), agents: [] };
      sections.push(section);
    }
    section.agents.push(agent);
  }
  return sections.filter((section) => section.agents.length > 0);
}

/** An agent row and, for a row on an SSH device, the device's name. */
export type ListedAgent = { agent: AgentRow; device: string | null };

/**
 * Every current agent the snapshot carries (S6 B13, D-12): this machine's
 * and each connected device's, each device's rows in the order its core
 * gave them. A device that is not connected lists nothing, since what it
 * last reported is not current.
 */
export function allAgents(rest: SnapshotRest | null, localAgents: AgentRow[]): ListedAgent[] {
  const listed: ListedAgent[] = localAgents.map((agent) => ({ agent, device: null }));
  for (const status of rest?.status?.remote ?? []) {
    if (status.state !== "connected") continue;
    const device = rest?.navigator?.devices?.find((row) => row.id === status.target_id)?.label ?? status.target_id;
    for (const agent of status.session?.agents ?? []) listed.push({ agent, device });
  }
  return listed;
}

/** How many live descendants an agent has among the rows the core lists (B13). */
export function liveDescendants(agent: AgentRow, agents: AgentRow[]): number {
  const byPane = new Map(agents.map((row) => [row.pane_id, row]));
  const seen = new Set<string>();
  const queue = [...(agent.lineage_child_pane_ids ?? [])];
  while (queue.length > 0) {
    const id = queue.shift()!;
    if (seen.has(id) || id === agent.pane_id) continue;
    const row = byPane.get(id);
    if (!row) continue;
    seen.add(id);
    queue.push(...(row.lineage_child_pane_ids ?? []));
  }
  return seen.size;
}

/** The agents of one checkout. */
export function checkoutAgents(checkout: Checkout, agents: AgentRow[]): AgentRow[] {
  const panes = new Set(checkout.tabs.flatMap((tab) => tab.panes.map((pane) => pane.id)));
  return agents.filter((agent) => panes.has(agent.pane_id));
}

/**
 * This machine's Projects count agents only while its Herdr answers: an
 * unreachable Herdr has no agents to list, and "No agents" would be a zero
 * nobody measured (B3). The core reconnects on its own, so there is no Retry.
 */
function localAvailability(rest: SnapshotRest | null): DeviceAvailability {
  const herdr = rest?.status?.herdr;
  if (!herdr?.state || herdr.state === "connected") return { state: "ready" };
  if (herdr.state === "not_connected") return { state: "loading", text: "Connecting to Herdr…" };
  const reason = herdr.message ?? `Herdr is ${herdr.state.replace(/_/g, " ")}`;
  return { state: "unavailable", text: `${reason}. Agent counts show once it answers; Hide keeps trying.`, retry: false };
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

function entryOf(workspace: Workspace, agents: AgentRow[], trusted: boolean, checkoutsTrusted = trusted): ProjectEntry {
  return {
    id: workspace.id,
    label: workspace.label,
    path: workspace.path,
    deviceId: workspace.device_id,
    pinned: workspace.pinned,
    workspace,
    workspaceCount: checkoutsTrusted ? workspace.checkouts.length : null,
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
    const availability = localAvailability(rest);
    // The checkouts are this machine's own facts; only the agents need Herdr.
    const projects = (rest?.navigator?.workspaces ?? []).map((workspace) => entryOf(workspace, localAgents, availability.state === "ready", true));
    sections.push({ device: local, local: true, availability, projects: [...projects].sort(byPinThenLabel) });
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

/** Herdr states in which a catalog will not change on its own soon. */
const SETTLED_HERDR = new Set(["connected", "unconfigured", "socket_missing", "unreachable", "stale", "incompatible"]);

/**
 * The first screen (S6 B19, D-11): the Workspace the core kept in front once
 * it resolves, Main once the device in front has settled without one, and
 * null while that device is still being reached. The device in front decides:
 * a device Workspace is known only once that device's session arrives, and
 * this machine's Herdr settling first says nothing about it.
 */
export function startupScreen(rest: SnapshotRest | null, hasFront: boolean): "workspace" | "main" | null {
  if (!rest) return null;
  if (hasFront) return "workspace";
  const device = focusedRemoteDevice(rest);
  if (!device) {
    const state = rest.status?.herdr?.state;
    return state && SETTLED_HERDR.has(state) ? "main" : null;
  }
  const status = rest.status?.remote?.find((row) => row.target_id === device.id);
  if (!status || status.state === "not_connected") return null;
  if (status.state === "connected" && !status.session) return null;
  return "main";
}
