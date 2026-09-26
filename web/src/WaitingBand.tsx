import { ChevronRightIcon } from "lucide-react";
import { markTone, rowAccessibleName, rowLine } from "./agentRow";
import { StatusMark } from "./components/status-mark";
import { cn } from "./lib/utils";
import type { WaitingRow } from "./projectBoard";

/**
 * The waiting band (PRD task-agents-views D-11, B10): between a scope's
 * header and its tabs, one row per agent that waits on the operator, and
 * nothing at all when none does. A row is its mark, where the agent works
 * (mono), its request in the foreground (the mark carries the tone), its
 * age and `>`; the whole row opens the agent's pane
 * as one event, and the answer is given there. There is no label, hint or
 * separate open button.
 */
export function WaitingBand({ rows, onOpen }: { rows: WaitingRow[]; onOpen: (paneId: string) => void }) {
  if (rows.length === 0) return null;
  return (
    <ul className="flex flex-col gap-xxs" aria-label="기다리는 것" data-waiting-band={rows.length}>
      {rows.map(({ agent, place, where }) => {
        const line = rowLine(agent);
        return (
          <li key={`${place.projectId}:${agent.pane_id}`}>
            <button
              type="button"
              aria-label={rowAccessibleName(agent, null)}
              onClick={() => onOpen(agent.pane_id)}
              className="flex w-full min-w-0 items-center gap-sm rounded-md bg-card px-md py-xs text-left outline-none hover:bg-accent focus-visible:ring-1 focus-visible:ring-ring"
              data-waiting-row={agent.pane_id}
              data-waiting-group={agent.group}
            >
              <StatusMark symbol={agent.symbol} className={cn("shrink-0", markTone(agent))} />
              <span className="min-w-0 max-w-1/2 shrink-0 truncate font-mono text-caption text-subtle-foreground" data-waiting-where="true">
                {where}
              </span>
              <span className={cn("min-w-0 flex-1 truncate text-body", line ? "text-foreground" : "text-muted-foreground")} data-waiting-line="true">
                {line?.text ?? agent.status_label}
              </span>
              <span className="shrink-0 font-mono text-micro text-muted-foreground">{agent.elapsed}</span>
              <ChevronRightIcon aria-hidden="true" className="size-(--size-icon) shrink-0 text-muted-foreground" />
            </button>
          </li>
        );
      })}
    </ul>
  );
}
