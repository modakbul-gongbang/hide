import { REGISTRY, isCycleCommand, type CommandId } from "./shortcuts";
import { agentCommands, type AgentCommand, type AgentFrame } from "./agentLayout";
// The two palettes' data (PRD B12, B13, D-05): ⌘K searches the snapshot the
// shell already holds, and ⌘P shows what hided's index ranked. The search
// entries, the fuzzy score and the grouping are pure functions, so the
// palette's behavior is testable without a browser; ⌘P's ranking happens in
// hided, beside the walk.

import { projectPaneIds } from "./navigation";
import { frontDeviceId, localDeviceId } from "./devices";
import { projectsOf } from "./remote";
import type { AgentRow, Device, SnapshotRest, Workspace } from "./snapshot";
import { besideUnavailable, shownTool, viewCommands, type Geometry, type LayoutSizes, type ToolsPlacement, type ViewCommandId } from "./viewLayout";
import { PANEL_STATES, workspaceViewOf, type PanelState, type Tool } from "./workspace";

/** The header an entry is drawn under, in the search view's form (`herdr-ide > AGENTS`). */
export type SearchGroup = { id: string; label: string };

const COMMANDS_GROUP: SearchGroup = { id: "commands", label: "WORKSPACE > COMMANDS" };
/** Commands every screen has, unlike the Workspace's. */
const GLOBAL_COMMANDS_GROUP: SearchGroup = { id: "global-commands", label: "COMMANDS" };
const PROJECTS_GROUP: SearchGroup = { id: "projects", label: "WORKSPACES > PROJECTS" };
const CHECKOUTS_GROUP: SearchGroup = { id: "checkouts", label: "WORKSPACES > CHECKOUTS" };
const DEVICES_GROUP: SearchGroup = { id: "devices", label: "DEVICES" };
/** An agent whose pane is in none of the listed projects' checkouts. */
const AGENTS_GROUP: SearchGroup = { id: "agents", label: "AGENTS" };

export type SearchEntry = {
  id: string;
  title: string;
  subtitle: string;
  kind: "agent" | "project" | "checkout" | "command" | "device";
  group: SearchGroup;
  /** The device the entry is on, which activating it brings forward; absent on a command. */
  deviceId?: string;
  /** The device's chip while the entry is not on the device in front (PRD home-device-rail B40). */
  chip?: { label: string; local: boolean };
  /** An agent's kind, which picks its mark. */
  agentKind?: string;
  /** The ids the entry activates: a pane, or a workspace/checkout pair. */
  paneId?: string;
  workspaceId?: string;
  checkoutId?: string;
  /** What a command entry changes on the Workspace in front. */
  command?: { panel: PanelState } | { pinned: boolean } | { tool: Tool; visible: boolean } | { agent: AgentCommand } | { view: ViewCommandId } | { openBeside: true } | { startAgent: true } | { navigation: CommandId };
  /** Why a command cannot run now; the palette shows it and runs nothing. */
  unavailable?: string | null;
};

/** The fuzzy score of `query` against `candidate`, the same scorer hided's
 * index runs (`hided/src/index.rs`): characters in order, early and
 * adjacent matches higher, shorter candidates first on a tie. */
export function fuzzyScore(candidate: string, query: string): number | null {
  if (query.length === 0) return 0;
  const haystack = candidate;
  let cursor = 0;
  let score = 0;
  let previous = -1;
  for (const wanted of query) {
    const found = haystack.indexOf(wanted, cursor);
    if (found === -1) return null;
    score += 100 - Math.min(found, 90);
    if (previous !== -1 && previous + 1 === found) score += 35;
    if (found === 0 || "/_- .".includes(haystack[found - 1] ?? "")) score += 25;
    previous = found;
    cursor = found + 1;
  }
  score -= haystack.length;
  return score;
}

/**
 * The Workspace on screen as the page draws it: what it last drew of the
 * View areas (a split's room is judged on it) and where its tools stand (a
 * narrow panel's closed overlay shows none of them, S7 B12).
 */
export type WorkspaceOnScreen = { drawn: { geometry: Geometry; sizes: LayoutSizes } | null; placement: ToolsPlacement; agent?: AgentFrame | null };

/**
 * The Workspace commands ⌘K offers while a Workspace is on screen: the side
 * panel's other states and its pin (issue 170), each tool shown or hidden by
 * what it would do on screen, and the View area commands (S7 B20) with the
 * reason any of them cannot run now.
 */
export function workspaceCommands(rest: SnapshotRest | null, screen: WorkspaceOnScreen): SearchEntry[] {
  const view = workspaceViewOf(rest);
  if (!view) return [];
  const shown = shownTool(view, screen.placement);
  const entries: SearchEntry[] = PANEL_STATES.filter((state) => state.panel !== view.panel).map((state) => ({
    id: `command:panel:${state.panel}`,
    title: state.command,
    subtitle: "Workspace side panel",
    kind: "command",
    group: COMMANDS_GROUP,
    command: { panel: state.panel },
  }));
  entries.push({
    id: "command:panel-pin",
    title: view.pinned ? "Unpin side panel" : "Pin side panel",
    subtitle: "Workspace side panel",
    kind: "command",
    group: COMMANDS_GROUP,
    command: { pinned: !view.pinned },
  });
  entries.push({
    id: "command:tool:explorer",
    title: shown === "explorer" ? "Hide Explorer" : "Show Explorer",
    subtitle: "Workspace tool",
    kind: "command",
    group: COMMANDS_GROUP,
    command: { tool: "explorer", visible: shown !== "explorer" },
  });
  entries.push({
    id: "command:tool:changes",
    title: shown === "changes" ? "Hide History" : "Show History",
    subtitle: "Workspace tool",
    kind: "command",
    group: COMMANDS_GROUP,
    command: { tool: "changes", visible: shown !== "changes" },
  });
  if (screen.agent) for (const command of agentCommands(screen.agent)) {
    entries.push({ id: `command:agent:${command.id}`, title: command.label, subtitle: "Agent areas", kind: "command", group: COMMANDS_GROUP, command: { agent: command.id }, unavailable: command.unavailable });
  }
  if (!view.layout) return entries;
  for (const command of viewCommands(view.layout, screen.drawn)) {
    entries.push({
      id: `command:view:${command.id}`,
      title: command.title,
      subtitle: "View areas",
      kind: "command",
      group: COMMANDS_GROUP,
      command: { view: command.id },
      unavailable: command.unavailable,
    });
  }
  entries.push({
    id: "command:open_beside",
    title: "Open file to the side",
    subtitle: "View areas",
    kind: "command",
    group: COMMANDS_GROUP,
    command: { openBeside: true },
    unavailable: besideUnavailable(view.layout, screen.drawn),
  });
  return entries;
}

/** `에이전트 시작…` opens the start panel on every screen, and is how a browser tab reaches it, where ⌘N is the browser's (PRD home-device-rail D-21). */
const START_AGENT_ENTRY: SearchEntry = {
  id: "command:start-agent",
  title: "에이전트 시작…",
  subtitle: "Start an agent",
  kind: "command",
  group: GLOBAL_COMMANDS_GROUP,
  command: { startAgent: true },
};

const THIS_MAC = { id: "local", label: "This Mac", kind: "local" } as Device;

/** What ⌘K reads from one device: its label, its agents and its projects (not its Home, which the device entry stands for). */
type SearchDevice = { device: Device; agents: AgentRow[]; workspaces: Workspace[]; allWorkspaces: Workspace[] };

/** This machine and each connected device, in the rail's order; a device that is not connected has no current agents or projects to find. */
function searchDevices(rest: SnapshotRest): SearchDevice[] {
  const rows: SearchDevice[] = [];
  // A snapshot that names no device is this machine's alone.
  const devices = rest.navigator?.devices?.length ? rest.navigator.devices : [THIS_MAC];
  for (const device of devices) {
    if (device.id === localDeviceId(rest)) {
      const all = rest.navigator?.workspaces ?? [];
      rows.push({ device, agents: rest.navigator?.agents ?? [], workspaces: projectsOf(all), allWorkspaces: all });
      continue;
    }
    const status = rest.status?.remote?.find((row) => row.target_id === device.id);
    const session = status?.state === "connected" ? status.session : null;
    rows.push({ device, agents: session?.agents ?? [], workspaces: projectsOf(session?.workspaces ?? []), allWorkspaces: session?.workspaces ?? [] });
  }
  return rows;
}

/**
 * The snapshot rows ⌘K searches: Workspace commands when a Workspace is on
 * screen, then every device's agents, projects and checkouts and the devices
 * themselves (PRD home-device-rail D-16, B40), so a pick on another device
 * moves rail, sidebar and center there. A row not on the device in front
 * carries that device's chip. An agent is grouped under the first project
 * whose checkouts hold its pane, the device's Home when that holds it.
 */
export function searchEntries(rest: SnapshotRest | null, screen: WorkspaceOnScreen | null = null): SearchEntry[] {
  if (!rest) return [];
  const entries: SearchEntry[] = [...(screen ? workspaceCommands(rest, screen) : []), START_AGENT_ENTRY];
  entries.push(...REGISTRY.filter((command) => isCycleCommand(command.id)).map((command): SearchEntry => ({
    id: `command:navigation:${command.id}`, title: command.title, subtitle: command.id.endsWith("area_tab") ? "Focused Agent or View area" : command.id.endsWith("panel") ? "Across all panels and screens" : "Across projects", kind: "command", group: GLOBAL_COMMANDS_GROUP, command: { navigation: command.id },
  })));
  const front = frontDeviceId(rest);
  const devices = searchDevices(rest);
  for (const { device, agents, workspaces, allWorkspaces } of devices) {
    const chip = device.id === front ? undefined : { label: device.label, local: device.kind !== "remote" };
    const agentGroups = new Map<string, SearchGroup>();
    for (const workspace of allWorkspaces) {
      const group = { id: `agents:${workspace.id}`, label: `${workspace.is_home ? "Home" : workspace.label} > AGENTS` };
      for (const paneId of projectPaneIds(workspace)) if (!agentGroups.has(paneId)) agentGroups.set(paneId, group);
    }
    for (const agent of agents) {
      entries.push({
        id: `agent:${agent.pane_id}`,
        title: agent.identity_label,
        subtitle: agent.detail || agent.status_label,
        kind: "agent",
        group: agentGroups.get(agent.pane_id) ?? AGENTS_GROUP,
        agentKind: agent.agent_kind,
        paneId: agent.pane_id,
        deviceId: device.id,
        chip,
      });
    }
    for (const workspace of workspaces) {
      entries.push({
        id: `project:${workspace.id}`,
        title: workspace.label,
        subtitle: workspace.path,
        kind: "project",
        group: PROJECTS_GROUP,
        workspaceId: workspace.id,
        deviceId: device.id,
        chip,
      });
      for (const checkout of workspace.checkouts) {
        entries.push({
          id: `checkout:${checkout.id}`,
          title: `${workspace.label} / ${checkout.label}`,
          subtitle: checkout.path,
          kind: "checkout",
          group: CHECKOUTS_GROUP,
          workspaceId: workspace.id,
          checkoutId: checkout.id,
          deviceId: device.id,
          chip,
        });
      }
    }
  }
  // With this Mac alone there is no device to move to, so no device rows.
  for (const { device } of devices.length > 1 ? devices : []) {
    entries.push({
      id: `device:${device.id}`,
      title: device.label,
      subtitle: device.kind === "remote" ? "Remote device" : "This device",
      kind: "device",
      group: DEVICES_GROUP,
      deviceId: device.id,
    });
  }
  return entries;
}

/** The entries matching `query`, best first. An empty query lists them all. */
export function filterEntries(entries: SearchEntry[], query: string, limit = 80): SearchEntry[] {
  const needle = query.trim().toLowerCase();
  if (!needle) return entries.slice(0, limit);
  const scored = entries
    .map((entry) => ({ entry, score: fuzzyScore(`${entry.title} ${entry.subtitle}`.toLowerCase(), needle) }))
    .filter((row): row is { entry: SearchEntry; score: number } => row.score !== null);
  scored.sort((left, right) => right.score - left.score || left.entry.title.localeCompare(right.entry.title));
  return scored.slice(0, limit).map((row) => row.entry);
}

export type SearchSection = { group: SearchGroup; entries: SearchEntry[] };

/**
 * The ranked entries under their headers. A group stands where its best entry
 * ranked and its entries keep their rank, so the first row is still the best
 * match and grouping never reorders what `filterEntries` ranked inside a group.
 */
export function groupEntries(entries: SearchEntry[]): SearchSection[] {
  const sections = new Map<string, SearchSection>();
  for (const entry of entries) {
    const section = sections.get(entry.group.id);
    if (section) section.entries.push(entry);
    else sections.set(entry.group.id, { group: entry.group, entries: [entry] });
  }
  return [...sections.values()];
}
