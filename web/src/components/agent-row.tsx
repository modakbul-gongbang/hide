import { badgeLabel, badgeParts } from "../agentRow";
import { useInterfaceTranslation } from "../i18n/client";
import type { AgentRow } from "../snapshot";
import { AgentChildrenPopover } from "./agent-children-popover";
import { BadgeMarks } from "./status-badge";
import { Badge } from "./ui/badge";

/**
 * A folded parent's badge (docs/status-model.md, The descendant badge): one
 * mark and count per state over every live descendant, worst first, or `↳N`
 * when all of them are merely ready. It is a button whose popover lists the
 * direct children; every list of agents that folds draws this one.
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
  const parts = badgeParts(agent.direct_child_counts ?? agent.descendant_counts);
  return (
    <AgentChildrenPopover
      parent={agent}
      childRows={childRows}
      onOpenChild={onOpenChild}
      onUnfold={onUnfold}
      returnFocus={returnFocus}
      triggerLabel={badgeLabel(agent.direct_child_counts ?? agent.descendant_counts, descendants, t)}
      trigger={
        <button
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
