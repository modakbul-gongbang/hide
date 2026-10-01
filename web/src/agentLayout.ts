import { adjacentInOrder, locateDisplay, neighbourArea, resizeTarget, splitEligibility, type AreaLayout, type Edge, type Geometry, type LayoutSizes } from "./areaLayout";
import type { MenuEntry } from "./components/entry-menu";

export type AgentItem = { id: string };
export type AgentLayout = AreaLayout<AgentItem> & { waiting?: number; canvases: Record<string, string> };
export const AGENT_WORDS = { item: "tab", area: "Agent area", plural: "Agent areas" };
export const REMOTE_GROUP_REASON = "Agent groups are available in local Workspaces only.";
const EDGES: Edge[] = ["right", "left", "up", "down"];
export type AgentCommand = "rename_tab" | "new_tab" | "copy_name" | "close_tab" | `split_${Edge}` | `move_${Edge}` | "focus_next" | "focus_previous" | "grow" | "shrink";
export type AgentFrame = { workspace: { device_id: string; path: string }; layout: AgentLayout; geometry: Geometry; sizes: LayoutSizes };
export function agentMenu(frame: AgentFrame, id: string): MenuEntry<AgentCommand>[] {
  const located = locateDisplay(frame.layout.root, id);
  if (!located) return [];
  const entries: MenuEntry<AgentCommand>[] = [{ id: "new_tab", label: "New tab", unavailable: null }];
  for (const edge of EDGES) {
    const eligibility = splitEligibility(frame.layout, frame.geometry, frame.sizes, id, located.area.id, edge, AGENT_WORDS);
    entries.push({ id: `split_${edge}`, label: `Split ${edge}`, unavailable: frame.workspace.device_id !== "local" ? REMOTE_GROUP_REASON : eligibility.ok ? null : eligibility.reason });
  }
  for (const edge of EDGES) {
    const neighbour = neighbourArea(frame.layout.root, located.area.id, edge);
    if (neighbour) entries.push({ id: `move_${edge}`, label: `Move ${edge}`, unavailable: null });
  }
  entries.push({ id: "rename_tab", label: "Rename…", unavailable: null });
  entries.push({ id: "copy_name", label: "Copy name", unavailable: null, separated: true }, { id: "close_tab", label: "Close tab…", unavailable: null, separated: true });
  return entries;
}
export type AgentAreaStep = "focus_next" | "focus_previous" | "grow" | "shrink";

/** Why an Agent area focus or resize step cannot run now, or null when it can. */
export function agentAreaStepUnavailable(frame: AgentFrame, step: AgentAreaStep): string | null {
  if (step === "focus_next" || step === "focus_previous") return adjacentInOrder(frame.layout.root, frame.layout.active_area, step === "focus_next" ? 1 : -1) ? null : "There is only one Agent area.";
  const target = resizeTarget(frame.layout, frame.geometry, step === "grow", AGENT_WORDS);
  return "reason" in target ? target.reason : null;
}

/** Shared one-line capacity notice; authoritative tabs remain available upstream. */
export function agentCapacityNotice(waiting: number): string {
  return `${waiting} Agent ${waiting === 1 ? "tab is" : "tabs are"} waiting. Close a tab to make room.`;
}
