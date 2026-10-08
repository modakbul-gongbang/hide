// Look up core-selected Sessions members. Grouping, order and counts belong
// to agent_state; this adapter only attaches the objects a row draws.
import { deviceScope, type AgentScope } from "./agentScope";
import { deviceConnected, frontDeviceId } from "./devices";
import { boardProjects } from "./navigation";
import { scopeAgents, type LensAgent } from "./overviewLens";
import { catalogWorkspaces, frontCheckout, type AgentRow, type SnapshotRest, type Workspace } from "./snapshot";

export type SessionsModel = {
  project: Workspace | null;
  scope: AgentScope | null;
  members: LensAgent[];
  projects: Workspace[];
  agents: AgentRow[];
  front: ReturnType<typeof frontCheckout>;
  deviceId: string;
  available: boolean;
  reason: string | null;
};

const key = (project: string, checkout: string, pane: string) => `${project}\0${checkout}\0${pane}`;

export function sessionsModel(rest: SnapshotRest | null, agents: AgentRow[], onlyCheckout: string | null): SessionsModel {
  const deviceId = frontDeviceId(rest);
  const front = frontCheckout(rest);
  const owner = catalogWorkspaces(rest).find((project) => project.device_id === deviceId && project.checkouts.some((checkout) => checkout.id === front?.id));
  const project = owner && !owner.is_home ? owner : null;
  const projects = boardProjects(rest, agents, deviceId).filter((item) => !project || item.workspace.id === project.id);
  const lenses = scopeAgents(projects);
  const byPlace = new Map(lenses.map((row) => [key(row.project.id, row.checkout.id, row.agent.pane_id), row]));
  const scope = project
    ? onlyCheckout === front?.id ? front.agent_scope : project.agent_scope
    : deviceScope(rest, deviceId);
  const members = (scope?.members ?? []).map((member) => {
    const row = byPlace.get(key(member.project_id, member.checkout_id, member.pane_id));
    if (!row) throw new Error("Sessions scope references a missing member");
    return row;
  });
  const remote = rest?.status?.remote?.find((status) => status.target_id === deviceId);
  return {
    project, scope, members, front, deviceId, projects: projects.map((item) => item.workspace),
    // Lineage can cross devices even when the panel is scoped to one device.
    agents: [...agents, ...(rest?.status?.remote ?? []).flatMap((status) => status.session?.agents ?? [])],
    available: deviceConnected(rest, deviceId),
    reason: remote?.message ?? null,
  };
}
