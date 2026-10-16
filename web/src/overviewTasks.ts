// The Overview's Tasks tab as data (PRD overview-device-tasks D-05, D-06,
// B6-B11): one section per project of the device, its task cards in the
// core's order, the resting and cleanup ones folded, the dashed entries other
// devices left. The core decides every card (`agent_scope.tasks`); this only
// filters, orders the sections and words what a card already says.

import type { IncomingTaskScope, TaskCardScope } from "./agentScope";
import { frontDeviceId } from "./devices";
import type { BoardProject } from "./projectBoard";
import type { AgentRow, AgentStatusCode, PrState, RaiseVerb, SnapshotRest, Workspace } from "./snapshot";
import type { TaskFilter } from "./ui";

/**
 * The device the Overview shows (PRD overview-device-tasks D-02): the one its
 * own switch picked, else the one a device Home row named, else the rail's
 * front device. A device removed since it was picked leaves the front one.
 */
export function overviewDeviceOf(rest: SnapshotRest | null, picked: string | null | undefined, named: string | null | undefined): string {
  const listed = (id: string | null | undefined): id is string => Boolean(id) && (rest?.navigator?.devices?.some((device) => device.id === id) ?? false);
  return listed(picked) ? picked : listed(named) ? named : frontDeviceId(rest);
}

export type TaskSection = {
  project: Workspace;
  /** Cards drawn open: Needs You, blocked, working, waiting, done. */
  open: TaskCardScope[];
  /** `쉬는 일감 N` (B7). */
  resting: TaskCardScope[];
  /** `정리할 것 N`: merged with a worktree still on disk. */
  cleanup: TaskCardScope[];
  incoming: IncomingTaskScope[];
  /** Every card of the project, the filter aside; dashed entries are not counted (D-18). */
  count: number;
  needsYou: number;
  working: number;
  /** The newest activity of the project's tasks, for the section order. */
  recency: string;
};

/** Every agent row the snapshot holds, this machine's and each device's, by pane. */
export function everyAgent(rest: SnapshotRest | null, local: readonly AgentRow[]): Map<string, AgentRow> {
  const rows = new Map<string, AgentRow>();
  for (const status of rest?.status?.remote ?? []) for (const agent of status.session?.agents ?? []) rows.set(agent.pane_id, agent);
  for (const agent of local) rows.set(agent.pane_id, agent);
  return rows;
}

/** Whether a backlog key names a folder memo (`local:<path>#N`) rather than a GitHub issue. */
export function isMemoKey(key: string): boolean {
  return key.startsWith("local:");
}

/**
 * What an issue or memo is called on a chip (`#N`, `L-N`): the id its source
 * shows, else read from its key (`local:<path>#N`, `github:owner/repo#N`).
 */
export function itemToken(key: string, project: Pick<Workspace, "tasks">): string | null {
  const listed = project.tasks?.tasks.find((task) => task.key === key)?.id;
  if (listed) return listed;
  const number = key.slice(key.lastIndexOf("#") + 1);
  if (!/^\d+$/.test(number)) return null;
  return isMemoKey(key) ? `L-${number}` : key.startsWith("github:") ? `#${number}` : null;
}

/** A task's name: its root session's label (B8). */
export function taskTitle(card: Pick<TaskCardScope, "root">, rows: ReadonlyMap<string, AgentRow>): string {
  return rows.get(card.root)?.identity_label ?? "";
}

/** Whether a card answers the chip (B11): Needs You takes the blocked ones too. */
function chipMatches(card: TaskCardScope, filter: TaskFilter): boolean {
  if (filter === "needs_you") return card.state === "needs_you" || card.state === "blocked";
  if (filter === "working") return card.state === "working";
  return true;
}

/** What the search box finds a card by: its members' titles, its checkouts' branches, its PRs and its issue chips. */
export function cardWords(card: TaskCardScope, project: Workspace, rows: ReadonlyMap<string, AgentRow>): string {
  const words: string[] = [];
  for (const pane of card.members) words.push(rows.get(pane)?.identity_label ?? "");
  for (const id of card.checkouts) {
    const checkout = project.checkouts.find((row) => row.id === id);
    if (!checkout) continue;
    words.push(checkout.branch ?? "", checkout.label);
    const token = checkout.task_key ? itemToken(checkout.task_key, project) : null;
    if (token) words.push(token);
  }
  for (const pull of cardPulls(card, rows)) words.push(`#${pull.number}`);
  return words.join("\n").toLocaleLowerCase();
}

function matches(card: TaskCardScope, project: Workspace, rows: ReadonlyMap<string, AgentRow>, filter: TaskFilter, query: string): boolean {
  if (!chipMatches(card, filter)) return false;
  const needle = query.trim().toLocaleLowerCase();
  return needle === "" || cardWords(card, project, rows).includes(needle);
}

/**
 * The device's sections (B6): a project with a task waiting on the operator
 * first, then the most recently active, then by name. A filter keeps every
 * section's order and drops the sections it empties; `only` is the project
 * chip (B5).
 */
export function taskSections(projects: readonly BoardProject[], rows: ReadonlyMap<string, AgentRow>, filter: TaskFilter, query: string, only: string | null): TaskSection[] {
  const filtering = filter !== "all" || query.trim() !== "";
  const sections: TaskSection[] = [];
  for (const { workspace } of projects) {
    if (only !== null && workspace.id !== only) continue;
    const tasks = workspace.agent_scope.tasks;
    const cards = tasks?.cards ?? [];
    const shown = cards.filter((card) => matches(card, workspace, rows, filter, query));
    const incoming = filtering ? [] : (tasks?.incoming ?? []);
    if (filtering && shown.length === 0) continue;
    sections.push({
      project: workspace,
      open: shown.filter((card) => card.state !== "resting" && card.state !== "cleanup"),
      resting: shown.filter((card) => card.state === "resting"),
      cleanup: shown.filter((card) => card.state === "cleanup"),
      incoming,
      count: cards.length,
      needsYou: tasks?.needs_you ?? 0,
      working: cards.filter((card) => card.state === "working").length,
      recency: tasks?.recency ?? "",
    });
  }
  return sections.sort((a, b) => Number(b.needsYou > 0) - Number(a.needsYou > 0) || b.recency.localeCompare(a.recency) || a.project.label.localeCompare(b.project.label));
}

/** The Tasks tab's numbers (B3): cards, Needs You, and the share of each state for its thin bar. */
export type TaskTally = { total: number; needsYou: number; working: number; waiting: number; resting: number };

export function taskTally(projects: readonly BoardProject[]): TaskTally {
  const tally: TaskTally = { total: 0, needsYou: 0, working: 0, waiting: 0, resting: 0 };
  for (const { workspace } of projects) {
    const tasks = workspace.agent_scope.tasks;
    tally.needsYou += tasks?.needs_you ?? 0;
    for (const card of tasks?.cards ?? []) {
      tally.total += 1;
      if (card.state === "working") tally.working += 1;
      else if (card.state === "waiting" || card.state === "done") tally.waiting += 1;
      else if (card.state === "resting" || card.state === "cleanup") tally.resting += 1;
    }
  }
  return tally;
}

/** One pull request of a task, the worst state any member's mark gives it. */
export type CardPull = { number: number; url: string; state: PrState };

const PR_WORST: readonly PrState[] = ["failed", "pending", "mergeable", "draft", "merged", "closed"];

/** The task's pull requests: every one a member's request names, once, worst state kept (B10). */
export function cardPulls(card: Pick<TaskCardScope, "members">, rows: ReadonlyMap<string, AgentRow>): CardPull[] {
  const pulls = new Map<string, CardPull>();
  for (const pane of card.members) {
    const agent = rows.get(pane);
    const listed = agent?.request?.pull_requests ?? [];
    for (const own of agent?.state.pr?.pulls ?? []) {
      const pull = listed[own.index];
      if (!pull) continue;
      const held = pulls.get(pull.url);
      if (!held || PR_WORST.indexOf(own.state) < PR_WORST.indexOf(held.state)) pulls.set(pull.url, { number: pull.number, url: pull.url, state: own.state });
    }
  }
  return [...pulls.values()];
}

/** The issue and memo chips a task wears (`#N`/`L-N`, B8): its checkouts' issue links, once each. */
export function cardItems(card: Pick<TaskCardScope, "checkouts">, project: Workspace): { key: string; token: string }[] {
  const items: { key: string; token: string }[] = [];
  for (const id of card.checkouts) {
    const key = project.checkouts.find((row) => row.id === id)?.task_key;
    const token = key ? itemToken(key, project) : null;
    if (key && token && !items.some((item) => item.key === key)) items.push({ key, token });
  }
  return items;
}

/** The card a task key is on, for the ways in that name an issue (D-31). */
/** The fold a project's resting cards stand in until the operator opens it (B7). */
export const restFold = (projectId: string) => `rest:${projectId}`;
/** The fold a project's merged cards with a worktree still on disk stand in (B7). */
export const cleanupFold = (projectId: string) => `cleanup:${projectId}`;

/**
 * The open folds with the one `card` stands in, so a card the Overview opens
 * on (B59, a started item's card) is drawn selected rather than folded away.
 */
export function foldsShowing(open: readonly string[], card: Pick<TaskCardScope, "state">, projectId: string): readonly string[] {
  const fold = card.state === "resting" ? restFold(projectId) : card.state === "cleanup" ? cleanupFold(projectId) : null;
  return fold === null || open.includes(fold) ? open : [...open, fold];
}

export function cardOfItem(projects: readonly BoardProject[], key: string): { project: Workspace; card: TaskCardScope } | null {
  for (const { workspace } of projects) {
    for (const card of workspace.agent_scope.tasks?.cards ?? []) {
      if (cardItems(card, workspace).some((item) => item.key === key)) return { project: workspace, card };
    }
  }
  return null;
}

/** The card a pane belongs to, root or member, on any of the projects (B59). */
export function cardOfPane(projects: readonly BoardProject[], pane: string): { project: Workspace; card: TaskCardScope } | null {
  for (const { workspace } of projects) {
    for (const card of workspace.agent_scope.tasks?.cards ?? []) if (card.members.includes(pane)) return { project: workspace, card };
  }
  return null;
}

/**
 * What a card's third line says (B9), as a kind the card words: the reason it
 * waits on the operator or is blocked, that it merged, its open PR's state,
 * that it is working, that it finished unread, or that it rests.
 */
export type CardLine =
  | { kind: "needs_you"; verb: RaiseVerb | null; what: string | null; agent: string }
  | { kind: "blocked"; what: string | null }
  | { kind: "merged"; cleanup: boolean }
  | { kind: "pr"; number: number; state: PrState }
  | { kind: "working"; agents: number }
  | { kind: "done"; status: AgentStatusCode | null }
  | { kind: "resting" };

export function cardLine(card: TaskCardScope, rows: ReadonlyMap<string, AgentRow>): CardLine {
  const root = rows.get(card.root);
  if (card.state === "needs_you") return { kind: "needs_you", verb: root?.state.ask?.verb ?? null, what: root?.state.ask?.what ?? null, agent: root?.agent_kind ?? "" };
  if (card.state === "blocked") return { kind: "blocked", what: root?.request?.line ?? root?.detail ?? null };
  if (card.stage === 4) return { kind: "merged", cleanup: card.state === "cleanup" };
  const open = cardPulls(card, rows).find((pull) => pull.state !== "merged" && pull.state !== "closed");
  if (open) return { kind: "pr", number: open.number, state: open.state };
  if (card.state === "working") return { kind: "working", agents: card.members.filter((pane) => rows.get(pane)?.activity === "working").length };
  if (card.state === "done") return { kind: "done", status: root?.status_code ?? null };
  return { kind: "resting" };
}

/** When the task last changed: its newest member's change, or null when the core has not seen one (design #10). */
export function cardChangedAt(card: Pick<TaskCardScope, "members">, rows: ReadonlyMap<string, AgentRow>): number | null {
  let newest: number | null = null;
  for (const pane of card.members) {
    const at = rows.get(pane)?.changed_at_unix_ms ?? null;
    if (at !== null && (newest === null || at > newest)) newest = at;
  }
  return newest;
}

/** The question a task asks in one line, when its root is asking (B8). */
export function cardQuestion(card: TaskCardScope, rows: ReadonlyMap<string, AgentRow>): string | null {
  if (card.state !== "needs_you") return null;
  const root = rows.get(card.root);
  const text = root?.user_turn?.content?.text ?? root?.state.ask?.what ?? null;
  return text ? text.split(/\r?\n/).find((line) => line.trim() !== "")?.trim() ?? null : null;
}
