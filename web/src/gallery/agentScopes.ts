// Static values from the pre-refactor gallery. No state rules run in a scene.
import type { AgentScope } from "../agentScope";
import type { SnapshotRest, Workspace } from "../snapshot";
import data from "./agentScopes.json";

export const EMPTY_GALLERY_SCOPE = data.presets[0] as unknown as AgentScope;
export function galleryScopes(rest: SnapshotRest, scene: string, expandedAgents: string[]): SnapshotRest {
  scene += `:${data.foldable.filter((id) => expandedAgents.includes(id)).join(",")}`;
  const scopes = data.snapshots[scene as keyof typeof data.snapshots];
  if (!scopes) throw new Error(`Unknown gallery scope fixture: ${scene}`);
  const scope = (key: string) => {
    const value = data.presets[(scopes as Record<string, number>)[key]!] as AgentScope | undefined;
    if (!value) throw new Error(`Missing gallery scope fixture: ${key}`);
    return value;
  };
  const project = (workspace: Workspace): Workspace => ({ ...workspace,
    agent_scope: scope(`project:${workspace.device_id}:${workspace.id}`),
    checkouts: workspace.checkouts.map((checkout) => ({ ...checkout, agent_scope: scope(`checkout:${workspace.device_id}:${checkout.id}`) })),
  });
  return { ...rest, navigator: { ...rest.navigator, agent_scope: scope("overall"),
    workspaces: rest.navigator?.workspaces?.map(project),
    devices: rest.navigator?.devices?.map((device) => ({ ...device, agent_scope: scope(`device:${device.id}`) })),
  }, status: { ...rest.status, remote: rest.status?.remote?.map((remote) => ({ ...remote,
    session: remote.session ? { ...remote.session, workspaces: remote.session.workspaces.map(project) } : remote.session,
  })) } };
}
