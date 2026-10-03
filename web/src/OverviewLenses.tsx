import { ChevronDownIcon, ChevronRightIcon, GitPullRequestIcon, TriangleAlertIcon } from "lucide-react";
import { type MouseEvent, type ReactNode } from "react";
import { AgentMark } from "./AgentMark";
import { markTone } from "./agentRow";
import { CheckoutCardHint } from "./components/pr-card";
import { Elapsed } from "./components/elapsed";
import { StatusMark } from "./components/status-mark";
import { Badge } from "./components/ui/badge";
import { Hint, Tooltip, TooltipContent, TooltipTrigger, useHintOpen } from "./components/ui/tooltip";
import { cn } from "./lib/utils";
import { ageWords, type AgentBucket, type Tile, type TileSegment } from "./overviewLens";
import { prChip } from "./projectBoard";
import type { Actions } from "./actions";
import { laneCheckoutCard, shownPullRequest } from "./projects";
import type { AgentRow, Checkout, Task, Workspace } from "./snapshot";
import { PR_TONE, ReviewMarks, TaskGlyph } from "./TaskBoards";
import { useUiStore, type OverviewTab } from "./ui";

// The Overview's lenses (PRD overview-lenses-tiles-agents, agents-graph-view):
// the tiles in the tab row's place, and the chips, popover and fold line the
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
  /** `정리`: the Delete worktree dialog for that worktree. */
  cleanup: (project: Workspace, checkout: Checkout) => void;
  /** A fold line, by `foldId`: the line of one project opens or closes in place. */
  toggleFold: (fold: string) => void;
};

/** The handlers both scopes share, around the two that are each page's own: where an issue chip goes and how a fold opens. */
export function lensHandlers(actions: Actions, page: Pick<LensHandlers, "openIssue" | "toggleFold">): LensHandlers {
  return {
    ...page,
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

/** The Issues tile's `진행 중` is the warning tone, the Agents tile's `일하는 중` the success one. */
function segmentTone(tile: Tile, segment: TileSegment): string {
  if (tile.id === "issues" && segment.key === "working") return "bg-warning";
  return SEGMENT_TONE[segment.key] ?? "bg-muted-foreground";
}

function legend(segments: readonly TileSegment[]): string {
  return segments.map((segment) => `${segment.label} ${segment.count}`).join("\n");
}

/**
 * The tiles where the tab row was (D-02, B1-B6): same width each, the chosen
 * one outlined, a click choosing its tab. A tile holds its name, the yellow
 * badge of the operator's turn, the big number and its unit, and one bar;
 * the bar's legend and the badge's breakdown are in their popovers, and a
 * failed read is a ⚠ by the name whose popover says what failed and how old
 * the value is. Two rows once the window is narrower than four tiles.
 */
export function LensTiles({ tiles, selected, onSelect, onSegment }: { tiles: readonly Tile[]; selected: OverviewTab; onSelect: (tab: OverviewTab) => void; onSegment?: (bucket: AgentBucket) => void }) {
  return (
    <div role="tablist" aria-label="Project view" className="grid gap-md" style={{ gridTemplateColumns: "repeat(auto-fit, minmax(var(--lens-tile-min), 1fr))" }} data-lens-tiles="true">
      {tiles.map((tile) => (
        <TileView key={tile.id} tile={tile} selected={tile.id === selected} onSelect={() => onSelect(tile.id)} onSegment={tile.id === "agents" ? onSegment : undefined} />
      ))}
    </div>
  );
}

function TileView({ tile, selected, onSelect, onSegment }: { tile: Tile; selected: boolean; onSelect: () => void; onSegment?: (bucket: AgentBucket) => void }) {
  const badgeWords = tile.badge ? `${tile.badge.label ?? "내 차례"} ${tile.badge.count}` : null;
  const named = [
    tile.label,
    tile.value === null ? null : `${tile.value}${tile.unit ? ` ${tile.unit}` : ""}`,
    badgeWords,
    tile.failure,
  ]
    .filter(Boolean)
    .join(", ");
  return (
    <div className="relative min-w-0" data-lens-tile={tile.id} data-selected={selected ? "true" : undefined}>
      <button
        type="button"
        role="tab"
        aria-selected={selected}
        aria-label={named}
        onClick={onSelect}
        className={cn("absolute inset-0 rounded-md border bg-card outline-none hover:bg-accent focus-visible:ring-1 focus-visible:ring-ring", selected ? "border-primary" : "border-border")}
        data-lens-tile-button={tile.id}
      />
      <div className="pointer-events-none relative flex min-w-0 flex-col gap-xs p-sm">
        <div className="flex min-w-0 items-center gap-xs text-body font-medium text-foreground">
          <span className="truncate">{tile.label}</span>
          {tile.failure ? (
            <Hint label={tile.failure}>
              <span className="pointer-events-auto text-warning" data-lens-tile-failure={tile.id}>
                <TriangleAlertIcon aria-hidden="true" className="size-(--size-icon-sm)" />
              </span>
            </Hint>
          ) : null}
          <span className="flex-1" />
          {tile.badge ? (
            <Hint label={[badgeWords, tile.badge.parts.map((part) => `${part.label} ${part.count}`).join(" · ")].filter(Boolean).join("\n")}>
              <span className="pointer-events-auto rounded-xs px-xs font-mono text-caption text-warning" data-lens-tile-badge={tile.badge.count}>
                {tile.badge.count}
              </span>
            </Hint>
          ) : null}
        </div>
        <div className="flex min-h-(--size-control) items-baseline gap-xs" data-lens-tile-value={tile.value ?? "unread"}>
          {tile.value === null ? null : (
            <>
              <span className="font-mono text-headline font-semibold text-foreground">{tile.value}</span>
              {tile.unit ? <span className="text-caption text-muted-foreground">{tile.unit}</span> : null}
            </>
          )}
        </div>
        {tile.bar ? (
          <Hint label={legend(tile.bar)}>
            <span className="pointer-events-auto flex h-(--lens-bar-height) w-full gap-xxs" data-lens-tile-bar={tile.bar.map((segment) => `${segment.key}:${segment.count}`).join(" ")}>
              {tile.bar.every((segment) => segment.count === 0) ? (
                <span className="flex-1 rounded-full bg-muted" />
              ) : (
                tile.bar
                  .filter((segment) => segment.count > 0)
                  .map((segment) =>
                    onSegment ? (
                      // A segment of the Agents bar lights its status chip alone and opens the graph (agents-graph-view B25).
                      <button
                        key={segment.key}
                        type="button"
                        aria-label={`${segment.label} ${segment.count}만 보기`}
                        data-lens-tile-segment={segment.key}
                        className={cn("pointer-events-auto rounded-full outline-none focus-visible:ring-1 focus-visible:ring-ring", segmentTone(tile, segment))}
                        style={{ flexGrow: segment.count }}
                        onClick={() => onSegment(segment.key as AgentBucket)}
                      />
                    ) : (
                      <span key={segment.key} className={cn("rounded-full", segmentTone(tile, segment))} style={{ flexGrow: segment.count }} />
                    ),
                  )
              )}
            </span>
          </Hint>
        ) : (
          <span className="h-(--lens-bar-height)" />
        )}
      </div>
    </div>
  );
}

// --- chips -------------------------------------------------------------------

/** ⌘-click means GitHub wherever it lands (D-09). */
export function gitHubClick(event: MouseEvent, url: string | null | undefined, deviceId: string, handlers: LensHandlers): boolean {
  if (!event.metaKey || !url) return false;
  event.preventDefault();
  handlers.openGitHub(url, deviceId);
  return true;
}

/** The issue a checkout works on, by its id; it opens the Issues tab's card, its half-second card the issue itself (B24). */
export function IssueChip({ project, task, handlers, now }: { project: Workspace; task: Task; handlers: LensHandlers; now: number }) {
  const preview = [
    [task.id ?? task.title, task.open ? "열림" : "닫힘"].join(" · "),
    task.title,
    task.updated_at_unix_ms != null ? `갱신 ${ageWords(now - task.updated_at_unix_ms)}` : null,
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
const CHECKS_WORDS = { passing: "CI 통과", failed: "CI 실패", pending: "CI 진행 중" } as const;

export function PullRequestChip({ project, checkout, onOpen, onRow, now }: { project: Workspace; checkout: Checkout; onOpen: (url: string) => void; onRow: (number: number) => void; now: number }) {
  const pr = shownPullRequest(checkout);
  if (!pr) return null;
  const chip = prChip(pr);
  return (
    <CheckoutCardHint card={laneCheckoutCard(project, checkout, now)} description={`PR #${pr.number} · ${pr.title}`} onOpenPullRequest={onOpen}>
      <button
        type="button"
        data-lens-pr-chip={pr.number}
        data-graph-focus="chip"
        data-pr-tone={chip.tone}
        aria-label={[`PR #${pr.number}`, chip.tone, chip.checks ? CHECKS_WORDS[chip.checks] : null, chip.review === "changes_requested" ? "변경 요청" : null].filter(Boolean).join(" · ")}
        className="pointer-events-auto relative z-10 rounded-xs outline-none focus-visible:ring-1 focus-visible:ring-ring"
        onClick={(event) => {
          event.stopPropagation();
          if (event.metaKey) onOpen(pr.url);
          else onRow(pr.number);
        }}
      >
        <Badge variant="outline" className={PR_TONE[chip.tone]}>
          <GitPullRequestIcon aria-hidden="true" />#{pr.number}
          <ReviewMarks pr={chip} review={false} />
          {chip.review === "changes_requested" ? (
            <span className="font-sans text-warning" data-pr-review="changes_requested">
              변경 요청
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
 * D-50), not the line cut to the row, with `↵ 패널에서 답하기`, the row's
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
                <dt className="shrink-0">위치</dt>
                <dd className="min-w-0 break-words text-foreground">{[context.checkout, context.tab].filter(Boolean).join(" · ")}</dd>
              </div>
              {context.peers.length > 0 ? (
                <div className="flex gap-sm">
                  <dt className="shrink-0">같은 탭</dt>
                  <dd className="min-w-0 break-words text-foreground">{context.peers.join(" · ")}</dd>
                </div>
              ) : null}
              <div className="flex gap-sm">
                <dt className="shrink-0">맡긴 에이전트</dt>
                <dd className="min-w-0 break-words text-foreground">{context.parent ?? "직접 시작"}</dd>
              </div>
              {context.line ? (
                <div className="flex gap-sm">
                  <dt className="shrink-0">선</dt>
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
              ↵ 패널에서 답하기
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
