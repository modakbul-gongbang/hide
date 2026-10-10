import { ChevronDownIcon, ChevronRightIcon, GitMergeIcon, GitPullRequestIcon, ListTreeIcon } from "lucide-react";
import type { TFunction } from "i18next";
import type { ReactNode } from "react";
import { useInterfaceTranslation } from "../i18n/client";
import { formatDateTime } from "../i18n/format";
import { requireInterfaceLanguage } from "../i18n/locale";
import { cn } from "../lib/utils";
import { catalogWorkspaces, type AgentPullRequest, type AgentRow, type PrState, type RaiseVerb, type SnapshotRest } from "../snapshot";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuLabel, DropdownMenuTrigger } from "./ui/dropdown-menu";
import { Hint, Tooltip, TooltipContent, TooltipTrigger, useHintOpen } from "./ui/tooltip";

// The parts every agent tree draws the same way (PRD agent-hierarchy-screens
// D-37 to D-43): the verb a Needs You row starts with, a row's own PRs as a
// chip or a sidebar icon, the one descendant mark a folded parent wears, the
// elbow rails and the chevron lane of a tree row, and the tree button. The
// core decides every value; these only draw it.

/** 승인 · 답변 · 확인 · 초안: colored text, no box (D-37). */
export function VerbText({ verb }: { verb: RaiseVerb }) {
  const { t } = useInterfaceTranslation();
  return (
    <span className="shrink-0 font-semibold text-warning" data-verb={verb}>
      {t(`agentSessions.verb.${verb}`)}
    </span>
  );
}

/** What to do, or the verb's own words when no label line or letter says it (B3). */
export function askWhat(t: TFunction<"translation">, verb: RaiseVerb, what: string | null | undefined): string {
  return what ?? t(`agentSessions.ask.${verb}`);
}

/** A Needs You row's second line: the verb and what to do, nothing else (B2). */
export function AskLine({ ask, className }: { ask: NonNullable<AgentRow["state"]["ask"]>; className?: string }) {
  const { t } = useInterfaceTranslation();
  const what = askWhat(t, ask.verb, ask.what);
  return (
    <span className={cn("flex min-w-0 items-baseline gap-xs text-caption", className)} data-ask-line={ask.verb}>
      <VerbText verb={ask.verb} />
      <span className="min-w-0 truncate text-warning" title={what}>
        {what}
      </span>
    </span>
  );
}

const PR_ICON_TONE: Record<PrState, string> = {
  failed: "text-destructive",
  pending: "text-pr-pending",
  mergeable: "text-success",
  merged: "text-pr-merged",
};
const PR_GLYPH: Record<PrState, { glyph: string; tone: string }> = {
  failed: { glyph: "✕", tone: "text-destructive" },
  pending: { glyph: "◷", tone: "text-muted-foreground" },
  mergeable: { glyph: "✓", tone: "text-success" },
  merged: { glyph: "⇥", tone: "text-pr-merged" },
};

/** The row's own PRs, worst first, as the core ordered them. */
export function ownPulls(agent: AgentRow): { pull: AgentPullRequest; state: PrState }[] {
  const pulls = agent.request?.pull_requests ?? [];
  return (agent.state.pr?.pulls ?? []).map(({ index, state }) => {
    const pull = pulls[index];
    if (!pull) throw new Error("Own PR summary points past the row's pull requests");
    return { pull, state };
  });
}

/** When GitHub could not be read again, the last read time a PR mark dims for (B24). */
export type PrStaleness = { stale: boolean; lastRead: number | null };

/** The GitHub read state of the project that holds the row's own PRs. */
export function prStaleness(rest: SnapshotRest | null, agent: AgentRow): PrStaleness | undefined {
  const urls = new Set(ownPulls(agent).map(({ pull }) => pull.url));
  if (urls.size === 0) return undefined;
  const project = catalogWorkspaces(rest).find((row) => row.pull_requests?.some((pull) => urls.has(pull.url)));
  const github = project?.checkouts.find((checkout) => checkout.github)?.github;
  return github ? { stale: github.stale === true, lastRead: github.last_success_at_unix_ms ?? null } : undefined;
}

function useStaleLabel(staleness: PrStaleness | undefined): string | null {
  const { t, i18n } = useInterfaceTranslation();
  if (!staleness?.stale) return null;
  const time = staleness.lastRead == null ? "-" : formatDateTime(requireInterfaceLanguage(i18n.language), staleness.lastRead, { dateStyle: "short", timeStyle: "short" });
  return t("agentSessions.pr.stale", { time });
}

function StateGlyph({ state }: { state: PrState }) {
  const { glyph, tone } = PR_GLYPH[state];
  return (
    <span aria-hidden="true" className={cn("font-semibold", tone)}>
      {glyph}
    </span>
  );
}

/**
 * The PR chip of Sessions and the pane header (B22): one PR is its number and
 * state; several are `PR n` and the worst state with its count. Pressing it
 * lists each PR with its state and title; choosing one opens the PR panel.
 */
export function PrChip({ agent, staleness, disabled = false, onOpen }: { agent: AgentRow; staleness?: PrStaleness; disabled?: boolean; onOpen: (pull: AgentPullRequest) => void }) {
  const { t } = useInterfaceTranslation();
  const summary = agent.state.pr;
  const staleLabel = useStaleLabel(staleness);
  if (!summary) return null;
  const pulls = ownPulls(agent);
  const single = pulls.length === 1 ? pulls[0]! : null;
  const label = single
    ? `#${single.pull.number} ${t(`agentSessions.pr.${single.state}`)}`
    : `${t("agentSessions.pr.count", { count: summary.count })}, ${t(`agentSessions.pr.${summary.worst}`)} ${summary.worst_count}`;
  return (
    <DropdownMenu>
      <Hint label={[label, staleLabel].filter(Boolean).join("\n")}>
        <DropdownMenuTrigger asChild>
          <button
            type="button"
            disabled={disabled}
            aria-label={[label, staleLabel].filter(Boolean).join(", ")}
            data-pr-chip={single ? single.pull.number : `n${summary.count}`}
            data-pr-state={summary.worst}
            data-stale={staleness?.stale ? "true" : undefined}
            onClick={(event) => event.stopPropagation()}
            className={cn(
              "flex h-(--size-sidebar-line-detail) shrink-0 items-center gap-xxs rounded-xs border border-border px-xs font-mono text-caption text-subtle-foreground outline-none hover:bg-accent focus-visible:ring-1 focus-visible:ring-ring data-[state=open]:bg-secondary disabled:opacity-50",
              staleness?.stale && "opacity-(--opacity-dimmed)",
            )}
          >
            <span className={summary.worst === "merged" && single ? "text-muted-foreground" : undefined}>{single ? `#${single.pull.number}` : t("agentSessions.pr.count", { count: summary.count })}</span>
            <StateGlyph state={summary.worst} />
            {single ? null : <span className={PR_GLYPH[summary.worst].tone}>{summary.worst_count}</span>}
          </button>
        </DropdownMenuTrigger>
      </Hint>
      <PrList agent={agent} pulls={pulls} onOpen={onOpen} />
    </DropdownMenu>
  );
}

function PrList({ agent, pulls, onOpen }: { agent: AgentRow; pulls: ReturnType<typeof ownPulls>; onOpen: (pull: AgentPullRequest) => void }) {
  const { t } = useInterfaceTranslation();
  return (
    <DropdownMenuContent align="start" className="w-(--size-pr-popover)" onClick={(event) => event.stopPropagation()} data-pr-list={agent.pane_id}>
      <DropdownMenuLabel className="flex min-w-0 items-baseline gap-xs">
        <span className="shrink-0">{t("agentSessions.pr.count", { count: pulls.length })}</span>
        <span className="min-w-0 truncate font-normal text-muted-foreground">{agent.identity_label}</span>
      </DropdownMenuLabel>
      {pulls.map(({ pull, state }) => (
        <DropdownMenuItem key={pull.url} onSelect={() => onOpen(pull)} data-pr-list-item={pull.number}>
          <span className="flex shrink-0 items-center gap-xxs rounded-xs border border-border px-xs font-mono text-caption text-subtle-foreground">
            #{pull.number}
            <StateGlyph state={state} />
          </span>
          <span className="min-w-0 flex-1 truncate" title={pull.title}>
            {pull.title}
          </span>
        </DropdownMenuItem>
      ))}
    </DropdownMenuContent>
  );
}

/**
 * The sidebar's PR mark (D-39): one icon, no number, in the worst state's
 * color, dimmed while GitHub cannot be read. Under the pointer it shows the
 * card `card` draws (the existing PR card for one PR, the list for several).
 */
export function PrIcon({ agent, staleness, card }: { agent: AgentRow; staleness?: PrStaleness; card?: (icon: ReactNode) => ReactNode }) {
  const { t } = useInterfaceTranslation();
  const staleLabel = useStaleLabel(staleness);
  const summary = agent.state.pr;
  if (!summary) return null;
  const Icon = summary.worst === "merged" ? GitMergeIcon : GitPullRequestIcon;
  const icon = (
    <span
      role="img"
      aria-label={[t(`agentSessions.pr.${summary.worst}`), staleLabel].filter(Boolean).join(", ")}
      data-pr-icon={summary.worst}
      data-stale={staleness?.stale ? "true" : undefined}
      className={cn("inline-flex shrink-0 items-center", PR_ICON_TONE[summary.worst], staleness?.stale && "opacity-(--opacity-dimmed)")}
    >
      <Icon aria-hidden="true" className="size-(--size-pr-icon)" />
    </span>
  );
  if (card) return <>{card(icon)}</>;
  // With no card of its own, a stale icon still says when GitHub was last read (B24).
  return staleLabel ? <Hint label={staleLabel}>{icon}</Hint> : icon;
}

/**
 * Under the pointer, each of a row's own PRs with its state and title, and
 * when GitHub was last read if it cannot be read now (B23, B24). Choosing a
 * PR opens it; the list stays open while the pointer moves into it.
 */
export function PrHoverList({ agent, staleness, onOpen, children }: { agent: AgentRow; staleness?: PrStaleness; onOpen: (pull: AgentPullRequest) => void; children: ReactNode }) {
  const { t } = useInterfaceTranslation();
  const staleLabel = useStaleLabel(staleness);
  const { open, onOpenChange, triggerProps } = useHintOpen();
  const pulls = ownPulls(agent);
  return (
    <Tooltip open={open} onOpenChange={onOpenChange} disableHoverableContent={false}>
      <TooltipTrigger asChild {...triggerProps}>
        {children}
      </TooltipTrigger>
      <TooltipContent side="right" align="start" aria-label={t("agentSessions.pr.list", { name: agent.identity_label })} className="pointer-events-auto flex w-(--size-pr-popover) flex-col gap-xxs rounded-md p-xs text-left" data-pr-list={agent.pane_id}>
        {pulls.map(({ pull, state }) => (
          <button key={pull.url} type="button" onClick={() => onOpen(pull)} data-pr-list-item={pull.number} className="flex min-w-0 items-center gap-xs rounded-xs px-xs py-xxs text-left outline-none hover:bg-accent focus-visible:ring-1 focus-visible:ring-ring">
            <span className="flex shrink-0 items-center gap-xxs rounded-xs border border-border px-xs font-mono text-caption text-subtle-foreground">
              #{pull.number}
              <StateGlyph state={state} />
            </span>
            <span className="min-w-0 flex-1 truncate" title={pull.title}>
              {pull.title}
            </span>
          </button>
        ))}
        {staleLabel ? <span className="px-xs text-caption text-muted-foreground">{staleLabel}</span> : null}
      </TooltipContent>
    </Tooltip>
  );
}

/** The one mark a folded parent wears for its descendants (B17): `! N` raised, else `● N` working. */
export function DescendantMark({ agent }: { agent: AgentRow }) {
  const { t } = useInterfaceTranslation();
  const mark = agent.descendant_mark;
  if (!mark) return null;
  const raised = mark.kind === "raised";
  return (
    <span
      role="img"
      aria-label={t(raised ? "agentSessions.tree.raised" : "agentSessions.tree.working", { count: mark.count })}
      data-descendant-mark={mark.kind}
      className={cn("inline-flex shrink-0 items-center gap-xxs font-mono text-caption font-semibold", raised ? "text-warning" : "text-agent-working")}
    >
      {raised ? <span aria-hidden="true">!</span> : <span aria-hidden="true" className="size-(--size-status-mark) rounded-full bg-current" />}
      <span aria-hidden="true">{mark.count}</span>
    </span>
  );
}

/**
 * Where a drawn tree row sits: its depth and, per level above it, whether a
 * rail runs past it. `chain` marks a row below an indent limit (`cappedPlaces`).
 */
export type TreePlace = { depth: number; last: boolean; rails: boolean[]; chain?: TreeChain };

/**
 * A row below the indent limit keeps the limit's column: `pass` says whether
 * the rail of its ancestor at the limit runs on past it, and its chevron lane
 * takes its parent's rail from above and hands it on down when `on`.
 */
export type TreeChain = { pass: boolean; on: boolean };

/**
 * The rails of a flattened tree in display order: each row is the last of
 * its siblings when no later row at its depth comes before a shallower one,
 * and an ancestor level carries a rail while that ancestor has a later sibling.
 */
export function treePlaces(depths: readonly number[]): TreePlace[] {
  const last = depths.map((depth, index) => {
    for (let next = index + 1; next < depths.length; next++) {
      if (depths[next]! < depth) return true;
      if (depths[next] === depth) return false;
    }
    return true;
  });
  const open: boolean[] = [];
  return depths.map((depth, index) => {
    open.length = depth;
    const rails = open.slice(1, depth);
    open[depth] = !last[index];
    return { depth, last: last[index]!, rails };
  });
}

/**
 * `treePlaces` for a panel too narrow for every level: a row deeper than
 * `limit` stands in the limit's column and hangs from its parent's chevron
 * lane, so the levels below the limit read as one rail running down.
 */
export function cappedPlaces(depths: readonly number[], limit: number): TreePlace[] {
  return treePlaces(depths).map((place, index) => place.depth <= limit ? place : {
    depth: limit,
    last: place.last,
    rails: place.rails.slice(0, limit - 1),
    chain: { pass: place.rails[limit - 1] ?? false, on: (depths[index + 1] ?? 0) > limit },
  });
}

/**
 * The rails left of a tree row (D-38): a rail down each level whose ancestor
 * has a later sibling, then this row's elbow from its parent's chevron lane.
 * No tint is laid behind an opened subtree.
 */
export function TreeRails({ place }: { place: TreePlace }) {
  if (place.depth === 0) return null;
  return (
    <span aria-hidden="true" className="flex shrink-0 self-stretch" data-tree-rails={place.depth}>
      {place.rails.map((rail, index) => (
        <span key={index} className="relative w-(--size-lineage-indent) shrink-0">
          {rail ? <span className="absolute inset-y-0 left-(--size-lineage-rail-x) w-(--size-hairline) bg-lineage-rail" /> : null}
        </span>
      ))}
      {place.chain ? (
        <span className="relative w-(--size-lineage-indent) shrink-0" data-tree-pass={place.chain.pass ? "rail" : "none"}>
          {place.chain.pass ? <span className="absolute inset-y-0 left-(--size-lineage-rail-x) w-(--size-hairline) bg-lineage-rail" /> : null}
        </span>
      ) : (
        <span className="relative w-(--size-lineage-indent) shrink-0" data-tree-elbow={place.last ? "last" : "middle"}>
          <span className={cn("absolute top-0 left-(--size-lineage-rail-x) w-(--size-hairline) bg-lineage-rail", place.last ? "h-(--size-lineage-elbow-y)" : "bottom-0")} />
          <span className="absolute top-(--size-lineage-elbow-y) right-0 left-(--size-lineage-rail-x) h-(--size-hairline) bg-lineage-rail" />
        </span>
      )}
    </span>
  );
}

/**
 * The chevron lane left of a tree row's mark: a chevron where the row has
 * children, and while it is open a rail leaving it down to the children.
 * A row with none keeps the lane, crossed by its elbow when it hangs from a parent.
 */
export function TreeChevron({ name, open, onToggle, hangs, chain, disabled = false, popup = false, ...data }: { name: string; open: boolean | null; onToggle?: () => void; hangs: boolean; chain?: TreeChain; disabled?: boolean; popup?: boolean } & Record<`data-${string}`, string>) {
  // The lane spans the row; its chevron sits on line one, below the row's top padding.
  const { t } = useInterfaceTranslation();
  if (open === null || !onToggle) {
    return (
      <span aria-hidden="true" className="relative w-(--size-lineage-chevron) shrink-0 self-stretch">
        {chain ? <>
          {/* Below the indent limit the parent's rail comes down this lane and turns to the mark. */}
          <span className={cn("absolute top-0 left-(--size-lineage-rail-x) w-(--size-hairline) bg-lineage-rail", chain.on ? "bottom-0" : "h-(--size-lineage-elbow-y)")} />
          <span className="absolute top-(--size-lineage-elbow-y) right-xxs left-(--size-lineage-rail-x) h-(--size-hairline) bg-lineage-rail" />
        </> : hangs ? <span className="absolute top-(--size-lineage-elbow-y) right-xxs left-none h-(--size-hairline) bg-lineage-rail" /> : null}
      </span>
    );
  }
  const label = t(open ? "agentSessions.tree.collapse" : "agentSessions.tree.expand", { name });
  return (
    <span className="relative w-(--size-lineage-chevron) shrink-0 self-stretch">
      {chain ? <span aria-hidden="true" className="absolute top-0 left-(--size-lineage-rail-x) h-xs w-(--size-hairline) bg-lineage-rail" /> : null}
      {open || chain?.on ? <span aria-hidden="true" className="absolute top-(--size-lineage-departure-y) bottom-0 left-(--size-lineage-rail-x) w-(--size-hairline) bg-lineage-rail" /> : null}
      <button
        type="button"
        tabIndex={-1}
        aria-label={label}
        aria-expanded={popup ? undefined : open}
        aria-haspopup={popup ? "dialog" : undefined}
        disabled={disabled}
        onClick={(event) => {
          event.stopPropagation();
          onToggle();
        }}
        className="relative z-10 mt-xs flex h-(--size-sidebar-line) w-full items-center justify-center rounded-xs text-muted-foreground outline-none hover:text-foreground focus-visible:ring-1 focus-visible:ring-ring"
        {...data}
      >
        {open ? <ChevronDownIcon aria-hidden="true" className="size-(--size-icon-sm)" /> : <ChevronRightIcon aria-hidden="true" className="size-(--size-icon-sm)" />}
      </button>
    </span>
  );
}

/** The pane header's child button (B21): a tree icon and the direct child count, no word. */
export function TreeButtonFace({ count }: { count: number }) {
  return (
    <span className="flex items-center gap-xxs">
      <ListTreeIcon aria-hidden="true" className="size-(--size-icon-sm) text-muted-foreground" />
      <span className="font-mono text-caption text-subtle-foreground">{count}</span>
    </span>
  );
}
