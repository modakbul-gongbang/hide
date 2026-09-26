import { ChevronDownIcon, ChevronRightIcon } from "lucide-react";
import { memo, useRef } from "react";
import { AgentMark } from "../AgentMark";
import { branchChip, lineTone, markTone, rowAccessibleName, sidebarLine } from "../agentRow";
import { cn } from "../lib/utils";
import type { AgentRow } from "../snapshot";
import { DescendantBadge } from "./agent-row";
import { StatusMark } from "./status-mark";
import { Badge } from "./ui/badge";
import { Hint } from "./ui/tooltip";

/**
 * A control on the right of a sidebar row that waits for the pointer: its
 * slot is always there, so the title and the time never move, and it shows
 * under the pointer, while focus is anywhere inside the row, and always on
 * an input with no hover (PRD sidebar-readability D-3, B2, B3). An agent row
 * has no menu of its own; the Projects rows' `ROW_REVEALED` in sidebar.tsx
 * adds the menu-open case for rows that have one.
 */
export const REVEALED_CONTROL =
  "opacity-0 group-hover/row:opacity-100 group-focus-within/row:opacity-100 focus-visible:opacity-100 hoverless:opacity-100";

/**
 * One agent in the sidebar, in Agents and under a checkout in Projects (PRD
 * sidebar-readability D-2..D-6, B2-B7, B12-B15). Line one is the status mark,
 * the provider mark, the stable task name, a branch chip when a delegated
 * row's checkout differs from its parent's, the device chip, the descendant
 * badge while folded, the elapsed time, and the lineage chevron on a row that
 * has children. A request or news takes one line of its own from the moment it
 * exists; a quiet sentence is only in the tooltip. `place` is the Agents
 * list's context line; Projects passes none because the rows above say it,
 * and for the same reason a Projects root drawn in its own checkout carries
 * no branch chip, while a child drawn under a parent elsewhere keeps it.
 *
 * Nothing here grows or moves on hover, focus, selection or an open menu:
 * those change a fill, a ring and the chevron's opacity only.
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
}) {
  const main = useRef<HTMLButtonElement>(null);
  const line = sidebarLine(agent);
  const branch = branchShown ? branchChip(agent) : null;
  const folded = agent.lineage_collapsed !== false;
  const foldable = onToggleTree !== null && childRows.length > 0;
  const attention = agent.group === "needs_you" || agent.unread;
  const titleTone = agent.delegated && !attention ? "text-subtle-foreground" : attention || agent.emphasized || selected ? "text-foreground" : "text-subtle-foreground";
  const hint = [device ? `${agent.identity_label} · ${device}` : agent.identity_label, agent.detail?.trim(), place].filter(Boolean).join("\n");
  const Chevron = folded ? ChevronRightIcon : ChevronDownIcon;
  return (
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
          aria-label={[rowAccessibleName(agent, device), place].filter(Boolean).join(", ")}
          aria-current={selected ? "true" : undefined}
          data-agent-open={agent.pane_id}
          className="absolute inset-0 rounded-sm outline-none focus-visible:ring-1 focus-visible:ring-inset focus-visible:ring-ring"
          onClick={() => onOpen(agent.pane_id)}
        />
      </Hint>
      <span className="pointer-events-none flex min-h-(--size-control-sm) shrink-0 items-center gap-xs">
        <StatusMark symbol={agent.symbol} className={markTone(agent)} data-agent-status-mark={agent.waiting_on_descendants ? "waiting" : agent.status_label} />
        <AgentMark kind={agent.agent_kind} />
      </span>
      <span className="flex min-w-0 flex-1 flex-col">
        <span className="flex min-h-(--size-control-sm) min-w-0 items-center gap-xs">
          {/* The row button's name already reads all of this out. */}
          <span aria-hidden="true" className={cn("pointer-events-none min-w-0 flex-auto truncate", titleTone, (attention || selected) && "font-medium")} data-agent-title="true">
            {agent.identity_label}
          </span>
          {branch ? (
            <Badge aria-hidden="true" variant="secondary" className="pointer-events-none min-w-0 max-w-2/5 shrink font-mono" data-branch-chip={branch}>
              <span className="truncate">{branch}</span>
            </Badge>
          ) : null}
          {device ? (
            <Badge aria-hidden="true" variant="outline" className="pointer-events-none min-w-0 max-w-2/5 shrink" data-device-chip={device}>
              <span className="truncate">{device}</span>
            </Badge>
          ) : null}
          {descendants > 0 && folded ? (
            <DescendantBadge
              agent={agent}
              descendants={descendants}
              childRows={childRows}
              onOpenChild={onOpen}
              onUnfold={onToggleTree ? () => onToggleTree(agent.pane_id) : null}
              returnFocus={() => main.current?.focus()}
            />
          ) : null}
          {/* An empty elapsed is a time nobody measured, and nothing stands in for it. */}
          {agent.elapsed ? (
            <span aria-hidden="true" className="pointer-events-none shrink-0 font-mono text-micro text-muted-foreground" data-agent-elapsed="true">
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
                "relative flex h-(--size-control-sm) w-(--size-lineage-chevron) shrink-0 items-center justify-center rounded-xs text-muted-foreground outline-none hover:text-foreground focus-visible:ring-1 focus-visible:ring-ring",
                !folded && REVEALED_CONTROL,
              )}
              onClick={() => onToggleTree(agent.pane_id)}
            >
              <Chevron aria-hidden="true" className="size-(--size-icon-sm)" />
            </button>
          ) : null}
        </span>
        {line ? (
          <span aria-hidden="true" data-agent-line={line.mode} className={cn("pointer-events-none truncate text-caption", lineTone(line, agent.demand))}>
            {line.text}
          </span>
        ) : null}
        {place ? (
          <span aria-hidden="true" data-agent-place={place} className="pointer-events-none truncate text-caption text-muted-foreground">
            {place}
          </span>
        ) : null}
      </span>
    </li>
  );
});
