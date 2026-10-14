import type { AgentRow, RemoteStatus, SnapshotRest } from "./snapshot";

let lastLocal: AgentRow[] | null = null;
let lastRemote: RemoteStatus[] | undefined;
let lastRows: AgentRow[] = [];

/**
 * Every agent row this page knows: this machine's and each device's retained
 * topology. The answer keeps its identity while no row moved, even when the
 * device rows around the sessions did, so a snapshot that left the agents alone
 * wakes nobody who selects it.
 */
export function allAgents(local: AgentRow[], remote: RemoteStatus[] | undefined): AgentRow[] {
  if (local === lastLocal && remote === lastRemote) return lastRows;
  const rows = remote?.some((row) => row.session?.agents?.length) ? [...local, ...remote.flatMap((row) => row.session?.agents ?? [])] : local;
  lastLocal = local;
  lastRemote = remote;
  if (rows.length === lastRows.length && rows.every((row, index) => row === lastRows[index])) return lastRows;
  lastRows = rows;
  return rows;
}

const indexes = new WeakMap<AgentRow[], ReadonlyMap<string, AgentRow>>();

/** The rows by pane, built once per agents array instead of once per header that asks. */
export function agentsByPane(agents: AgentRow[]): ReadonlyMap<string, AgentRow> {
  let index = indexes.get(agents);
  if (!index) {
    index = new Map(agents.map((agent) => [agent.pane_id, agent]));
    indexes.set(agents, index);
  }
  return index;
}

let lastDevices: NonNullable<SnapshotRest["navigator"]>["devices"];
let lastStatus: RemoteStatus[] | undefined;
let lastFacts: SnapshotRest = {};

/**
 * The devices and their connection rows, shaped as the `rest` the device
 * helpers read. It keeps its identity while neither moved, so a header that
 * only needs to know whether a device is reachable ignores the rest of the
 * snapshot.
 */
export function deviceFacts(rest: SnapshotRest | null): SnapshotRest {
  const devices = rest?.navigator?.devices;
  const remote = rest?.status?.remote;
  if (devices !== lastDevices || remote !== lastStatus) {
    lastDevices = devices;
    lastStatus = remote;
    lastFacts = { navigator: { devices }, status: { remote } };
  }
  return lastFacts;
}
