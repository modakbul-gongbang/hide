import { agentMarkOf } from "./agentLogos";
import hideMark from "./assets/hide-mark.png";
import { cn } from "./lib/utils";

// The agent's own mark: the logo of each supported agent (agentLogos.ts picks it
// from the adapter, the one rule every surface shares) and a neutral terminal
// mark for a tab of plain shells or an agent Hide does not know. The mark never
// carries the name alone; every caller also gives the full identity in its
// tooltip and label.

/** The panel a badge sits on, so its ring is that panel's colour (the sidebar's, or a Tools column's card). */
export type BadgeGround = "sidebar" | "card";

const GROUND: Record<BadgeGround, string> = { sidebar: "bg-sidebar", card: "bg-card" };

/**
 * Hide's own mark, the owl face at a small size: the head of the Hide AI group
 * in a project's tree, the mark a Hide AI-started row draws alone in the
 * place of the provider logo while logos are hidden (PRD agent-mark-quiet
 * D-04), and (as `HideAiBadge`) the badge a shown logo wears. The full
 * square mark is used at every size (docs/BRAND.md); decorative, because the
 * words `Hide AI` stand beside it or in the row's name.
 */
export function HideMark({ className = "" }: { className?: string }) {
  return <img src={hideMark} alt="" aria-hidden="true" data-hide-mark="true" className={cn("shrink-0 rounded-xs object-contain", className)} />;
}

/** The 11 badge on a provider icon's lower-right corner: Hide AI started this agent (PRD hide-ai-origin-mark D-04). */
function HideAiBadge({ ground }: { ground: BadgeGround }) {
  return (
    <span
      aria-hidden="true"
      data-hide-ai-badge="true"
      className={cn("pointer-events-none absolute -right-(--size-hairline) -bottom-(--size-hairline) flex size-(--size-hide-badge) items-center justify-center rounded-xs", GROUND[ground])}
    >
      <HideMark className="size-(--size-hide-badge-mark)" />
    </span>
  );
}

/**
 * The agent's mark. `hideAi` marks an agent Hide AI started: the provider's
 * icon wears the badge. The badge is part of the row's name through the
 * caller's accessible name, never carried here (D-09).
 */
export function AgentMark({ kind, className = "", hideAi = null }: { kind: string | null | undefined; className?: string; hideAi?: BadgeGround | null }) {
  if (!hideAi) return <PlainMark kind={kind} className={className} />;
  return (
    <span data-agent-mark-badged="true" className="relative inline-flex shrink-0">
      <PlainMark kind={kind} className={className} />
      <HideAiBadge ground={hideAi} />
    </span>
  );
}

function PlainMark({ kind, className }: { kind: string | null | undefined; className: string }) {
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
