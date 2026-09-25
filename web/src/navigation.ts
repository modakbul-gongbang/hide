// Main and Project Overview (PRD S6 D-02, B1-B3, B21): every registered
// Project on every device, and one Project's Workspaces and agents, read from
// the snapshot the core already publishes - the navigator for this machine,
// each device's projected Herdr session, the registrations for a device that
// has no session to show. Nothing here is counted that the snapshot does not
// carry, and a device that cannot answer says so instead of showing zeros.

import { focusedRemoteDevice, frontCheckout, type AgentRow, type Device, type RemoteStatus, type SnapshotRest, type Workspace, type WorkspaceRegistration } from "./snapshot";

export type AgentGroup = "needs_you" | "done" | "working" | "seen";
export const AGENT_GROUPS: readonly { group: AgentGroup; label: string }[] = [
  { group: "needs_you", label: "Needs You" },
  { group: "done", label: "Done" },
  { group: "working", label: "Working" },
  { group: "seen", label: "Seen" },
];

export type GroupCounts = Record<AgentGroup, number>;

/**
 * What retrying an unavailable device does: reconnect its session, or
 * restart its helper, whose new connection reads the catalog again.
 */
export type AvailabilityRetry = "connect" | "helper";

/** How far a device's projects can be trusted right now. */
export type DeviceAvailability =
  | { state: "ready" }
  | { state: "loading"; text: string }
  | { state: "unavailable"; text: string; retry: AvailabilityRetry | null };

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
export function allAgents(remote: RemoteStatus[] | undefined, devices: Device[] | undefined, localAgents: AgentRow[]): ListedAgent[] {
  const listed: ListedAgent[] = localAgents.map((agent) => ({ agent, device: null }));
  for (const status of remote ?? []) {
    if (status.state !== "connected") continue;
    const device = devices?.find((row) => row.id === status.target_id)?.label ?? status.target_id;
    for (const agent of status.session?.agents ?? []) listed.push({ agent, device });
  }
  return listed;
}

/**
 * How many live descendants each agent has among the rows the core lists
 * (B13), keyed by pane, from one index of the rows rather than one per row.
 */
export function liveDescendantCounts(agents: AgentRow[]): Map<string, number> {
  const byPane = new Map(agents.map((row) => [row.pane_id, row]));
  return new Map(agents.map((agent) => [agent.pane_id, descendantsIn(agent, byPane)]));
}

function descendantsIn(agent: AgentRow, byPane: Map<string, AgentRow>): number {
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

/**
 * This machine's Projects count agents only while its Herdr answers: an
 * unreachable Herdr has no agents to list, and "No agents" would be a zero
 * nobody measured (B3). The core reconnects on its own and has no event that
 * reconnects sooner, so there is no Retry.
 */
function localAvailability(rest: SnapshotRest | null): DeviceAvailability {
  const herdr = rest?.status?.herdr;
  if (!herdr?.state || herdr.state === "connected") return { state: "ready" };
  if (herdr.state === "not_connected") return { state: "loading", text: "Connecting to Herdr…" };
  const reason = herdr.message ?? `Herdr is ${herdr.state.replace(/_/g, " ")}`;
  return { state: "unavailable", text: `${reason}. Agent counts show once it answers; Hide keeps trying.`, retry: null };
}

function deviceAvailability(device: Device, status: RemoteStatus | undefined): DeviceAvailability {
  if (device.kind !== "remote") return { state: "ready" };
  if (!status || status.state === "not_connected") return { state: "loading", text: "Connecting…" };
  if (status.state !== "connected" && status.state !== "stale") {
    return { state: "unavailable", text: status.message ?? `${device.label} is ${status.state.replace(/_/g, " ")}`, retry: status.state === "disabled" ? null : "connect" };
  }
  if (status.state === "stale") {
    return { state: "unavailable", text: status.message ?? `${device.label} is not connected; showing what it last reported`, retry: "connect" };
  }
  const catalog = status.catalog;
  if (catalog?.state === "resolving") return { state: "loading", text: "Reading projects…" };
  if (catalog?.state === "unavailable") return { state: "unavailable", text: catalog.message ?? "Projects could not be read", retry: "helper" };
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
  return left.label.localeCompare(right.label);
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

export type OverviewProject = {
  workspace: Workspace;
  /** The Project's agents, or null while its device cannot say which are current. */
  agents: AgentRow[] | null;
  /**
   * Every agent its device last reported, current or not: the Overview board
   * keeps the last rows while a device is unreachable, and a descendant may
   * work in another project's checkout (web-project-overview D-06).
   */
  deviceAgents: AgentRow[];
  device: Device | null;
  availability: DeviceAvailability;
};

/**
 * The Project an Overview shows, on whichever device it lives, with the
 * agents of that device. Like Main, it counts nothing its device cannot
 * answer for right now: an unreachable Herdr has no agents to list, and a
 * stale device's last report is not current (B3, B21).
 */
export function overviewProject(rest: SnapshotRest | null, localAgents: AgentRow[], projectId: string): OverviewProject | null {
  const local = rest?.navigator?.workspaces?.find((row) => row.id === projectId);
  if (local) {
    const availability = localAvailability(rest);
    return {
      workspace: local,
      agents: availability.state === "ready" ? projectAgents(local, localAgents) : null,
      deviceAgents: localAgents,
      device: rest?.navigator?.devices?.find((row) => row.kind !== "remote") ?? null,
      availability,
    };
  }
  for (const status of rest?.status?.remote ?? []) {
    const workspace = status.session?.workspaces.find((row) => row.id === projectId);
    if (workspace) {
      const device = rest?.navigator?.devices?.find((row) => row.id === status.target_id) ?? null;
      const availability = device ? deviceAvailability(device, status) : { state: "loading" as const, text: "Connecting…" };
      return {
        workspace,
        agents: availability.state === "ready" ? projectAgents(workspace, status.session?.agents ?? []) : null,
        deviceAgents: status.session?.agents ?? [],
        device,
        availability,
      };
    }
  }
  return null;
}

/** Herdr states in which a catalog will not change on its own soon. */
const SETTLED_HERDR = new Set(["connected", "unconfigured", "socket_missing", "unreachable", "stale", "incompatible"]);

/**
 * The first screen (S6 B19, D-11): the Workspace in front when the core
 * marks it as the one the operator last chose, now or before a restart,
 * Main when it is not (a first run, or a last Workspace that is gone) or when the device in front
 * has settled without one, and null while that device is still being
 * reached. The device in front decides: a device Workspace is known only
 * once that device's session arrives, and this machine's Herdr settling
 * first says nothing about it.
 */
export function startupScreen(rest: SnapshotRest | null, hasFront: boolean): "workspace" | "main" | null {
  if (!rest) return null;
  if (hasFront) return rest.workspace_view?.resumed ? "workspace" : "main";
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

/** What Main, an Overview or the Agents list asked to bring forward (B2, B12, B21). */
export type OpenTarget = { checkoutId: string; deviceId: string; path: string | null } | { paneId: string };

/**
 * A Workspace or agent asked for and not yet in front. The screen changes
 * only once the core has moved there, so a refusal leaves the operator where
 * they were, with the reason. `errorBefore` is the core's last error when the
 * request went out; a newer one is this request's refusal.
 */
export type Opening = { target: OpenTarget; errorBefore: number | null; failure: string | null };

/** How long an open may take before it is reported as not having happened. */
export const OPEN_ANSWER_TIMEOUT_MS = 15_000;

/** Whether the Workspace in front is the one an open asked for. */
export function openingLanded(rest: SnapshotRest | null, target: OpenTarget): boolean {
  const front = frontCheckout(rest);
  if (!front) return false;
  if ("paneId" in target) return front.tabs.some((tab) => tab.panes.some((pane) => pane.id === target.paneId));
  if (front.id === target.checkoutId) return true;
  // A device project with no Herdr workspace yet gets one at its folder, under a new id.
  return target.path !== null && front.path === target.path && (rest?.navigator?.focused_device_id ?? "local") === target.deviceId;
}

/** Where an open stands: "landed", the core's refusal, or null while it is on its way. */
export function openingProgress(rest: SnapshotRest | null, opening: Opening): "landed" | string | null {
  if (openingLanded(rest, opening.target)) return "landed";
  const error = rest?.status?.last_error;
  if (error && error.occurred_at !== opening.errorBefore) return error.message;
  return null;
}
