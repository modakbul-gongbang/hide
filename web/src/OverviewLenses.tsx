import { ChevronDownIcon, ChevronRightIcon, GitPullRequestIcon, TriangleAlertIcon } from "lucide-react";
import { type MouseEvent, type ReactNode } from "react";
import { AgentMark } from "./AgentMark";
import { markTone } from "./agentRow";
import { CheckoutCardHint } from "./components/pr-card";
import { Elapsed } from "./components/elapsed";
import { StatusMark } from "./components/status-mark";
import { Badge } from "./components/ui/badge";
import { Tabs, TabsList, TabsTrigger } from "./components/ui/tabs";
import { Hint, Tooltip, TooltipContent, TooltipTrigger, useHintOpen } from "./components/ui/tooltip";
import { useInterfaceTranslation } from "./i18n/client";
import { requireInterfaceLanguage } from "./i18n/locale";
import { cn } from "./lib/utils";
import { ageWords, type Tile, type TileSegment } from "./overviewLens";
import { prChip } from "./projectBoard";
import type { Actions } from "./actions";
import { laneCheckoutCard, shownPullRequest } from "./projects";
import type { AgentRow, Checkout, Task, Workspace } from "./snapshot";
import { CHECKS_LABEL, PR_TONE, ReviewMarks, TaskGlyph } from "./TaskBoards";
import { useUiStore, type OverviewTab } from "./ui";
import { holdsCommandKey } from "./host";

// The Overview's lenses (PRD overview-lenses-tiles-agents, agents-graph-view):
// the lens tabs, and the chips, popover and fold line the
// Agents graph (`GraphView.tsx`) and the other tabs share. The rules are
// `overviewLens.ts`'s and `agentGraph.ts`'s; this file draws them and routes
// the clicks. Hover, focus and a half-second rest are local: they brighten a
// background or open a card and publish nothing (D-09, B38).

/** Where each part of the lenses goes, supplied by the page around them. */
export type LensHandlers = {
  /** A box head or a chip: that checkout's Workspace. */
  openCheckout: (project: Workspace, checkout: Checkout) => void;
  /** A row: that agent's pane. */
  openAgent: (paneId: string) => void;
  /** An issue chip: the Issues tab's card for it. */
  openIssue: (project: Workspace, task: Task) => void;
  /** ⌘-click anywhere, a PR chip's included: the page on GitHub. */
  openGitHub: (url: string, deviceId: string) => void;
  /** A PR chip: its row on the Project's PRs tab (PRD overview-lenses-prs B21). */
  openPullRequestRow: (project: Workspace, number: number) => void;
  /** `Clean up`: the Delete worktree dialog for that worktree. */
  cleanup: (project: Workspace, checkout: Checkout) => void;
  /** A fold line, by `foldId`: the line of one project opens or closes in place. */
  toggleFold: (fold: string) => void;
  /** A cross-project chip whose box this page draws: that box selected, its fold opened and a filter that hides it cleared (issue 718). */
  selectBox: (box: string, reveal: { fold: string | null; clearFilter: boolean }) => void;
  /** A cross-project chip whose box is on another project: that project's Overview with the box selected and its fold opened. */
  openProjectBox: (project: Workspace, box: string, fold: string | null) => void;
};

/** The handlers both scopes share, around the ones that are each page's own: where an issue chip goes, how a fold opens and how a box is selected. */
export function lensHandlers(actions: Actions, page: Pick<LensHandlers, "openIssue" | "toggleFold" | "selectBox">): LensHandlers {
  return {
    ...page,
    openProjectBox: (project, box, fold) => actions.openOverview(project.device_id, project.id, { box, fold }),
    openCheckout: (project, checkout) => actions.openWorkspace(project.device_id, checkout.workspace_id, checkout.id),
    openAgent: actions.openAgent,
    openGitHub: (url, deviceId) => actions.openPullRequest(url, deviceId, true),
    openPullRequestRow: (project, number) => actions.openPullRequestRow(project.id, number),
    cleanup: (project, checkout) => useUiStore.getState().setWorkspaceDialog({ kind: "delete_worktree", workspaceId: project.id, checkoutId: checkout.id }),
  };
}

// --- tiles -------------------------------------------------------------------

const SEGMENT_TONE: Record<string, string> = {
  turn: "bg-warning",
  working: "bg-success",
  delegating: "bg-agent-working",
  resting: "bg-muted-foreground",
  backlog: "bg-muted-foreground",
  review: "bg-success",
  claude: "bg-file-orange",
  codex: "bg-agent-working",
  fixing: "bg-agent-working",
  blocked: "bg-destructive",
  answer: "bg-warning",
  fix: "bg-destructive",
  stopped: "bg-muted-foreground",
  result: "bg-primary",
};

/** The Issues tile's In progress is the warning tone, the Agents tile's Working the success one. */
function segmentTone(tile: Tile, segment: TileSegment): string {
  if (tile.id === "issues" && segment.key === "working") return "bg-warning";
  return SEGMENT_TONE[segment.key] ?? "bg-muted-foreground";
}

function legend(segments: readonly TileSegment[]): string {
  return segments.map((segment) => `${segment.label} ${segment.count}`).join("\n");
}

/**
 * The lens tabs where the tiles were: one row, each tab its name, its number
 * (absent until read), the yellow count of what waits on the operator and a
 * ⚠ when its read failed. The chosen tab carries its bar as a thin line along
 * its foot; resting on a tab shows its unit, the badge's breakdown, the bar's
 * legend and what failed, in one popover.
 */
export function LensTabs({ tiles, selected, onSelect }: { tiles: readonly Tile[]; selected: OverviewTab; onSelect: (tab: OverviewTab) => void }) {
  const { t } = useInterfaceTranslation();
  return (
    <Tabs value={selected} onValueChange={(value) => onSelect(value as OverviewTab)}>
      <TabsList aria-label={t("overview.projectView")} data-lens-tiles="true">
        {tiles.map((tile) => (
          <LensTab key={tile.id} tile={tile} selected={tile.id === selected} />
        ))}
      </TabsList>
    </Tabs>
  );
}

function LensTab({ tile, selected }: { tile: Tile; selected: boolean }) {
  const { t } = useInterfaceTranslation();
  const badgeWords = tile.badge ? `${tile.badge.label ?? t("board.prGroup.turn")} ${tile.badge.count}` : null;
  const named = [tile.label, tile.value === null ? null : `${tile.value}${tile.unit ? ` ${tile.unit}` : ""}`, badgeWords, tile.failure].filter(Boolean).join(", ");
  const popover = [
    tile.value !== null && tile.unit ? `${tile.value} ${tile.unit}` : null,
    badgeWords && tile.badge ? (tile.badge.parts.length > 0 ? `${badgeWords} (${tile.badge.parts.map((part) => `${part.label} ${part.count}`).join(" · ")})` : badgeWords) : null,
    tile.bar ? legend(tile.bar) : null,
    tile.failure,
  ]
    .filter(Boolean)
    .join("\n");
  const trigger = (
    <TabsTrigger
      value={tile.id}
      aria-label={named}
      className="relative"
      data-lens-tile-button={tile.id}
      data-lens-tile-bar={tile.bar ? tile.bar.map((segment) => `${segment.key}:${segment.count}`).join(" ") : undefined}
    >
      {tile.label}
      {tile.failure ? <TriangleAlertIcon aria-hidden="true" className="size-(--size-icon-sm) text-warning" data-lens-tile-failure={tile.id} /> : null}
      <span className={cn("font-mono text-caption text-muted-foreground", tile.value === null && "hidden")} data-lens-tile-value={tile.value ?? "unread"}>
        {tile.value}
      </span>
      {tile.badge ? (
        // Raised beside the number, so the two counts never read as one.
        <span className="-ml-xxs self-start pt-xs font-mono text-micro leading-none text-warning" data-lens-tile-badge={tile.badge.count}>
          {tile.badge.count}
        </span>
      ) : null}
      {selected && tile.bar ? <TabBar tile={tile} bar={tile.bar} /> : null}
    </TabsTrigger>
  );
  return (
    <span className="inline-flex" data-lens-tile={tile.id} data-selected={selected ? "true" : undefined}>
      {popover ? <Hint label={popover}>{trigger}</Hint> : trigger}
    </span>
  );
}

/** The chosen tab's bar: its parts by count along the tab's foot, a muted line when every part is zero. */
function TabBar({ tile, bar }: { tile: Tile; bar: readonly TileSegment[] }) {
  return (
    <span aria-hidden="true" className="absolute inset-x-sm bottom-0 flex h-(--lens-tab-bar-height) gap-xxs" data-lens-tab-bar="true">
      {bar.every((segment) => segment.count === 0) ? (
        <span className="flex-1 rounded-full bg-muted" />
      ) : (
        bar.filter((segment) => segment.count > 0).map((segment) => <span key={segment.key} className={cn("rounded-full", segmentTone(tile, segment))} style={{ flexGrow: segment.count }} />)
      )}
    </span>
  );
}

// --- chips -------------------------------------------------------------------

/** ⌘-click (Ctrl-click off macOS) means GitHub wherever it lands (D-09). */
export function gitHubClick(event: MouseEvent, url: string | null | undefined, deviceId: string, handlers: LensHandlers): boolean {
  if (!holdsCommandKey(event) || !url) return false;
  event.preventDefault();
  handlers.openGitHub(url, deviceId);
  return true;
}

/** The issue a checkout works on, by its id; it opens the Issues tab's card, its half-second card the issue itself (B24). */
export function IssueChip({ project, task, handlers, now }: { project: Workspace; task: Task; handlers: LensHandlers; now: number }) {
  const { t, i18n } = useInterfaceTranslation();
  const preview = [
    [task.id ?? task.title, t(task.open ? "issue.state.open" : "issue.state.closed")].join(" · "),
    task.title,
    task.updated_at_unix_ms != null ? t("overview.updatedAgo", { age: ageWords(requireInterfaceLanguage(i18n.language), now - task.updated_at_unix_ms, t) }) : null,
  ]
    .filter(Boolean)
    .join("\n");
  return (
    <Hint label={preview}>
      <button
        type="button"
        data-lens-issue-chip={task.key}
        data-graph-focus="chip"
        className="pointer-events-auto relative z-10 inline-flex max-w-(--size-pane-child-chip-max) items-center gap-xxs rounded-xs font-mono text-caption text-muted-foreground outline-none hover:text-foreground focus-visible:ring-1 focus-visible:ring-ring"
        onClick={(event) => {
          event.stopPropagation();
          if (!gitHubClick(event, task.url, project.device_id, handlers)) handlers.openIssue(project, task);
        }}
      >
        <TaskGlyph task={task} />
        <span className="truncate">{task.id ?? task.title}</span>
      </button>
    </Hint>
  );
}

/**
 * A checkout's pull request in its lifecycle colour, on a lens or an issue
 * card; it opens the pull request's row on the PRs tab, ⌘-click the pull
 * request on GitHub, and its half-second card is the checkout's PR card
 * (B17, B24; overview-lenses-issues B8, B9; overview-lenses-prs B21).
 */
export function PullRequestChip({ project, checkout, onOpen, onRow, now }: { project: Workspace; checkout: Checkout; onOpen: (url: string) => void; onRow: (number: number) => void; now: number }) {
  const { t } = useInterfaceTranslation();
  const pr = shownPullRequest(checkout);
  if (!pr) return null;
  const chip = prChip(pr);
  return (
    <CheckoutCardHint card={laneCheckoutCard(project, checkout, now, t)} description={`PR #${pr.number} · ${pr.title}`} onOpenPullRequest={onOpen}>
      <button
        type="button"
        data-lens-pr-chip={pr.number}
        data-graph-focus="chip"
        data-pr-tone={chip.tone}
        aria-label={[`PR #${pr.number}`, chip.tone, chip.checks ? t(CHECKS_LABEL[chip.checks]) : null, chip.review === "changes_requested" ? t("board.review.changes") : null].filter(Boolean).join(" · ")}
        className="pointer-events-auto relative z-10 rounded-xs outline-none focus-visible:ring-1 focus-visible:ring-ring"
        onClick={(event) => {
          event.stopPropagation();
          if (holdsCommandKey(event)) onOpen(pr.url);
          else onRow(pr.number);
        }}
      >
        <Badge variant="outline" className={PR_TONE[chip.tone]}>
          <GitPullRequestIcon aria-hidden="true" />#{pr.number}
          <ReviewMarks pr={chip} review={false} />
          {chip.review === "changes_requested" ? (
            <span className="font-sans text-warning" data-pr-review="changes_requested">
              {t("board.review.changes")}
            </span>
          ) : null}
        </Badge>
      </button>
    </CheckoutCardHint>
  );
}

/**
 * An agent's second line, on a lens node or an issue card's agent row, and
 * after a half-second rest on it everything the agent last said (`message`,
 * D-50), not the line cut to the row, with `↵ Answer in panel`, the row's
 * own click. `place` is the checkout it works in.
 */
export function AgentMessageHint({ agent, place, line, tone, onOpen }: { agent: AgentRow; place: string; line: string; tone: string; onOpen: () => void }) {
  return (
    <AgentMessagePopover agent={agent} place={place} fallback={line} tone={tone} onOpen={onOpen}>
      <span className={cn("relative z-10 line-clamp-1 cursor-pointer break-all text-caption", tone)} data-lens-node-line={agent.pane_id} onClick={onOpen}>
        {line}
      </span>
    </AgentMessagePopover>
  );
}

/** Where an agent stands, for the graph row's popover (agents-graph-view B19): its checkout and tab, its tab's other agents, and who delegated it. */
export type PopoverContext = { checkout: string; tab: string | null; peers: readonly string[]; parent: string | null; line: string | null };

/**
 * Everything an agent last said, after a half-second rest on `children`
 * (D-50): on its line, or on its mark in a PRs tab row (overview-lenses-prs
 * B7). `fallback` is what is said when the agent has no message.
 */
export function AgentMessagePopover({ agent, place, fallback, tone, onOpen, context, children }: { agent: AgentRow; place: string; fallback: string; tone: string; onOpen: () => void; context?: PopoverContext; children: ReactNode }) {
  const { t } = useInterfaceTranslation();
  const { open, onOpenChange, triggerProps } = useHintOpen();
  const message = agent.message?.trim() || fallback;
  return (
    <Tooltip open={open} onOpenChange={onOpenChange} disableHoverableContent={false}>
      <TooltipTrigger asChild {...triggerProps}>
        {children}
      </TooltipTrigger>
      <TooltipContent side="bottom" align="start" className="pointer-events-auto w-(--size-pr-popover) max-w-(--radix-tooltip-content-available-width) text-left text-wrap rounded-md p-md" data-lens-message={agent.pane_id}>
        <div className="flex flex-col gap-sm">
          <span className="flex min-w-0 items-center gap-xs text-body font-medium text-foreground">
            <StatusMark symbol={agent.symbol} className={markTone(agent)} />
            <AgentMark kind={agent.agent_kind} />
            <span className="min-w-0 flex-1 truncate">{agent.identity_label}</span>
            <Elapsed since={agent.changed_at_unix_ms} className="font-mono text-caption text-muted-foreground" />
          </span>
          <p className={cn("whitespace-pre-wrap break-words text-caption", tone)}>{message}</p>
          {context ? (
            <dl className="flex flex-col gap-xxs text-caption text-muted-foreground" data-lens-message-context={agent.pane_id}>
              <div className="flex gap-sm">
                <dt className="shrink-0">{t("overview.location")}</dt>
                <dd className="min-w-0 break-words text-foreground">{[context.checkout, context.tab].filter(Boolean).join(" · ")}</dd>
              </div>
              {context.peers.length > 0 ? (
                <div className="flex gap-sm">
                  <dt className="shrink-0">{t("overview.context.sameTab")}</dt>
                  <dd className="min-w-0 break-words text-foreground">{context.peers.join(" · ")}</dd>
                </div>
              ) : null}
              <div className="flex gap-sm">
                <dt className="shrink-0">{t("overview.context.delegator")}</dt>
                <dd className="min-w-0 break-words text-foreground">{context.parent ?? t("overview.context.manualStart")}</dd>
              </div>
              {context.line ? (
                <div className="flex gap-sm">
                  <dt className="shrink-0">{t("overview.context.line")}</dt>
                  <dd className="min-w-0 break-words text-foreground">{context.line}</dd>
                </div>
              ) : null}
            </dl>
          ) : null}
          <span className="flex items-center gap-sm text-caption text-muted-foreground">
            <span className="min-w-0 flex-1 truncate font-mono">{place}</span>
            <button
              type="button"
              data-lens-message-open={agent.pane_id}
              className="shrink-0 rounded-xs font-medium text-foreground outline-none hover:underline focus-visible:ring-1 focus-visible:ring-ring"
              onClick={(event) => {
                event.stopPropagation();
                onOpen();
              }}
            >
              {t("overview.replyInPanel")}
            </button>
          </span>
        </div>
      </TooltipContent>
    </Tooltip>
  );
}

// --- folds -------------------------------------------------------------------

/** A folded line (agents-graph-view B21, B23): its words and count, a click unfolding it in place; none at zero. `fold` is its `foldId`. */
export function FoldLine({ fold, kind, label, count, names, open, onToggle }: { fold: string; kind: string; label: string; count: number; names: readonly string[]; open: boolean; onToggle: () => void }) {
  if (count === 0) return null;
  const shown = names.slice(0, 6);
  const help = [...shown, names.length > shown.length ? `+${names.length - shown.length}` : null].filter(Boolean).join(" · ");
  return (
    <Hint label={help || label} reveals>
      <button
        type="button"
        aria-expanded={open}
        onClick={onToggle}
        data-graph-fold={kind}
        data-graph-fold-id={fold}
        data-graph-focus="fold"
        className="flex w-full items-center gap-sm rounded-sm border border-border px-sm py-xs text-left text-caption text-subtle-foreground outline-none hover:bg-accent focus-visible:ring-1 focus-visible:ring-ring"
      >
        <span className="min-w-0 flex-1 truncate">
          {label} {count}
        </span>
        {open ? <ChevronDownIcon aria-hidden="true" className="size-(--size-icon)" /> : <ChevronRightIcon aria-hidden="true" className="size-(--size-icon)" />}
      </button>
    </Hint>
  );
}
