import type { HTMLAttributes, ReactNode } from "react";
import { useInterfaceTranslation } from "../i18n/client";
import { requireInterfaceLanguage } from "../i18n/locale";
import { cn } from "../lib/utils";
import { agentMark, agentStaleness, ownPulls, PR_LOOK, reviewWord, staleLabel, type PrStaleness } from "../prMark";
import type { PrChip } from "../projectBoard";
import type { AgentPullRequest, AgentRow, PrState } from "../snapshot";
import { useShellStore } from "../store";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuLabel, DropdownMenuTrigger } from "./ui/dropdown-menu";
import { Hint, Tooltip, TooltipContent, TooltipTrigger, useHintOpen } from "./ui/tooltip";

/**
 * The one PR mark (docs/UI_BEHAVIOR.md, PR mark): the state's icon in its
 * colour and the number, `+N` for the other PRs the mark stands for. No
 * border; the icon's colour is the state. `compact` is the same mark without
 * the number, where a row already names the pull request (a checkout's kind
 * glyph, a search row's icon). A stale mark dims.
 */
export function PrMark({
  state,
  number,
  more = 0,
  stale = false,
  compact = false,
  className,
  ...rest
}: { state: PrState; number?: number; more?: number; stale?: boolean; compact?: boolean } & HTMLAttributes<HTMLSpanElement>) {
  const { icon: Icon, tone } = PR_LOOK[state];
  return (
    <span
      data-pr-mark={state}
      data-stale={stale ? "true" : undefined}
      className={cn("inline-flex shrink-0 items-center gap-xxs font-mono text-caption", stale && "opacity-(--opacity-dimmed)", className)}
      {...rest}
    >
      <Icon aria-hidden="true" className={cn("size-(--size-pr-icon) shrink-0", tone)} />
      {compact || number === undefined ? null : <span className="text-subtle-foreground">#{number}</span>}
      {compact || more === 0 ? null : <span className="text-muted-foreground">+{more}</span>}
    </span>
  );
}

/**
 * A checkout's pull request on a card, a lens or a cleanup row: the mark and,
 * while a reviewer asks for changes, that one word in the failed colour; the
 * mark's colour says the rest.
 */
export function PrChipMark({ chip, stale = false }: { chip: PrChip; stale?: boolean }) {
  const { t } = useInterfaceTranslation();
  return (
    <span className="inline-flex shrink-0 items-center gap-xs">
      <PrMark state={chip.state} number={chip.number} stale={stale} />
      {chip.changesRequested ? (
        <span className={cn("font-sans text-caption", CHANGES.tone)} data-pr-review="changes_requested">
          {t(CHANGES.key)}
        </span>
      ) : null}
    </span>
  );
}

const CHANGES = reviewWord("changes_requested")!;

/** The GitHub read behind a row's own PRs, read as two plain values so a snapshot that moves neither renders nothing. */
export function useAgentStaleness(agent: AgentRow): PrStaleness | undefined {
  const stale = useShellStore((s) => agentStaleness(s.rest, agent)?.stale);
  const lastRead = useShellStore((s) => agentStaleness(s.rest, agent)?.lastRead);
  return stale === undefined ? undefined : { stale, lastRead: lastRead ?? null };
}

/** The words a mark reads out: its number and state, how many more, and when GitHub was last read. */
function useMarkLabel(state: PrState, number: number, more: number, stale: PrStaleness | undefined): string {
  const { t, i18n } = useInterfaceTranslation();
  const head = `#${number} ${t(PR_LOOK[state].word)}`;
  return [more > 0 ? `${head} +${more}` : head, staleLabel(stale, t, requireInterfaceLanguage(i18n.language))].filter(Boolean).join(" · ");
}

/** The class that makes a mark pressable: the fill under the pointer, the focus ring. */
export const PR_MARK_PRESS = "flex shrink-0 items-center rounded-xs px-xxs outline-none hover:bg-accent focus-visible:ring-1 focus-visible:ring-ring data-[state=open]:bg-secondary disabled:opacity-(--opacity-disabled)";

/**
 * An agent row's own PRs as one mark (Sessions rows, the pane header):
 * pressing it lists each PR, worst first; choosing one opens the PR panel.
 */
export function AgentPrMark({ agent, staleness, disabled = false, onOpen }: { agent: AgentRow; staleness?: PrStaleness; disabled?: boolean; onOpen: (pull: AgentPullRequest) => void }) {
  const mark = agentMark(agent);
  const label = useMarkLabel(mark?.state ?? "pending", mark?.number ?? 0, mark?.more ?? 0, staleness);
  if (!mark) return null;
  return (
    <DropdownMenu>
      <Hint label={label}>
        <DropdownMenuTrigger asChild>
          <button type="button" disabled={disabled} aria-label={label} data-agent-pr={agent.pane_id} onClick={(event) => event.stopPropagation()} className={PR_MARK_PRESS}>
            <PrMark state={mark.state} number={mark.number} more={mark.more} stale={staleness?.stale} />
          </button>
        </DropdownMenuTrigger>
      </Hint>
      <DropdownMenuContent align="start" className="w-(--size-pr-popover)" onClick={(event) => event.stopPropagation()} data-pr-list={agent.pane_id}>
        <PrListHead agent={agent} />
        {ownPulls(agent).map(({ pull, state }) => (
          <DropdownMenuItem key={pull.url} onSelect={() => onOpen(pull)} data-pr-list-item={pull.number}>
            <PrListItem pull={pull} state={state} />
          </DropdownMenuItem>
        ))}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

function PrListHead({ agent }: { agent: AgentRow }) {
  const { t } = useInterfaceTranslation();
  return (
    <DropdownMenuLabel className="flex min-w-0 items-baseline gap-xs">
      <span className="shrink-0">{t("agentSessions.pr.count", { count: agent.state.pr?.count ?? 0 })}</span>
      <span className="min-w-0 truncate font-normal text-muted-foreground">{agent.identity_label}</span>
    </DropdownMenuLabel>
  );
}

function PrListItem({ pull, state }: { pull: AgentPullRequest; state: PrState }) {
  return (
    <>
      <PrMark state={state} number={pull.number} />
      <span className="min-w-0 flex-1 truncate" title={pull.title}>
        {pull.title}
      </span>
    </>
  );
}

/**
 * The sidebar row's and the tree popover's mark: the same mark, not a menu.
 * Under the pointer `card` draws what it opens (the PR card for one PR, the
 * list for several); with none, a stale mark still says when GitHub was read.
 */
export function AgentPrHover({ agent, staleness, card }: { agent: AgentRow; staleness?: PrStaleness; card?: (mark: ReactNode) => ReactNode }) {
  const mark = agentMark(agent);
  const label = useMarkLabel(mark?.state ?? "pending", mark?.number ?? 0, mark?.more ?? 0, staleness);
  if (!mark) return null;
  const node = <PrMark role="img" aria-label={label} data-agent-pr={agent.pane_id} state={mark.state} number={mark.number} more={mark.more} stale={staleness?.stale} />;
  if (card) return <>{card(node)}</>;
  return staleness?.stale ? <Hint label={label}>{node}</Hint> : node;
}

/**
 * Under the pointer, each of a row's own PRs with its mark and title, and
 * when GitHub was last read if it cannot be read now. Choosing a PR opens it;
 * the list stays open while the pointer moves into it.
 */
export function PrHoverList({ agent, staleness, onOpen, children }: { agent: AgentRow; staleness?: PrStaleness; onOpen: (pull: AgentPullRequest) => void; children: ReactNode }) {
  const { t, i18n } = useInterfaceTranslation();
  const { open, onOpenChange, triggerProps } = useHintOpen();
  const stale = staleLabel(staleness, t, requireInterfaceLanguage(i18n.language));
  return (
    <Tooltip open={open} onOpenChange={onOpenChange} disableHoverableContent={false}>
      <TooltipTrigger asChild {...triggerProps}>
        {children}
      </TooltipTrigger>
      <TooltipContent side="right" align="start" aria-label={t("agentSessions.pr.list", { name: agent.identity_label })} className="pointer-events-auto flex w-(--size-pr-popover) flex-col gap-xxs rounded-md p-xs text-left" data-pr-list={agent.pane_id}>
        {ownPulls(agent).map(({ pull, state }) => (
          <button key={pull.url} type="button" onClick={() => onOpen(pull)} data-pr-list-item={pull.number} className="flex min-w-0 items-center gap-xs rounded-xs px-xs py-xxs text-left outline-none hover:bg-accent focus-visible:ring-1 focus-visible:ring-ring">
            <PrListItem pull={pull} state={state} />
          </button>
        ))}
        {stale ? <span className="px-xs text-caption text-muted-foreground">{stale}</span> : null}
      </TooltipContent>
    </Tooltip>
  );
}
