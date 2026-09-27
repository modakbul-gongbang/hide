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

export function readLineage(child: Participant): { parentPane: string | null; parentMachine: string | null } {
  if (child.pane === null) throw new HcoordError("lineage_child_unavailable", `child ${child.id} has no Herdr pane`);
  const result = herdrRoute(child.machine, child.hostScope).run!(["pane", "get", child.pane], undefined, 20_000);
  if (result.status !== 0) throw new HcoordError("runtime_unavailable", `Herdr could not inspect child pane ${child.pane}: ${(result.stderr || result.stdout).trim().slice(0, 200) || "no diagnostic"}`);
  let parsed: unknown;
  try { parsed = JSON.parse(result.stdout); } catch { throw new HcoordError("runtime_unavailable", `Herdr returned invalid JSON for child pane ${child.pane}`); }
  const pane = (parsed as { result?: { pane?: { tokens?: unknown } } }).result?.pane;
  const tokens = pane?.tokens;
  const record = tokens !== null && typeof tokens === "object" ? tokens as Record<string, unknown> : {};
  return {
    parentPane: typeof record[PARENT_PANE_TOKEN] === "string" ? record[PARENT_PANE_TOKEN] : null,
    parentMachine: typeof record[PARENT_MACHINE_TOKEN] === "string" ? record[PARENT_MACHINE_TOKEN] : null,
  };
}
