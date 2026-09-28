import { activeDisplay, adjacentInOrder, locateDisplay, neighbourArea, resizeTarget, splitEligibility, type AreaLayout, type Edge, type Geometry, type LayoutSizes } from "./areaLayout";
import type { MenuEntry } from "./components/entry-menu";

export type AgentItem = { id: string };
export type AgentLayout = AreaLayout<AgentItem> & { waiting?: number; canvases: Record<string, string> };
export const AGENT_WORDS = { item: "tab", area: "Agent area", plural: "Agent areas" };
export const REMOTE_GROUP_REASON = "Agent groups are available in local Workspaces only.";
const EDGES: Edge[] = ["right", "left", "up", "down"];
export type AgentCommand = "rename_tab" | "new_tab" | "copy_name" | "close_tab" | `split_${Edge}` | `move_${Edge}` | "focus_next" | "focus_previous" | "grow" | "shrink";
export type AgentFrame = { workspace: { device_id: string; path: string }; layout: AgentLayout; geometry: Geometry; sizes: LayoutSizes };
export function agentMenu(frame: AgentFrame, id: string, palette = false): MenuEntry<AgentCommand>[] {
  const located = locateDisplay(frame.layout.root, id);
  if (!located) return [];
  const entries: MenuEntry<AgentCommand>[] = [{ id: "new_tab", label: "New tab", unavailable: null }];
  for (const edge of EDGES) {
    const eligibility = splitEligibility(frame.layout, frame.geometry, frame.sizes, id, located.area.id, edge, AGENT_WORDS);
    entries.push({ id: `split_${edge}`, label: `Split ${edge}`, unavailable: frame.workspace.device_id !== "local" ? REMOTE_GROUP_REASON : eligibility.ok ? null : eligibility.reason });
  }
  for (const edge of EDGES) {
    const neighbour = neighbourArea(frame.layout.root, located.area.id, edge);
    if (neighbour || palette) entries.push({ id: `move_${edge}`, label: `Move ${edge}`, unavailable: neighbour ? null : `There is no Agent area ${edge}.` });
  }
  if (!palette) entries.push({ id: "rename_tab", label: "Rename…", unavailable: null });
  entries.push({ id: "copy_name", label: "Copy name", unavailable: null, separated: true }, { id: "close_tab", label: "Close tab…", unavailable: null, separated: true });
  return entries;
}
export function agentCommands(frame: AgentFrame): MenuEntry<AgentCommand>[] {
  const active = activeDisplay(frame.layout);
  const entries = active ? agentMenu(frame, active.display.id, true) : [{ id: "new_tab" as const, label: "New tab", unavailable: null }];
  for (const forward of [true, false]) {
    entries.push({ id: forward ? "focus_next" : "focus_previous", label: `Focus ${forward ? "next" : "previous"} Agent area`, unavailable: adjacentInOrder(frame.layout.root, frame.layout.active_area, forward ? 1 : -1) ? null : "There is only one Agent area." });
  }
  for (const grow of [true, false]) {
    const target = resizeTarget(frame.layout, frame.geometry, grow, AGENT_WORDS);
    entries.push({ id: grow ? "grow" : "shrink", label: `${grow ? "Grow" : "Shrink"} Agent area`, unavailable: "reason" in target ? target.reason : null });
  }
  return entries;
}

/** Shared one-line capacity notice; authoritative tabs remain available upstream. */
export function agentCapacityNotice(waiting: number): string {
  return `${waiting} Agent ${waiting === 1 ? "tab is" : "tabs are"} waiting. Close a tab to make room.`;
}
