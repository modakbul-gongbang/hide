import { createHash } from "node:crypto";
import os from "node:os";
import { machineId } from "./identity";
import { HcoordError, type Participant } from "./model";
import { herdrRoute, isLocalMachine, remoteCall } from "./remote";

export const LINEAGE_SOURCE = "hcoord";
export const PARENT_PANE_TOKEN = "parent_pane";
export const PARENT_MACHINE_TOKEN = "parent_machine";
export const CHILD_SESSION_TOKEN = "child_session";
export const PARENT_SESSION_TOKEN = "parent_session";
const LINEAGE_TOKENS = [PARENT_PANE_TOKEN, PARENT_MACHINE_TOKEN, CHILD_SESSION_TOKEN, PARENT_SESSION_TOKEN];

/**
 * How a session is written into a token: the lowercase hex SHA-256 of the
 * value Herdr reports in `agent_session.value`. Herdr cuts a token value at
 * 80 characters and a session can be a path, so a digest keeps two sessions
 * apart whatever they look like. Hide's `wire::session_digest` is the same
 * function and compares it with the session each pane reports.
 */
export const sessionDigest = (session: string): string => createHash("sha256").update(session, "utf8").digest("hex");

export interface LineageWrite {
  status: "written" | "failed";
  parentPane: string;
  parentMachine: string | null;
  message: string;
  nextAction: string | null;
}

const sameMachine = (left: string, right: string): boolean => left === right || (isLocalMachine(left) && isLocalMachine(right));

function parentMachineId(parent: Participant, child: Participant): string | null {
  if (sameMachine(parent.machine, child.machine)) return null;
  if (isLocalMachine(parent.machine)) return machineId();
  const hello = remoteCall(parent.machine, ["hello", "--hq", os.hostname()]);
  const value = hello["machineId"];
  if (typeof value !== "string" || value === "") throw new HcoordError("machine_id_unavailable", `hcoord on ${parent.machine} did not report its machine id`);
  return value;
}

/** Write the complete desired lineage patch to the child pane. Repeats converge. */
export function writeLineage(parent: Participant, child: Participant): LineageWrite {
  if (parent.pane === null) throw new HcoordError("lineage_parent_unavailable", `parent ${parent.id} has no Herdr pane`);
  if (child.pane === null) throw new HcoordError("lineage_child_unavailable", `child ${child.id} has no Herdr pane`);
  const parentMachine = parentMachineId(parent, child);
  const args = ["pane", "report-metadata", child.pane, "--source", LINEAGE_SOURCE, "--token", `${PARENT_PANE_TOKEN}=${parent.pane}`,
    "--token", `${CHILD_SESSION_TOKEN}=${sessionDigest(child.session)}`, "--token", `${PARENT_SESSION_TOKEN}=${sessionDigest(parent.session)}`,
    ...(parentMachine === null ? ["--clear-token", PARENT_MACHINE_TOKEN] : ["--token", `${PARENT_MACHINE_TOKEN}=${parentMachine}`])];
  const result = herdrRoute(child.machine, child.hostScope).run!(args, undefined, 20_000);
  if (result.status !== 0) {
    const detail = (result.stderr || result.stdout).trim().slice(0, 300) || "Herdr returned no diagnostic";
    return { status: "failed", parentPane: parent.pane, parentMachine, message: `agent is running but Herdr did not record its lineage tokens: ${detail}`, nextAction: "retry the same register or spawn intent; hcoord will write only the missing tokens" };
  }
  return { status: "written", parentPane: parent.pane, parentMachine, message: "Herdr recorded the child lineage tokens", nextAction: null };
}

/**
 * Take the lineage tokens off a pane whose child no longer is the execution
 * they were written for. Tokens are written under this source, so clearing
 * them here leaves every other writer's tokens alone.
 */
export function clearLineage(child: Participant): { status: "cleared" | "failed"; message: string } {
  if (child.pane === null) throw new HcoordError("lineage_child_unavailable", `child ${child.id} has no Herdr pane`);
  const args = ["pane", "report-metadata", child.pane, "--source", LINEAGE_SOURCE, ...LINEAGE_TOKENS.flatMap((token) => ["--clear-token", token])];
  const result = herdrRoute(child.machine, child.hostScope).run!(args, undefined, 20_000);
  if (result.status !== 0) return { status: "failed", message: (result.stderr || result.stdout).trim().slice(0, 300) || "Herdr returned no diagnostic" };
  return { status: "cleared", message: "Herdr cleared the lineage tokens" };
}

/** A pane as one route's Herdr reports it: the execution it hosts and the lineage tokens it carries. */
export interface PaneLineage { session: string | null; instance: string | null; parentPane: string | null; parentMachine: string | null; childSession: string | null; parentSession: string | null }

/** Whether a pane carries any lineage token, so a cleared pane is not cleared again. */
export function carriesLineage(pane: PaneLineage): boolean {
  return pane.parentPane !== null || pane.parentMachine !== null || pane.childSession !== null || pane.parentSession !== null;
}

/** Whether a child's pane already carries the lineage its ledger parent implies. */
export function lineageCurrent(parent: Participant, child: Participant, pane: PaneLineage): boolean {
  if (pane.parentPane !== parent.pane || pane.childSession !== sessionDigest(child.session) || pane.parentSession !== sessionDigest(parent.session)) return false;
  return sameMachine(parent.machine, child.machine) ? pane.parentMachine === null : pane.parentMachine !== null;
}

/**
 * Whether the pane hosts a session other than the one the child was
 * registered with. A pane Herdr reports without a session proves nothing
 * either way, so it is not different: only a session that is there and is not
 * the recorded one ends a relationship.
 */
export function hostsAnotherSession(child: Participant, pane: PaneLineage): boolean {
  return pane.session !== null && pane.session !== child.session;
}

/**
 * Every pane on one Herdr route in one `pane list`, or null when that server
 * does not answer. A restarted Herdr server drops every pane token (Socket
 * API: token metadata is not restored after a server restart), so this is
 * read again on a timer rather than trusted from the last write.
 */
export function readRouteLineage(machine: string, hostScope: string, timeoutMs: number): Map<string, PaneLineage> | null {
  const result = herdrRoute(machine, hostScope).run!(["pane", "list"], undefined, timeoutMs);
  if (result.status !== 0) return null;
  let parsed: unknown;
  try { parsed = JSON.parse(result.stdout); } catch { return null; }
  const panes = (parsed as { result?: { panes?: unknown } }).result?.panes;
  if (!Array.isArray(panes)) return null;
  const text = (value: unknown): string | null => typeof value === "string" ? value : null;
  const lineage = new Map<string, PaneLineage>();
  for (const candidate of panes) {
    if (candidate === null || typeof candidate !== "object") continue;
    const pane = candidate as Record<string, unknown>;
    const id = text(pane["pane_id"]);
    if (id === null) continue;
    const session = pane["agent_session"] !== null && typeof pane["agent_session"] === "object" ? text((pane["agent_session"] as Record<string, unknown>)["value"]) : null;
    const tokens = pane["tokens"] !== null && typeof pane["tokens"] === "object" ? pane["tokens"] as Record<string, unknown> : {};
    lineage.set(id, { session, instance: text(pane["terminal_id"]), parentPane: text(tokens[PARENT_PANE_TOKEN]), parentMachine: text(tokens[PARENT_MACHINE_TOKEN]), childSession: text(tokens[CHILD_SESSION_TOKEN]), parentSession: text(tokens[PARENT_SESSION_TOKEN]) });
  }
  return lineage;
}
