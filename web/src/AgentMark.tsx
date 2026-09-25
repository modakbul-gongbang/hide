import claudeMark from "./assets/agent-claude.png";
import codexMark from "./assets/agent-codex.png";
import { knownProvider } from "./workspace";

// The agent's own mark (PRD S6 D-09, B17): the provider artwork the macOS
// shell bundles for the providers Hide knows, and a neutral terminal mark for
// any other agent or a tab of plain shells. The mark never carries the name
// alone; every caller also gives the full identity in its tooltip and label.

const MARKS = { claude: claudeMark, codex: codexMark } as const;

export function AgentMark({ kind, className = "" }: { kind: string | null | undefined; className?: string }) {
  const provider = knownProvider(kind);
  if (provider) {
    return (
      <img
        src={MARKS[provider]}
        alt=""
        aria-hidden="true"
        data-agent-mark={provider}
        className={`h-[var(--size-agent-badge-compact)] w-[var(--size-agent-badge-compact)] shrink-0 rounded-xs ${className}`}
      />
    );
  }
  return (
    <span
      aria-hidden="true"
      data-agent-mark="neutral"
      className={`flex h-[var(--size-agent-badge-compact)] w-[var(--size-agent-badge-compact)] shrink-0 items-center justify-center rounded-xs bg-elevated font-mono text-micro text-secondary ${className}`}
    >
      {">_"}
    </span>
  );
}
