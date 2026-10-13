import { childrenOf } from "./components/agent-tree-popover";
import { deviceConnected, localDeviceId } from "./devices";
import type { MessageKey } from "./i18n/catalogs";
import { remoteTargetOfPane } from "./remote";
import type { AgentRow, SnapshotRest } from "./snapshot";

export type ChildCountPart = {
  kind: "raised" | "working" | "result" | "waiting" | "unknown" | "idle";
  symbol: string;
  count: number;
  tone: string;
  label: Extract<MessageKey, `panes.children.${string}`>;
};

/** Presentation of the core's direct-child counts, not another status projection. */
export function paneChildren(paneId: string, agents: AgentRow[], rest: SnapshotRest | null) {
  const byPane = new Map(agents.map((agent) => [agent.pane_id, agent]));
  const parent = byPane.get(paneId);
  if (!parent) return null;
  const children = childrenOf(parent, byPane);
  if (children.length === 0) return null;
  // Topology can survive a connection. Subtree totals cannot claim current
  // work when one of their owning devices is unavailable.
  const unavailable = agents.find((agent) => {
    if (agent.pane_id !== paneId && agent.lineage_parent_pane_id !== paneId && !agent.lineage_path_pane_ids?.includes(paneId)) return false;
    return !deviceConnected(rest, agent.device_id ?? remoteTargetOfPane(rest, agent.pane_id) ?? localDeviceId(rest));
  });
  const unavailableDevice = unavailable ? unavailable.device_id ?? remoteTargetOfPane(rest, unavailable.pane_id) ?? localDeviceId(rest) : null;
  const raised = parent.descendant_mark?.kind === "raised" ? parent.descendant_mark.count : 0;
  const parts: ChildCountPart[] = [];
  if (raised > 0 && unavailableDevice === null) parts.push({ kind: "raised", symbol: "!", count: raised, tone: "text-warning", label: "panes.children.raised" });
  const counts = parent.direct_child_counts;
  if (counts && unavailableDevice === null) {
    // The core omits its unknown count from the wire; its final row status
    // already applies demand precedence and identifies these children.
    const unknown = children.filter((child) => child.status_code === "unknown").length;
    const waiting = counts.error + counts.approval + counts.question;
    const idle = children.length - waiting - counts.working - counts.done - unknown;
    parts.push(
      { kind: "working", symbol: "●", count: counts.working, tone: "text-agent-working", label: "panes.children.working" },
      { kind: "result", symbol: "✓", count: counts.done, tone: "text-success", label: "panes.children.result" },
      { kind: "waiting", symbol: "○", count: waiting, tone: "text-agent-working", label: "panes.children.waiting" },
      { kind: "unknown", symbol: "~", count: unknown, tone: "text-muted-foreground", label: "panes.children.unknown" },
      { kind: "idle", symbol: "○", count: idle, tone: "text-muted-foreground", label: "panes.children.idle" },
    );
  }
  return { parent, children, raised, parts: parts.filter((part) => part.count > 0), lead: children[0]!, unavailableDevice };
}

export type PaneChildren = NonNullable<ReturnType<typeof paneChildren>>;
