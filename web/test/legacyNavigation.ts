// Pre-refactor device membership and context-line fixture inputs.
import { allAgents as drawAll, deviceListedAgents as drawDevice, agentPlaces as drawPlaces } from "../src/navigation";
import { emptyScope, legacyProject } from "./legacyAgentScope";
import type { AgentRow, Device, RemoteStatus, Workspace } from "../src/snapshot";

function listed(remote: RemoteStatus[] | undefined, devices: Device[] | undefined, local: AgentRow[]) {
  const scope = emptyScope();
  const here = devices?.find((d) => d.kind !== "remote");
  scope.listed = local.map((a, index) => ({ pane_id: a.pane_id, device_id: here?.id ?? "", device_label: here?.label ?? null, remote: false, index }));
  for (const r of remote ?? []) if (r.state === "connected") scope.listed.push(...(r.session?.agents ?? []).map((a, index) => ({ pane_id: a.pane_id, device_id: r.target_id, device_label: devices?.find((d) => d.id === r.target_id)?.label ?? r.target_id, remote: true, index })));
  return scope;
}
export function allAgents(remote: RemoteStatus[] | undefined, devices: Device[] | undefined, local: AgentRow[]) {
  return drawAll(remote, local, listed(remote, devices, local));
}
export function deviceListedAgents(remote: RemoteStatus[] | undefined, devices: Device[] | undefined, local: AgentRow[], id: string) {
  const scope = listed(remote, devices, local);
  scope.listed = scope.listed.filter((r) => r.device_id === id);
  return drawDevice(remote, devices?.map((d) => d.id === id ? { ...d, agent_scope: scope } : d), local, id);
}
export function agentPlaces(local: Workspace[] | undefined, remote: RemoteStatus[] | undefined, devices: Device[] | undefined) {
  return drawPlaces((local ?? []).map((p) => legacyProject(p, [])), (remote ?? []).map((r) => ({ ...r, session: r.session ? { ...r.session, workspaces: (r.session.workspaces ?? []).map((p) => { const value = legacyProject(p, []); value.agent_scope.places_live = r.state === "connected"; return value; }) } : r.session })), devices);
}
