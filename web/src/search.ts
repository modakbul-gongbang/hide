// The two palettes' data (PRD B12, B13, D-05): ⌘K searches the snapshot the
// shell already holds, and ⌘P shows what hided's index ranked. The search
// entries, the fuzzy score and the grouping are pure functions, so the
// palette's behavior is testable without a browser; ⌘P's ranking happens in
// hided, beside the walk.

import { projectPaneIds } from "./navigation";
import { contextAgents, contextWorkspaces } from "./remote";
import type { SnapshotRest } from "./snapshot";
import { besideUnavailable, shownTools, viewCommands, type Geometry, type LayoutSizes, type ToolsPlacement, type ViewCommandId } from "./viewLayout";
import { LAYOUTS, canShowViewsOverAgents, workspaceViewOf, type ViewMode } from "./workspace";

/** The header an entry is drawn under, in the Swift search view's form (`herdr-ide > AGENTS`). */
export type SearchGroup = { id: string; label: string };

const COMMANDS_GROUP: SearchGroup = { id: "commands", label: "WORKSPACE > COMMANDS" };
const PROJECTS_GROUP: SearchGroup = { id: "projects", label: "WORKSPACES > PROJECTS" };
const CHECKOUTS_GROUP: SearchGroup = { id: "checkouts", label: "WORKSPACES > CHECKOUTS" };
/** An agent whose pane is in none of the listed projects' checkouts. */
const AGENTS_GROUP: SearchGroup = { id: "agents", label: "AGENTS" };

export type SearchEntry = {
  id: string;
  title: string;
  subtitle: string;
  kind: "agent" | "project" | "checkout" | "command";
  group: SearchGroup;
  /** An agent's kind, which picks its mark. */
  agentKind?: string;
  /** The ids the entry activates: a pane, or a workspace/checkout pair. */
  paneId?: string;
  workspaceId?: string;
  checkoutId?: string;
  /** What a command entry changes on the Workspace in front. */
  command?: { layout: ViewMode } | { viewsOverAgents: boolean } | { tool: "explorer" | "changes"; visible: boolean } | { view: ViewCommandId } | { openBeside: true };
  /** Why a command cannot run now; the palette shows it and runs nothing. */
  unavailable?: string | null;
};

/** The fuzzy score of `query` against `candidate`, mirroring the Swift scorer
 * (`WorkspaceFileSearchIndex.fuzzyScore`): characters in order, early and
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
 * View areas (a split's room is judged on it), and where its tools stand
 * (a narrow window's closed overlay shows none of them, S7 B12).
 */
export type WorkspaceOnScreen = { drawn: { geometry: Geometry; sizes: LayoutSizes } | null; placement: ToolsPlacement };

/**
 * The Workspace commands ⌘K offers while a Workspace is on screen: the three
 * layouts by the names the layout menu uses (S6 B5), each tool shown or
 * hidden by what it would do on screen, and the View area commands (S7 B20)
 * with the reason any of them cannot run now.
 */
export function workspaceCommands(rest: SnapshotRest | null, screen: WorkspaceOnScreen): SearchEntry[] {
  const view = workspaceViewOf(rest);
  if (!view) return [];
  const shown = shownTools(view, screen.placement);
  // Agents only is offered while the View areas are over the agents too,
  // since choosing it takes them down.
  const over = canShowViewsOverAgents(view) && view.views_over_agents === true;
  const entries: SearchEntry[] = LAYOUTS.filter((layout) => layout.mode !== view.mode || over).map((layout) => ({
    id: `command:layout:${layout.mode}`,
    title: `Layout: ${layout.label}`,
    subtitle: "Workspace layout",
    kind: "command",
    group: COMMANDS_GROUP,
    command: { layout: layout.mode },
  }));
  if (canShowViewsOverAgents(view)) {
    entries.push({
      id: "command:views-over-agents",
      title: over ? "Hide Views over agents" : "Show Views over agents",
      subtitle: "Workspace layout",
      kind: "command",
      group: COMMANDS_GROUP,
      command: { viewsOverAgents: !over },
    });
  }
  entries.push({
    id: "command:tool:explorer",
    title: shown.explorer ? "Hide Explorer" : "Show Explorer",
    subtitle: "Workspace tool",
    kind: "command",
    group: COMMANDS_GROUP,
    command: { tool: "explorer", visible: !shown.explorer },
  });
  entries.push({
    id: "command:tool:changes",
    title: shown.changes ? "Hide History" : "Show History",
    subtitle: "Workspace tool",
    kind: "command",
    group: COMMANDS_GROUP,
    command: { tool: "changes", visible: !shown.changes },
  });
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

/**
 * The snapshot rows ⌘K searches: Workspace commands when a Workspace is on
 * screen, then agents, projects and checkouts of the context on screen, so a
 * pick on a selected SSH device focuses that host's row rather than one on
 * this machine behind it. An agent is grouped under the first project whose
 * checkouts hold its pane, as the Swift search view groups it.
 */
export function searchEntries(rest: SnapshotRest | null, screen: WorkspaceOnScreen | null = null): SearchEntry[] {
  if (!rest) return [];
  const entries: SearchEntry[] = screen ? workspaceCommands(rest, screen) : [];
  const workspaces = contextWorkspaces(rest);
  const agentGroups = new Map<string, SearchGroup>();
  for (const workspace of workspaces) {
    const group = { id: `agents:${workspace.id}`, label: `${workspace.label} > AGENTS` };
    for (const paneId of projectPaneIds(workspace)) if (!agentGroups.has(paneId)) agentGroups.set(paneId, group);
  }
  for (const agent of contextAgents(rest, rest.navigator?.agents ?? [])) {
    entries.push({
      id: `agent:${agent.pane_id}`,
      title: agent.identity_label,
      subtitle: agent.detail || agent.status_label,
      kind: "agent",
      group: agentGroups.get(agent.pane_id) ?? AGENTS_GROUP,
      agentKind: agent.agent_kind,
      paneId: agent.pane_id,
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
      });
    }
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
