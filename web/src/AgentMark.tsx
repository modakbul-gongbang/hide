import { agentMarkOf } from "./agentLogos";
import hideMark from "./assets/hide-mark.png";
import { cn } from "./lib/utils";

// The agent's own mark: the logo of each supported agent (agentLogos.ts picks it
// from the adapter, the one rule every surface shares) and a neutral terminal
// mark for a tab of plain shells or an agent Hide does not know. The mark never
// carries the name alone; every caller also gives the full identity in its
// tooltip and label.

/**
 * Hide's own mark, the owl face at a small size: the mark a Hide AI-started
 * row draws beside its status mark in the place of the provider logo (PRD agent-mark-quiet D-04). The full
 * square mark is used at every size (docs/BRAND.md); decorative, because the
 * words `Hide AI` stand beside it or in the row's name.
 */
export function HideMark({ className = "" }: { className?: string }) {
  return <img src={hideMark} alt="" aria-hidden="true" data-hide-mark="true" className={cn("shrink-0 rounded-xs object-contain", className)} />;
}

/**
 * The agent's mark. A list row does not draw it: the status mark leads and the
 * logo stays on the pane's own header (PRD agent-mark-quiet), so a row names its
 * agent in its tooltip and accessible name instead.
 */
export function AgentMark({ kind, className = "" }: { kind: string | null | undefined; className?: string }) {
  const mark = agentMarkOf(kind);
  if (mark) {
    return (
      <img
        src={mark.logo}
        alt=""
        aria-hidden="true"
        data-agent-mark={mark.id}
        className={`h-[var(--size-agent-badge-compact)] w-[var(--size-agent-badge-compact)] shrink-0 rounded-xs object-contain ${mark.plated ? "bg-(--logo-plate) p-xxs" : ""} ${className}`}
      />
    );
  }
  return (
    <span
      aria-hidden="true"
      data-agent-mark="neutral"
      className={`flex h-[var(--size-agent-badge-compact)] w-[var(--size-agent-badge-compact)] shrink-0 items-center justify-center rounded-xs bg-secondary font-mono text-micro text-subtle-foreground ${className}`}
    >
      {">_"}
    </span>
  );
}
