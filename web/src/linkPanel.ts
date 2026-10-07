// What the sessions section of a PR or Issue panel shows (PRD link-graph
// B6-B22), as pure rules over the core's link record and the live snapshot.
// The record says which sessions worked on what; whether one is live, whether
// its worktree is still here and whether its device answers are the current
// snapshot's to say (B5), so they are joined here, never stored.

import type { MessageKey } from "./i18n/catalogs";
import type { AgentRow, Checkout, Device, LinkedSession, Workspace } from "./snapshot";

/** A line's live pane: working, asking the operator, or open and quiet. */
export type LiveState = { kind: "working" | "question" | "idle"; agent: AgentRow };

export type SessionLine = { line: LinkedSession; live: LiveState | null };

function liveState(agent: AgentRow): LiveState {
  return { kind: agent.state.link, agent };
}

const LIVE_ORDER: Record<LiveState["kind"], number> = { question: 0, working: 1, idle: 2 };

/**
 * The lines in the order the section draws them (B6, B18): asking and working
 * sessions first, then every other line newest first as the record keeps them.
 * A line is live when any of its joined ids is a pane's session now.
 */
export function sessionLines(sessions: readonly LinkedSession[], agents: readonly AgentRow[]): SessionLine[] {
  const bySession = new Map<string, AgentRow>();
  for (const agent of agents) if (agent.session_id) bySession.set(agent.session_id, agent);
  const lines = sessions.map((line) => {
    const agent = [...line.ids, line.id].map((id) => bySession.get(id)).find(Boolean);
    return { line, live: agent ? liveState(agent) : null };
  });
  const active = lines.filter((entry) => entry.live && entry.live.kind !== "idle").sort((a, b) => LIVE_ORDER[a.live!.kind] - LIVE_ORDER[b.live!.kind]);
  return [...active, ...lines.filter((entry) => !entry.live || entry.live.kind === "idle")];
}

/** Six lines or more fold to the newest five (B16). */
export const FOLD_AT = 6;
export const FOLD_SHOWN = 5;

/** The lines drawn and how many the `N earlier sessions` fold holds; zero when there is no fold. */
export function foldLines<T>(lines: readonly T[], unfolded: boolean): { shown: readonly T[]; earlier: number } {
  if (lines.length < FOLD_AT) return { shown: lines, earlier: 0 };
  return { shown: unfolded ? lines : lines.slice(0, FOLD_SHOWN), earlier: lines.length - FOLD_SHOWN };
}

/** Why a button is off, as its message; null when it works. */
export type Blocked = { key: "links.why.worktree" | "links.why.opencode" | "links.why.file" } | { key: "links.why.deviceOffline" | "links.why.deviceView"; device: string } | null;

/** A device's name for its chip and its tooltips, or its id when the snapshot no longer lists it. */
export function deviceLabel(devices: readonly Device[] | undefined, id: string): string {
  return devices?.find((device) => device.id === id)?.label ?? id;
}

function connected(devices: readonly Device[] | undefined, id: string, node: string): boolean {
  return id === node || devices?.find((device) => device.id === id)?.state === "ready";
}

/** `View conversation` (B11, B15, B21): another device's conversation and a gone file cannot be opened; `node` is the core's own. */
export function viewBlock(line: LinkedSession, devices: readonly Device[] | undefined, node: string): Blocked {
  if (line.device_id !== node) return { key: "links.why.deviceView", device: deviceLabel(devices, line.device_id) };
  if (line.file === "missing") return { key: "links.why.file" };
  return null;
}

/** The agents Hide starts with a resume (D-10, D-42). */
export function resumable(agent: string): agent is "claude" | "codex" {
  return agent === "claude" || agent === "codex";
}

/** `Resume` (B12-B15, B20-B22): the agent, the file, the device and the worktree, in that order. */
export function resumeBlock(line: LinkedSession, checkout: Checkout | null, devices: readonly Device[] | undefined, node: string): Blocked {
  if (!resumable(line.agent)) return { key: "links.why.opencode" };
  if (line.file === "missing") return { key: "links.why.file" };
  if (!connected(devices, line.device_id, node)) return { key: "links.why.deviceOffline", device: deviceLabel(devices, line.device_id) };
  if (!checkout) return { key: "links.why.worktree" };
  return null;
}

function within(path: string, root: string): boolean {
  return path === root || path.startsWith(root.endsWith("/") ? root : `${root}/`);
}

/**
 * Where a line resumes (B12, B21): the checkout on the line's device that
 * holds the folder it worked in, the deepest one, else the one on the pull
 * request's branch; a checkout whose folder is gone is no place.
 */
export function resumeCheckout(line: LinkedSession, workspaces: readonly Workspace[], branch: string | null, node: string): Checkout | null {
  const here = workspaces.filter((workspace) => (workspace.device_id ?? node) === line.device_id).flatMap((workspace) => workspace.checkouts).filter((checkout) => checkout.exists);
  const cwd = line.cwd;
  const holding = cwd ? here.filter((checkout) => within(cwd, checkout.path)).sort((a, b) => b.path.length - a.path.length)[0] : undefined;
  if (holding) return holding;
  return branch ? (here.find((checkout) => checkout.branch === branch) ?? null) : null;
}

/** A path's last name, the way a worktree line names its folder. */
export function folderName(path: string): string {
  return path.replace(/\/+$/, "").split("/").pop() || path;
}

function pad(value: number): string {
  return String(value).padStart(2, "0");
}

function day(at: Date): string {
  return `${at.getMonth() + 1}/${at.getDate()}`;
}

/** `10/6 13:10 - 13:52` (B6), the day again on the end when the line ran past midnight. */
export function spanText(start: number | null, end: number | null): string | null {
  if (start === null) return null;
  const from = new Date(start);
  const head = `${day(from)} ${pad(from.getHours())}:${pad(from.getMinutes())}`;
  if (end === null) return head;
  const to = new Date(end);
  const tail = `${pad(to.getHours())}:${pad(to.getMinutes())}`;
  return day(to) === day(from) && to.getFullYear() === from.getFullYear() ? `${head} - ${tail}` : `${head} - ${day(to)} ${tail}`;
}

/** How a request is compared with a turn: its first words, whitespace folded. */
function words(text: string): string {
  return text.replace(/\s+/g, " ").trim().slice(0, 120);
}

/**
 * The turn `View conversation` scrolls to (B11): the last person's turn that
 * starts with the line's request, the record keeping only its first words.
 */
export function requestTurn<T extends { role: string; text: string }>(turns: readonly T[], request: string | null): T | null {
  if (!request) return null;
  const wanted = words(request);
  if (!wanted) return null;
  for (let index = turns.length - 1; index >= 0; index -= 1) {
    const turn = turns[index]!;
    if (turn.role === "user" && words(turn.text).startsWith(wanted.slice(0, Math.min(wanted.length, 80)))) return turn;
  }
  return null;
}

/** The words for a failure code the core sent (B25); an unknown code gets the general line. */
export function failureKey(code: string): MessageKey {
  if (code === "links_store_full") return "links.failed.full";
  if (code === "links_store_newer") return "links.failed.newer";
  if (code === "links_store_busy") return "links.failed.busy";
  return "links.failed.default";
}

/** Whether two task keys name one issue: the record keeps a GitHub key in lower case. */
export function sameIssue(a: string, b: string): boolean {
  const fold = (key: string) => (key.startsWith("github:") ? key.toLowerCase() : key);
  return fold(a) === fold(b);
}
