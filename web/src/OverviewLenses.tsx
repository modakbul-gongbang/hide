import { ChevronDownIcon, ChevronRightIcon, GitBranchIcon, GitMergeIcon, GitPullRequestIcon, HouseIcon, TriangleAlertIcon, XIcon } from "lucide-react";
import { useLayoutEffect, useRef, type KeyboardEvent, type MouseEvent, type ReactNode, type RefObject } from "react";
import { AgentMark } from "./AgentMark";
import { lineTone, markTone, rowAccessibleName, rowLine } from "./agentRow";
import { CHECKOUT_KIND_ICON } from "./components/checkout-icon";
import { CheckoutCardHint } from "./components/pr-card";
import { Elapsed } from "./components/elapsed";
import { StatusMark } from "./components/status-mark";
import { Badge } from "./components/ui/badge";
import { ToggleGroup, ToggleGroupItem } from "./components/ui/toggle-group";
import { Hint, Tooltip, TooltipContent, TooltipTrigger, useHintOpen } from "./components/ui/tooltip";
import { cn } from "./lib/utils";
import { useMeasuredPaths, type MeasuredPath } from "./measuredPaths";
import {
  ageWords,
  childSummary,
  lineageAgentCount,
  type Delegation,
  type Lane,
  type LanesBoard,
  type LensAgent,
  type Lineage,
  type LineageBoard,
  type Tile,
  type TileSegment,
} from "./overviewLens";
import { prChip } from "./projectBoard";
import type { Actions } from "./actions";
import { checkoutPresentation, distanceText, filesText, laneCheckoutCard, shownPullRequest } from "./projects";
import type { AgentRow, Checkout, Task, Workspace } from "./snapshot";
import { PR_TONE, TaskGlyph } from "./TaskBoards";
import { useUiStore, type AgentsMode, type LensFold, type OverviewTab } from "./ui";

// The Overview's lenses (PRD overview-lenses-tiles-agents): the tiles in the
// tab row's place, and the Agents tab's checkout lanes and lineage. The rules
// are `overviewLens.ts`'s; this file draws them and routes the clicks. At rest
// a node says three things, what, how far, and who; only the operator's turn
// is coloured. Hover, focus and a half-second rest are local: they brighten a
// background or open a card and publish nothing (D-09, B27).

/** Where each part of the lenses goes, supplied by the page around them. */
export type LensHandlers = {
  /** A lane head or a checkout chip: that checkout's Workspace. */
  openCheckout: (project: Workspace, checkout: Checkout) => void;
  /** A node: that agent's pane. */
  openAgent: (paneId: string) => void;
  /** An issue chip: the Issues tab's card for it. */
  openIssue: (project: Workspace, task: Task) => void;
  /** ⌘-click anywhere, a PR chip's included: the page on GitHub. */
  openGitHub: (url: string, deviceId: string) => void;
  /** A PR chip: its row on the Project's PRs tab (PRD overview-lenses-prs B21). */
  openPullRequestRow: (project: Workspace, number: number) => void;
  /** `정리`: the Delete worktree dialog for that worktree. */
  cleanup: (project: Workspace, checkout: Checkout) => void;
  toggleFold: (fold: LensFold) => void;
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
export function LensTiles({ tiles, selected, onSelect }: { tiles: readonly Tile[]; selected: OverviewTab; onSelect: (tab: OverviewTab) => void }) {
  return (
    <div role="tablist" aria-label="Project view" className="grid gap-md" style={{ gridTemplateColumns: "repeat(auto-fit, minmax(var(--lens-tile-min), 1fr))" }} data-lens-tiles="true">
      {tiles.map((tile) => (
        <TileView key={tile.id} tile={tile} selected={tile.id === selected} onSelect={() => onSelect(tile.id)} />
      ))}
    </div>
  );
}

function TileView({ tile, selected, onSelect }: { tile: Tile; selected: boolean; onSelect: () => void }) {
  const named = [
    tile.label,
    tile.value === null ? null : `${tile.value}${tile.unit ? ` ${tile.unit}` : ""}`,
    tile.badge ? `내 차례 ${tile.badge.count}` : null,
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
            <Hint label={[`내 차례 ${tile.badge.count}`, tile.badge.parts.map((part) => `${part.label} ${part.count}`).join(" · ")].filter(Boolean).join("\n")}>
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
                  .map((segment) => <span key={segment.key} className={cn("rounded-full", segmentTone(tile, segment))} style={{ flexGrow: segment.count }} />)
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

/** `체크아웃 · 계보` at the right end of the facts line while the Agents tile is chosen (D-02, D-03). */
export function AgentsModeToggle({ mode, onChange }: { mode: AgentsMode; onChange: (mode: AgentsMode) => void }) {
  return (
    <ToggleGroup type="single" value={mode} onValueChange={(value) => value && onChange(value as AgentsMode)} aria-label="Agents mode" data-agents-mode={mode}>
      <ToggleGroupItem value="checkouts" data-agents-mode-item="checkouts">
        체크아웃
      </ToggleGroupItem>
      <ToggleGroupItem value="lineage" data-agents-mode-item="lineage">
        계보
      </ToggleGroupItem>
    </ToggleGroup>
  );
}

// --- keyboard ----------------------------------------------------------------

/**
 * Arrow keys between the lenses' nodes and lane heads (B26): the nearest one
 * in the pressed direction by where it is drawn, so lanes, columns and
 * lineages all move the way they look. Enter is the button's own click.
 */
function moveFocus(event: KeyboardEvent<HTMLElement>) {
  const direction = { ArrowRight: [1, 0], ArrowLeft: [-1, 0], ArrowDown: [0, 1], ArrowUp: [0, -1] }[event.key];
  if (!direction || event.metaKey || event.ctrlKey || event.altKey) return;
  const from = (event.target as HTMLElement).closest<HTMLElement>("[data-lens-focus]");
  if (!from) return;
  const [dx, dy] = direction as [number, number];
  const origin = from.getBoundingClientRect();
  const ox = origin.left + origin.width / 2;
  const oy = origin.top + origin.height / 2;
  let best: { element: HTMLElement; score: number } | null = null;
  for (const element of event.currentTarget.querySelectorAll<HTMLElement>("[data-lens-focus]")) {
    if (element === from) continue;
    const rect = element.getBoundingClientRect();
    const x = rect.left + rect.width / 2 - ox;
    const y = rect.top + rect.height / 2 - oy;
    const along = x * dx + y * dy;
    if (along <= 1) continue;
    const across = Math.abs(dx !== 0 ? y : x);
    const score = along + across * 2;
    if (!best || score < best.score) best = { element, score };
  }
  if (!best) return;
  event.preventDefault();
  best.element.focus();
  best.element.scrollIntoView({ block: "nearest", inline: "nearest" });
}

// --- chips -------------------------------------------------------------------

/** ⌘-click means GitHub wherever it lands (D-09). */
function gitHubClick(event: MouseEvent, url: string | null | undefined, deviceId: string, handlers: LensHandlers): boolean {
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
  const pr = shownPullRequest(checkout);
  if (!pr) return null;
  const chip = prChip(pr);
  return (
    <CheckoutCardHint card={laneCheckoutCard(project, checkout, now)} description={`PR #${pr.number} · ${pr.title}`} onOpenPullRequest={onOpen}>
      <button
        type="button"
        data-lens-pr-chip={pr.number}
        data-pr-tone={chip.tone}
        aria-label={`PR #${pr.number} · ${chip.tone}`}
        className="pointer-events-auto relative z-10 rounded-xs outline-none focus-visible:ring-1 focus-visible:ring-ring"
        onClick={(event) => {
          event.stopPropagation();
          if (event.metaKey) onOpen(pr.url);
          else onRow(pr.number);
        }}
      >
        <Badge variant="outline" className={PR_TONE[chip.tone]}>
          <GitPullRequestIcon aria-hidden="true" />#{pr.number}
        </Badge>
      </button>
    </CheckoutCardHint>
  );
}

/** Where a lineage node works: the house for the primary checkout, else its branch; it opens that Workspace, its half-second card is the checkout card (B24). */
function CheckoutChip({ project, checkout, handlers, now }: { project: Workspace; checkout: Checkout; handlers: LensHandlers; now: number }) {
  const Glyph = checkout.is_primary === true || !checkout.is_worktree ? HouseIcon : GitBranchIcon;
  const name = checkout.branch ?? checkout.label;
  return (
    <CheckoutCardHint card={laneCheckoutCard(project, checkout, now)} description={`${name} · ${checkout.path}`} onOpenPullRequest={(url) => handlers.openGitHub(url, project.device_id)} onOpenWorkspace={() => handlers.openCheckout(project, checkout)}>
      <button
        type="button"
        data-lens-checkout-chip={checkout.id}
        aria-label={`Workspace ${name}`}
        className="pointer-events-auto relative z-10 inline-flex min-w-0 max-w-(--size-pane-child-chip-max) items-center gap-xxs rounded-xs font-mono text-caption text-muted-foreground outline-none hover:text-foreground focus-visible:ring-1 focus-visible:ring-ring"
        onClick={(event) => {
          event.stopPropagation();
          if (!gitHubClick(event, shownPullRequest(checkout)?.url, project.device_id, handlers)) handlers.openCheckout(project, checkout);
        }}
      >
        <Glyph aria-hidden="true" className="size-(--size-icon-sm) shrink-0" />
        <span className="truncate">{name}</span>
      </button>
    </CheckoutCardHint>
  );
}

// --- nodes -------------------------------------------------------------------

/**
 * One agent (B21, B22): its mark, provider, title and age, and the core's
 * second line. The operator's turn is the only colour: a yellow outline and
 * the question in yellow, or a finished one's result; an agent waiting on its
 * children says how they are doing; a resting one is dimmed and says nothing.
 * The node is one button that opens the agent's pane; resting half a second
 * on its line opens everything the agent last said, with the same move.
 */
function AgentNode({ value, chips, handlers, now }: { value: LensAgent; chips: boolean; handlers: LensHandlers; now: number }) {
  const { agent, bucket, project, checkout, task, device } = value;
  const turn = bucket === "turn";
  const said = rowLine(agent);
  const line = bucket === "resting" ? null : bucket === "delegating" ? childSummary(agent) : (said?.text ?? null);
  const tone = bucket !== "delegating" && said ? lineTone(said, agent.demand) : "text-muted-foreground";
  const open = () => handlers.openAgent(agent.pane_id);
  return (
    <div
      className={cn("relative flex w-(--lens-node-width) min-w-0 flex-col gap-xxs rounded-md border bg-card px-sm py-xs", turn ? "border-warning" : "border-border", bucket === "resting" && "opacity-(--opacity-secondary)")}
      data-lens-node={agent.pane_id}
      data-bucket={bucket}
    >
      <button
        type="button"
        aria-label={rowAccessibleName(agent, device)}
        data-lens-focus="node"
        data-lens-open={agent.pane_id}
        className="absolute inset-0 rounded-md outline-none hover:bg-accent focus-visible:ring-1 focus-visible:ring-ring"
        onClick={(event) => {
          if (!gitHubClick(event, shownPullRequest(checkout)?.url ?? task?.url, project.device_id, handlers)) open();
        }}
      />
      <span className="pointer-events-none relative flex min-w-0 items-center gap-xs">
        <StatusMark symbol={agent.symbol} className={markTone(agent)} />
        <AgentMark kind={agent.agent_kind} />
        <span className={cn("min-w-0 flex-1 truncate text-body text-foreground", turn && "font-semibold")}>{agent.identity_label}</span>
        <Elapsed since={agent.changed_at_unix_ms} className="shrink-0 font-mono text-caption text-muted-foreground" />
      </span>
      {line ? <AgentMessageHint agent={agent} place={checkout.branch ?? checkout.label} line={line} tone={tone} onOpen={open} /> : null}
      {chips ? (
        <span className="pointer-events-none relative flex min-w-0 items-center gap-sm" data-lens-node-chips="true">
          <CheckoutChip project={project} checkout={checkout} handlers={handlers} now={now} />
          {task ? <IssueChip project={project} task={task} handlers={handlers} now={now} /> : null}
          <PullRequestChip project={project} checkout={checkout} onOpen={(url) => handlers.openGitHub(url, project.device_id)} onRow={(number) => handlers.openPullRequestRow(project, number)} now={now} />
        </span>
      ) : null}
    </div>
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

/**
 * Everything an agent last said, after a half-second rest on `children`
 * (D-50): on its line, or on its mark in a PRs tab row (overview-lenses-prs
 * B7). `fallback` is what is said when the agent has no message.
 */
export function AgentMessagePopover({ agent, place, fallback, tone, onOpen, children }: { agent: AgentRow; place: string; fallback: string; tone: string; onOpen: () => void; children: ReactNode }) {
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

// --- delegation lines ----------------------------------------------------------

/**
 * The delegation lines over the lanes or the lineages (B14, B23): from a
 * parent to its child, down across lanes from the parent's bottom to the
 * child's top, or across from the parent's right to the child's left.
 */
function DelegationLines({ paths, marker }: { paths: readonly MeasuredPath[]; marker: string }) {
  return (
    <svg aria-hidden="true" className="pointer-events-none absolute inset-0 size-full overflow-visible text-muted-foreground" data-lens-lines={paths.length}>
      <defs>
        <marker id={marker} viewBox="0 0 8 8" refX="7" refY="4" markerWidth="8" markerHeight="8" orient="auto-start-reverse">
          <path d="M 1 1 L 7 4 L 1 7" fill="none" stroke="currentColor" />
        </marker>
      </defs>
      {paths.map((path) => (
        <path key={path.id} d={path.d} fill="none" stroke="currentColor" strokeWidth={1} markerEnd={`url(#${marker})`} data-lens-line={path.id} />
      ))}
    </svg>
  );
}

function linePath(origin: DOMRect, from: DOMRect, to: DOMRect, across: boolean): string {
  if (across) {
    const forward = to.left >= from.right;
    const x1 = (forward ? from.right : from.left) - origin.left;
    const x2 = (forward ? to.left : to.right) - origin.left;
    const y1 = from.top + from.height / 2 - origin.top;
    const y2 = to.top + to.height / 2 - origin.top;
    const mid = (x1 + x2) / 2;
    return y1 === y2 ? `M ${x1} ${y1} H ${x2}` : `M ${x1} ${y1} H ${mid} V ${y2} H ${x2}`;
  }
  const down = to.top >= from.bottom;
  const x1 = from.left + from.width / 2 - origin.left;
  const x2 = to.left + to.width / 2 - origin.left;
  const y1 = (down ? from.bottom : from.top) - origin.top;
  const y2 = (down ? to.top : to.bottom) - origin.top;
  const mid = (y1 + y2) / 2;
  return x1 === x2 ? `M ${x1} ${y1} V ${y2}` : `M ${x1} ${y1} V ${mid} H ${x2} V ${y2}`;
}

function useDelegationPaths(root: RefObject<HTMLDivElement | null>, delegations: readonly Delegation[], across: (delegation: Delegation) => boolean, deps: readonly unknown[]) {
  return useMeasuredPaths(
    root,
    "[data-lens-node]",
    (origin, at) =>
      delegations.flatMap((delegation) => {
        const from = at("data-lens-node", delegation.from);
        const to = at("data-lens-node", delegation.to);
        return from && to ? [{ id: `${delegation.from}>${delegation.to}`, d: linePath(origin, from, to, across(delegation)) }] : [];
      }),
    deps,
  );
}

// --- folds -------------------------------------------------------------------

/** A folded line (B20, B25): its words and count, a click unfolding it in place; none at zero. */
function FoldLine({ fold, label, count, names, open, onToggle }: { fold: LensFold; label: string; count: number; names: readonly string[]; open: boolean; onToggle: () => void }) {
  if (count === 0) return null;
  const shown = names.slice(0, 6);
  const help = [...shown, names.length > shown.length ? `+${names.length - shown.length}` : null].filter(Boolean).join(" · ");
  return (
    <Hint label={help || label} reveals>
      <button
        type="button"
        aria-expanded={open}
        onClick={onToggle}
        data-lens-fold={fold}
        data-lens-focus="fold"
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

// --- checkout lanes -----------------------------------------------------------

const CLEANUP_HELP = {
  merged: "머지됐거나 폴더가 없는 워크트리와 거기서 쉬는 에이전트를 지운다. 확인 대화상자가 먼저 뜬다.",
  missing: "폴더가 없는 워크트리의 기록을 지운다.",
} as const;

/**
 * A lane's head (B15-B18): the kind glyph in its pull request's colour and
 * the branch, the purpose (else the pull request's title), then the issue
 * chip, the PR chip, `↑N ↓N` and the changed files; the primary checkout's
 * says how many agents it has. A merged worktree is dimmed with the merge
 * glyph, a folder-less one is `× 폴더 없음`, and both offer `정리`. The head
 * is one button to the Workspace, and resting on it opens the checkout card
 * with `↵ Workspace`, the same move.
 */
function LaneHead({ lane, scope, handlers, now }: { lane: Lane; scope: "project" | "all"; handlers: LensHandlers; now: number }) {
  const { project, checkout, cleanup } = lane;
  const view = checkoutPresentation(project, checkout, now);
  const pr = view.pullRequest;
  const Glyph = cleanup === "missing" ? XIcon : cleanup === "merged" && !pr ? GitMergeIcon : CHECKOUT_KIND_ICON[view.kind];
  const glyphTone = cleanup === "missing" ? "text-destructive" : cleanup === "merged" && !pr ? "text-pr-merged" : view.kindTone;
  const name = checkout.branch ?? checkout.label;
  const purpose = checkout.purpose?.text ?? pr?.title ?? null;
  const worktree = checkout.worktree;
  const unread = project.is_git === true && !worktree && checkout.exists;
  const ahead = checkout.ahead ?? 0;
  const behind = worktree?.behind_upstream ?? 0;
  const files = worktree?.changed_file_count ?? 0;
  const open = () => handlers.openCheckout(project, checkout);
  return (
    <div className={cn("relative flex min-w-0 flex-col gap-xxs px-sm py-sm", cleanup && "opacity-(--opacity-secondary)")} data-lens-head={checkout.id}>
      <CheckoutCardHint card={laneCheckoutCard(project, checkout, now)} description={view.detail} onOpenPullRequest={(url) => handlers.openGitHub(url, project.device_id)} onOpenWorkspace={open}>
        <button
          type="button"
          aria-label={`Workspace ${name}${purpose ? ` · ${purpose}` : ""}`}
          data-lens-focus="head"
          data-lens-head-open={checkout.id}
          className="absolute inset-0 rounded-sm outline-none hover:bg-accent focus-visible:ring-1 focus-visible:ring-ring"
          onClick={(event) => {
            if (!gitHubClick(event, pr?.url ?? lane.task?.url, project.device_id, handlers)) open();
          }}
        />
      </CheckoutCardHint>
      {scope === "all" ? <span className="pointer-events-none relative truncate text-micro text-muted-foreground" data-lens-head-project={project.id}>{project.label}</span> : null}
      <span className="pointer-events-none relative flex min-w-0 items-center gap-xs">
        <Glyph aria-hidden="true" className={cn("size-(--size-checkout-icon) shrink-0", glyphTone)} />
        <span className="min-w-0 flex-1 truncate font-mono text-body text-foreground">{name}</span>
        {cleanup ? (
          <Hint label={CLEANUP_HELP[cleanup]}>
            <button
              type="button"
              data-lens-cleanup={checkout.id}
              className="pointer-events-auto relative z-10 shrink-0 rounded-xs px-xs text-caption text-subtle-foreground outline-none hover:bg-secondary hover:text-foreground focus-visible:ring-1 focus-visible:ring-ring"
              onClick={(event) => {
                event.stopPropagation();
                handlers.cleanup(project, checkout);
              }}
            >
              정리
            </button>
          </Hint>
        ) : null}
      </span>
      {purpose ? <span className="pointer-events-none relative truncate text-caption text-muted-foreground">{purpose}</span> : null}
      <span className="pointer-events-none relative flex min-w-0 flex-wrap items-center gap-sm font-mono text-caption text-muted-foreground">
        {lane.primary ? (
          <span className="pointer-events-none" data-lens-head-agents={lane.nodes.length}>
            에이전트 {lane.nodes.length}
          </span>
        ) : cleanup === "missing" ? (
          <span className="pointer-events-none text-destructive">폴더 없음</span>
        ) : (
          <>
            {lane.task ? <IssueChip project={project} task={lane.task} handlers={handlers} now={now} /> : null}
            <PullRequestChip project={project} checkout={checkout} onOpen={(url) => handlers.openGitHub(url, project.device_id)} onRow={(number) => handlers.openPullRequestRow(project, number)} now={now} />
            {ahead > 0 || behind > 0 ? (
              <span className="pointer-events-none" data-lens-head-distance={`${ahead}:${behind}`}>
                {distanceText(ahead, behind)}
              </span>
            ) : null}
            {unread ? (
              <Hint label="Git 상태를 아직 읽지 못함">
                <span className="pointer-events-auto relative z-10" data-lens-head-files="unread">
                  ?
                </span>
              </Hint>
            ) : files > 0 ? (
              <span className={cn("pointer-events-none", worktree?.dirty && "text-warning")} data-lens-head-files={files}>
                {filesText(files)}
              </span>
            ) : null}
          </>
        )}
      </span>
    </div>
  );
}

function LaneRow({ lane, scope, selected, columns, handlers, now }: { lane: Lane; scope: "project" | "all"; selected: boolean; columns: number; handlers: LensHandlers; now: number }) {
  return (
    <div className="relative flex min-w-0 border-b border-border" data-lens-lane={lane.id} data-lane-rank={lane.rank} data-selected={selected ? "true" : undefined}>
      {/* The selected outline sits over the sticky head, which would otherwise paint over it. */}
      {selected ? <span aria-hidden="true" className="pointer-events-none absolute inset-0 z-30 rounded-sm ring-1 ring-inset ring-primary" /> : null}
      <div className="sticky left-0 z-20 w-(--lens-lane-head) shrink-0 border-r border-border bg-background">
        <LaneHead lane={lane} scope={scope} handlers={handlers} now={now} />
      </div>
      <div className="grid items-start gap-x-xl p-sm" style={{ gridTemplateColumns: `repeat(${Math.max(columns, 1)}, var(--lens-node-width))` }}>
        {lane.nodes.map((node) => (
          <div key={node.agent.pane_id} style={{ gridColumn: node.column + 1, gridRow: 1 }}>
            <AgentNode value={node} chips={false} handlers={handlers} now={now} />
          </div>
        ))}
      </div>
    </div>
  );
}

/**
 * The checkout mode (D-05, B12-B20): a lane per checkout, its head on the
 * left and its agents to the right, delegation lines between them; the
 * worktrees with no agent and the ones only there to be removed fold into
 * one line each. The lanes scroll sideways together when the window is too
 * narrow, the heads staying in place. The selected lane is outlined and
 * brought into view.
 */
export function CheckoutLanes({ board, scope, selectedLane, folds, handlers, now }: { board: LanesBoard; scope: "project" | "all"; selectedLane: string | null; folds: readonly LensFold[]; handlers: LensHandlers; now: number }) {
  const box = useRef<HTMLDivElement>(null);
  const marker = "lens-lane-arrow";
  const open = new Set<LensFold>(folds);
  // The selected lane's own fold opens with it, so ⌘⇧H always shows it (B12).
  if (selectedLane && board.empty.some((lane) => lane.id === selectedLane)) open.add("empty");
  if (selectedLane && board.cleanup.some((lane) => lane.id === selectedLane)) open.add("cleanup");
  const paths = useDelegationPaths(box, board.delegations, (delegation) => delegation.within, [board, open.has("cleanup"), open.has("empty")]);
  useLayoutEffect(() => {
    if (!selectedLane) return;
    box.current?.querySelector(`[data-lens-lane="${CSS.escape(selectedLane)}"]`)?.scrollIntoView({ block: "nearest", inline: "nearest" });
  }, [selectedLane]);
  if (board.lanes.length === 0 && board.empty.length === 0 && board.cleanup.length === 0) {
    return (
      <div className="flex flex-col items-center justify-center gap-sm p-xl text-center text-caption text-muted-foreground" data-lens-empty="true">
        <p>실행 중인 에이전트가 없습니다</p>
      </div>
    );
  }
  const row = (lane: Lane) => <LaneRow key={lane.id} lane={lane} scope={scope} selected={lane.id === selectedLane} columns={board.columns} handlers={handlers} now={now} />;
  return (
    <div className="flex min-w-0 flex-col gap-sm px-lg pb-xl" data-lens-mode="checkouts" onKeyDown={moveFocus}>
      <div className="min-w-0 overflow-x-auto">
        <div ref={box} className="relative flex w-max min-w-full flex-col">
          <div className="flex text-caption text-muted-foreground">
            <span className="sticky left-0 z-20 w-(--lens-lane-head) shrink-0 bg-background px-sm py-xs">체크아웃</span>
            <span className="px-sm py-xs">에이전트</span>
          </div>
          {board.lanes.map(row)}
          {open.has("empty") ? board.empty.map(row) : null}
          {open.has("cleanup") ? board.cleanup.map(row) : null}
          <DelegationLines paths={paths} marker={marker} />
        </div>
      </div>
      <FoldLine fold="empty" label="에이전트 없는 워크트리" count={board.empty.length} names={board.empty.map((lane) => lane.checkout.branch ?? lane.checkout.label)} open={open.has("empty")} onToggle={() => handlers.toggleFold("empty")} />
      <FoldLine fold="cleanup" label="정리할 것" count={board.cleanup.length} names={board.cleanup.map((lane) => lane.checkout.branch ?? lane.checkout.label)} open={open.has("cleanup")} onToggle={() => handlers.toggleFold("cleanup")} />
    </div>
  );
}

// --- lineage -----------------------------------------------------------------

const COLUMN_NAMES = ["Observer · 보통 main", "Implementor · 워크트리"];

function LineageRows({ lineage, columns, handlers, now }: { lineage: Lineage; columns: number; handlers: LensHandlers; now: number }) {
  return (
    <div
      className="grid items-start gap-x-xxl gap-y-sm"
      style={{ gridTemplateColumns: `repeat(${Math.max(columns, 1)}, var(--lens-node-width))` }}
      data-lens-lineage={lineage.rootPaneId}
      data-lineage-rank={lineage.rank}
    >
      {lineage.nodes.map((node) => (
        <div key={node.agent.pane_id} style={{ gridColumn: node.depth + 1, gridRow: node.row + 1 }}>
          <AgentNode value={node} chips handlers={handlers} now={now} />
        </div>
      ))}
    </div>
  );
}

/**
 * The lineage mode (D-06, B23-B25): who asked whom, Observer then
 * Implementor then each level below, a row per lineage with the asking ones
 * first; each node's third line names its checkout, issue and pull request.
 * Resting lineages and the ones in worktrees only there to be removed fold
 * into one line each.
 */
export function LineageLens({ board, folds, handlers, now }: { board: LineageBoard; folds: readonly LensFold[]; handlers: LensHandlers; now: number }) {
  const box = useRef<HTMLDivElement>(null);
  const open = new Set<LensFold>(folds);
  const shown = [...board.lineages, ...(open.has("resting") ? board.resting : []), ...(open.has("cleanup") ? board.cleanup : [])];
  // A folded lineage draws no node, so its lines find no end and are not drawn.
  const paths = useDelegationPaths(box, board.delegations, () => true, [board, open.has("resting"), open.has("cleanup")]);
  if (shown.length === 0 && board.resting.length === 0 && board.cleanup.length === 0) {
    return (
      <div className="flex flex-col items-center justify-center gap-sm p-xl text-center text-caption text-muted-foreground" data-lens-empty="true">
        <p>실행 중인 에이전트가 없습니다</p>
      </div>
    );
  }
  const names = Array.from({ length: Math.max(board.columns, 1) }, (_, index) => COLUMN_NAMES[index] ?? "하위 에이전트");
  return (
    <div className="flex min-w-0 flex-col gap-sm px-lg pb-xl" data-lens-mode="lineage" onKeyDown={moveFocus}>
      <div className="min-w-0 overflow-x-auto">
        <div ref={box} className="relative flex w-max min-w-full flex-col gap-md">
          <div className="grid gap-x-xxl text-caption text-muted-foreground" style={{ gridTemplateColumns: `repeat(${names.length}, var(--lens-node-width))` }}>
            {names.map((name, index) => (
              <span key={index} className="truncate py-xs">
                {name}
              </span>
            ))}
          </div>
          {shown.map((lineage) => (
            <LineageRows key={lineage.rootPaneId} lineage={lineage} columns={board.columns} handlers={handlers} now={now} />
          ))}
          <DelegationLines paths={paths} marker="lens-lineage-arrow" />
        </div>
      </div>
      <FoldLine fold="resting" label="쉬는 에이전트" count={lineageAgentCount(board.resting)} names={board.resting.flatMap((lineage) => lineage.nodes.map((node) => node.agent.identity_label))} open={open.has("resting")} onToggle={() => handlers.toggleFold("resting")} />
      <FoldLine fold="cleanup" label="정리할 것" count={lineageAgentCount(board.cleanup)} names={board.cleanup.flatMap((lineage) => lineage.nodes.map((node) => node.agent.identity_label))} open={open.has("cleanup")} onToggle={() => handlers.toggleFold("cleanup")} />
    </div>
  );
}

/** The Agents tab's body in its chosen mode. */
export function AgentsLens({ mode, ...props }: { mode: AgentsMode; lanes: LanesBoard; lineages: LineageBoard; scope: "project" | "all"; selectedLane: string | null; folds: readonly LensFold[]; handlers: LensHandlers; now: number }): ReactNode {
  return mode === "lineage" ? <LineageLens board={props.lineages} folds={props.folds} handlers={props.handlers} now={props.now} /> : <CheckoutLanes board={props.lanes} scope={props.scope} selectedLane={props.selectedLane} folds={props.folds} handlers={props.handlers} now={props.now} />;
}
