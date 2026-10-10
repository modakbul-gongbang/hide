import { agentMarkOf } from "./agentLogos";

// The agent's own mark: the logo of each supported agent (agentLogos.ts picks it
// from the adapter, the one rule every surface shares) and a neutral terminal
// mark for a tab of plain shells or an agent Hide does not know. The mark never
// carries the name alone; every caller also gives the full identity in its
// tooltip and label.

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
