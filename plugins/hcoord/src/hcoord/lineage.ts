import os from "node:os";
import { machineId } from "./identity";
import { HcoordError, type Participant } from "./model";
import { herdrRoute, isLocalMachine, remoteCall } from "./remote";

export const LINEAGE_SOURCE = "hcoord";
export const PARENT_PANE_TOKEN = "parent_pane";
export const PARENT_MACHINE_TOKEN = "parent_machine";

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
    ...(parentMachine === null ? ["--clear-token", PARENT_MACHINE_TOKEN] : ["--token", `${PARENT_MACHINE_TOKEN}=${parentMachine}`])];
  const result = herdrRoute(child.machine, child.hostScope).run!(args, undefined, 20_000);
  if (result.status !== 0) {
    const detail = (result.stderr || result.stdout).trim().slice(0, 300) || "Herdr returned no diagnostic";
    return { status: "failed", parentPane: parent.pane, parentMachine, message: `agent is running but Herdr did not record its lineage tokens: ${detail}`, nextAction: "retry the same register or spawn intent; hcoord will write only the missing tokens" };
  }
  return { status: "written", parentPane: parent.pane, parentMachine, message: "Herdr recorded the child lineage tokens", nextAction: null };
}

/** A pane as one route's Herdr reports it: the execution it hosts and the lineage tokens it carries. */
export interface PaneLineage { session: string | null; instance: string | null; parentPane: string | null; parentMachine: string | null }

/** Whether a child's pane already carries the lineage its ledger parent implies. */
export function lineageCurrent(parent: Participant, child: Participant, pane: PaneLineage): boolean {
  if (pane.parentPane !== parent.pane) return false;
  return sameMachine(parent.machine, child.machine) ? pane.parentMachine === null : pane.parentMachine !== null;
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
    lineage.set(id, { session, instance: text(pane["terminal_id"]), parentPane: text(tokens[PARENT_PANE_TOKEN]), parentMachine: text(tokens[PARENT_MACHINE_TOKEN]) });
  }
  return lineage;
}
