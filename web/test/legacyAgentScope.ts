import { legacyAgentRow } from "./legacyAgentRow";
// Frozen scope fixture adapter from main 9f144877. Test data only.
// Assertions keep the old screen values; production reads the core wire.
import type { AgentScope } from "../src/agentScope";
import type { AgentRow, Workspace, Checkout, RequestVerb, SnapshotRest } from "../src/snapshot";
import type { BoardProject } from "../src/projectBoard";
import { agentsTile as drawAgentsTile, scopeAgents as drawScopeAgents, type LensAgent } from "../src/overviewLens";
import { requestRows as drawRequestRows, requestGroups as drawRequestGroups, requestsTile as drawRequestsTile, type RequestRow } from "../src/requestList";

const verbs: RequestVerb[] = ["answer", "fix", "review", "stopped", "result", "working", "waiting", "idle"];
const todo = verbs.slice(0, 5);
export function emptyScope(): AgentScope {
  return { pane_ids: [], total: 0, overview_total: 0, roots: [], groups: { needs_you: 0, done: 0, working: 0, seen: 0 }, marks: { error: 0, approval: 0, question: 0, working: 0, done: 0, idle: 0 }, sections: [], members: [], buckets: { turn: 0, working: 0, delegating: 0, resting: 0 }, turns: { question: 0, approval: 0, error: 0, done: 0 }, requests: { rows: [], groups: [], counts: Object.fromEntries(verbs.map((verb) => [verb, 0])) as Record<RequestVerb, number>, todo: 0, answer: 0 } };
}

export function legacyScope(lens: readonly LensAgent[], all: readonly AgentRow[] = lens.map((l) => l.agent)): AgentScope {
  const scope = emptyScope();
  scope.members = lens.map(({ agent, project, checkout }) => ({ pane_id: agent.pane_id, project_id: project.id, checkout_id: checkout.id }));
  scope.overview_total = lens.length;
  const inScope = new Set(lens.map((l) => l.agent.pane_id));
  const byPane = new Map(all.map((a) => [a.pane_id, a]));
  lens.forEach(({ agent }, member) => {
    const bucket = agent.group === "needs_you" || agent.group === "done" ? "turn" : agent.waiting_on_descendants ? "delegating" : agent.group === "working" ? "working" : "resting";
    scope.buckets[bucket]++;
    if (agent.group === "done") scope.turns.done++;
    if (agent.group === "needs_you" && ["question", "approval", "error"].includes(agent.demand ?? "")) scope.turns[agent.demand as "question" | "approval" | "error"]++;
    if (agent.delegated && agent.lineage_parent_pane_id && inScope.has(agent.lineage_parent_pane_id)) return;
    scope.requests.rows.push({ member, children: (agent.close_descendant_pane_ids ?? []).filter((id) => byPane.has(id)).reverse() });
  });
  const agentAt = (index: number) => lens[scope.requests.rows[index]!.member]!.agent;
  for (const verb of verbs) {
    const rows = scope.requests.rows.map((_, i) => i).filter((i) => (agentAt(i).request?.verb ?? (agentAt(i).group === "working" ? "working" : "idle")) === verb);
    rows.sort(todo.includes(verb) ? (a, b) => (agentAt(a).request?.verb_since_unix_ms ?? Number.MAX_SAFE_INTEGER) - (agentAt(b).request?.verb_since_unix_ms ?? Number.MAX_SAFE_INTEGER) : (a, b) => (agentAt(b).last_activity ?? "").localeCompare(agentAt(a).last_activity ?? ""));
    if (rows.length) scope.requests.groups.push({ verb, rows });
    scope.requests.counts[verb] = rows.length;
    if (todo.includes(verb)) scope.requests.todo += rows.length;
  }
  scope.requests.answer = scope.requests.counts.answer;
  return scope;
}

export function legacyProject<T extends Workspace>(project: T, agents: readonly AgentRow[]): T {
  const owners = new Map<string, Checkout>();
  for (const checkout of project.checkouts) for (const tab of checkout.tabs ?? []) for (const pane of tab.panes) if (!owners.has(pane.id)) owners.set(pane.id, checkout);
  const seen = new Set<string>();
  const members: LensAgent[] = [];
  for (const agent of agents) {
    const checkout = owners.get(agent.pane_id);
    if (!checkout || seen.has(agent.pane_id)) continue;
    seen.add(agent.pane_id);
    members.push({ agent, checkout, project, device: null, bucket: agent.state?.bucket ?? "resting", task: null });
  }
  const scope = legacyScope(members, agents);
  const physical = agents.filter((agent) => owners.has(agent.pane_id));
  physicalScope(scope, physical);
  for (const checkout of project.checkouts) {
    const marks = checkout.agent_summary?.marks;
    if (marks) for (const key of Object.keys(scope.marks) as (keyof AgentScope["marks"])[]) scope.marks[key] += marks[key];
  }
  return { ...project, agent_scope: scope };
}

function physicalScope(scope: AgentScope, agents: readonly AgentRow[]) {
  scope.pane_ids = agents.map((a) => a.pane_id);
  scope.total = agents.length;
  scope.roots = agents.filter((a) => !a.delegated).map((a) => a.pane_id);
  for (const agent of agents) if (agent.group in scope.groups) scope.groups[agent.group as keyof AgentScope["groups"]]++;
}

export function scopeAgents(projects: readonly BoardProject[]): LensAgent[] {
  return projects.flatMap((p) => {
    const agents = p.agents.map((a) => a.state ? a : legacyAgentRow(a));
    return drawScopeAgents([{ ...p, agents, workspace: legacyProject(p.workspace, agents) }]).map((row) => ({ ...row, project: p.workspace }));
  });
}
export function agentsTile(agents: readonly LensAgent[], ...args: Parameters<typeof drawAgentsTile> extends [unknown, ...infer R] ? R : never) {
  return drawAgentsTile(legacyScope(agents), ...args);
}
const scopeOfRows = new WeakMap<readonly RequestRow[], AgentScope>();
export function requestRows(agents: readonly LensAgent[], all: readonly AgentRow[]): RequestRow[] {
  const scope = legacyScope(agents, all);
  const rows = drawRequestRows(agents, all, scope);
  scopeOfRows.set(rows, scope);
  return rows;
}
export function requestScope(rows: readonly RequestRow[]): AgentScope {
  return scopeOfRows.get(rows) ?? legacyScope(rows.map((row) => row.lens));
}
export function requestGroups(rows: readonly RequestRow[]) { return drawRequestGroups(rows, requestScope(rows)); }
export function requestsTile(rows: readonly RequestRow[], ...args: Parameters<typeof drawRequestsTile> extends [unknown, ...infer R] ? R : never) { return drawRequestsTile(requestScope(rows), ...args); }

export function legacyRest(rest: SnapshotRest, agents: AgentRow[]): SnapshotRest {
  const localId = rest.navigator?.devices?.find((d) => d.kind !== "remote")?.id ?? "";
  const projects = (rest.navigator?.workspaces ?? []).map((p) => legacyProject(p, agents));
  const remotes = (rest.status?.remote ?? []).map((r) => ({ ...r, session: r.session ? { ...r.session, workspaces: r.session.workspaces.map((p) => legacyProject(p, r.session!.agents)) } : r.session }));
  const deviceScopes = new Map<string, AgentScope>();
  const local = legacyScope(scopeAgents(projects.filter((p) => !p.is_home).map((workspace) => ({ workspace, agents, device: null }))), agents);
  physicalScope(local, agents);
  deviceScopes.set(localId, local);
  for (const remote of remotes) {
    const all = remote.session?.agents ?? [];
    const members = scopeAgents((remote.session?.workspaces ?? []).filter((p) => !p.is_home).map((workspace) => ({ workspace, agents: all, device: remote.target_id })));
    const scope = legacyScope(members, all);
    physicalScope(scope, remote.state === "connected" ? all : []);
    deviceScopes.set(remote.target_id, scope);
  }
  return { ...rest, navigator: { ...rest.navigator, workspaces: projects, devices: rest.navigator?.devices?.map((d) => ({ ...d, agent_scope: deviceScopes.get(d.id) ?? emptyScope() })) }, status: { ...rest.status, remote: remotes } };
}
