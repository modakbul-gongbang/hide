// ⌘K's data (PRD cmdk-navigation): the snapshot rows the palette finds by
// name or #number and the entries it draws for what is in front. The entries,
// the fuzzy score, the number-first ranking and the grouping are pure
// functions, so the palette's behavior is testable without a browser; ⌘P's
// ranking happens in hided, beside the walk.

import { checkoutPlaces } from "./navigation";
import { frontDeviceId, localDeviceId } from "./devices";
import { projectsOf } from "./remote";
import type { AgentRow, Checkout, Device, GithubSearchResult, PullRequest, SnapshotRest, Task, Workspace } from "./snapshot";

/** The header an entry is drawn under: one per kind of thing. */
export type SearchGroup = { id: string; label: string };

export const ISSUES_GROUP: SearchGroup = { id: "issues", label: "Issues" };
export const PULL_REQUESTS_GROUP: SearchGroup = { id: "pull-requests", label: "Pull requests" };
export const AGENTS_GROUP: SearchGroup = { id: "agents", label: "Agents" };
export const PROJECTS_GROUP: SearchGroup = { id: "projects", label: "Projects" };
export const CHECKOUTS_GROUP: SearchGroup = { id: "checkouts", label: "Checkouts" };
export const DEVICES_GROUP: SearchGroup = { id: "devices", label: "Devices" };
/** The one command ⌘K keeps. */
const COMMANDS_GROUP: SearchGroup = { id: "commands", label: "Commands" };
export const GITHUB_GROUP: SearchGroup = { id: "github", label: "GitHub" };
/** The Related list's own header; an entry in it keeps its kind's group for search. */
export const RELATED_GROUP: SearchGroup = { id: "related", label: "Related" };

export type EntryKind = "agent" | "project" | "checkout" | "device" | "issue" | "pr" | "command" | "github";

/** A colour family for a state; the palette maps each to a token class. */
export type Tone = "working" | "attention" | "done" | "open" | "pending" | "failed" | "muted";

export type EntryStatus = { tone: Tone; label: string };

export type SearchEntry = {
  id: string;
  title: string;
  subtitle: string;
  kind: EntryKind;
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
  /** An issue's or pull request's number, which `#N` ranks first. */
  number?: number;
  taskKey?: string;
  /** Where a pull request or issue lives on GitHub; a result the core searched is opened there. */
  url?: string | null;
  /** `owner/repo` of a GitHub search result. */
  repository?: string;
  /** The rows the snapshot carried, which the detail reads facts from. */
  agent?: AgentRow;
  checkout?: Checkout;
  workspace?: Workspace;
  pr?: PullRequest;
  task?: Task;
  /** The state drawn at the row's end, when the snapshot names one. */
  status?: EntryStatus;
  /** A pull request's CI rollup, when it is known. */
  ci?: EntryStatus;
  /** The row of a relation list: how deep it nests and what it is to the thing in front. */
  depth?: number;
  tag?: "here" | "parent";
  /** The one command ⌘K keeps. */
  command?: "start_agent";
  /** A GitHub search result: its row is opened on GitHub, never in Hide. */
  external?: boolean;
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

/** `에이전트 시작…` opens the start panel on every screen, and is how a browser tab reaches it, where ⌘N is the browser's (PRD home-device-rail D-21). */
export const START_AGENT_ENTRY: SearchEntry = {
  id: "command:start-agent",
  title: "에이전트 시작…",
  subtitle: "Start an agent",
  kind: "command",
  group: COMMANDS_GROUP,
  command: "start_agent",
};

const THIS_MAC = { id: "local", label: "This Mac", kind: "local" } as Device;

/** What ⌘K reads from one device: its label, its agents and its projects (not its Home, which the device entry stands for). */
export type SearchDevice = { device: Device; agents: AgentRow[]; workspaces: Workspace[]; allWorkspaces: Workspace[]; local: boolean };

/** This machine and each connected device, in the rail's order; a device that is not connected has no current agents or projects to find. */
export function searchDevices(rest: SnapshotRest): SearchDevice[] {
  const rows: SearchDevice[] = [];
  // A snapshot that names no device is this machine's alone.
  const devices = rest.navigator?.devices?.length ? rest.navigator.devices : [THIS_MAC];
  for (const device of devices) {
    if (device.id === localDeviceId(rest)) {
      const all = rest.navigator?.workspaces ?? [];
      rows.push({ device, agents: rest.navigator?.agents ?? [], workspaces: projectsOf(all), allWorkspaces: all, local: true });
      continue;
    }
    const status = rest.status?.remote?.find((row) => row.target_id === device.id);
    const session = status?.state === "connected" ? status.session : null;
    rows.push({ device, agents: session?.agents ?? [], workspaces: projectsOf(session?.workspaces ?? []), allWorkspaces: session?.workspaces ?? [], local: false });
  }
  return rows;
}

export function deviceChip(device: Device, front: string): SearchEntry["chip"] {
  return device.id === front ? undefined : { label: device.label, local: device.kind !== "remote" };
}

/** An agent's state as the sidebar colours it (`chipTone`'s rules, as tones). */
export function agentStatus(agent: Pick<AgentRow, "demand" | "activity" | "emphasized" | "status_label">): EntryStatus {
  const tone: Tone =
    agent.demand === "error"
      ? "failed"
      : agent.demand === "question" || agent.demand === "approval"
        ? "attention"
        : agent.activity === "working"
          ? "working"
          : agent.activity === "stopped" && agent.emphasized
            ? "done"
            : "muted";
  return { tone, label: agent.status_label };
}

export function agentEntry(scope: SearchDevice, agent: AgentRow, place: string | null, front: string): SearchEntry {
  const sentence = agent.detail || agent.status_label;
  return {
    id: `agent:${agent.pane_id}`,
    title: agent.identity_label,
    subtitle: place ? `${place} · ${sentence}` : sentence,
    kind: "agent",
    group: AGENTS_GROUP,
    agentKind: agent.agent_kind,
    paneId: agent.pane_id,
    deviceId: scope.device.id,
    chip: deviceChip(scope.device, front),
    agent,
    status: agentStatus(agent),
  };
}

export function projectEntry(scope: SearchDevice, workspace: Workspace, front: string): SearchEntry {
  return {
    id: `project:${workspace.id}`,
    title: workspace.label,
    subtitle: workspace.path,
    kind: "project",
    group: PROJECTS_GROUP,
    workspaceId: workspace.id,
    deviceId: scope.device.id,
    chip: deviceChip(scope.device, front),
    workspace,
  };
}

/** A checkout row; `compact` names it by its branch alone, as a relation group's head does under its project. */
export function checkoutEntry(scope: SearchDevice, workspace: Workspace, checkout: Checkout, front: string, compact = false): SearchEntry {
  return {
    id: `checkout:${checkout.id}`,
    title: compact ? (checkout.branch ?? checkout.label) : `${workspace.label} / ${checkout.label}`,
    subtitle: compact ? workspace.label : checkout.path,
    kind: "checkout",
    group: CHECKOUTS_GROUP,
    workspaceId: workspace.id,
    checkoutId: checkout.id,
    deviceId: scope.device.id,
    chip: deviceChip(scope.device, front),
    workspace,
    checkout,
  };
}

const CI_STATUS: Partial<Record<NonNullable<PullRequest["checks"]>, EntryStatus>> = {
  pending: { tone: "pending", label: "CI 진행 중" },
  failed: { tone: "failed", label: "CI 실패" },
  passing: { tone: "done", label: "CI 통과" },
};

const PR_STATE: Record<PullRequest["badge"], { tone: Tone; label: string }> = {
  open: { tone: "open", label: "Open" },
  review: { tone: "open", label: "Open" },
  merged: { tone: "muted", label: "Merged" },
  closed: { tone: "muted", label: "Closed" },
};

export function prStateLabel(pr: PullRequest): string {
  return PR_STATE[pr.badge].label;
}

export function pullRequestEntry(scope: SearchDevice, workspace: Workspace, pr: PullRequest, front: string): SearchEntry {
  return {
    id: `pr:${workspace.id}:${pr.number}`,
    title: `#${pr.number} ${pr.title}`,
    subtitle: ["PR", prStateLabel(pr), pr.head_branch].filter(Boolean).join(" · "),
    kind: "pr",
    group: PULL_REQUESTS_GROUP,
    workspaceId: workspace.id,
    deviceId: scope.device.id,
    chip: deviceChip(scope.device, front),
    number: pr.number,
    url: pr.url,
    workspace,
    pr,
    status: PR_STATE[pr.badge],
    ci: pr.checks ? CI_STATUS[pr.checks] : undefined,
  };
}

export function issueEntry(scope: SearchDevice, workspace: Workspace, task: Task, front: string): SearchEntry {
  return {
    id: `issue:${task.key}`,
    title: `${task.id ? `${task.id} ` : ""}${task.title}`,
    subtitle: `Issue · ${workspace.label}`,
    kind: "issue",
    group: ISSUES_GROUP,
    workspaceId: workspace.id,
    deviceId: scope.device.id,
    chip: deviceChip(scope.device, front),
    number: taskNumber(task),
    taskKey: task.key,
    url: task.url,
    workspace,
    task,
    status: task.open ? { tone: "open", label: "Open" } : { tone: "muted", label: "Closed" },
  };
}

/** The number a task shows: `#N` and `owner/repo#N` end in it, a local issue's `L-N` too. */
function taskNumber(task: Task): number | undefined {
  const match = /(\d+)$/.exec(task.id ?? "");
  return match ? Number(match[1]) : undefined;
}

/** A GitHub search result as a row: opened on GitHub, whichever project it came from. */
export function githubEntry(result: GithubSearchResult): SearchEntry {
  const pr = result.kind === "pr";
  const state = result.state.toLowerCase();
  return {
    id: `github:${result.kind}:${result.repository}#${result.number}`,
    title: `#${result.number} ${result.title}`,
    subtitle: [pr ? "PR" : "Issue", result.repository, state === "open" ? "Open" : state === "merged" ? "Merged" : "Closed"].join(" · "),
    kind: pr ? "pr" : "issue",
    group: GITHUB_GROUP,
    number: result.number,
    url: result.url,
    repository: result.repository,
    external: true,
    status: state === "open" ? { tone: "open", label: result.is_draft ? "Draft" : "Open" } : { tone: "muted", label: state === "merged" ? "Merged" : "Closed" },
  };
}

/**
 * The results the core searched, without the ones ⌘K already holds: a result
 * whose GitHub address a held issue or pull request has is that row, so it
 * shows once (PRD B17).
 */
export function githubEntries(results: readonly GithubSearchResult[], held: readonly SearchEntry[]): SearchEntry[] {
  const known = new Set(held.map((entry) => entry.url).filter((url): url is string => Boolean(url)));
  return results.filter((result) => !known.has(result.url)).map(githubEntry);
}

/**
 * The snapshot rows ⌘K searches: every device's agents, projects and
 * checkouts and the devices themselves (PRD home-device-rail D-16, B40), so a
 * pick on another device moves rail, sidebar and center there, and this Mac's
 * issues and pull requests the core already holds (PRD cmdk-navigation D-12,
 * D-14). A row not on the device in front carries that device's chip.
 */
export function searchEntries(rest: SnapshotRest | null): SearchEntry[] {
  if (!rest) return [];
  const entries: SearchEntry[] = [START_AGENT_ENTRY];
  const front = frontDeviceId(rest);
  const devices = searchDevices(rest);
  for (const scope of devices) {
    const places = checkoutPlaces(scope.allWorkspaces);
    for (const agent of scope.agents) entries.push(agentEntry(scope, agent, places.get(agent.pane_id) ?? null, front));
    for (const workspace of scope.workspaces) {
      entries.push(projectEntry(scope, workspace, front));
      for (const checkout of workspace.checkouts) entries.push(checkoutEntry(scope, workspace, checkout, front));
      if (!scope.local) continue;
      const seen = new Set<number>();
      for (const pr of [...(workspace.pull_requests ?? []), ...workspace.checkouts.flatMap((checkout) => (checkout.pull_request ? [checkout.pull_request] : []))]) {
        if (seen.has(pr.number)) continue;
        seen.add(pr.number);
        entries.push(pullRequestEntry(scope, workspace, pr, front));
      }
      for (const task of workspace.tasks?.tasks ?? []) entries.push(issueEntry(scope, workspace, task, front));
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

/** The digits of `273` or `#273`, which name an issue or a pull request, else null. */
export function numberQuery(query: string): number | null {
  const match = /^#?(\d{1,9})$/.exec(query.trim());
  return match ? Number(match[1]) : null;
}

/**
 * The entries matching `query`, best first. A number (`273`, `#273`) puts the
 * issues and then the pull requests numbered exactly that ahead of every title
 * match, each as its own row (PRD B12); the other rows must hold the digits.
 */
export function filterEntries(entries: SearchEntry[], query: string, limit = 80): SearchEntry[] {
  const needle = query.trim().toLowerCase();
  if (!needle) return entries.slice(0, limit);
  const number = numberQuery(needle);
  const exact = number === null ? [] : [...entries.filter((entry) => entry.kind === "issue" && entry.number === number), ...entries.filter((entry) => entry.kind === "pr" && entry.number === number)];
  const taken = new Set(exact);
  const digits = number === null ? null : String(number);
  const scored = entries
    .filter((entry) => !taken.has(entry))
    .map((entry) => {
      const text = `${entry.title} ${entry.subtitle}`.toLowerCase();
      return { entry, score: digits !== null && !text.includes(digits) ? null : fuzzyScore(text, needle) };
    })
    .filter((row): row is { entry: SearchEntry; score: number } => row.score !== null);
  scored.sort((left, right) => right.score - left.score || left.entry.title.localeCompare(right.entry.title));
  return [...exact, ...scored.map((row) => row.entry)].slice(0, limit);
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
