// Frozen cross-project graph computation from main 148b0e0b (PR 749).
// Test data only: the incoming assertions keep their original screen values.
import type { CrossChip } from "../src/agentGraph";
import type { AgentGraphScope } from "../src/agentScope";
import type { LensAgent } from "../src/overviewLens";
import type { Workspace } from "../src/snapshot";
const THIS_DEVICE = "\u0000this-device";
function push<K, V>(map: Map<K, V[]>, key: K, value: V) {
  const values = map.get(key);
  if (values) values.push(value); else map.set(key, [value]);
}
function byAttentionThenActivity(a: LensAgent, b: LensAgent): number {
  return a.agent.state.graph_rank - b.agent.state.graph_rank || (b.agent.last_activity ?? "").localeCompare(a.agent.last_activity ?? "");
}
type Lineage = { byPane: Map<string, LensAgent>; children: Map<string, LensAgent[]> };

function lineageOf(everyone: readonly LensAgent[]): Lineage {
  const byPane = new Map<string, LensAgent>();
  const children = new Map<string, LensAgent[]>();
  for (const value of everyone) {
    if (byPane.has(value.agent.pane_id)) continue;
    byPane.set(value.agent.pane_id, value);
    const parent = value.agent.lineage_parent_pane_id;
    if (parent && parent !== value.agent.pane_id) push(children, parent, value);
  }
  return { byPane, children };
}

/** The chips of one row: one per project its children work in, then one for a parent in another project (issue 718). */
function crossChipsOf(value: LensAgent, lineage: Lineage): Omit<CrossChip, "count">[] {
  const chip = (direction: CrossChip["direction"], others: LensAgent[]): Omit<CrossChip, "count"> => {
    const ordered = others.slice().sort(byAttentionThenActivity);
    const first = ordered[0]!;
    const sameDevice = value.project.device_id === first.project.device_id;
    const theirs = first.device ?? THIS_DEVICE;
    return { direction, project: first.project, device: sameDevice ? null : theirs, paneIds: ordered.map((other) => other.agent.pane_id), names: ordered.map((other) => other.agent.identity_label), box: first.checkout.id };
  };
  const chips: Omit<CrossChip, "count">[] = [];
  const byProject = new Map<string, LensAgent[]>();
  for (const other of lineage.children.get(value.agent.pane_id) ?? []) if (other.project.id !== value.project.id) push(byProject, other.project.id, other);
  // Projects in the order of their most urgent agent, so a snapshot that only reorders the list keeps the chips in place.
  const outs = [...byProject.values()].map((others) => chip("out", others));
  const urgency = (out: Omit<CrossChip, "count">) => lineage.byPane.get(out.paneIds[0]!)!;
  outs.sort((a, b) => byAttentionThenActivity(urgency(a), urgency(b)));
  chips.push(...outs);
  const parentId = value.agent.lineage_parent_pane_id;
  const parent = parentId ? lineage.byPane.get(parentId) : undefined;
  if (parent && parent.project.id !== value.project.id) chips.push(chip("in", [parent]));
  return chips;
}

export function legacyGraphCross(project: Workspace, everyone: readonly LensAgent[]): AgentGraphScope["cross"] {
  const lineage = lineageOf(everyone);
  const cross: AgentGraphScope["cross"] = {};
  for (const value of everyone.filter((v) => v.project.id === project.id && v.project.device_id === project.device_id)) {
    cross[value.agent.pane_id] ??= crossChipsOf(value, lineage).map((chip) => ({
      direction: chip.direction, project_id: chip.project.id, project_device_id: chip.project.device_id,
      device: chip.device === null ? null : { label: chip.device === THIS_DEVICE ? null : chip.device },
      pane_ids: chip.paneIds, names: chip.names, box_id: chip.box, count: chip.paneIds.length,
    }));
  }
  return cross;
}
