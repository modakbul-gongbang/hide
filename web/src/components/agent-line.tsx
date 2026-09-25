import { GitBranchIcon } from "lucide-react";
import { memo } from "react";
import { AgentMark } from "../AgentMark";
import { chipTone } from "../lineage";
import { showsDetail, waitingOnDescendants } from "../projectBoard";
import type { AgentRow } from "../snapshot";
import { cn } from "../lib/utils";
import { Hint } from "./ui/tooltip";

// One agent in an Overview card (web-project-overview D-05, following the
// sidebar-agent-status row rules): the status mark, the provider mark, the
// stable title and the elapsed time on one line; a second line only while
// the agent waits on the operator (warning), changed since it was seen, or
// is the selected pane; a branch chip only for a descendant working in
// another checkout. The whole row is the button that opens the pane.
// Memoized like the sidebar's rows: the store shares an unchanged agent's
// row object across snapshots, so one agent's elapsed tick redraws only it.

export const AgentLine = memo(function AgentLine({
  agent,
  depth,
  foreignBranch,
  selected,
  onOpen,
}: {
  agent: AgentRow;
  depth: number;
  foreignBranch: string | null;
  selected: boolean;
  onOpen: (paneId: string) => void;
}) {
  const waiting = waitingOnDescendants(agent);
  const detail = showsDetail(agent, selected) ? agent.detail : null;
  const asks = agent.group === "needs_you";
  const attention = asks || agent.unread;
  // Somebody else's work is drawn quieter and never loud (docs/status-model.md).
  const tone = agent.delegated && !attention ? "text-muted-foreground" : attention || agent.emphasized ? "text-foreground" : "text-subtle-foreground";
  const help = [agent.identity_label, agent.status_label, agent.detail, foreignBranch].filter(Boolean).join(" · ");
  return (
    <Hint label={help}>
      <button
        type="button"
        onClick={() => onOpen(agent.pane_id)}
        data-overview-agent={agent.pane_id}
        data-agent-depth={depth}
        data-waiting-on-descendants={waiting ? "true" : undefined}
        data-agent-detail={detail ? "true" : undefined}
        aria-current={selected ? "true" : undefined}
        style={depth > 0 ? { paddingInlineStart: `calc(var(--spacing-xs) + ${depth} * var(--home-child-indent))` } : undefined}
        className={cn(
          "relative flex w-full items-start gap-xs rounded-sm px-xs py-xxs text-left outline-none hover:bg-accent focus-visible:bg-accent",
          selected && "bg-secondary",
          tone,
        )}
      >
        {depth > 0 ? <span aria-hidden="true" className="absolute inset-y-0 w-(--size-hairline) bg-border" style={{ insetInlineStart: `calc(${depth} * var(--home-child-indent) - var(--spacing-xxs))` }} /> : null}
        <span
          aria-hidden="true"
          className={cn("w-(--size-agent-mark) shrink-0 text-center font-mono text-body", waiting ? "text-agent-working" : chipTone({ demand: agent.demand ?? "none", activity: agent.activity ?? "", emphasized: agent.emphasized }))}
        >
          {waiting ? "○" : agent.symbol}
        </span>
        <AgentMark kind={agent.agent_kind} />
        <span className="flex min-w-0 flex-1 flex-col gap-xxs">
          <span className="flex min-w-0 items-baseline gap-xs">
            <span className={cn("min-w-0 truncate text-body", attention && "font-medium")}>{agent.identity_label}</span>
            {foreignBranch ? (
              <span className="inline-flex min-w-0 max-w-(--size-pane-child-chip-max) shrink items-center gap-xxs rounded-xs bg-secondary px-xs font-mono text-micro text-subtle-foreground" data-branch-chip="true">
                <GitBranchIcon aria-hidden="true" className="size-(--size-icon-sm) shrink-0" />
                <span className="truncate">{foreignBranch}</span>
              </span>
            ) : null}
            <span className="ml-auto shrink-0 font-mono text-caption text-muted-foreground">{agent.elapsed}</span>
          </span>
          {detail ? (
            <span className={cn("line-clamp-2 break-words text-caption", asks ? (agent.demand === "error" ? "text-destructive" : "text-warning") : agent.unread ? "text-foreground" : "text-subtle-foreground")}>
              {detail}
            </span>
          ) : null}
        </span>
      </button>
    </Hint>
  );
});
