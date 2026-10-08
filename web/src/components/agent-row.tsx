import { badgeLabel, badgeParts } from "../agentRow";
import { useRef } from "react";
import { useInterfaceTranslation } from "../i18n/client";
import type { AgentRow } from "../snapshot";
import { AgentChildrenPopover } from "./agent-children-popover";
import { BadgeMarks } from "./status-badge";
import { Badge } from "./ui/badge";

/**
 * A parent's badge: one mark and count per state over its direct children,
 * worst first, or `↳N` when all are merely ready. The sidebar, Sessions and
 * pane header share this button and the direct-child popover it opens.
 */
export function DescendantBadge({
  agent,
  descendants,
  childRows,
  onOpenChild,
  onUnfold,
  returnFocus,
}: {
  agent: AgentRow;
  descendants: number;
  childRows: AgentRow[];
  onOpenChild: (paneId: string) => void;
  onUnfold: (() => void) | null;
  returnFocus: () => void;
}) {
  const { t } = useInterfaceTranslation();
  const trigger = useRef<HTMLButtonElement>(null);
  const parts = badgeParts(agent.direct_child_counts ?? agent.descendant_counts);
  return (
    <AgentChildrenPopover
      parent={agent}
      childRows={childRows}
      onOpenChild={onOpenChild}
      onUnfold={onUnfold}
      returnFocus={() => trigger.current ? trigger.current.focus() : returnFocus()}
      triggerLabel={badgeLabel(agent.direct_child_counts ?? agent.descendant_counts, descendants, t)}
      trigger={
        <button
          ref={trigger}
          type="button"
          aria-label={badgeLabel(agent.direct_child_counts ?? agent.descendant_counts, descendants, t)}
          aria-haspopup="dialog"
          data-descendant-badge={descendants}
          className="relative shrink-0 rounded-sm outline-none focus-visible:ring-1 focus-visible:ring-ring data-[state=open]:ring-1 data-[state=open]:ring-ring"
        >
          <Badge variant="secondary" className="gap-xs font-mono">
            {parts.length > 0 ? <BadgeMarks parts={parts} /> : `↳${descendants}`}
          </Badge>
        </button>
      }
    />
  );
}
