import { ChevronRightIcon } from "lucide-react";
import { memo, useRef } from "react";
import { AgentMark } from "../AgentMark";
import { badgeLabel, badgeParts, branchChip, lineShownAtRest, lineTone, markTone, rowAccessibleName, rowLine } from "../agentRow";
import type { AgentRow } from "../snapshot";
import { AgentChildrenPopover } from "./agent-children-popover";
import { Badge } from "./ui/badge";
import { Hint } from "./ui/tooltip";

/**
 * One agent in a list (PRD sidebar-agent-status D-05, B1-B9). Line one is
 * always the stable task name with the status mark, the provider mark, a
 * branch chip when the checkout differs from the parent's, the descendant
 * badge while the descendants are folded, and the elapsed time. The second
 * line follows `agentRow.ts`: a request stays in its warning colour, news is
 * bright until read, and a quiet row reveals its full sentence (two lines,
 * the rest in the tooltip) only while selected or under the pointer.
 *
 * The whole row opens the agent; the chevron and the badge are their own
 * controls beside it, so the badge's list is a real button's popover.
 */
export const AgentRowItem = memo(function AgentRowItem({
  agent,
  device,
  depth,
  descendants,
  childRows,
  selected,
  onOpen,
  onToggleTree,
}: {
  agent: AgentRow;
  device: string | null;
  /** How deep under the root drawn above it; a root is 0. */
  depth: number;
  /** Live descendants among the listed rows. */
  descendants: number;
  /** The direct children still listed, for the badge's popover. */
  childRows: AgentRow[];
  selected: boolean;
  onOpen: (paneId: string) => void;
  /** Null for a list that draws every descendant and so has nothing to fold. */
  onToggleTree: ((paneId: string) => void) | null;
}) {
  const main = useRef<HTMLButtonElement>(null);
  const line = rowLine(agent);
  const branch = branchChip(agent);
  const folded = agent.lineage_collapsed !== false;
  const hasChildren = childRows.length > 0;
  const parts = badgeParts(agent.descendant_counts);
  const attention = agent.group === "needs_you" || agent.unread;
  const titleTone = agent.delegated && !attention ? "text-subtle-foreground" : attention || agent.emphasized ? "text-foreground" : "text-subtle-foreground";
  const label = rowAccessibleName(agent, device);
  const hint = [device ? `${agent.identity_label} · ${device}` : agent.identity_label, line?.text].filter(Boolean).join("\n");
  const shownAtRest = line ? lineShownAtRest(line, selected) : false;
  return (
    <li
      data-pane={agent.pane_id}
      data-attention={attention ? "true" : "false"}
      data-delegated={agent.delegated ? "true" : "false"}
      data-waiting={agent.waiting_on_descendants ? "true" : "false"}
      data-agent-device={device ?? "local"}
      data-depth={depth}
      className={`group/row relative flex items-start gap-xs py-xs pr-md text-body ${selected ? "bg-secondary" : "hover:bg-accent"} focus-within:bg-accent`}
      style={{ paddingLeft: `calc(var(--spacing-xs) + ${depth} * var(--size-lineage-indent))` }}
    >
      <Hint label={hint} reveals>
        <button
          ref={main}
          type="button"
          aria-label={label}
          aria-current={selected ? "true" : undefined}
          data-agent-open={agent.pane_id}
          className="absolute inset-0 outline-none"
          onClick={() => onOpen(agent.pane_id)}
        />
      </Hint>
      {/* A list that draws every descendant (an Overview card) has no fold, so no chevron column. */}
      {onToggleTree ? (
        <span className="relative flex w-(--size-agent-badge-compact) shrink-0 justify-center self-center">
          {hasChildren ? (
            <button
              type="button"
              aria-label={folded ? `Show ${agent.identity_label}'s agents` : `Hide ${agent.identity_label}'s agents`}
              aria-expanded={!folded}
              data-agent-tree-toggle={agent.pane_id}
              className="rounded-xs text-muted-foreground outline-none hover:text-foreground focus-visible:ring-1 focus-visible:ring-ring"
              onClick={() => onToggleTree(agent.pane_id)}
            >
              <ChevronRightIcon className={`size-(--size-icon) transition-transform ${folded ? "" : "rotate-90"}`} />
            </button>
          ) : null}
        </span>
      ) : null}
      <span
        className={`pointer-events-none w-(--size-agent-mark) shrink-0 pt-xxs text-center font-mono text-caption ${markTone(agent)}`}
        data-agent-status-mark={agent.waiting_on_descendants ? "waiting" : agent.status_label}
        aria-hidden="true"
      >
        {agent.symbol}
      </span>
      <AgentMark kind={agent.agent_kind} className="pointer-events-none" />
      {/* The row button's name already reads all of this out. */}
      <span className="pointer-events-none flex min-w-0 flex-1 flex-col" aria-hidden="true">
        <span className="flex min-w-0 items-center gap-xs">
          <span className={`min-w-0 flex-1 truncate ${titleTone} ${attention ? "font-medium" : ""}`}>{agent.identity_label}</span>
          {branch ? (
            <Badge variant="secondary" className="min-w-0 shrink font-mono" data-branch-chip={branch}>
              <span className="truncate">{branch}</span>
            </Badge>
          ) : null}
          {device ? (
            <Badge variant="outline" className="min-w-0 shrink" data-device-chip={device}>
              <span className="truncate">{device}</span>
            </Badge>
          ) : null}
        </span>
        {line ? (
          <span
            data-agent-line={line.mode}
            className={`break-keep text-caption ${lineTone(line, agent.demand)} ${
              shownAtRest ? "" : "hidden group-hover/row:block group-focus-within/row:block"
            } ${selected ? "line-clamp-2 break-words" : "truncate group-hover/row:line-clamp-2 group-hover/row:whitespace-normal group-hover/row:break-words"}`}
          >
            {line.text}
          </span>
        ) : null}
      </span>
      {descendants > 0 && folded ? (
        <AgentChildrenPopover
          parent={agent}
          childRows={childRows}
          onOpenChild={onOpen}
          onUnfold={onToggleTree ? () => onToggleTree(agent.pane_id) : null}
          returnFocus={() => main.current?.focus()}
          trigger={
            <button
              type="button"
              aria-label={badgeLabel(agent.descendant_counts, descendants)}
              aria-haspopup="dialog"
              data-descendant-badge={descendants}
              className="relative shrink-0 rounded-sm outline-none focus-visible:ring-1 focus-visible:ring-ring data-[state=open]:ring-1 data-[state=open]:ring-ring"
            >
              <Badge variant="secondary" className="gap-xs font-mono">
                {parts.length > 0
                  ? parts.map((part) => (
                      <span key={part.state} className="inline-flex items-center gap-xxs" data-badge-part={part.state}>
                        <span className={part.tone}>{part.symbol}</span>
                        {part.count}
                      </span>
                    ))
                  : `↳${descendants}`}
              </Badge>
            </button>
          }
        />
      ) : null}
      <span className="pointer-events-none shrink-0 self-start pt-xxs text-micro text-muted-foreground">{agent.elapsed}</span>
    </li>
  );
});
