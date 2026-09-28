import {
  ArrowRightIcon,
  CheckIcon,
  ChevronDownIcon,
  ChevronRightIcon,
  CircleDashedIcon,
  CircleDotIcon,
  CircleHelpIcon,
  ContrastIcon,
  EllipsisIcon,
  FileTextIcon,
  GitBranchIcon,
  GitMergeIcon,
  GitPullRequestIcon,
  HouseIcon,
  Link2Icon,
  LockIcon,
  PlayIcon,
  PlusIcon,
  XIcon,
} from "lucide-react";
import { createContext, useContext, useId, useMemo, useRef, useState, type KeyboardEvent, type ReactNode } from "react";
import type { Actions } from "./actions";
import { markTone } from "./agentRow";
import { useMeasuredPaths } from "./measuredPaths";
import { AgentRowItem } from "./components/agent-row";
import { StatusMark } from "./components/status-mark";
import { Badge } from "./components/ui/badge";
import { Button } from "./components/ui/button";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuSeparator, DropdownMenuTrigger } from "./components/ui/dropdown-menu";
import { Kbd } from "./components/ui/kbd";
import { ToggleGroup, ToggleGroupItem } from "./components/ui/toggle-group";
import { Hint } from "./components/ui/tooltip";
import { LinkIssuePopover } from "./IssueDialogs";
import { cn } from "./lib/utils";
import {
  STAGES,
  buildDependencies,
  stageCards,
  type BoardScope,
  type DependencyGraph,
  type PrChip,
  type Stage,
  type TaskCard,
  type TasksBoard,
} from "./projectBoard";
import { relativeActivity } from "./projects";
import type { AgentRow, Task } from "./snapshot";
import type { TasksMode } from "./ui";

// The Tasks views (PRD task-agents-views, reworked issue-first on
// 2026-09-28), drawn the same for a Project and for All projects:
// `buildTasks` and `buildDependencies` decide every card and
// row, and this file only draws them and routes the clicks. A card's title
// opens its checkout, an agent row its pane, the id its issue and the PR chip
// its pull request; a backlog card starts work, a worktree with no issue
// links one. The page scrolls as one; a column grows with its cards.

/** A column shows this many cards, then the rest on request. */
const COLUMN_LIMIT = 20;
/** The List's backlog shows this many rows, then the rest on request. */
const LIST_BACKLOG_LIMIT = 8;
/** Done folds to this many names before `+N`. */
const DONE_NAMES = 6;

/** A card draws its agents without their descendants' fold. */
const NO_ROWS: AgentRow[] = [];

/**
 * The issue an issue chip elsewhere on the Overview asked for (PRD
 * overview-lenses-tiles-agents B17, B24): its card wears the selection ring
 * and is brought into view.
 */
export const FocusedTask = createContext<string | null>(null);

/** What a board's cards ask of the page around them. */
export type BoardHandlers = {
  openCheckout: (card: TaskCard) => void;
  /** Opens the Start dialog for a backlog issue. */
  startIssue: (card: TaskCard) => void;
  /** Opens the New issue dialog, for the page's Project or its default one. */
  newIssue: () => void;
};

/** `S` on a focused backlog card opens its Start dialog, as its button does. */
function startsOnKey(event: KeyboardEvent, card: TaskCard, handlers: BoardHandlers) {
  if (!card.canStart || event.key.toLowerCase() !== "s" || event.metaKey || event.ctrlKey || event.altKey) return;
  if (event.target instanceof HTMLInputElement || event.target instanceof HTMLTextAreaElement) return;
  event.preventDefault();
  handlers.startIssue(card);
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
  focusedPaneId,
  actions,
  handlers,
  doneOpen,
  onToggleDone,
}: {
  board: TasksBoard;
  scope: BoardScope;
  focusedPaneId: string | null;
  actions: Actions;
  handlers: BoardHandlers;
  doneOpen: boolean;
  onToggleDone: () => void;
}) {
  // Backlog always stands, so a new issue has somewhere to go; another
  // stage's column only while it holds a card. The tracks fill the page's
  // width whatever the columns shown, so a column keeps its width as stages
  // come and go, one column never spans the page, and a narrow page wraps
  // the last column rather than clipping it.
  const columns = STAGES.map(({ stage, label }) => ({ stage, label, cards: stageCards(board, stage) })).filter((column) => column.stage === "backlog" || column.cards.length > 0);
  return (
    <div
      className="grid items-start gap-md px-lg pb-xl"
      style={{ gridTemplateColumns: "repeat(auto-fill, minmax(var(--home-column-width), 1fr))" }}
      data-tasks-board="true"
      data-overview-columns="tasks"
    >
      {columns.map(({ stage, label, cards }) =>
        stage === "done" ? (
          <DoneColumn key={stage} label={label} cards={cards} scope={scope} open={doneOpen} onToggle={onToggleDone} focusedPaneId={focusedPaneId} actions={actions} handlers={handlers} />
        ) : (
          <StageColumn key={stage} stage={stage} label={label} board={board} cards={cards} focusedPaneId={focusedPaneId} actions={actions} handlers={handlers} />
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

/** A one-line button at a column's foot: `+N`, or the idle worktrees' fold. */
function FoldLine({ children, onClick, open, data }: { children: ReactNode; onClick: () => void; open?: boolean; data: Record<string, string> }) {
  return (
    <button
      type="button"
      onClick={onClick}
      aria-expanded={open}
      className="flex w-full items-center gap-xs rounded-sm border border-border px-sm py-xxs text-left text-caption text-muted-foreground outline-none hover:bg-accent hover:text-foreground focus-visible:ring-1 focus-visible:ring-ring"
      {...data}
    >
      <span className="min-w-0 flex-1 truncate">{children}</span>
      {open ? <ChevronDownIcon aria-hidden="true" className="size-(--size-icon) shrink-0" /> : <ChevronRightIcon aria-hidden="true" className="size-(--size-icon) shrink-0" />}
    </button>
  );
}

/**
 * Backlog, In progress or Review. Past `COLUMN_LIMIT` the rest wait behind
 * `+N`; In progress folds its worktrees with no issue and no agent into one
 * line at its foot, since nothing on them needs a look.
 */
function StageColumn({
  stage,
  label,
  board,
  cards,
  focusedPaneId,
  actions,
  handlers,
}: {
  stage: Stage;
  label: string;
  board: TasksBoard;
  cards: TaskCard[];
  focusedPaneId: string | null;
  actions: Actions;
  handlers: BoardHandlers;
}) {
  const [all, setAll] = useState(false);
  const [idleOpen, setIdleOpen] = useState(false);
  const active = cards.filter((value) => !value.idle);
  const idle = cards.filter((value) => value.idle);
  const shown = all ? active : active.slice(0, COLUMN_LIMIT);
  const hidden = active.length - shown.length;
  const count = `${cards.length}${stage === "backlog" && board.overflow ? "+" : ""}`;
  const backlog = stage === "backlog";
  const draw = (value: TaskCard) => <TaskCardView key={value.id} card={value} focusedPaneId={focusedPaneId} actions={actions} handlers={handlers} />;
  return (
    <section aria-label={label} data-overview-column={stage} className="flex min-w-0 flex-col gap-sm">
      <ColumnHead
        action={
          backlog ? (
            <Hint label="새 이슈" shortcut={<Kbd>C</Kbd>}>
              <Button variant="ghost" size="icon-sm" aria-label="새 이슈" onClick={handlers.newIssue} data-backlog-new-issue="true">
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
      {shown.map(draw)}
      {backlog && cards.length === 0 ? <EmptyBacklog board={board} onNew={handlers.newIssue} /> : null}
      {hidden > 0 ? (
        <FoldLine onClick={() => setAll(true)} data={{ "data-column-more": stage }}>
          +{hidden}
          {backlog ? " · 최근 갱신 순" : ""}
        </FoldLine>
      ) : null}
      {idle.length > 0 ? (
        <>
          <FoldLine open={idleOpen} onClick={() => setIdleOpen((open) => !open)} data={{ "data-idle-fold": String(idle.length) }}>
            에이전트 없는 워크트리 {idle.length}
          </FoldLine>
          {idleOpen ? idle.map(draw) : null}
        </>
      ) : null}
    </section>
  );
}

/** A source still reading, or one that could not be read, as one small mark beside the Backlog count (design 9, 13). */
function BacklogSource({ board }: { board: TasksBoard }) {
  if (board.source.failure) {
    return (
      <Hint label={board.source.failure}>
        <span className="text-warning" tabIndex={0} data-backlog-source-failure="true">
          <CircleHelpIcon aria-hidden="true" className="size-(--size-icon)" />
        </span>
      </Hint>
    );
  }
  if (board.source.reading) {
    return (
      <span className="text-caption font-normal text-muted-foreground" data-backlog-reading="true">
        읽는 중…
      </span>
    );
  }
  return null;
}

/** An empty backlog: one quiet line and the way to add the first issue. */
function EmptyBacklog({ board, onNew }: { board: TasksBoard; onNew: () => void }) {
  if (board.source.reading) return null;
  return (
    <div className="flex flex-col items-start gap-xs rounded-md border border-dashed border-border p-sm text-caption text-muted-foreground" data-backlog-empty="true">
      <p>열린 이슈가 없습니다</p>
      <Button variant="secondary" size="sm" onClick={onNew} data-backlog-empty-new="true">
        <PlusIcon aria-hidden="true" />새 이슈
      </Button>
    </div>
  );
}

/**
 * Done starts folded to one line per card, its branch (on All projects, one
 * line per Project with its count); its head unfolds it into cards (D-03).
 */
function DoneColumn({
  label,
  cards,
  scope,
  open,
  onToggle,
  focusedPaneId,
  actions,
  handlers,
}: {
  label: string;
  cards: TaskCard[];
  scope: BoardScope;
  open: boolean;
  onToggle: () => void;
  focusedPaneId: string | null;
  actions: Actions;
  handlers: BoardHandlers;
}) {
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
  const nameLine = "flex w-full min-w-0 items-center gap-xs rounded-xs px-xs py-xxs text-left font-mono text-caption text-subtle-foreground outline-none hover:bg-accent hover:text-foreground focus-visible:ring-1 focus-visible:ring-ring";
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
        cards.map((value) => <TaskCardView key={value.id} card={value} focusedPaneId={focusedPaneId} actions={actions} handlers={handlers} />)
      ) : scope === "all" ? (
        <ul className="flex flex-col" data-overview-collapsed-names="done">
          {perProject.map(([id, row]) => (
            <li key={id}>
              <button type="button" className={nameLine} onClick={onToggle} data-done-project={id}>
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
            <li key={value.id}>
              <button type="button" className={nameLine} onClick={() => handlers.openCheckout(value)}>
                <GitMergeIcon aria-hidden="true" className="size-(--size-icon-sm) shrink-0 text-pr-merged" />
                <span className="min-w-0 truncate">{value.branch ?? value.title}</span>
              </button>
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

/** `Board | List | Dependencies`, a mode of the Tasks view rather than a tab (D-02): on the right of All projects' tab row, and of a Project's facts line while its Issues tile is chosen. */
export function TasksModeToggle({ mode, onChange }: { mode: TasksMode; onChange: (mode: TasksMode) => void }) {
  return (
    <ToggleGroup type="single" value={mode} onValueChange={(value) => value && onChange(value as TasksMode)} aria-label="Tasks mode" data-tasks-mode={mode}>
      <ToggleGroupItem value="board" data-tasks-mode-item="board">
        Board
      </ToggleGroupItem>
      <ToggleGroupItem value="list" data-tasks-mode-item="list">
        List
      </ToggleGroupItem>
      <ToggleGroupItem value="dependencies" data-tasks-mode-item="dependencies">
        Dependencies
      </ToggleGroupItem>
    </ToggleGroup>
  );
}

/** The mark a task's id carries: an issue's circle-dot, a local issue's page (D-06). */
export function TaskGlyph({ task, className }: { task: Task; className?: string }) {
  const Glyph = task.source === "github" ? CircleDotIcon : FileTextIcon;
  return <Glyph aria-hidden="true" className={cn("size-(--size-icon-sm) shrink-0", className)} />;
}

/** Where a checkout is: its branch in mono for a worktree, a house for the primary checkout or a folder. */
function Place({ card, className }: { card: TaskCard; className?: string }) {
  const { checkout } = card;
  if (!checkout || !card.branch) return null;
  const Glyph = checkout.is_worktree ? GitBranchIcon : HouseIcon;
  return (
    <span className={cn("inline-flex min-w-0 items-center gap-xxs font-mono text-caption text-muted-foreground", className)} data-card-branch={card.branch}>
      <Glyph aria-hidden="true" className="size-(--size-icon-sm) shrink-0" />
      <span className="min-w-0 truncate">{card.branch}</span>
    </span>
  );
}

/**
 * A task card, top to bottom as the work flows: the issue (its id and title;
 * a worktree with no issue leads with its branch), then where the work is
 * and what it delivered (the PR and its CI, the branch, ↑ahead, changed
 * files), then at most two agents and `+N`. A card waiting on the operator
 * wears a warning halo. Hover or focus offers the card's next step: 시작 on
 * a backlog issue, 이슈 연결 on a worktree with none; `⋯` holds the rest. In
 * Dependencies (`graph`) it also carries its stage word, and a blocked card
 * is dimmed like a done one (D-09, D-10).
 */
export function TaskCardView({
  card,
  focusedPaneId,
  actions,
  handlers,
  graph,
}: {
  card: TaskCard;
  focusedPaneId: string | null;
  actions: Actions;
  handlers: BoardHandlers;
  graph?: { status: string | null };
}) {
  const { checkout, task, facts } = card;
  const focusedTask = useContext(FocusedTask);
  const focused = focusedTask !== null && focusedTask === task?.key;
  const halo = card.needsYou ? (card.error ? "border-destructive shadow-[0_0_var(--home-halo-radius)_var(--destructive)]" : "border-warning shadow-[0_0_var(--home-halo-radius)_var(--warning)]") : "border-border";
  const blocked = card.blockedBy.length > 0;
  const dimmed = card.stage === "done" || card.idle || (graph !== undefined && blocked);
  const hasFacts = facts.files !== null || facts.ahead !== null || facts.pr !== null || facts.behind !== null || (task !== null && card.branch !== null);
  return (
    <article
      className={cn("group/card flex min-w-0 flex-col gap-xs rounded-md border bg-card p-sm outline-none focus-visible:ring-1 focus-visible:ring-ring", halo, dimmed && "opacity-(--opacity-secondary)", focused && "ring-1 ring-primary")}
      data-focused-task={focused ? "true" : undefined}
      data-overview-card={card.id}
      data-stage={card.stage}
      data-needs-you={card.needsYou ? "true" : undefined}
      data-task-key={task?.key}
      data-blocked={blocked ? "true" : undefined}
      data-idle={card.idle ? "true" : undefined}
      tabIndex={card.canStart ? 0 : undefined}
      onKeyDown={(event) => startsOnKey(event, card, handlers)}
    >
      <div className="flex min-w-0 items-center gap-xs">
        {task ? <TaskId task={task} help={card.idHelp} /> : <Place card={card} />}
        {card.project ? (
          <span className="min-w-0 truncate text-caption text-muted-foreground" data-card-project="true">
            {card.project}
          </span>
        ) : null}
        <span className="flex-1" />
        {card.sourceFailure ? (
          <Hint label={card.sourceFailure}>
            <span className="shrink-0 text-warning" data-source-failure="true" tabIndex={0}>
              <CircleHelpIcon aria-hidden="true" className="size-(--size-icon)" />
            </span>
          </Hint>
        ) : null}
        {graph?.status ? (
          <span className="shrink-0 text-caption text-muted-foreground" data-card-status="true">
            {graph.status}
          </span>
        ) : null}
      </div>
      <p className="line-clamp-2 min-w-0 break-words text-title font-semibold text-foreground">
        {checkout ? (
          <button type="button" onClick={() => handlers.openCheckout(card)} data-overview-workspace={checkout.id} className="rounded-xs text-left outline-none hover:underline focus-visible:ring-1 focus-visible:ring-ring">
            {card.title}
          </button>
        ) : (
          <span>{card.title}</span>
        )}
      </p>
      {blocked ? (
        <Hint label={`먼저 끝나야 하는 태스크: ${card.blockedBy.map((blocker) => blocker.label).join(", ")}`}>
          <p className="flex w-fit min-w-0 items-center gap-xxs text-caption text-warning" data-blocked-by={card.blockedBy.map((blocker) => blocker.key).join(" ")} tabIndex={0}>
            <LockIcon aria-hidden="true" className="size-(--size-icon-sm) shrink-0" />
            <span className="truncate">{card.blockedBy.map((blocker) => blocker.label).join(", ")}</span>
          </p>
        </Hint>
      ) : null}
      {hasFacts ? <CardFacts card={card} /> : null}
      {card.shown.length > 0 ? (
        <ul className="-mx-sm flex flex-col" role="list" data-overview-rows="true">
          {card.shown.map((agent) => (
            <AgentRowItem
              key={agent.pane_id}
              agent={agent}
              device={null}
              depth={0}
              descendants={0}
              childRows={NO_ROWS}
              selected={agent.pane_id === focusedPaneId}
              onOpen={actions.openAgent}
              onToggleTree={null}
              inset="var(--spacing-sm)"
            />
          ))}
        </ul>
      ) : null}
      {card.more > 0 ? (
        <Hint label={card.rows.map((row) => row.agent.identity_label).join(", ")}>
          <span className="w-fit text-caption text-muted-foreground" data-overview-more={card.more} tabIndex={0}>
            +{card.more} 에이전트
          </span>
        </Hint>
      ) : null}
      <CardActions card={card} actions={actions} handlers={handlers} />
    </article>
  );
}

/** Where the work is and what it delivered: the PR and its CI, the branch under an issue, ↑ahead, changed files, ↓behind. */
function CardFacts({ card }: { card: TaskCard }) {
  const { facts, task } = card;
  return (
    <div className="flex min-w-0 flex-wrap items-center gap-x-sm gap-y-xxs font-mono text-caption text-muted-foreground" data-overview-facts="true">
      {facts.pr ? <PrChipView pr={facts.pr} /> : null}
      {task ? <Place card={card} className="max-w-(--home-collapsed-width)" /> : null}
      {facts.ahead !== null ? <span data-fact="ahead">↑{facts.ahead}</span> : null}
      {facts.files !== null ? (
        <span className="text-warning" data-fact="files">
          {facts.files} files
        </span>
      ) : null}
      {facts.behind !== null ? (
        <span className="text-warning" data-fact="behind">
          ↓{facts.behind}
        </span>
      ) : null}
    </div>
  );
}

/**
 * The card's next step, shown on hover or focus (B7): 시작 on a backlog
 * issue (`S` while the card has focus), 이슈 연결 on a worktree with none;
 * `⋯` holds starting an agent in the checkout and closing a local issue.
 */
function CardActions({ card, actions, handlers }: { card: TaskCard; actions: Actions; handlers: BoardHandlers }) {
  const { checkout, task } = card;
  const localTask = task !== null && task.source === "local";
  const startable = checkout !== null && card.stage !== "done";
  const menu = localTask || startable;
  if (!card.canStart && !card.canLink && !menu) return null;
  return (
    <div className="hidden items-center gap-xs group-focus-within/card:flex group-hover/card:flex has-data-[state=open]:flex" data-card-actions="true">
      {card.canStart ? (
        <Button variant="secondary" size="sm" onClick={() => handlers.startIssue(card)} data-card-start="true">
          <PlayIcon aria-hidden="true" />
          시작
          <Kbd>S</Kbd>
        </Button>
      ) : null}
      {card.canLink && checkout ? (
        <LinkIssuePopover workspaceId={card.place.projectId} checkout={checkout} actions={actions}>
          <Button variant="secondary" size="sm" data-card-link="true">
            <Link2Icon aria-hidden="true" />
            이슈 연결
          </Button>
        </LinkIssuePopover>
      ) : null}
      <span className="flex-1" />
      {menu ? (
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <Button variant="ghost" size="icon-sm" aria-label="카드 동작" data-card-menu="true">
              <EllipsisIcon aria-hidden="true" />
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end">
            {startable && checkout ? (
              <>
                <DropdownMenuItem onSelect={() => actions.startAgent(checkout.path, "claude")}>Claude 시작</DropdownMenuItem>
                <DropdownMenuItem onSelect={() => actions.startAgent(checkout.path, "codex")}>Codex 시작</DropdownMenuItem>
                <DropdownMenuItem onSelect={() => actions.startAgent(checkout.path, "terminal")}>터미널만 열기</DropdownMenuItem>
              </>
            ) : null}
            {startable && localTask ? <DropdownMenuSeparator /> : null}
            {localTask && task ? (
              <DropdownMenuItem onSelect={() => actions.setIssueOpen(task.key, !task.open)} data-card-issue-open={task.open ? "close" : "reopen"}>
                {task.open ? "이슈 닫기" : "이슈 다시 열기"}
              </DropdownMenuItem>
            ) : null}
          </DropdownMenuContent>
        </DropdownMenu>
      ) : null}
    </div>
  );
}

/** A task's id, small and muted; it opens the issue, and its tooltip names the source and branch (D-05, D-06). */
function TaskId({ task, help }: { task: Task; help: string }) {
  if (!task.id) return null;
  const content = (
    <>
      <TaskGlyph task={task} />
      {task.id}
    </>
  );
  const className = "inline-flex shrink-0 items-center gap-xxs rounded-xs font-mono text-caption text-muted-foreground outline-none hover:text-foreground focus-visible:ring-1 focus-visible:ring-ring";
  return (
    <Hint label={[task.id, help].filter(Boolean).join(" · ")}>
      {task.url ? (
        <a href={task.url} target="_blank" rel="noopener noreferrer" className={className} data-task-id={task.id}>
          {content}
        </a>
      ) : (
        <span className={className} data-task-id={task.id} tabIndex={0}>
          {content}
        </span>
      )}
    </Hint>
  );
}

export const PR_TONE: Record<PrChip["tone"], string> = {
  open: "text-pr-open",
  draft: "text-pr-draft",
  merged: "text-pr-merged",
  closed: "text-pr-closed",
};

const CHECKS: Record<"passing" | "failed" | "pending", string> = {
  passing: "CI 통과",
  failed: "CI 실패",
  pending: "CI 진행 중",
};

const REVIEW: Record<NonNullable<PrChip["review"]>, { label: string; tone: string }> = {
  review_required: { label: "리뷰 필요", tone: "text-muted-foreground" },
  changes_requested: { label: "변경 요청", tone: "text-warning" },
  approved: { label: "승인됨", tone: "text-success" },
};

/** The result: `#n` in its lifecycle colour, the CI mark once read, and the review GitHub asks for; the chip opens the pull request (D-06). */
function PrChipView({ pr, review = true }: { pr: PrChip; review?: boolean }) {
  const asked = review && pr.review ? REVIEW[pr.review] : null;
  return (
    <span className="inline-flex items-center gap-xxs">
      <Hint label={`PR #${pr.number} · ${pr.tone}`}>
        <a href={pr.url} target="_blank" rel="noopener noreferrer" className="rounded-xs outline-none focus-visible:ring-1 focus-visible:ring-ring" data-pr-chip={pr.number} data-pr-tone={pr.tone}>
          <Badge variant="outline" className={PR_TONE[pr.tone]}>
            <GitPullRequestIcon aria-hidden="true" />#{pr.number}
          </Badge>
        </a>
      </Hint>
      {pr.checks ? (
        <span role="img" aria-label={CHECKS[pr.checks]} className={pr.checks === "passing" ? "text-success" : pr.checks === "failed" ? "text-destructive" : "text-muted-foreground"} data-pr-checks={pr.checks}>
          {pr.checks === "passing" ? <CheckIcon aria-hidden="true" className="size-(--size-icon-sm)" /> : pr.checks === "failed" ? <XIcon aria-hidden="true" className="size-(--size-icon-sm)" /> : <StatusMark symbol="●" className="size-(--size-icon-sm)" />}
        </span>
      ) : null}
      {asked ? (
        <span className={cn("font-sans", asked.tone)} data-pr-review={pr.review}>
          {asked.label}
        </span>
      ) : null}
    </span>
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
 * The List mode: the same cards, one row each, grouped by stage with the
 * moving work first (In progress, Review, Backlog, Done). A row is its stage,
 * id and title with the values it has on the right; a row with agents
 * unfolds them under it, and a row waiting on the operator starts unfolded.
 * No column headers: the rows are read, not compared field by field.
 */
export function TasksListView({
  board,
  focusedPaneId,
  actions,
  handlers,
}: {
  board: TasksBoard;
  focusedPaneId: string | null;
  actions: Actions;
  handlers: BoardHandlers;
}) {
  const order: Stage[] = ["working", "review", "backlog", "done"];
  const [closed, setClosed] = useState<Set<Stage>>(() => new Set(["done"]));
  const [backlogAll, setBacklogAll] = useState(false);
  const [idleOpen, setIdleOpen] = useState(false);
  const now = Date.now();
  const row = (value: TaskCard) => <ListRow key={value.id} card={value} now={now} focusedPaneId={focusedPaneId} actions={actions} handlers={handlers} />;
  return (
    <div className="flex flex-col gap-md px-lg pb-xl" data-tasks-list="true">
      {order.map((stage) => {
        const label = STAGES.find((entry) => entry.stage === stage)?.label ?? stage;
        const cards = stageCards(board, stage);
        if (cards.length === 0 && stage !== "backlog") return null;
        const open = !closed.has(stage);
        const active = cards.filter((value) => !value.idle);
        const idle = cards.filter((value) => value.idle);
        const shown = stage === "backlog" && !backlogAll ? active.slice(0, LIST_BACKLOG_LIMIT) : active;
        return (
          <section key={stage} aria-label={label} data-list-group={stage} className="flex flex-col">
            <GroupHead open={open} onToggle={() => setClosed((current) => toggled(current, stage))} label={label} count={cards.length} data={{ "data-list-group-toggle": stage }}>
              {stage === "backlog" ? <BacklogSource board={board} /> : null}
              {stage === "backlog" && open && cards.length > 1 ? <span className="ml-auto text-caption font-normal text-muted-foreground">최근 갱신 순</span> : null}
            </GroupHead>
            {open ? (
              <ul className="flex flex-col" role="list">
                {shown.map(row)}
                {stage === "backlog" && cards.length === 0 && !board.source.reading ? (
                  <li className="px-xl py-xs text-caption text-muted-foreground" data-backlog-empty="true">
                    열린 이슈가 없습니다
                  </li>
                ) : null}
                {active.length > shown.length ? (
                  <li>
                    <button type="button" className="px-xl py-xs text-caption text-muted-foreground hover:text-foreground" onClick={() => setBacklogAll(true)} data-list-more={active.length - shown.length}>
                      +{active.length - shown.length}
                    </button>
                  </li>
                ) : null}
                {idle.length > 0 ? (
                  <li>
                    <button type="button" aria-expanded={idleOpen} className="inline-flex items-center gap-xxs px-xl py-xs text-caption text-muted-foreground hover:text-foreground" onClick={() => setIdleOpen((value) => !value)} data-idle-fold={idle.length}>
                      에이전트 없는 워크트리 {idle.length}
                      {idleOpen ? <ChevronDownIcon aria-hidden="true" className="size-(--size-icon-sm)" /> : <ChevronRightIcon aria-hidden="true" className="size-(--size-icon-sm)" />}
                    </button>
                  </li>
                ) : null}
                {idleOpen ? idle.map(row) : null}
              </ul>
            ) : null}
          </section>
        );
      })}
    </div>
  );
}

/** What a List row asks of the operator, as one small word: an agent's question, or a result not yet looked at. */
function turnWord(card: TaskCard): string | null {
  if (!card.needsYou) return null;
  return card.rows.some((row) => row.agent.group === "needs_you") ? "질문" : "확인";
}

function ListRow({ card, now, focusedPaneId, actions, handlers }: { card: TaskCard; now: number; focusedPaneId: string | null; actions: Actions; handlers: BoardHandlers }) {
  const [open, setOpen] = useState(card.needsYou);
  const { task, facts } = card;
  const word = turnWord(card);
  const age = card.shown[0]?.elapsed ?? relativeActivity(card.updatedAt, now);
  const hasRows = card.rows.length > 0;
  const reveal = "hidden group-focus-within/card:inline-flex group-hover/card:inline-flex";
  return (
    <li className={cn("group/card flex flex-col rounded-sm", card.needsYou && "bg-warning/10", card.idle && "opacity-(--opacity-secondary)")} data-list-row={card.id} data-needs-you={card.needsYou ? "true" : undefined}>
      <div className="flex min-h-(--size-control) min-w-0 items-center gap-sm px-xs outline-none focus-visible:ring-1 focus-visible:ring-ring" tabIndex={card.canStart ? 0 : undefined} onKeyDown={(event) => startsOnKey(event, card, handlers)}>
        <button
          type="button"
          aria-label={open ? "에이전트 접기" : "에이전트 펼치기"}
          aria-expanded={hasRows ? open : undefined}
          disabled={!hasRows}
          onClick={() => setOpen((value) => !value)}
          className="inline-flex size-(--size-icon) shrink-0 items-center justify-center rounded-xs text-muted-foreground outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:invisible"
        >
          {open ? <ChevronDownIcon aria-hidden="true" className="size-(--size-icon)" /> : <ChevronRightIcon aria-hidden="true" className="size-(--size-icon)" />}
        </button>
        <StageGlyph stage={card.stage} />
        <span className="flex min-w-(--size-tab-icon-identity) shrink-0">{task ? <TaskId task={task} help={card.idHelp} /> : <GitBranchIcon aria-hidden="true" className="size-(--size-icon-sm) text-muted-foreground" />}</span>
        <span className="flex min-w-0 flex-1 items-center gap-xs">
          {card.checkout ? (
            <button type="button" onClick={() => handlers.openCheckout(card)} className="min-w-0 truncate rounded-xs text-left text-body text-foreground outline-none hover:underline focus-visible:ring-1 focus-visible:ring-ring" data-overview-workspace={card.checkout.id}>
              {card.title}
            </button>
          ) : (
            <span className="min-w-0 truncate text-body text-foreground">{card.title}</span>
          )}
          {card.project ? <span className="shrink-0 text-caption text-muted-foreground">{card.project}</span> : null}
          {word ? (
            <Badge variant="outline" className="shrink-0 border-warning text-warning" data-turn={word}>
              {word}
            </Badge>
          ) : null}
          {card.canStart ? (
            <Button variant="ghost" size="sm" className={reveal} onClick={() => handlers.startIssue(card)} data-card-start="true">
              <PlayIcon aria-hidden="true" />
              시작
            </Button>
          ) : null}
          {card.canLink && card.checkout ? (
            <LinkIssuePopover workspaceId={card.place.projectId} checkout={card.checkout} actions={actions}>
              <Button variant="ghost" size="sm" className={cn(reveal, "data-[state=open]:inline-flex")} data-card-link="true">
                <Link2Icon aria-hidden="true" />
                이슈 연결
              </Button>
            </LinkIssuePopover>
          ) : null}
        </span>
        <span className="flex shrink-0 items-center gap-sm font-mono text-caption text-muted-foreground">
          {card.shown.length > 0 ? (
            <span className="inline-flex items-center gap-xxs" role="img" aria-label={card.rows.map((entry) => `${entry.agent.identity_label} ${entry.agent.status_label}`).join(", ")}>
              {card.shown.map((agent) => (
                <StatusMark key={agent.pane_id} symbol={agent.symbol} className={markTone(agent)} />
              ))}
              {card.rows.length > 1 ? <span>{card.rows.length}</span> : null}
            </span>
          ) : null}
          {facts.pr ? <PrChipView pr={facts.pr} review={false} /> : null}
          {card.branch ? <Place card={card} className="max-w-(--home-collapsed-width)" /> : null}
          {facts.ahead !== null ? <span>↑{facts.ahead}</span> : null}
          {age ? <span className="min-w-(--size-control-compact) text-right">{age}</span> : null}
        </span>
      </div>
      {open && hasRows ? (
        <ul className="flex flex-col pb-xs pl-xxxl" role="list">
          {card.rows.map((entry) => (
            <AgentRowItem
              key={entry.agent.pane_id}
              agent={entry.agent}
              device={null}
              depth={entry.depth}
              descendants={0}
              childRows={NO_ROWS}
              selected={entry.agent.pane_id === focusedPaneId}
              onOpen={actions.openAgent}
              onToggleTree={null}
            />
          ))}
        </ul>
      ) : null}
    </li>
  );
}

/**
 * The Dependencies mode (D-09, D-10): the same task cards with a quiet stage
 * word, laid out left to right with an arrow from each blocker to what it
 * blocks, the tasks with no relation below. Arrows are measured from the
 * drawn cards, so a card that grows or a window that narrows redraws them.
 */
export function DependenciesView({
  board,
  focusedPaneId,
  actions,
  handlers,
}: {
  board: TasksBoard;
  focusedPaneId: string | null;
  actions: Actions;
  handlers: BoardHandlers;
}) {
  const graph = useMemo(() => buildDependencies(board), [board]);
  const draw = (value: TaskCard) => (
    <div key={value.id} className="w-(--home-column-width)" data-dependency-node={value.id}>
      <TaskCardView card={value} focusedPaneId={focusedPaneId} actions={actions} handlers={handlers} graph={{ status: STAGES.find((row) => row.stage === value.stage)?.label ?? null }} />
    </div>
  );
  return (
    <div className="flex w-fit min-w-full flex-col gap-lg px-lg pb-xl" data-tasks-dependencies="true">
      <p className="flex items-center gap-xs text-caption text-muted-foreground" data-dependency-legend="true">
        <ArrowRightIcon aria-hidden="true" className="size-(--size-icon) text-warning" />
        선행 · 왼쪽 태스크가 끝나야 화살표가 향하는 태스크를 시작할 수 있음
      </p>
      {graph.layers.length > 0 ? <DependencyGraphView graph={graph} draw={draw} /> : null}
      {graph.unrelated.length > 0 ? (
        <section className="flex flex-col gap-sm" aria-label="관계 없는 태스크" data-dependency-unrelated="true">
          {graph.layers.length > 0 ? <h2 className="text-subhead font-semibold text-subtle-foreground">관계 없는 태스크</h2> : null}
          <div className="flex flex-wrap items-start gap-md">{graph.unrelated.map(draw)}</div>
        </section>
      ) : null}
      {graph.layers.length === 0 && graph.unrelated.length === 0 ? (
        <p className="text-caption text-muted-foreground" data-dependency-empty="true">
          의존 관계를 그릴 태스크가 없음
        </p>
      ) : null}
    </div>
  );
}

/** The layered graph: one column per depth of blockers, and the arrows drawn over it from each card's right middle to the next one's left middle. */
function DependencyGraphView({ graph, draw }: { graph: DependencyGraph; draw: (value: TaskCard) => ReactNode }) {
  const box = useRef<HTMLDivElement>(null);
  const marker = `dependency-arrow-${useId()}`;
  const arrows = useMeasuredPaths(
    box,
    "[data-dependency-node]",
    (origin, at) =>
      graph.edges.flatMap((edge) => {
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
    [graph],
  );
  return (
    <div ref={box} className="relative flex w-fit items-start gap-(--home-dependency-gap)" data-dependency-graph="true" data-dependency-edges={graph.edges.length}>
      {graph.layers.map((layer, index) => (
        <div key={index} className="flex flex-col gap-xl" data-dependency-layer={index}>
          {layer.map(draw)}
        </div>
      ))}
      <svg aria-hidden="true" className="pointer-events-none absolute inset-0 size-full overflow-visible text-warning">
        <defs>
          <marker id={marker} viewBox="0 0 8 8" refX="7" refY="4" markerWidth="8" markerHeight="8" orient="auto-start-reverse">
            <path d="M 1 1 L 7 4 L 1 7" fill="none" stroke="currentColor" />
          </marker>
        </defs>
        {arrows.map((arrow) => (
          <path key={arrow.id} d={arrow.d} fill="none" stroke="currentColor" strokeWidth={1.5} markerEnd={`url(#${marker})`} data-dependency-edge={arrow.id} />
        ))}
      </svg>
    </div>
  );
}
