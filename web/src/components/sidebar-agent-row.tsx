import { ChevronDownIcon, ChevronRightIcon } from "lucide-react";
import { memo, useRef } from "react";
import { AgentMark } from "../AgentMark";
import { agentClosing } from "../close";
import { useShellStore } from "../store";
import { branchChip, lineTone, markTone, rowAccessibleName, sidebarLine } from "../agentRow";
import { cn } from "../lib/utils";
import type { AgentRow } from "../snapshot";
import type { AgentMenuItem } from "../workspaceManage";
import type { FoldedLineage } from "../lineageSummary";
import { DescendantBadge } from "./agent-row";
import { EntryContextMenu, type MenuEntry } from "./entry-menu";
import { DeviceChip } from "./device-chip";
import { StatusMark } from "./status-mark";
import { Badge } from "./ui/badge";
import { Keycap } from "./ui/keycap";
import { Hint } from "./ui/tooltip";

/**
 * A control on the right of a sidebar row that waits for the pointer: its
 * slot is always there, so the title and the time never move, and it shows
 * under the pointer, while focus is anywhere inside the row, while the row's
 * menu is open, and always on an input with no hover (PRD sidebar-readability
 * D-3, B2, B3), as the Projects rows' `ROW_REVEALED` in sidebar.tsx does.
 */
export const REVEALED_CONTROL =
  "opacity-0 group-hover/row:opacity-100 group-focus-within/row:opacity-100 group-data-[state=open]/row:opacity-100 focus-visible:opacity-100 hoverless:opacity-100";

/**
 * An agent row's right-click menu (PRD sidebar-context-menus D-04): its items
 * are read when it opens, and the choice names the row it was opened on. One
 * object serves every row of a list, so the rows stay memoized.
 */
export type AgentRowMenu = {
  items: (agent: AgentRow) => MenuEntry<AgentMenuItem["id"]>[];
  onSelect: (agent: AgentRow, id: AgentMenuItem["id"]) => void;
};

/**
 * The fold slot of a sidebar row with nothing to fold. Every row, agent,
 * checkout or project, ends in its time and then this slot, so the times and
 * the chevrons of the whole list stand in one column each (D-3).
 */
export function FoldLane() {
  return <span aria-hidden="true" data-fold-slot="true" className="w-(--size-lineage-chevron) shrink-0" />;
}

/**
 * One agent in the sidebar, in Agents and under a checkout in Projects (PRD
 * sidebar-readability D-2..D-6, B2-B7, B12-B15). Line one is the status mark,
 * the provider mark, the stable task name, a branch chip when a delegated
 * row's checkout differs from its parent's, the device chip, the descendant
 * badge while folded, the elapsed time, and the lineage chevron on a row that
 * has children, whose slot every other row keeps empty. A request or news takes one line of its own from the moment it
 * exists; a quiet sentence is only in the tooltip. `place` is the Agents
 * list's context line; Projects passes none because the rows above say it,
 * and for the same reason a Projects root drawn in its own checkout carries
 * no branch chip, while a child drawn under a parent elsewhere keeps it.
 *
 * Nothing here grows or moves on hover, focus, selection or an open menu:
 * those change a fill, a ring and the chevron's opacity only. The title is
 * 12/400 in every state; attention and selection brighten it rather than
 * thicken it (PRD sidebar-typography D-02). Line one is 20 high and line two
 * 16, so a row is 28 or 44 with its padding (D-03). `number` is the digit an
 * ⌥ hold shows at the row's top right (PRD electron-digit-shortcuts-hints B5),
 * floating over the time and the fold slot, which stay where they are.
 */
export const SidebarAgentRow = memo(function SidebarAgentRow({
  agent,
  device,
  place,
  depth,
  descendants,
  childRows,
  selected,
  onOpen,
  onToggleTree,
  inset,
  branchShown = true,
  foldedLineage,
  number = null,
  menu,
}: {
  agent: AgentRow;
  device: string | null;
  place: string | null;
  /** How deep under the root drawn above it; a root is 0. */
  depth: number;
  /** Live descendants among the device's rows. */
  descendants: number;
  /** The direct children still listed, for the chevron and the badge's popover. */
  childRows: AgentRow[];
  selected: boolean;
  onOpen: (paneId: string) => void;
  /** Null where the tree is drawn with nothing folded (a selected SSH device's Projects). */
  onToggleTree: ((paneId: string) => void) | null;
  /** Where a root's first column starts. */
  inset: string;
  /** False where the row above already names the row's checkout. */
  branchShown?: boolean;
  /** Derived C lines and the same-checkout portion that remains in the badge. */
  foldedLineage?: FoldedLineage;
  /** The digit a modifier hold shows on this row, or null while none shows. */
  number?: number | null;
  /** The row's right-click menu; the menu key or ⇧F10 on the focused row opens it too. */
  menu: AgentRowMenu;
}) {
  const main = useRef<HTMLButtonElement>(null);
  const closing = useShellStore((s) => agentClosing(s.rest?.status?.async_operations, agent.pane_id));
  const line = sidebarLine(agent);
  const branch = branchShown ? branchChip(agent) : null;
  const folded = agent.lineage_collapsed !== false;
  const foldable = onToggleTree !== null && (agent.lineage_child_pane_ids?.length ?? 0) > 0;
  const attention = agent.group === "needs_you" || agent.unread;
  const titleTone = agent.delegated && !attention ? "text-subtle-foreground" : attention || agent.emphasized || selected ? "text-foreground" : "text-subtle-foreground";
  const hint = [device ? `${agent.identity_label} · ${device}` : agent.identity_label, agent.detail?.trim(), place].filter(Boolean).join("\n");
  const Chevron = folded ? ChevronRightIcon : ChevronDownIcon;
  return (
    <EntryContextMenu
      asChild
      label={`${agent.identity_label} actions`}
      items={() => menu.items(agent)}
      onSelect={(id) => menu.onSelect(agent, id)}
      data-agent-menu={agent.pane_id}
    >
    <li
      data-pane={agent.pane_id}
      data-attention={attention ? "true" : "false"}
      data-delegated={agent.delegated ? "true" : "false"}
      data-waiting={agent.waiting_on_descendants ? "true" : "false"}
      data-agent-device={device ?? "local"}
      data-depth={depth}
      className={cn("group/row relative flex items-start gap-xs rounded-sm py-xs pr-xs text-body", selected ? "bg-secondary" : "hover:bg-accent")}
      style={{ paddingLeft: `calc(${inset} + ${depth} * var(--size-lineage-indent))` }}
    >
      <Hint label={hint} reveals>
        <button
          ref={main}
          type="button"
          aria-label={[rowAccessibleName(agent, device), closing ? "closing" : null, place].filter(Boolean).join(", ")}
          aria-current={selected ? "true" : undefined}
          data-agent-open={agent.pane_id}
          className="absolute inset-0 rounded-sm outline-none focus-visible:ring-1 focus-visible:ring-inset focus-visible:ring-ring"
          onClick={() => onOpen(agent.pane_id)}
        />
      </Hint>
      <span className="pointer-events-none flex min-h-(--size-sidebar-line) shrink-0 items-center gap-xs">
        <StatusMark symbol={agent.symbol} className={markTone(agent)} data-agent-status-mark={agent.waiting_on_descendants ? "waiting" : agent.status_label} />
        <AgentMark kind={agent.agent_kind} />
      </span>
      <span className="flex min-w-0 flex-1 flex-col">
        <span className="flex min-h-(--size-sidebar-line) min-w-0 items-center gap-xs">
          {/* The row button's name already reads all of this out. */}
          <span aria-hidden="true" className={cn("pointer-events-none min-w-0 flex-auto truncate", titleTone)} data-agent-title="true">
            {agent.identity_label}
          </span>
          {closing ? (
            <span aria-hidden="true" className="pointer-events-none shrink-0 text-caption text-muted-foreground" data-agent-closing="true">
              closing…
            </span>
          ) : null}
          {branch ? (
            <Badge aria-hidden="true" variant="secondary" className="pointer-events-none min-w-0 max-w-2/5 shrink font-mono" data-branch-chip={branch}>
              <span className="truncate">{branch}</span>
            </Badge>
          ) : null}
          {device ? (
            <DeviceChip label={device} className="pointer-events-none max-w-2/5" />
          ) : null}
          {descendants > 0 && folded ? (
            <DescendantBadge
              agent={foldedLineage ? { ...agent, descendant_counts: foldedLineage.badgeCounts } : agent}
              descendants={descendants}
              childRows={childRows}
              onOpenChild={onOpen}
              onUnfold={onToggleTree ? () => onToggleTree(agent.pane_id) : null}
              returnFocus={() => main.current?.focus()}
            />
          ) : null}
          {/* An empty elapsed is a time nobody measured, and nothing stands in for it. */}
          {agent.elapsed ? (
            <span aria-hidden="true" className="pointer-events-none shrink-0 font-mono text-caption text-muted-foreground" data-agent-elapsed="true">
              {agent.elapsed}
            </span>
          ) : null}
          {foldable ? (
            <button
              type="button"
              aria-label={folded ? `Show ${agent.identity_label}'s agents` : `Hide ${agent.identity_label}'s agents`}
              aria-expanded={!folded}
              data-agent-tree-toggle={agent.pane_id}
              className={cn(
                "relative flex h-(--size-sidebar-line) w-(--size-lineage-chevron) shrink-0 items-center justify-center rounded-xs text-muted-foreground outline-none hover:text-foreground focus-visible:ring-1 focus-visible:ring-ring",
                !folded && REVEALED_CONTROL,
              )}
              onClick={() => onToggleTree(agent.pane_id)}
            >
              <Chevron aria-hidden="true" className="size-(--size-icon-sm)" />
            </button>
          ) : (
            <FoldLane />
          )}
        </span>
        {line ? (
          <span aria-hidden="true" data-agent-line={line.mode} className={cn("pointer-events-none truncate text-caption leading-(--size-sidebar-line-detail)", lineTone(line, agent.demand))}>
            {line.text}
          </span>
        ) : null}
        {place ? (
          <span aria-hidden="true" data-agent-place={place} className="pointer-events-none truncate text-caption leading-(--size-sidebar-line-detail) text-muted-foreground">
            {place}
          </span>
        ) : null}
        {folded && foldedLineage ? (
          <span className="pointer-events-none flex min-w-0 flex-col" aria-hidden="true" data-checkout-lines={agent.pane_id}>
            {foldedLineage.lines.map((summary) => (
              <span key={summary.key} className="flex min-h-(--size-badge-height) min-w-0 items-center gap-xs text-caption" data-checkout-line={summary.branch}>
                <StatusMark symbol={summary.symbol} className={markTone(summary.agent)} />
                <span className="min-w-0 flex-1 truncate font-mono text-subtle-foreground">{summary.branch}</span>
                {summary.pullRequest !== null ? <span className="shrink-0 text-muted-foreground">#{summary.pullRequest}</span> : null}
                {summary.device ? <DeviceChip label={summary.device} className="max-w-2/5" /> : null}
              </span>
            ))}
            {foldedLineage.overflow > 0 ? (
              <span className="min-h-(--size-badge-height) truncate pl-(--size-lineage-indent) text-caption text-muted-foreground" data-checkout-line-overflow={foldedLineage.overflow}>
                +{foldedLineage.overflow}
              </span>
            ) : null}
          </span>
        ) : null}
      </span>
      {number !== null ? <Keycap number={number} /> : null}
    </li>
    </EntryContextMenu>
  );
});
