import { GitForkIcon } from "lucide-react";
import { forkLabel } from "../agentRow";
import { useInterfaceTranslation } from "../i18n/client";
import type { AgentRow } from "../snapshot";
import { Hint } from "./ui/tooltip";

/**
 * A fork's mark on its Sessions and sidebar rows (PRD fork-operator-root B8): the
 * fork icon after the title, naming where it was forked from under the pointer.
 * The row's own name reads the same words, so the mark itself is not announced.
 */
export function AgentForkMark({ agent }: { agent: Pick<AgentRow, "pane_id" | "forked_from"> }) {
  const { t } = useInterfaceTranslation();
  const label = forkLabel(t, agent);
  if (!label) return null;
  return (
    <Hint label={label}>
      <span className="flex shrink-0 items-center text-subtle-foreground" aria-hidden="true" data-agent-fork={agent.pane_id}>
        <GitForkIcon className="size-(--size-icon-sm)" />
      </span>
    </Hint>
  );
}
