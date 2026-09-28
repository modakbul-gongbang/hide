// The pure rules behind the sidebar's row menus: which actions a project,
// checkout or agent row offers and why one is disabled, the purpose field's
// limits, and what the worktree dialogs read out of a task or removal the
// core reports. The core re-checks every one of these; they decide what the
// screen offers, not what is allowed.

import { shownPullRequest } from "./projects";
import { supportsRemotePurpose } from "./remote";
import type { AgentRow, Checkout, RemoteStatus, TaskOperation, Workspace, WorktreeRemoval } from "./snapshot";

/** The native purpose field's limits (`PurposeInputPresentation`). */
export const PURPOSE_RECOMMENDED = 40;
export const PURPOSE_HARD_LIMIT = 80;

/** Unicode scalars, which is what the core counts (`chars()`), not UTF-16 units. */
export function scalarCount(text: string): number {
  return [...text].length;
}

/** One line, at most the hard limit of scalars; the field shows what will be saved. */
export function normalizePurpose(text: string): string {
  return [...text.replace(/[\r\n]/g, " ")].slice(0, PURPOSE_HARD_LIMIT).join("");
}

export function purposeCountLabel(text: string): string {
  return `${scalarCount(text)} / ${PURPOSE_RECOMMENDED}`;
}

/**
 * Where a saved purpose is kept (PRD S5.5 B30). A device's purpose is its
 * Herdr's workspace metadata only; on this machine a branch also keeps it as
 * its Git description, which outlives the Herdr workspace.
 */
export function purposeScope(deviceLabel: string | null, branch: string | null): string {
  if (deviceLabel) return `Kept in Herdr's workspace metadata on ${deviceLabel}; its Git config is not changed.`;
  if (branch) return `Kept in Herdr's workspace metadata and as the Git description of ${branch} on this machine.`;
  return "Kept in Herdr's workspace metadata on this machine.";
}

export function purposeIsLong(text: string): boolean {
  return scalarCount(text) > PURPOSE_RECOMMENDED;
}

/**
 * A branch name the dialog can send. Git's own `check-ref-format` runs on the
 * daemon before anything is created; this catches the obvious cases while
 * the operator types, so the field says why before a round trip.
 */
export function branchProblem(name: string): string | null {
  const branch = name.trim();
  if (!branch) return "Enter a branch name.";
  if (branch.startsWith("-")) return "A branch name cannot start with -.";
  const control = [...branch].some((character) => character.charCodeAt(0) < 0x20 || character.charCodeAt(0) === 0x7f);
  if (control || /[\s~^:?*[\\]|\.\.|@\{|\/\/|\.lock$|^\/|\/$|\.$/.test(branch)) {
    return "Git does not accept this branch name.";
  }
  return null;
}

export type MenuItem = {
  id:
    | "open_overview"
    | "new_worktree"
    | "new_tab_primary"
    | "reveal_finder"
    | "copy_path"
    | "pin"
    | "unpin"
    | "remove_project"
    | "open_checkout"
    | "new_tab_here"
    | "open_pull_request"
    | "set_purpose"
    | "set_primary"
    | "copy_branch"
    | "delete_worktree";
  label: string;
  /** Why the action is not offered here; the item is drawn disabled with this as its hint. */
  unavailable: string | null;
  /** Drawn after a separator: where a menu turns to another group of actions. */
  separated?: boolean;
  /** The chord that does the same, drawn at the item's end. */
  shortcut?: string;
  /** An action that removes something from disk, drawn in the destructive color. */
  destructive?: boolean;
};

/** What a row's menu reads from where the shell runs, so the rules stay pure. */
export type MenuHost = {
  /** The desktop app can show a folder in Finder; a browser tab cannot, so it offers no such item. */
  finder: boolean;
  /** The new-tab chord on this host, or "" where it has none. */
  newTabChord: string;
};

/** The device a receipt names; the daemon's own machine when it names none. */
function receiptDevice(deviceId: string | null | undefined): string {
  return deviceId ?? "local";
}

const ON_ANOTHER_DEVICE = "Not available for a checkout on another device.";
const FINDER_HERE_ONLY = "Only for folders on this Mac.";

function onDevice(workspace: Workspace): boolean {
  return workspace.device_id !== "local";
}

/**
 * The checkout "New tab in main" opens a tab in: the one the home glyph
 * marks, or a folder project's own checkout, which has no primary.
 */
export function primaryCheckout(workspace: Workspace): Checkout | null {
  return workspace.checkouts.find((checkout) => checkout.is_primary === true) ?? (workspace.is_git ? null : (workspace.checkouts[0] ?? null));
}

function revealItem(workspace: Workspace, host: MenuHost, separated: boolean): MenuItem[] {
  if (!host.finder) return [];
  return [{ id: "reveal_finder", label: "Reveal in Finder", unavailable: onDevice(workspace) ? FINDER_HERE_ONLY : null, ...(separated ? { separated } : {}) }];
}

/**
 * The project row's menu on any device, registered or not (PRD
 * sidebar-context-menus D-02, D-14): open the project, start work in it, find
 * its folder, then keep or drop it. A row Herdr shows without a registration
 * offers the same Pin and Remove: Pin registers it with its device and root
 * and pins it in the same event, and Remove closes its panes, after which the
 * row leaves with Herdr's workspace because there is no registration to keep it.
 */
export function projectMenu(workspace: Workspace, host: MenuHost): MenuItem[] {
  const primary = primaryCheckout(workspace);
  const reveal = revealItem(workspace, host, true);
  return [
    { id: "open_overview", label: "Open Overview", unavailable: null },
    { id: "new_worktree", label: "New worktree…", unavailable: workspace.is_git === false ? "This project is not a Git repository." : null },
    {
      id: "new_tab_primary",
      label: "New tab in main",
      unavailable: !primary ? "This project has no default checkout to open a tab in." : primary.exists ? null : "The default checkout's folder is missing.",
      shortcut: host.newTabChord,
    },
    ...reveal,
    { id: "copy_path", label: "Copy path", unavailable: null, ...(reveal.length ? {} : { separated: true }) },
    { id: workspace.pinned ? "unpin" : "pin", label: workspace.pinned ? "Unpin" : "Pin", unavailable: null, separated: true },
    { id: "remove_project", label: "Remove project…", unavailable: null },
  ];
}

/**
 * What removing a project does, spelled out before it is confirmed (D-10): a
 * registered project loses its registration, and a row Herdr shows without
 * one loses its panes, which takes the row with them (PRD sidebar-context-menus D-14).
 */
export function projectRemovalConsequences(workspace: Workspace): string[] {
  const panes = workspace.removal?.pane_count ?? 0;
  const running = workspace.removal?.running_agent_count ?? 0;
  const lines: string[] = [];
  if (panes > 0) {
    const stopping = running > 0 ? `, stopping ${running === 1 ? "1 running agent" : `${running} running agents`}` : "";
    lines.push(`${panes === 1 ? "1 pane in this project closes" : `${panes} panes in this project close`} first${stopping}.`);
  }
  lines.push(
    workspace.registered
      ? "Only the registration is removed: the folder, its repository and its worktrees stay on disk."
      : "Hide keeps no registration for this project, so its row leaves once Herdr closes the workspace. The folder, its repository and its worktrees stay on disk.",
  );
  return lines;
}

/** The checkout items a plain folder's row adds after its project's; its row routes these to the checkout. */
export const FOLDER_CHECKOUT_ITEMS: ReadonlySet<MenuItem["id"]> = new Set<MenuItem["id"]>(["open_checkout", "open_pull_request", "set_purpose"]);

/**
 * A plain folder's one row (`folderCheckout`): the project's items, then its
 * checkout's that the project's do not already cover. The folder is the
 * checkout, so its new tab, path and Finder items are the project's, and it
 * has no other checkout to make the default.
 */
export function folderMenu(workspace: Workspace, checkout: Checkout, host: MenuHost, purposeProblem: string | null = null): MenuItem[] {
  const [first, ...rest] = checkoutMenu(workspace, checkout, host, purposeProblem).filter((item) => FOLDER_CHECKOUT_ITEMS.has(item.id));
  return first ? [...projectMenu(workspace, host), { ...first, separated: true }, ...rest.map((item) => ({ ...item, separated: false }))] : projectMenu(workspace, host);
}

/**
 * Why a remote checkout's purpose cannot be written: the core stores it in
 * that host's Herdr, which takes it from 0.9.1 (the native `purposeUnavailableReason`).
 */
export function remotePurposeProblem(workspace: Workspace, remote: RemoteStatus[] | undefined): string | null {
  const targetId = workspace.remote_target_id;
  if (!targetId) return null;
  const version = remote?.find((row) => row.target_id === targetId)?.herdr_version ?? null;
  if (supportsRemotePurpose(version)) return null;
  return `Set purpose requires Herdr 0.9.1 or newer on the remote device${version ? `; ${version} is installed` : "; its version is unavailable"}.`;
}

/**
 * Why a checkout cannot become its project's default (`set_primary_checkout`
 * refuses the same cases): the choice is stored on this machine's
 * registration of a Git project, for a checkout whose folder exists.
 */
function primaryProblem(workspace: Workspace, checkout: Checkout): string | null {
  if (onDevice(workspace)) return ON_ANOTHER_DEVICE;
  if (workspace.is_git === false) return "A plain folder has only this checkout.";
  if (checkout.is_primary) return "Already the default checkout.";
  if (!workspace.registered) return "Pin the project first to keep a default checkout.";
  if (!checkout.exists) return "The folder is missing.";
  return null;
}

/**
 * The checkout row's menu (PRD sidebar-context-menus D-03, D-07, D-09): open
 * it or a tab in it, its pull request while GitHub knows one (PRD
 * checkout-pr-glyph-card D-07), then what describes it, then deleting a
 * linked worktree. Choices stored on this machine and Finder are this Mac's
 * only, so a device's checkout lists them disabled.
 */
export function checkoutMenu(workspace: Workspace, checkout: Checkout, host: MenuHost, purposeProblem: string | null = null): MenuItem[] {
  const pr = shownPullRequest(checkout);
  const items: MenuItem[] = [
    { id: "open_checkout", label: "Open", unavailable: null },
    { id: "new_tab_here", label: "New tab here", unavailable: checkout.exists ? null : "The folder is missing.", shortcut: host.newTabChord },
  ];
  if (pr) items.push({ id: "open_pull_request", label: `Open pull request #${pr.number}`, unavailable: null });
  items.push(
    { id: "set_purpose", label: "Set purpose…", unavailable: purposeProblem, separated: true },
    { id: "set_primary", label: "Set as default checkout", unavailable: primaryProblem(workspace, checkout) },
    { id: "copy_branch", label: "Copy branch name", unavailable: checkout.branch ? null : "Detached HEAD has no branch name." },
    { id: "copy_path", label: "Copy path", unavailable: null },
    ...revealItem(workspace, host, false),
  );
  // Never disabled: what deleting would lose is the dialog's to say, with
  // the choice beside it, on any device and before Git has been read.
  if (checkout.is_worktree) items.push({ id: "delete_worktree", label: "Delete worktree…", unavailable: null, separated: true, destructive: true });
  return items;
}

export type AgentMenuItem = {
  id: "show_agent" | "copy_title" | "copy_session_id" | "close_tab";
  label: string;
  unavailable: string | null;
  separated?: boolean;
  shortcut?: string;
};

/**
 * An agent row's menu (PRD sidebar-context-menus D-04, D-06): go to its pane,
 * copy what names it, close the tab it is in through the tab close
 * confirmation. `showChord` is the ⌥n that selects the same row, or "" when
 * it holds no number on this host. Herdr 0.9.1 can neither mark a pane seen
 * nor stop an agent, so neither is offered (D-05).
 */
export function agentMenu(agent: AgentRow, showChord: string): AgentMenuItem[] {
  return [
    { id: "show_agent", label: "Show", unavailable: null, shortcut: showChord },
    { id: "copy_title", label: "Copy title", unavailable: null, separated: true },
    { id: "copy_session_id", label: "Copy session id", unavailable: agent.session_id ? null : "Herdr has reported no session id for this agent." },
    { id: "close_tab", label: "Close tab…", unavailable: null, separated: true },
  ];
}

/** The agents deleting a checkout stops: every pane an agent runs in, by the name and state the sidebar shows. */
export function stoppedAgents(checkout: Checkout): string[] {
  return checkout.tabs.flatMap((tab) => tab.panes).flatMap((pane) => (pane.identity_label ? [`${pane.identity_label} (${pane.status_label})`] : []));
}

/** The deletion consequences the confirmation spells out, from the checkout's panes and the core's gate. */
export function deletionConsequences(checkout: Checkout, paneCount: number): string[] {
  const lines: string[] = [];
  lines.push(`The folder ${checkout.path} is removed from disk. This cannot be undone.`);
  if (paneCount > 0) lines.push(`${paneCount === 1 ? "1 pane in this worktree closes" : `${paneCount} panes in this worktree close`} first, stopping whatever runs there.`);
  const agents = stoppedAgents(checkout);
  if (agents.length > 0) lines.push(`Stops ${agents.length === 1 ? "1 agent" : `${agents.length} agents`}: ${agents.join(", ")}.`);
  for (const warning of checkout.worktree?.deletion_gate.warnings ?? []) lines.push(warning);
  return lines;
}

/** The newest task the operator asked for on this page, once it names the request's own target. */
export function taskFor(
  operation: TaskOperation | null | undefined,
  request: { kind: string; afterId: number; deviceId?: string; repositoryRoot?: string; branch?: string; path?: string } | null,
): TaskOperation | null {
  if (!operation || !request) return null;
  if (operation.kind !== request.kind || operation.id <= request.afterId) return null;
  if (request.deviceId !== undefined && receiptDevice(operation.device_id) !== request.deviceId) return null;
  if (request.repositoryRoot !== undefined && operation.repository_root !== request.repositoryRoot) return null;
  if (request.branch !== undefined && operation.branch !== request.branch) return null;
  if (request.path !== undefined && operation.path !== request.path) return null;
  return operation;
}

/** The removal this page asked for, by the device and checkout it named; another checkout's removal is not its answer. */
export function removalFor(removal: WorktreeRemoval | null | undefined, deviceId: string, checkoutPath: string, afterId: number): WorktreeRemoval | null {
  if (!removal || receiptDevice(removal.device_id) !== deviceId || removal.checkout_path !== checkoutPath || removal.id <= afterId) return null;
  return removal;
}
