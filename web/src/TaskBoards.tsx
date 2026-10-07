import {
  ArrowRightIcon,
  CheckIcon,
  ChevronDownIcon,
  ChevronRightIcon,
  CircleDashedIcon,
  CircleDotIcon,
  ContrastIcon,
  EllipsisIcon,
  ExternalLinkIcon,
  FileTextIcon,
  GitBranchIcon,
  GitMergeIcon,
  GitPullRequestIcon,
  HouseIcon,
  ListChecksIcon,
  ListFilterIcon,
  LockIcon,
  PencilIcon,
  PlayIcon,
  PlusIcon,
  SquareTerminalIcon,
  TriangleAlertIcon,
  XIcon,
} from "lucide-react";
import { createContext, useContext, useEffect, useId, useLayoutEffect, useMemo, useRef, useState, type ComponentProps, type CSSProperties, type KeyboardEvent, type MouseEvent, type ReactNode, type RefObject } from "react";
import type { Actions } from "./actions";
import { AgentMark } from "./AgentMark";
import { lineTone, markTone, rowAccessibleName, rowLine } from "./agentRow";
import { CheckoutCardHint } from "./components/pr-card";
import { Elapsed, formatElapsed } from "./components/elapsed";
import { StatusMark } from "./components/status-mark";
import { Badge } from "./components/ui/badge";
import { Button } from "./components/ui/button";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuSeparator, DropdownMenuTrigger } from "./components/ui/dropdown-menu";
import { Input } from "./components/ui/input";
import { Kbd } from "./components/ui/kbd";
import { Popover, PopoverContent, PopoverTrigger } from "./components/ui/popover";
import { Switch } from "./components/ui/switch";
import { ToggleGroup, ToggleGroupItem } from "./components/ui/toggle-group";
import { Hint, Tooltip, TooltipContent, TooltipTrigger, useHintOpen } from "./components/ui/tooltip";
import { useInterfaceTranslation } from "./i18n/client";
import { statusText } from "./agentStatus";
import type { MessageKey } from "./i18n/catalogs";
import { requireInterfaceLanguage } from "./i18n/locale";
import { typing } from "./IssueDialogs";
import { previewRead, useCachedDetail } from "./issueDetails";
import { markdownPlainText } from "./markdownPlain";
import { cn } from "./lib/utils";
import { useShellStore } from "./store";
import { placeWithin, workerElk, type GraphEdge, type NodeBox, type Placement } from "./dependencyLayout";
import { useMeasuredPaths } from "./measuredPaths";
import { AgentMessageHint, PullRequestChip } from "./OverviewLenses";
import {
  NO_FILTER,
  STAGES,
  buildDependencies,
  filterActive,
  issueDate,
  readFailureText,
  stageCards,
  type BoardLabel,
  type BoardRow,
  type BoardScope,
  type CardSubIssues,
  type LayeredGraph,
  type IssueFilter,
  type LoosePullRequest,
  type LooseWorktree,
  type PrChip,
  type Stage,
  type TaskCard,
  type TasksBoard,
} from "./projectBoard";
import { laneCheckoutCard, relativeActivity } from "./projects";
import type { AgentRow, IssueDetail, IssueLabel, Task, Workspace } from "./snapshot";
import type { TasksMode } from "./ui";
import { holdsCommandKey } from "./host";

// The Issues views (PRD task-agents-views, reworked issue-first on
// 2026-09-28 and issues-only by PRD overview-lenses-issues), drawn the same
// for a Project and for All projects: `buildTasks` and `buildDependencies`
// decide every card and row, and this file only draws them and routes the
// clicks. A card is an issue. At rest it says three things, the issue, where
// its work is and who works on it, and only the operator's turn is coloured;
// hover shows its buttons in the id line's reserved slot, a half-second rest
// on a part opens that part's card, and each part has one destination: the
// card and its title the issue's panel, an agent row that agent's pane, the
// checkout chip its Workspace, the PR chip its row on the PRs tab, and ⌘-click
// GitHub (D-09, D-10). Hover, focus and rest publish nothing (B22).

/** A column shows this many cards, then the rest on request. */
const COLUMN_LIMIT = 20;
/** The List's backlog shows this many rows, then the rest on request. */
const LIST_BACKLOG_LIMIT = 8;
/** Done folds to this many lines before `+N`. */
const DONE_NAMES = 6;
/** A card's head shows at most this many labels (B1). */
const CARD_LABELS = 2;
/** A folded line's popover names this many items before `+N`. */
const FOLD_NAMES = 6;

/**
 * The issue an issue chip elsewhere on the Overview asked for (PRD
 * overview-lenses-tiles-agents B17, B24): its card wears the selection ring
 * and is brought into view.
 */
export const FocusedTask = createContext<string | null>(null);

/** What a board's cards ask of the page around them. */
export type BoardHandlers = {
  /** A card, its title, a done line or a List row: the issue's panel (D-08). */
  openPanel: (card: TaskCard) => void;
  /** The checkout chip and `O`: that checkout's Workspace. */
  openCheckout: (card: TaskCard) => void;
  /** Start and `S`: the Start dialog for a backlog issue. */
  startIssue: (card: TaskCard) => void;
  /** Edit on a Local issue: its panel, the title and body editable (D-41). */
  editIssue: (card: TaskCard) => void;
  /** The New issue dialog, for the page's Project or its default one. */
  newIssue: () => void;
  /** A page on GitHub: the issue, a pull request (⌘-click, the GitHub control). */
  openGitHub: (url: string, deviceId: string) => void;
  /** `N worktrees without an issue`: the Agents graph with its `Worktrees without agents` line open (B4). */
  showCheckouts: () => void;
  /** A PR chip, or `N PRs without an issue` with no row: the pull request's row on its Project's PRs tab (PRD overview-lenses-prs B21). */
  openPullRequestRow: (owner: Workspace, number: number | null) => void;
};

/** What Start and Edit say on a card and in the panel (B7). */
export const START_HINT: MessageKey = "board.startHint";
export const EDIT_HINT: MessageKey = "board.editHint";

/** The board's page state its cards read: the open panel's issue, and the agent in front. */
export type BoardPage = { panel: string | null; focusedPaneId: string | null };

/** ⌘-click (Ctrl-click off macOS) means GitHub wherever it lands (D-09). */
function gitHubClick(event: MouseEvent, url: string | null | undefined, deviceId: string, handlers: BoardHandlers): boolean {
  if (!holdsCommandKey(event) || !url) return false;
  event.preventDefault();
  event.stopPropagation();
  handlers.openGitHub(url, deviceId);
  return true;
}

/**
 * The card the arrow keys move to (B20): ↑↓ the next or previous card of the
 * same column, ←→ the card of the neighbouring column nearest the same
 * height; at a column's end, or with no column that way, focus stays.
 */
export function neighbourCard(from: HTMLElement, key: string): HTMLElement | null {
  const column = from.closest<HTMLElement>("[data-overview-column]");
  const board = from.closest<HTMLElement>("[data-tasks-board]");
  if (!column || !board) return null;
  const cards = (scope: HTMLElement) => [...scope.querySelectorAll<HTMLElement>("[data-issue-card]")];
  if (key === "ArrowUp" || key === "ArrowDown") {
    const own = cards(column);
    const at = own.indexOf(from);
    return own[at + (key === "ArrowDown" ? 1 : -1)] ?? null;
  }
  const columns = [...board.querySelectorAll<HTMLElement>("[data-overview-column]")].filter((value) => cards(value).length > 0);
  const next = columns[columns.indexOf(column) + (key === "ArrowRight" ? 1 : -1)];
  if (!next) return null;
  const middle = (value: HTMLElement) => {
    const rect = value.getBoundingClientRect();
    return rect.top + rect.height / 2;
  };
  const y = middle(from);
  return cards(next).reduce<HTMLElement | null>((best, value) => (best === null || Math.abs(middle(value) - y) < Math.abs(middle(best) - y) ? value : best), null);
}

function toggled<T>(set: Set<T>, value: T): Set<T> {
  const next = new Set(set);
  if (next.has(value)) next.delete(value);
  else next.add(value);
  return next;
}

export function TasksView({
  board,
  scope,
  page,
  actions,
  handlers,
  doneOpen,
  onToggleDone,
  filtered,
  onClearFilter,
}: {
  board: TasksBoard;
  scope: BoardScope;
  page: BoardPage;
  actions: Actions;
  handlers: BoardHandlers;
  doneOpen: boolean;
  onToggleDone: () => void;
  /** A filter hides cards; an empty board then offers to clear it rather than to add an issue. */
  filtered: boolean;
  onClearFilter: () => void;
}) {
  // Backlog always stands, so a new issue has somewhere to go; another
  // stage's column only while it holds a card or a line of work with no
  // issue. The tracks fill the page's width whatever the columns shown, so
  // a column keeps its width as stages come and go.
  const { t } = useInterfaceTranslation();
  const columns = STAGES.map(({ stage, labelKey }) => ({ stage, label: t(labelKey), cards: stageCards(board, stage) })).filter(
    (column) =>
      column.stage === "backlog" ||
      column.cards.length > 0 ||
      (column.stage === "working" && board.loose.worktrees.length > 0) ||
      (column.stage === "review" && board.loose.pullRequests.length > 0),
  );
  return (
    <div className="grid items-start gap-md px-lg pb-xl" style={{ gridTemplateColumns: "repeat(auto-fill, minmax(var(--home-column-width), 1fr))" }} data-tasks-board="true" data-overview-columns="tasks">
      {columns.map(({ stage, label, cards }) =>
        stage === "done" ? (
          <DoneColumn key={stage} label={label} cards={cards} scope={scope} open={doneOpen} onToggle={onToggleDone} page={page} actions={actions} handlers={handlers} />
        ) : (
          <StageColumn key={stage} stage={stage} label={label} board={board} cards={cards} page={page} actions={actions} handlers={handlers} filtered={filtered} onClearFilter={onClearFilter} />
        ),
      )}
    </div>
  );
}

/** A column head that stays in view while the page scrolls under it. */
function ColumnHead({ children, action }: { children: ReactNode; action?: ReactNode }) {
  return (
    <h2 className="sticky top-0 z-10 flex h-(--size-control) items-center gap-xs bg-background px-xs text-subhead font-semibold text-foreground">
      {children}
      {action ? <span className="ml-auto">{action}</span> : null}
    </h2>
  );
}

function Count({ value }: { value: string | number }) {
  return <span className="font-normal text-muted-foreground">{value}</span>;
}

/** A one-line button at a column's foot: `+N`, or the work with no issue folded. It passes a popover trigger's props through. */
function FoldLine({ children, data, ...props }: Omit<ComponentProps<"button">, "type"> & { data: Record<string, string> }) {
  return (
    <button
      {...props}
      type="button"
      className="flex w-full items-center gap-xs rounded-sm border border-border px-sm py-xxs text-left text-caption text-muted-foreground outline-none hover:bg-accent hover:text-foreground focus-visible:ring-1 focus-visible:ring-ring"
      {...data}
    >
      <span className="min-w-0 flex-1 truncate">{children}</span>
      <ChevronRightIcon aria-hidden="true" className="size-(--size-icon) shrink-0" />
    </button>
  );
}

/** The names a folded line's popover lists, the first few and `+N`. */
function foldNames(names: readonly string[]): string {
  const shown = names.slice(0, FOLD_NAMES).join(" · ");
  return names.length > FOLD_NAMES ? `${shown} · +${names.length - FOLD_NAMES}` : shown;
}

/**
 * Backlog, In progress or Review. Past `COLUMN_LIMIT` the rest wait behind
 * `+N`; In progress ends in `N worktrees without an issue` and Review in
 * `N PRs without an issue`, each only above zero (B4).
 */
function StageColumn({
  stage,
  label,
  board,
  cards,
  page,
  actions,
  handlers,
  filtered,
  onClearFilter,
}: {
  stage: Stage;
  label: string;
  board: TasksBoard;
  cards: TaskCard[];
  page: BoardPage;
  actions: Actions;
  handlers: BoardHandlers;
  filtered: boolean;
  onClearFilter: () => void;
}) {
  const { t } = useInterfaceTranslation();
  const [all, setAll] = useState(false);
  const shown = all ? cards : cards.slice(0, COLUMN_LIMIT);
  const hidden = cards.length - shown.length;
  const count = `${cards.length}${stage === "backlog" && board.overflow ? "+" : ""}`;
  const backlog = stage === "backlog";
  return (
    <section aria-label={label} data-overview-column={stage} className="flex min-w-0 flex-col gap-sm">
      <ColumnHead
        action={
          backlog ? (
            <Hint label={t("issue.newTitle")} shortcut={<Kbd>C</Kbd>}>
              <Button variant="ghost" size="icon-sm" aria-label={t("issue.newTitle")} onClick={handlers.newIssue} data-backlog-new-issue="true">
                <PlusIcon aria-hidden="true" />
              </Button>
            </Hint>
          ) : null
        }
      >
        <span>{label}</span>
        <Count value={count} />
        {backlog ? <BacklogSource board={board} /> : null}
      </ColumnHead>
      {shown.map((value) => (
        <IssueCardView key={value.id} card={value} page={page} actions={actions} handlers={handlers} />
      ))}
      {backlog && cards.length === 0 ? filtered ? <NoMatch onClear={onClearFilter} /> : <EmptyBacklog board={board} onNew={handlers.newIssue} /> : null}
      {hidden > 0 ? (
        <FoldLine onClick={() => setAll(true)} data={{ "data-column-more": stage }}>
          {backlog ? t("board.moreRecent", { count: hidden }) : `+${hidden}`}
        </FoldLine>
      ) : null}
      {stage === "working" ? <LooseWorktreesLine worktrees={board.loose.worktrees} onOpen={handlers.showCheckouts} /> : null}
      {stage === "review" ? <LoosePullRequestsLine pullRequests={board.loose.pullRequests} handlers={handlers} /> : null}
    </section>
  );
}

/** `N worktrees without an issue`: its popover says where it goes and names them; its click opens the Agents graph with that line open (B4). */
function LooseWorktreesLine({ worktrees, onOpen }: { worktrees: readonly LooseWorktree[]; onOpen: () => void }) {
  const { t } = useInterfaceTranslation();
  if (worktrees.length === 0) return null;
  return (
    <Hint label={t("board.showAgents", { names: foldNames(worktrees.map((value) => value.branch)) })}>
      <FoldLine onClick={onOpen} data={{ "data-loose-worktrees": String(worktrees.length) }}>
        {t("board.looseWorktrees", { count: worktrees.length })}
      </FoldLine>
    </Hint>
  );
}

/**
 * `N PRs without an issue`: its popover names them and says where it goes; its
 * click opens the PRs tab, where each has an issue cell to link (PRD
 * overview-lenses-prs B21; on the Overview of every project, the first one's
 * Project).
 */
function LoosePullRequestsLine({ pullRequests, handlers }: { pullRequests: readonly LoosePullRequest[]; handlers: BoardHandlers }) {
  const { t } = useInterfaceTranslation();
  const first = pullRequests[0];
  if (!first) return null;
  return (
    <Hint label={t("board.showPrs", { names: foldNames(pullRequests.map((value) => `#${value.number}`)) })}>
      <FoldLine onClick={() => handlers.openPullRequestRow(first.owner, null)} data={{ "data-loose-prs": String(pullRequests.length) }}>
        {t("board.loosePrs", { count: pullRequests.length })}
      </FoldLine>
    </Hint>
  );
}

/** A source still reading, or one that could not be read, as one small mark beside the Backlog count (design 9, 13). */
function BacklogSource({ board }: { board: TasksBoard }) {
  const { t } = useInterfaceTranslation();
  if (board.source.failure) {
    return (
      <Hint label={readFailureText(board.source.failure, t)}>
        <span className="text-warning" tabIndex={0} data-backlog-source-failure="true">
          <TriangleAlertIcon aria-hidden="true" className="size-(--size-icon-sm)" />
        </span>
      </Hint>
    );
  }
  if (board.source.reading) {
    return (
      <span className="text-caption font-normal text-muted-foreground" data-backlog-reading="true">
        {t("board.reading")}
      </span>
    );
  }
  return null;
}

/** An empty backlog: one quiet line and the way to add the first issue. */
function EmptyBacklog({ board, onNew }: { board: TasksBoard; onNew: () => void }) {
  const { t } = useInterfaceTranslation();
  if (board.source.reading) return null;
  return (
    <div className="flex flex-col items-start gap-xs rounded-md border border-dashed border-border p-sm text-caption text-muted-foreground" data-backlog-empty="true">
      <p>{t("board.empty")}</p>
      <Button variant="secondary" size="sm" onClick={onNew} data-backlog-empty-new="true">
        <PlusIcon aria-hidden="true" />
        {t("issue.newTitle")}
      </Button>
    </div>
  );
}

/** A filter that keeps nothing: it says so and offers to clear it (design 9). */
function NoMatch({ onClear }: { onClear: () => void }) {
  const { t } = useInterfaceTranslation();
  return (
    <div className="flex flex-col items-start gap-xs rounded-md border border-dashed border-border p-sm text-caption text-muted-foreground" data-filter-empty="true">
      <p>{t("board.noMatch")}</p>
      <Button variant="secondary" size="sm" onClick={onClear} data-filter-clear="true">
        {t("board.clearFilter")}
      </Button>
    </div>
  );
}

/**
 * Done starts folded to one line per issue, its id, title and the number of
 * the pull request that closed it (on All projects, one line per Project with
 * its count); resting on the number says when it merged, a line opens the
 * issue's panel, and the head unfolds it into cards (B3).
 */
function DoneColumn({
  label,
  cards,
  scope,
  open,
  onToggle,
  page,
  actions,
  handlers,
}: {
  label: string;
  cards: TaskCard[];
  scope: BoardScope;
  open: boolean;
  onToggle: () => void;
  page: BoardPage;
  actions: Actions;
  handlers: BoardHandlers;
}) {
  const { t, i18n } = useInterfaceTranslation();
  const [allNames, setAllNames] = useState(false);
  const perProject = useMemo(() => {
    const counts = new Map<string, { label: string; count: number }>();
    for (const value of cards) {
      const known = counts.get(value.place.projectId);
      if (known) known.count += 1;
      else counts.set(value.place.projectId, { label: value.place.projectLabel, count: 1 });
    }
    return [...counts.entries()];
  }, [cards]);
  const names = allNames ? cards : cards.slice(0, DONE_NAMES);
  const nameLine = "flex w-full min-w-0 items-center gap-xs rounded-xs px-xs py-xxs text-left text-caption text-subtle-foreground outline-none hover:bg-accent hover:text-foreground focus-visible:ring-1 focus-visible:ring-ring";
  return (
    <section aria-label={label} data-overview-column="done" data-collapsed={open ? "false" : "true"} className="flex min-w-0 flex-col gap-sm">
      <ColumnHead>
        <button type="button" aria-expanded={open} onClick={onToggle} data-overview-column-toggle="done" className="inline-flex items-center gap-xs rounded-xs outline-none hover:text-foreground focus-visible:ring-1 focus-visible:ring-ring">
          <span>{label}</span>
          <Count value={cards.length} />
          {open ? <ChevronDownIcon aria-hidden="true" className="size-(--size-icon)" /> : <ChevronRightIcon aria-hidden="true" className="size-(--size-icon)" />}
        </button>
      </ColumnHead>
      {open ? (
        cards.map((value) => <IssueCardView key={value.id} card={value} page={page} actions={actions} handlers={handlers} />)
      ) : scope === "all" ? (
        <ul className="flex flex-col" data-overview-collapsed-names="done">
          {perProject.map(([id, row]) => (
            <li key={id}>
              <button type="button" className={cn(nameLine, "font-mono")} onClick={onToggle} data-done-project={id}>
                <GitMergeIcon aria-hidden="true" className="size-(--size-icon-sm) shrink-0 text-pr-merged" />
                <span className="min-w-0 truncate">{row.label}</span>
                <span className="text-muted-foreground">· {row.count}</span>
              </button>
            </li>
          ))}
        </ul>
      ) : (
        <ul className="flex flex-col" data-overview-collapsed-names="done">
          {names.map((value) => (
            <li key={value.id} className="flex min-w-0 items-center">
              <button type="button" className={cn(nameLine, page.panel === value.task.key && "bg-accent text-foreground")} onClick={() => handlers.openPanel(value)} data-done-line={value.task.key}>
                <TaskGlyph task={value.task} className="text-pr-merged" />
                <span className="shrink-0 font-mono">{value.task.id}</span>
                <span className="min-w-0 flex-1 truncate">{value.title}</span>
              </button>
              {value.pr ? (
                <Hint label={[t("board.mergedPr", { number: String(value.pr.number) }), value.mergedAt === null ? null : issueDate(value.mergedAt, i18n.language)].filter(Boolean).join(" · ")}>
                  <span className="inline-flex shrink-0 items-center gap-xxs px-xs font-mono text-caption text-muted-foreground" tabIndex={0} data-done-pr={value.pr.number}>
                    <GitMergeIcon aria-hidden="true" className="size-(--size-icon-sm)" />
                    {value.pr.number}
                  </span>
                </Hint>
              ) : null}
            </li>
          ))}
          {cards.length > names.length ? (
            <li>
              <button type="button" className="px-xs text-caption text-muted-foreground hover:text-foreground" onClick={() => setAllNames(true)} data-done-more={cards.length - names.length}>
                +{cards.length - names.length}
              </button>
            </li>
          ) : null}
        </ul>
      )}
    </section>
  );
}

/** Board, List and Dependencies, a mode of the Issues view rather than a tab: at the right end of the facts line (B21), and on All projects' tab row. */
export function TasksModeToggle({ mode, onChange }: { mode: TasksMode; onChange: (mode: TasksMode) => void }) {
  const { t } = useInterfaceTranslation();
  return (
    <ToggleGroup type="single" value={mode} onValueChange={(value) => value && onChange(value as TasksMode)} aria-label={t("board.mode.label")} data-tasks-mode={mode}>
      <ToggleGroupItem value="board" data-tasks-mode-item="board">
        {t("board.mode.board")}
      </ToggleGroupItem>
      <ToggleGroupItem value="list" data-tasks-mode-item="list">
        {t("board.mode.list")}
      </ToggleGroupItem>
      <ToggleGroupItem value="dependencies" data-tasks-mode-item="dependencies">
        {t("board.mode.dependencies")}
      </ToggleGroupItem>
    </ToggleGroup>
  );
}

/**
 * The Issues filter, left of the mode control (B21): words in an issue's id
 * or title, the operator's turn only, and the labels the board's cards carry,
 * several at once, a card passing with any one of them; a lit label is taken
 * off by pressing it again. The label choices stand only while a card carries
 * a label or one is picked. The icon is filled while a filter holds; it is
 * page state and sends nothing.
 */
export function IssueFilterControl({ filter, labels, onChange }: { filter: IssueFilter; labels: readonly BoardLabel[]; onChange: (filter: IssueFilter) => void }) {
  const { t } = useInterfaceTranslation();
  const active = filterActive(filter);
  return (
    <Popover>
      <Hint label={t(active ? "board.filterActive" : "board.filter")}>
        <PopoverTrigger asChild>
          <Button variant={active ? "secondary" : "ghost"} size="icon-sm" data-issue-filter={active ? "active" : "none"}>
            <ListFilterIcon aria-hidden="true" className={cn(active && "text-primary")} />
          </Button>
        </PopoverTrigger>
      </Hint>
      <PopoverContent align="end" className="flex max-h-(--radix-popover-content-available-height) flex-col gap-sm overflow-y-auto" data-issue-filter-panel="true">
        <Input autoFocus value={filter.query} placeholder={t("board.filterQuery")} onChange={(event) => onChange({ ...filter, query: event.target.value })} data-issue-filter-query="true" />
        <label className="flex items-center justify-between gap-sm text-caption text-foreground">
          {t("board.myTurnOnly")}
          <Switch checked={filter.turn} onCheckedChange={(turn) => onChange({ ...filter, turn })} data-issue-filter-turn="true" />
        </label>
        {labels.length > 0 ? (
          <div className="flex flex-col gap-xs" data-issue-filter-labels="true">
            <span className="text-caption text-muted-foreground">{t("board.filterLabels")}</span>
            <ToggleGroup type="multiple" value={[...filter.labels]} onValueChange={(names) => onChange({ ...filter, labels: names })} aria-label={t("board.filterLabels")} className="w-full flex-wrap">
              {labels.map(({ label, count }) => (
                <ToggleGroupItem key={label.name} value={label.name} size="sm" className="min-w-0 max-w-full" data-issue-filter-label={label.name}>
                  <IssueLabelDot color={label.color} />
                  <span className="min-w-0 truncate">{label.name}</span>
                  {count > 0 ? <span className="font-mono text-muted-foreground">{count}</span> : null}
                </ToggleGroupItem>
              ))}
            </ToggleGroup>
          </div>
        ) : null}
        {active ? (
          <Button variant="ghost" size="sm" className="self-start" onClick={() => onChange(NO_FILTER)} data-issue-filter-reset="true">
            {t("board.clearFilter")}
          </Button>
        ) : null}
      </PopoverContent>
    </Popover>
  );
}

/** The mark a task's id carries: an issue's circle-dot, a local issue's page (D-06). */
export function TaskGlyph({ task, className }: { task: Task; className?: string }) {
  const Glyph = task.source === "github" ? CircleDotIcon : FileTextIcon;
  return <Glyph aria-hidden="true" className={cn("size-(--size-icon-sm) shrink-0", className)} />;
}

/** A label's dot in the colour its source gives it, grey without one. */
function IssueLabelDot({ color }: { color: string | null }) {
  // The colour is the source's data, not a design value, so it reaches the dot through a custom property.
  const tint = color ? { "--issue-label": `#${color}` } : undefined;
  return <span aria-hidden="true" className={cn("size-(--issue-label-dot) shrink-0 rounded-full", color ? "bg-(--issue-label)" : "bg-muted-foreground")} style={tint as CSSProperties} />;
}

/** One label as its source colours it: a dot in the label's colour and its name. */
export function IssueLabelView({ label }: { label: IssueLabel }) {
  return (
    <span className="inline-flex min-w-0 items-center gap-xxs text-caption text-muted-foreground" data-issue-label={label.name}>
      <IssueLabelDot color={label.color} />
      <span className="truncate">{label.name}</span>
    </span>
  );
}

/** A card's or a List row's labels, at most `CARD_LABELS` of them, from the task itself (B1). */
function CardLabels({ labels, className }: { labels: readonly IssueLabel[]; className?: string }) {
  if (labels.length === 0) return null;
  return (
    <span className={cn("flex min-w-0 items-center gap-xs", className)} data-card-labels="true">
      {labels.slice(0, CARD_LABELS).map((label) => (
        <IssueLabelView key={label.name} label={label} />
      ))}
    </span>
  );
}

/**
 * An issue card (B1, B2, B5-B9). The id line holds the source glyph, the id,
 * at most two of its labels, and at its end a slot
 * whose buttons show only under the pointer or focus, so the card keeps its
 * height. Under it the title in two lines, the lock line, then in progress
 * and review the checkout and PR chips (review adds the CI mark and the
 * review word), and at most two agents, the operator's turn first, and `+N`.
 * Only the operator's turn is coloured; a done card is dimmed.
 */
export function IssueCardView({ card, page, actions, handlers, graph }: { card: TaskCard; page: BoardPage; actions: Actions; handlers: BoardHandlers; graph?: { status: string | null } }) {
  const { t } = useInterfaceTranslation();
  const { task, checkout, owner } = card;
  const focusedTask = useContext(FocusedTask);
  const cached = useCachedDetail(task.key);
  const [previewPinned, setPreviewPinned] = useState(false);
  const selected = page.panel === task.key || focusedTask === task.key;
  const blocked = card.blockedBy.length > 0;
  const dimmed = card.stage === "done" || (graph !== undefined && blocked);
  const chips = card.chip !== null || card.pr !== null;
  const createdAge = relativeActivity(task.created_at_unix_ms, Date.now(), t);
  const onKeyDown = (event: KeyboardEvent<HTMLElement>) => {
    if (event.metaKey || event.ctrlKey || event.altKey || typing(event.target)) return;
    const self = event.target === event.currentTarget;
    if (event.key.startsWith("Arrow") && self) {
      const next = neighbourCard(event.currentTarget, event.key);
      event.preventDefault();
      next?.focus();
      return;
    }
    if (event.key === "Enter" && self) {
      event.preventDefault();
      handlers.openPanel(card);
    } else if (event.key === " " && self) {
      event.preventDefault();
      setPreviewPinned((value) => !value);
    } else if (event.key.toLowerCase() === "s" && card.first === "start") {
      event.preventDefault();
      handlers.startIssue(card);
    } else if (event.key.toLowerCase() === "o" && card.first === "workspace") {
      event.preventDefault();
      handlers.openCheckout(card);
    }
  };
  return (
    <article
      className={cn(
        "group/card flex min-w-0 cursor-pointer flex-col gap-xs rounded-md border bg-card p-sm outline-none hover:bg-accent focus-visible:ring-1 focus-visible:ring-ring",
        card.needsYou ? "border-warning" : selected ? "border-primary" : "border-border",
        dimmed && "opacity-(--opacity-secondary)",
      )}
      tabIndex={0}
      aria-label={[task.id, card.title].filter(Boolean).join(" ")}
      data-issue-card={task.key}
      data-overview-card={card.id}
      data-task-key={task.key}
      data-stage={card.stage}
      data-selected={selected ? "true" : undefined}
      data-focused-task={focusedTask === task.key ? "true" : undefined}
      data-needs-you={card.needsYou ? "true" : undefined}
      data-blocked={blocked ? "true" : undefined}
      onClick={(event) => {
        if (!gitHubClick(event, task.url, owner.device_id, handlers)) handlers.openPanel(card);
      }}
      onFocus={(event) => {
        // With the panel open, the panel follows the card the keyboard moves to (B10, B20).
        if (event.target === event.currentTarget && page.panel !== null && page.panel !== task.key) handlers.openPanel(card);
      }}
      onBlur={(event) => {
        if (!event.currentTarget.contains(event.relatedTarget as Node | null)) setPreviewPinned(false);
      }}
      onKeyDown={onKeyDown}
    >
      <div className="flex h-(--size-control-sm) min-w-0 items-center gap-xs">
        <IssueIdPreview card={card} detail={cached} pinned={previewPinned} actions={actions} handlers={handlers} />
        <CardLabels labels={task.labels ?? []} />
        {card.project ? (
          <span className="min-w-0 truncate text-caption text-muted-foreground" data-card-project="true">
            {card.project}
          </span>
        ) : null}
        <span className="flex-1" />
        {createdAge ? (
          <span className="shrink-0 font-mono text-caption text-muted-foreground" data-issue-created-age="true" aria-label={t("board.createdAgo", { age: createdAge })}>
            {createdAge}
          </span>
        ) : null}
        {card.sourceFailure ? <SourceFailureMark text={readFailureText(card.sourceFailure, t)} /> : null}
        {graph?.status ? (
          <span className="shrink-0 text-caption text-muted-foreground group-focus-within/card:hidden group-hover/card:hidden" data-card-status="true">
            {graph.status}
          </span>
        ) : null}
        <CardActions card={card} actions={actions} handlers={handlers} />
      </div>
      <p className="line-clamp-2 min-w-0 break-words text-title font-semibold text-foreground" data-card-title="true">
        {card.title}
      </p>
      {blocked ? <BlockedLine card={card} /> : null}
      {card.subIssues ? <SubIssuesLine subIssues={card.subIssues} /> : null}
      {chips ? (
        <div className="flex min-w-0 flex-wrap items-center gap-x-sm gap-y-xxs font-mono text-caption text-muted-foreground" data-card-chips="true">
          {card.chip && checkout ? <CheckoutChipView card={card} handlers={handlers} /> : null}
          {card.pr && checkout ? (
            <span className="inline-flex items-center gap-xxs" onClick={(event) => event.stopPropagation()}>
              <PullRequestChip project={owner} checkout={checkout} onOpen={(url) => handlers.openGitHub(url, owner.device_id)} onRow={(number) => handlers.openPullRequestRow(owner, number)} now={Date.now()} />
              {card.stage === "review" ? <ReviewMarks pr={card.pr} /> : null}
            </span>
          ) : null}
        </div>
      ) : null}
      {card.shown.length > 0 ? (
        <ul className="-mx-xs flex flex-col" role="list" data-overview-rows="true">
          {card.shown.map((agent) => (
            <CardAgentRow key={agent.pane_id} agent={agent} depth={0} place={card.chip?.branch ?? ""} selected={agent.pane_id === page.focusedPaneId} onOpen={actions.openAgent} />
          ))}
        </ul>
      ) : null}
      {card.more > 0 ? (
        <Hint label={card.rows.map((row) => row.agent.identity_label).join(", ")}>
          <span className="w-fit text-caption text-muted-foreground" data-overview-more={card.more} tabIndex={0} onClick={(event) => event.stopPropagation()}>
            +{card.more}
          </span>
        </Hint>
      ) : null}
    </article>
  );
}

/** A failed source read as one small ⚠ whose popover says what failed and how old the value is (B17). */
function SourceFailureMark({ text }: { text: string }) {
  return (
    <Hint label={text}>
      <span className="shrink-0 text-warning" data-source-failure="true" tabIndex={0} onClick={(event) => event.stopPropagation()}>
        <TriangleAlertIcon aria-hidden="true" className="size-(--size-icon-sm)" />
      </span>
    </Hint>
  );
}

/** The yellow lock and the ids of the issues that have to finish first; starting is still allowed (B5). */
function BlockedLine({ card }: { card: TaskCard }) {
  const { t } = useInterfaceTranslation();
  const blocked = t("board.blockedBy", { issues: card.blockedBy.map((blocker) => blocker.label).join(", ") });
  return (
    <Hint label={blocked}>
      <p className="flex w-fit min-w-0 items-center gap-xxs text-caption text-warning" data-blocked-by={card.blockedBy.map((blocker) => blocker.key).join(" ")} tabIndex={0}>
        <LockIcon aria-hidden="true" className="size-(--size-icon-sm) shrink-0" />
        <span className="truncate">{blocked}</span>
      </p>
    </Hint>
  );
}

/** GitHub's own progress over the issue's sub-issues, `하위 3/5`, as it counts them. */
function SubIssuesLine({ subIssues }: { subIssues: CardSubIssues }) {
  const { t } = useInterfaceTranslation();
  return (
    <p className="flex w-fit min-w-0 items-center gap-xxs text-caption text-muted-foreground" data-sub-issues={`${subIssues.completed}/${subIssues.total}`}>
      <ListChecksIcon aria-hidden="true" className="size-(--size-icon-sm) shrink-0" />
      <span className="truncate">{t("board.subIssues", { completed: subIssues.completed, total: subIssues.total })}</span>
    </p>
  );
}

/** Where the issue's work is (B2): the branch, `↑N`, and `N files` in warning; it opens that Workspace, its half-second card is the checkout card (B8, B9). */
function CheckoutChipView({ card, handlers }: { card: TaskCard; handlers: BoardHandlers }) {
  const { t } = useInterfaceTranslation();
  const { chip, checkout, owner } = card;
  if (!chip || !checkout) return null;
  const Glyph = chip.primary ? HouseIcon : GitBranchIcon;
  return (
    <CheckoutCardHint
      card={laneCheckoutCard(owner, checkout, Date.now(), t)}
      description={`${chip.branch} · ${checkout.path}`}
      onOpenPullRequest={(url) => handlers.openGitHub(url, owner.device_id)}
      onOpenWorkspace={() => handlers.openCheckout(card)}
    >
      <button
        type="button"
        aria-label={t("board.workspaceName", { branch: chip.branch })}
        className="inline-flex min-w-0 max-w-full items-center gap-xxs rounded-xs outline-none hover:text-foreground focus-visible:ring-1 focus-visible:ring-ring"
        data-card-checkout={checkout.id}
        onClick={(event) => {
          event.stopPropagation();
          handlers.openCheckout(card);
        }}
      >
        <Glyph aria-hidden="true" className="size-(--size-icon-sm) shrink-0" />
        <span className="min-w-0 max-w-(--home-collapsed-width) truncate">{chip.branch}</span>
        {chip.ahead !== null ? <span data-fact="ahead">↑{chip.ahead}</span> : null}
        {chip.files !== null ? (
          <span className="text-warning" data-fact="files">
            {t("issue.changedFiles", { count: chip.files })}
          </span>
        ) : null}
      </button>
    </CheckoutCardHint>
  );
}

export const PR_TONE: Record<PrChip["tone"], string> = {
  open: "text-pr-open",
  draft: "text-pr-draft",
  merged: "text-pr-merged",
  closed: "text-pr-closed",
};

export const CHECKS_LABEL: Record<"passing" | "failed" | "pending", MessageKey> = {
  passing: "requests.checks.passing",
  failed: "requests.checks.failed",
  pending: "requests.checks.pending",
};

export const REVIEW: Record<NonNullable<PrChip["review"]>, { label: MessageKey; tone: string }> = {
  review_required: { label: "board.review.required", tone: "text-muted-foreground" },
  changes_requested: { label: "board.review.changes", tone: "text-warning" },
  approved: { label: "board.review.approved", tone: "text-success" },
};

/** The CI mark once read: passing, failed or still running. */
export function ChecksMark({ checks }: { checks: keyof typeof CHECKS_LABEL }) {
  const { t } = useInterfaceTranslation();
  return (
    <span role="img" aria-label={t(CHECKS_LABEL[checks])} className={checks === "passing" ? "text-success" : checks === "failed" ? "text-destructive" : "text-muted-foreground"} data-pr-checks={checks}>
      {checks === "passing" ? <CheckIcon aria-hidden="true" className="size-(--size-icon-sm)" /> : checks === "failed" ? <XIcon aria-hidden="true" className="size-(--size-icon-sm)" /> : <StatusMark symbol="●" className="size-(--size-icon-sm)" />}
    </span>
  );
}

/** A review card's CI mark once read and the one word of the review GitHub asks for (B2). */
export function ReviewMarks({ pr, review = true }: { pr: PrChip; review?: boolean }) {
  const { t } = useInterfaceTranslation();
  const asked = review && pr.review ? REVIEW[pr.review] : null;
  return (
    <>
      {pr.checks ? <ChecksMark checks={pr.checks} /> : null}
      {asked ? (
        <span className={cn("font-sans", asked.tone)} data-pr-review={pr.review}>
          {t(asked.label)}
        </span>
      ) : null}
    </>
  );
}

/**
 * An agent on an issue card or in its panel: its mark, provider, title and
 * age, then the core's line, the question in warning. The row opens that
 * agent's pane; resting half a second on its line opens everything it last
 * said (B8, B9). A delegated child is indented one step per level (B13).
 */
export function CardAgentRow({ agent, depth, place, selected, onOpen }: { agent: AgentRow; depth: number; place: string; selected: boolean; onOpen: (paneId: string) => void }) {
  const { t } = useInterfaceTranslation();
  const said = rowLine(agent);
  const turn = agent.group === "needs_you" || agent.unread;
  const open = () => onOpen(agent.pane_id);
  return (
    <li
      className={cn("relative flex min-w-0 flex-col gap-xxs rounded-sm py-xxs pr-xs hover:bg-muted", selected && "bg-secondary")}
      style={{ paddingLeft: `calc(var(--spacing-xs) + ${depth} * var(--size-lineage-indent))` }}
      data-card-agent={agent.pane_id}
      data-depth={depth}
      onClick={(event) => event.stopPropagation()}
    >
      <button type="button" aria-label={rowAccessibleName(t, agent, null)} className="absolute inset-0 rounded-sm outline-none focus-visible:ring-1 focus-visible:ring-ring" onClick={open} data-agent-open={agent.pane_id} />
      <span className="pointer-events-none relative flex min-w-0 items-center gap-xs">
        <StatusMark symbol={agent.symbol} className={markTone(agent)} />
        <AgentMark kind={agent.agent_kind} />
        <span className={cn("min-w-0 flex-1 truncate text-body", turn ? "text-foreground" : "text-subtle-foreground")}>{agent.identity_label}</span>
        <Elapsed since={agent.changed_at_unix_ms} className="shrink-0 font-mono text-caption text-muted-foreground" />
      </span>
      {said ? (
        <span className="relative pl-(--size-icon)">
          <AgentMessageHint agent={agent} place={place} line={said.text} tone={lineTone(said, agent)} onOpen={open} />
        </span>
      ) : null}
    </li>
  );
}

/**
 * The issue's id at the head of its card: a click opens its panel, ⌘-click
 * GitHub, and a half-second rest (or Space on the card) opens the preview
 * card, the id, labels, state, title, the body's first three lines, the
 * author, the date and the comment count (B8, D-42). The preview reads the
 * issue once, and shows its body only once read.
 */
function IssueIdPreview({ card, detail, pinned, actions, handlers }: { card: TaskCard; detail: IssueDetail | null; pinned: boolean; actions: Actions; handlers: BoardHandlers }) {
  const { open, onOpenChange, triggerProps } = useHintOpen();
  const { task, owner } = card;
  const shown = open || pinned;
  const read = () => previewRead(task.key, () => actions.requestIssueDetail(card.place.projectId, task.key));
  if (!task.id) return null;
  return (
    <Tooltip
      open={shown}
      onOpenChange={(next) => {
        onOpenChange(next);
        if (next) read();
      }}
      disableHoverableContent={false}
    >
      <TooltipTrigger asChild {...triggerProps}>
        <button
          type="button"
          className="inline-flex shrink-0 items-center gap-xxs rounded-xs font-mono text-caption text-muted-foreground outline-none hover:text-foreground focus-visible:ring-1 focus-visible:ring-ring"
          data-task-id={task.id}
          aria-label={[task.id, card.idHelp].filter(Boolean).join(" · ")}
          onClick={(event) => {
            event.stopPropagation();
            if (!gitHubClick(event, task.url, owner.device_id, handlers)) handlers.openPanel(card);
          }}
          onFocus={() => {
            if (pinned) read();
          }}
        >
          <TaskGlyph task={task} />
          {task.id}
        </button>
      </TooltipTrigger>
      <TooltipContent side="bottom" align="start" className="pointer-events-auto w-(--size-pr-popover) max-w-(--radix-tooltip-content-available-width) text-left text-wrap rounded-md p-md" data-issue-preview={task.key}>
        <IssuePreviewBody card={card} detail={detail} />
      </TooltipContent>
    </Tooltip>
  );
}

function IssuePreviewBody({ card, detail }: { card: TaskCard; detail: IssueDetail | null }) {
  const { t, i18n } = useInterfaceTranslation();
  const { task } = card;
  const byline = [detail?.author ?? null, detail?.created_at_unix_ms != null ? issueDate(detail.created_at_unix_ms, i18n.language) : null, detail?.comment_count != null ? t("issue.commentCount", { count: detail.comment_count }) : null].filter(Boolean).join(" · ");
  return (
    <div className="flex flex-col gap-xs">
      <span className="flex min-w-0 items-center gap-xs text-caption text-muted-foreground">
        <TaskGlyph task={task} />
        <span className="font-mono">{task.id}</span>
        {(detail?.labels ?? []).slice(0, CARD_LABELS).map((label) => (
          <IssueLabelView key={label.name} label={label} />
        ))}
        <span className="flex-1" />
        <span className={task.open ? "text-success" : "text-muted-foreground"}>{t(task.open ? "issue.state.open" : "issue.state.closed")}</span>
      </span>
      <span className="line-clamp-2 text-body font-semibold text-foreground">{card.title}</span>
      {detail?.body ? (
        <p className="line-clamp-3 whitespace-pre-line break-words text-caption text-subtle-foreground" data-issue-preview-body="true">
          {markdownPlainText(detail.body)}
        </p>
      ) : null}
      {byline ? <span className="text-caption text-muted-foreground">{byline}</span> : null}
    </div>
  );
}

/**
 * The card's buttons in the id line's reserved slot, shown under the pointer
 * or focus (B6): the backlog's `▷ Start` and `S`, the Workspace icon and `O`
 * in progress, the PR icon in review, a Local issue's edit icon, and `⋯`
 * with Start, Workspace, GitHub, Edit and a Local issue's close. Each has its
 * popover (B7, D-10).
 */
function CardActions({ card, actions, handlers }: { card: TaskCard; actions: Actions; handlers: BoardHandlers }) {
  const { t } = useInterfaceTranslation();
  const { owner } = card;
  const reveal = "invisible group-focus-within/card:visible group-hover/card:visible has-data-[state=open]:visible";
  const stop = (event: MouseEvent) => event.stopPropagation();
  return (
    <span className={cn("flex shrink-0 items-center gap-xxs", reveal)} data-card-actions="true" onClick={stop}>
      {card.first === "start" ? (
        <Hint label={t(START_HINT)} shortcut={<Kbd>S</Kbd>}>
          <Button variant="secondary" size="sm" onClick={() => handlers.startIssue(card)} data-card-start="true">
            <PlayIcon aria-hidden="true" />
            {t("common.start")}
          </Button>
        </Hint>
      ) : null}
      {card.first === "start" ? <Kbd>S</Kbd> : null}
      {card.first === "workspace" ? (
        <Hint label={t("issue.openWorkspace")} shortcut={<Kbd>O</Kbd>}>
          <Button variant="ghost" size="icon-sm" onClick={() => handlers.openCheckout(card)} data-card-workspace="true">
            <SquareTerminalIcon aria-hidden="true" />
          </Button>
        </Hint>
      ) : null}
      {card.first === "workspace" ? <Kbd>O</Kbd> : null}
      {card.first === "pull_request" && card.pr ? (
        <Hint label={t("issue.openPr", { number: String(card.pr.number) })}>
          <Button variant="ghost" size="icon-sm" onClick={() => handlers.openGitHub(card.pr!.url, owner.device_id)} data-card-pr="true">
            <GitPullRequestIcon aria-hidden="true" />
          </Button>
        </Hint>
      ) : null}
      {card.editable ? (
        <Hint label={t(EDIT_HINT)}>
          <Button variant="ghost" size="icon-sm" onClick={() => handlers.editIssue(card)} data-card-edit="true">
            <PencilIcon aria-hidden="true" />
          </Button>
        </Hint>
      ) : null}
      <IssueMenu card={card} actions={actions} handlers={handlers} trigger={<EllipsisIcon aria-hidden="true" />} />
    </span>
  );
}

/** `⋯` on a card or in its panel: Start, Workspace, GitHub, Edit, and a Local issue's close or reopen. */
export function IssueMenu({ card, actions, handlers, trigger }: { card: TaskCard; actions: Actions; handlers: BoardHandlers; trigger: ReactNode }) {
  const { t } = useInterfaceTranslation();
  const { task, checkout, owner } = card;
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button variant="ghost" size="icon-sm" aria-label={t("board.issueActions")} data-card-menu="true">
          {trigger}
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end">
        {card.canStart ? <DropdownMenuItem onSelect={() => handlers.startIssue(card)}>{t("common.start")}</DropdownMenuItem> : null}
        {checkout ? <DropdownMenuItem onSelect={() => handlers.openCheckout(card)}>{t("issue.workspace")}</DropdownMenuItem> : null}
        {task.url ? (
          <DropdownMenuItem onSelect={() => handlers.openGitHub(task.url as string, owner.device_id)}>
            <ExternalLinkIcon aria-hidden="true" />
            GitHub
          </DropdownMenuItem>
        ) : null}
        {card.editable ? <DropdownMenuItem onSelect={() => handlers.editIssue(card)}>{t("common.edit")}</DropdownMenuItem> : null}
        {card.editable ? (
          <>
            <DropdownMenuSeparator />
            <DropdownMenuItem onSelect={() => actions.setIssueOpen(task.key, !task.open)} data-card-issue-open={task.open ? "close" : "reopen"}>
              {t(task.open ? "board.closeIssue" : "board.reopenIssue")}
            </DropdownMenuItem>
          </>
        ) : null}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

/** The stage glyph a List row leads with. */
function StageGlyph({ stage }: { stage: Stage }) {
  const glyph = {
    backlog: <CircleDashedIcon className="text-muted-foreground" />,
    working: <ContrastIcon className="text-warning" />,
    review: <GitPullRequestIcon className="text-pr-open" />,
    done: <GitMergeIcon className="text-pr-merged" />,
  }[stage];
  return (
    <span aria-hidden="true" className="inline-flex shrink-0 [&_svg]:size-(--size-icon)">
      {glyph}
    </span>
  );
}

/** A group head of the List: chevron, label and count, sticky while the page scrolls. */
function GroupHead({ open, onToggle, label, count, data, children }: { open: boolean; onToggle: () => void; label: string; count: number; data: Record<string, string>; children?: ReactNode }) {
  return (
    <h2 className="sticky top-0 z-10 flex h-(--size-control) items-center gap-xs bg-background text-subhead font-semibold">
      <button type="button" aria-expanded={open} onClick={onToggle} className="inline-flex items-center gap-xs rounded-xs text-foreground outline-none focus-visible:ring-1 focus-visible:ring-ring" {...data}>
        {open ? <ChevronDownIcon aria-hidden="true" className="size-(--size-icon) text-muted-foreground" /> : <ChevronRightIcon aria-hidden="true" className="size-(--size-icon) text-muted-foreground" />}
        <span>{label}</span>
        <Count value={count} />
      </button>
      {children}
    </h2>
  );
}

/**
 * The List mode: the same issues, one row each, grouped by stage with the
 * moving work first (In progress, Review, Backlog, Done). A row is its stage,
 * id and title with the values it has on the right, and opens the issue's
 * panel like a card (B21); a row with agents unfolds them under it, and one
 * waiting on the operator starts unfolded.
 */
export function TasksListView({ board, page, actions, handlers }: { board: TasksBoard; page: BoardPage; actions: Actions; handlers: BoardHandlers }) {
  const { t } = useInterfaceTranslation();
  const order: Stage[] = ["working", "review", "backlog", "done"];
  const [closed, setClosed] = useState<Set<Stage>>(() => new Set(["done"]));
  const [backlogAll, setBacklogAll] = useState(false);
  const now = Date.now();
  return (
    <div className="flex flex-col gap-md px-lg pb-xl" data-tasks-list="true">
      {order.map((stage) => {
        const label = t(STAGES.find((entry) => entry.stage === stage)!.labelKey);
        const cards = stageCards(board, stage);
        if (cards.length === 0 && stage !== "backlog") return null;
        const open = !closed.has(stage);
        const shown = stage === "backlog" && !backlogAll ? cards.slice(0, LIST_BACKLOG_LIMIT) : cards;
        return (
          <section key={stage} aria-label={label} data-list-group={stage} className="flex flex-col">
            <GroupHead open={open} onToggle={() => setClosed((current) => toggled(current, stage))} label={label} count={cards.length} data={{ "data-list-group-toggle": stage }}>
              {stage === "backlog" ? <BacklogSource board={board} /> : null}
              {stage === "backlog" && open && cards.length > 1 ? <span className="ml-auto text-caption font-normal text-muted-foreground">{t("board.recentOrder")}</span> : null}
            </GroupHead>
            {open ? (
              <ul className="flex flex-col" role="list">
                {shown.map((value) => (
                  <ListRow key={value.id} card={value} now={now} page={page} actions={actions} handlers={handlers} />
                ))}
                {stage === "backlog" && cards.length === 0 && !board.source.reading ? (
                  <li className="px-xl py-xs text-caption text-muted-foreground" data-backlog-empty="true">
                    {t("board.empty")}
                  </li>
                ) : null}
                {cards.length > shown.length ? (
                  <li>
                    <button type="button" className="px-xl py-xs text-caption text-muted-foreground hover:text-foreground" onClick={() => setBacklogAll(true)} data-list-more={cards.length - shown.length}>
                      +{cards.length - shown.length}
                    </button>
                  </li>
                ) : null}
              </ul>
            ) : null}
          </section>
        );
      })}
    </div>
  );
}

/** What a List row asks of the operator, as one small word: an agent's question, or a result not yet looked at. */
function turnKind(card: TaskCard): "question" | "review" | null {
  if (!card.needsYou) return null;
  return card.rows.some((row) => row.agent.group === "needs_you") ? "question" : "review";
}

function ListRow({ card, now, page, actions, handlers }: { card: TaskCard; now: number; page: BoardPage; actions: Actions; handlers: BoardHandlers }) {
  const { t, i18n } = useInterfaceTranslation();
  const [open, setOpen] = useState(card.needsYou);
  const { task } = card;
  const turn = turnKind(card);
  const lead = card.shown[0]?.changed_at_unix_ms;
  const age = lead != null ? formatElapsed(requireInterfaceLanguage(i18n.language), now - lead) : relativeActivity(card.updatedAt, now, t);
  const createdAge = relativeActivity(task.created_at_unix_ms, now, t);
  const hasRows = card.rows.length > 0;
  const selected = page.panel === task.key;
  return (
    <li className={cn("group/card flex flex-col rounded-sm", card.needsYou && "bg-warning/10", selected && "bg-accent")} data-list-row={card.id} data-issue-row={task.key} data-needs-you={card.needsYou ? "true" : undefined}>
      <div
        className="flex min-h-(--size-control) min-w-0 cursor-pointer items-center gap-sm px-xs outline-none hover:bg-accent focus-visible:ring-1 focus-visible:ring-ring"
        tabIndex={0}
        onClick={(event) => {
          if (!gitHubClick(event, task.url, card.owner.device_id, handlers)) handlers.openPanel(card);
        }}
        onKeyDown={(event) => {
          if (event.target !== event.currentTarget) return;
          if (event.key === "Enter") handlers.openPanel(card);
          else if (event.key.toLowerCase() === "s" && card.canStart) handlers.startIssue(card);
        }}
      >
        <button
          type="button"
          aria-label={t(open ? "board.collapseAgents" : "board.expandAgents")}
          aria-expanded={hasRows ? open : undefined}
          disabled={!hasRows}
          onClick={(event) => {
            event.stopPropagation();
            setOpen((value) => !value);
          }}
          className="inline-flex size-(--size-icon) shrink-0 items-center justify-center rounded-xs text-muted-foreground outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:invisible"
        >
          {open ? <ChevronDownIcon aria-hidden="true" className="size-(--size-icon)" /> : <ChevronRightIcon aria-hidden="true" className="size-(--size-icon)" />}
        </button>
        <StageGlyph stage={card.stage} />
        <span className="inline-flex min-w-(--size-tab-icon-identity) shrink-0 items-center gap-xxs font-mono text-caption text-muted-foreground">
          <TaskGlyph task={task} />
          {task.id}
        </span>
        <span className="flex min-w-0 flex-1 items-center gap-xs">
          {/* The title keeps its width up to the whole span and the labels take what is left, so a narrow row shortens and then drops the labels before the title gives way. */}
          <span className="flex min-w-0 items-center gap-xs overflow-hidden">
            <span className="max-w-full shrink-0 truncate text-body text-foreground">{card.title}</span>
            <CardLabels labels={task.labels ?? []} className="overflow-hidden" />
          </span>
          {card.project ? <span className="shrink-0 text-caption text-muted-foreground">{card.project}</span> : null}
          {turn ? (
            <Badge variant="outline" className="shrink-0 border-warning text-warning" data-turn={turn}>
              {t(turn === "question" ? "board.turn.question" : "board.turn.review")}
            </Badge>
          ) : null}
        </span>
        <span className="flex shrink-0 items-center gap-sm font-mono text-caption text-muted-foreground">
          {card.shown.length > 0 ? (
            <span className="inline-flex items-center gap-xxs" role="img" aria-label={card.rows.map((entry) => `${entry.agent.identity_label} ${statusText(t, entry.agent.status_code)}`).join(", ")}>
              {card.shown.map((agent) => (
                <StatusMark key={agent.pane_id} symbol={agent.symbol} className={markTone(agent)} />
              ))}
              {card.rows.length > 1 ? <span>{card.rows.length}</span> : null}
            </span>
          ) : null}
          {card.pr ? (
            <span className={cn("inline-flex items-center gap-xxs", PR_TONE[card.pr.tone])} data-pr-chip={card.pr.number}>
              <GitPullRequestIcon aria-hidden="true" className="size-(--size-icon-sm)" />#{card.pr.number}
            </span>
          ) : null}
          {card.chip ? <span className="max-w-(--home-collapsed-width) truncate">{card.chip.branch}</span> : null}
          {card.chip?.ahead != null ? <span>↑{card.chip.ahead}</span> : null}
          {createdAge ? <span data-issue-created-age="true" aria-label={t("board.createdAgo", { age: createdAge })}>{createdAge}</span> : null}
          {age ? <span className="min-w-(--size-control-compact) text-right">{age}</span> : null}
        </span>
      </div>
      {open && hasRows ? (
        <ul className="flex flex-col pb-xs pl-xxxl" role="list">
          {card.rows.map((entry: BoardRow) => (
            <CardAgentRow key={entry.agent.pane_id} agent={entry.agent} depth={entry.depth} place={card.chip?.branch ?? ""} selected={entry.agent.pane_id === page.focusedPaneId} onOpen={actions.openAgent} />
          ))}
        </ul>
      ) : null}
    </li>
  );
}

/**
 * The Dependencies mode (D-09, D-10): the same issue cards with a quiet stage
 * word, laid out left to right with an arrow from each blocker to what it
 * blocks, the issues with no relation below. A card opens the issue's panel
 * as on the Board (B21). Arrows are measured from the drawn cards, so a card
 * that grows or a window that narrows redraws them.
 */
export function DependenciesView({ board, page, actions, handlers }: { board: TasksBoard; page: BoardPage; actions: Actions; handlers: BoardHandlers }) {
  const { t } = useInterfaceTranslation();
  const graph = useMemo(() => buildDependencies(board), [board]);
  const draw = (value: TaskCard) => (
    <div key={value.id} className="w-(--home-column-width)" data-dependency-node={value.id}>
      <IssueCardView card={value} page={page} actions={actions} handlers={handlers} graph={{ status: t(STAGES.find((row) => row.stage === value.stage)!.labelKey) }} />
    </div>
  );
  return (
    <div className="flex w-fit min-w-full flex-col gap-lg px-lg pb-xl" data-tasks-dependencies="true">
      <p className="flex items-center gap-xs text-caption text-muted-foreground" data-dependency-legend="true">
        <ArrowRightIcon aria-hidden="true" className="size-(--size-icon) text-warning" />
        {t("board.dependencies.legend")}
      </p>
      {graph.layers.length > 0 ? <DependencyGraphView graph={graph} draw={draw} idOf={(value) => value.id} /> : null}
      {graph.unrelated.length > 0 ? (
        <section className="flex flex-col gap-sm" aria-label={t("board.dependencies.unrelated")} data-dependency-unrelated="true">
          {graph.layers.length > 0 ? <h2 className="text-subhead font-semibold text-subtle-foreground">{t("board.dependencies.unrelated")}</h2> : null}
          <div className="flex flex-wrap items-start gap-md">{graph.unrelated.map(draw)}</div>
        </section>
      ) : null}
      {graph.layers.length === 0 && graph.unrelated.length === 0 ? (
        <p className="text-caption text-muted-foreground" data-dependency-empty="true">
          {t("board.dependencies.empty")}
        </p>
      ) : null}
    </div>
  );
}

/**
 * The dependency graph: cards placed by elkjs's layered layout, with each
 * edge routed through the gaps between cards (`dependencyLayout.ts`). Until
 * the layout worker answers, or when it cannot, the cards stand in one
 * column per depth of blockers and the arrows run from each card's right
 * middle to the next one's left middle. Each drawn node carries
 * `data-dependency-node` with its id; the Factory graph draws its own cards
 * the same way (PRD software-factory-ui D-08).
 */
export function DependencyGraphView<T>({ graph, draw, idOf }: { graph: LayeredGraph<T>; draw: (value: T) => ReactNode; idOf: (value: T) => string }) {
  const box = useRef<HTMLDivElement>(null);
  const marker = `dependency-arrow-${useId()}`;
  const values = graph.layers.flat();
  // What the layout depends on: the nodes and the edges, not the object a snapshot rebuilt.
  const structure = JSON.stringify([values.map(idOf), graph.edges.map((edge) => `${edge.from}>${edge.to}`)]);
  const placement = useDependencyPlacement(box, structure, graph.edges);
  const places = placement ? values.map((value) => placement.nodes.get(idOf(value))) : [];
  const placed = placement !== null && places.every((place) => place !== undefined);
  const arrows = useMeasuredPaths(
    box,
    "[data-dependency-node]",
    (origin, at) =>
      placed
        ? []
        : graph.edges.flatMap((edge) => {
            const from = at("data-dependency-node", edge.from);
            const to = at("data-dependency-node", edge.to);
            if (!from || !to) return [];
            const x1 = from.right - origin.left;
            const y1 = from.top + from.height / 2 - origin.top;
            const x2 = to.left - origin.left;
            const y2 = to.top + to.height / 2 - origin.top;
            const bend = (x2 - x1) / 2;
            return [{ id: `${edge.from}>${edge.to}`, d: `M ${x1} ${y1} C ${x1 + bend} ${y1}, ${x2 - bend} ${y2}, ${x2} ${y2}` }];
          }),
    [structure, placed],
  );
  const lines = placed ? placement.edges : arrows;
  const svg = (
    <svg aria-hidden="true" className="pointer-events-none absolute inset-0 size-full overflow-visible text-warning">
      <defs>
        <marker id={marker} viewBox="0 0 8 8" refX="7" refY="4" markerWidth="8" markerHeight="8" orient="auto-start-reverse">
          <path d="M 1 1 L 7 4 L 1 7" fill="none" stroke="currentColor" />
        </marker>
      </defs>
      {lines.map((arrow) => (
        <path key={arrow.id} d={arrow.d} fill="none" stroke="currentColor" strokeWidth={1.5} markerEnd={`url(#${marker})`} data-dependency-edge={arrow.id} />
      ))}
    </svg>
  );
  if (placed) {
    return (
      <div ref={box} className="relative" style={{ width: placement.width, height: placement.height }} data-dependency-graph="true" data-dependency-edges={graph.edges.length} data-dependency-layout="layered">
        {values.map((value, index) => (
          <div key={idOf(value)} className="absolute" style={{ left: places[index]!.x, top: places[index]!.y }} data-dependency-layer={places[index]!.layer}>
            {draw(value)}
          </div>
        ))}
        {svg}
      </div>
    );
  }
  return (
    <div ref={box} className="relative flex w-fit items-start gap-(--home-dependency-gap)" data-dependency-graph="true" data-dependency-edges={graph.edges.length} data-dependency-layout="columns">
      {graph.layers.map((layer, index) => (
        <div key={index} className="flex flex-col gap-xl" data-dependency-layer={index}>
          {layer.map(draw)}
        </div>
      ))}
      {svg}
    </div>
  );
}

/**
 * Measures the drawn cards and asks the layout worker where they go, again
 * only when the graph's nodes or edges change or a card changes size: a
 * snapshot that rebuilds the same graph keeps the placement it has. A
 * structure the layout refused stays in columns without asking again, and
 * the reason goes to the diagnostic log once.
 */
function useDependencyPlacement(box: RefObject<HTMLDivElement | null>, structure: string, edges: readonly GraphEdge[]): Placement | null {
  const [placed, setPlaced] = useState<{ key: string; placement: Placement } | null>(null);
  const [measured, setMeasured] = useState<{ key: string; nodes: NodeBox[] } | null>(null);
  const refused = useRef(new Set<string>());
  const layout = placed?.key.startsWith(`${structure}|`) ? "layered" : "columns";
  useLayoutEffect(() => {
    const root = box.current;
    if (!root) return;
    const measure = () => {
      const nodes = [...root.querySelectorAll<HTMLElement>("[data-dependency-node]")].map((node) => ({ id: node.dataset.dependencyNode ?? "", width: node.offsetWidth, height: node.offsetHeight }));
      const key = `${structure}|${nodes.map((node) => `${node.width}x${node.height}`).join(",")}`;
      setMeasured((current) => (current?.key === key ? current : { key, nodes }));
    };
    measure();
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(measure);
    for (const node of root.querySelectorAll("[data-dependency-node]")) observer.observe(node);
    return () => observer.disconnect();
  }, [structure, layout]);
  const edgeList = useRef(edges);
  edgeList.current = edges;
  useEffect(() => {
    if (!measured || !measured.key.startsWith(`${structure}|`) || measured.nodes.length === 0) return;
    if (placed?.key === measured.key || refused.current.has(measured.key)) return;
    const root = box.current;
    if (!root) return;
    // The same gaps the columns keep: between columns, and between cards in one.
    const style = getComputedStyle(root);
    const spacing = { betweenLayers: parseFloat(style.getPropertyValue("--home-dependency-gap")), betweenNodes: parseFloat(style.getPropertyValue("--spacing-xl")) };
    let current = true;
    workerElk()
      .then((elk) => {
        if (!Number.isFinite(spacing.betweenLayers) || !Number.isFinite(spacing.betweenNodes)) throw new Error("the graph's spacing tokens are not set");
        return placeWithin(elk, measured.nodes, edgeList.current, spacing);
      })
      .then(
        (placement) => {
          if (current) setPlaced({ key: measured.key, placement });
        },
        (error: unknown) => {
          if (!current) return;
          refused.current.add(measured.key);
          useShellStore.getState().noteDiagnostic(`dependency graph stays in columns: ${error instanceof Error ? error.message : String(error)}`);
        },
      );
    return () => {
      current = false;
    };
  }, [measured, structure]);
  return placed?.key.startsWith(`${structure}|`) ? placed.placement : null;
}
