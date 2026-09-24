// The pure rules behind the project and checkout menus: which management
// actions a row offers, the purpose field's limits, and what the worktree
// dialogs read out of a task or removal the core reports. The core re-checks
// every one of these; they decide what the screen offers, not what is allowed.

import { supportsRemotePurpose } from "./remote";
import type { Checkout, RemoteStatus, TaskOperation, Workspace, WorktreeRemoval } from "./snapshot";

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
  id: "pin" | "unpin" | "new_worktree" | "remove_project" | "set_purpose" | "delete_worktree";
  label: string;
  /** Why the action is not offered here; the item is drawn disabled with this as its hint. */
  unavailable: string | null;
};

/** The device a receipt names; the daemon's own machine when it names none. */
function receiptDevice(deviceId: string | null | undefined): string {
  return deviceId ?? "local";
}

/** The project row's menu on any device: Pin/Unpin and Remove project for a registered project, and New worktree for a Git project. */
export function projectMenu(workspace: Workspace): MenuItem[] {
  const items: MenuItem[] = [];
  if (workspace.registered) {
    items.push({
      id: workspace.pinned ? "unpin" : "pin",
      label: workspace.pinned ? "Unpin" : "Pin",
      unavailable: null,
    });
  }
  items.push({
    id: "new_worktree",
    label: "New worktree…",
    unavailable: workspace.is_git === false ? "This project is not a Git repository." : null,
  });
  if (workspace.registered) items.push({ id: "remove_project", label: "Remove project…", unavailable: null });
  return items;
}

/** What removing a project's registration does, spelled out before it is confirmed (D-10). */
export function projectRemovalConsequences(workspace: Workspace): string[] {
  const panes = workspace.removal?.pane_count ?? 0;
  const running = workspace.removal?.running_agent_count ?? 0;
  const lines: string[] = [];
  if (panes > 0) {
    const stopping = running > 0 ? `, stopping ${running === 1 ? "1 running agent" : `${running} running agents`}` : "";
    lines.push(`${panes === 1 ? "1 pane in this project closes" : `${panes} panes in this project close`} first${stopping}.`);
  }
  lines.push("Only the registration is removed: the folder, its repository and its worktrees stay on disk.");
  return lines;
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

/** The checkout row's menu: purpose for any checkout its host can store it for, deletion for a linked worktree on any device. */
export function checkoutMenu(checkout: Checkout, purposeProblem: string | null = null): MenuItem[] {
  const items: MenuItem[] = [{ id: "set_purpose", label: "Set purpose…", unavailable: purposeProblem }];
  if (checkout.is_worktree) {
    const gate = checkout.worktree?.deletion_gate;
    items.push({
      id: "delete_worktree",
      label: "Delete worktree…",
      unavailable: !checkout.worktree ? "The worktree row has not been read yet." : (gate?.blocked_reason ?? null),
    });
  }
  return items;
}

/** The deletion consequences the confirmation spells out, from the core's row and gate. */
export function deletionConsequences(checkout: Checkout, paneCount: number): string[] {
  const row = checkout.worktree;
  const lines: string[] = [];
  lines.push(`The folder ${checkout.path} is removed from disk. This cannot be undone.`);
  if (paneCount > 0) lines.push(`${paneCount === 1 ? "1 pane in this worktree closes" : `${paneCount} panes in this worktree close`} first, stopping whatever runs there.`);
  if (row && row.running_agent_count > 0) lines.push(`${row.running_agent_count} running agent${row.running_agent_count === 1 ? "" : "s"} will be stopped.`);
  for (const warning of row?.deletion_gate.warnings ?? []) lines.push(warning);
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
