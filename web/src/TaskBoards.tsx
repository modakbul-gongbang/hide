import { CheckIcon, ChevronDownIcon, ChevronRightIcon, CircleDotIcon, CircleHelpIcon, FileTextIcon, GitPullRequestIcon, InboxIcon, Link2Icon, LinkIcon, PlugIcon, PlusIcon, ServerIcon, XIcon } from "lucide-react";
import { useState, type ReactNode } from "react";
import type { Actions } from "./actions";
import { AgentMark } from "./AgentMark";
import { lineTone, markTone, rowAccessibleName, rowLine } from "./agentRow";
import { AgentRowItem } from "./components/agent-row";
import { StatusMark } from "./components/status-mark";
import { Badge } from "./components/ui/badge";
import { Button } from "./components/ui/button";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from "./components/ui/dropdown-menu";
import { Popover, PopoverContent, PopoverTrigger } from "./components/ui/popover";
import { Hint } from "./components/ui/tooltip";
import { cn } from "./lib/utils";
import { AGENT_COLUMNS, STAGES, agentColumnCards, stageCards, type AgentCard, type AgentsBoard, type PrChip, type TaskCard, type TasksBoard } from "./projectBoard";
import type { AgentRow, Task } from "./snapshot";

// The Tasks and Agents boards (PRD task-agents-views), drawn the same for a
// Project and for All projects: `buildTasks` and `buildAgents` decide every
// card, and this file only draws them and routes the clicks (D-08). A card's
// head opens its checkout, an agent row its pane, the id its source and the
// PR chip its pull request.

/** A column past this many cards shows the rest on request and scrolls inside itself (D-13). */
const COLUMN_LIMIT = 40;

/** A card draws its agents without their descendants' fold. */
const NO_ROWS: AgentRow[] = [];

export function TasksView({
  board,
  focusedPaneId,
  actions,
  openCheckout,
  doneOpen,
  onToggleDone,
}: {
  board: TasksBoard;
  focusedPaneId: string | null;
  actions: Actions;
  openCheckout: (card: TaskCard) => void;
  doneOpen: boolean;
  onToggleDone: () => void;
}) {
  if (board.empty) return <EmptyTasks reason={board.unconnectedReason} />;
  return (
    <div className="flex min-h-0 flex-1 flex-col" data-tasks-board="true">
      {board.adHoc.length > 0 ? (
        <section className="flex shrink-0 items-start gap-lg overflow-x-auto px-lg pb-md" aria-label="즉석" data-overview-adhoc="true">
          <div className="flex w-(--home-collapsed-width) shrink-0 flex-col gap-xxs pt-xs">
            <h2 className="text-subhead font-semibold text-subtle-foreground">즉석</h2>
            <p className="text-caption text-muted-foreground">{board.columns ? "main · 폴더" : "폴더"}</p>
          </div>
          <div className="flex items-start gap-md">
            {board.adHoc.map((value) => (
              <TaskCardView key={value.id} card={value} focusedPaneId={focusedPaneId} actions={actions} onHead={() => openCheckout(value)} />
            ))}
          </div>
        </section>
      ) : null}
      {board.columns ? (
        <div className="flex min-h-0 shrink items-stretch gap-md overflow-x-auto px-lg pb-md" data-overview-columns="tasks">
          {STAGES.map(({ stage, label }) => {
            const cards = stageCards(board, stage);
            const count = `${cards.length}${stage === "backlog" && board.overflow ? "+" : ""}`;
            return stage === "done" ? (
              <DoneColumn key={stage} label={label} count={count} cards={cards} open={doneOpen} onToggle={onToggleDone} focusedPaneId={focusedPaneId} actions={actions} openCheckout={openCheckout} />
            ) : (
              <Column key={stage} id={stage} label={label} count={count} cards={cards}>
                {(value) => <TaskCardView key={value.id} card={value} focusedPaneId={focusedPaneId} actions={actions} onHead={() => openCheckout(value)} />}
              </Column>
            );
          })}
        </div>
      ) : null}
      {board.unconnected.length > 0 ? (
        <div className="flex shrink-0 flex-col gap-sm px-lg pb-lg" data-overview-unconnected="true">
          {board.unconnected.map((value) => (
            <section key={value.place.projectId} className="flex flex-col gap-xs rounded-md border border-border bg-card p-sm" aria-label={`${value.place.projectLabel} · 태스크 출처 연결 안 됨`} data-unconnected-project={value.place.projectId}>
              <h2 className="flex items-center gap-xs text-body font-semibold text-foreground">
                <PlugIcon aria-hidden="true" className="size-(--size-icon) text-muted-foreground" />
                {value.place.projectLabel} · 태스크 출처 연결 안 됨
              </h2>
              <p className="text-caption text-muted-foreground">{value.agents}개 에이전트가 작업 중이지만 이 프로젝트에는 태스크 출처가 없음</p>
              <ConnectSource reason={value.reason} />
            </section>
          ))}
        </div>
      ) : null}
    </div>
  );
}

/** A column of cards: past `COLUMN_LIMIT` it shows the rest on request, and a tall column scrolls inside itself. */
function Column<T>({ id, label, count, cards, children }: { id: string; label: string; count: string; cards: T[]; children: (value: T) => ReactNode }) {
  const [all, setAll] = useState(false);
  const shown = all ? cards : cards.slice(0, COLUMN_LIMIT);
  const hidden = cards.length - shown.length;
  return (
    <section aria-label={label} data-overview-column={id} className="flex min-h-0 w-(--home-column-width) shrink-0 flex-col gap-sm">
      <ColumnTitle label={label} count={count} />
      <div className="flex min-h-0 flex-col gap-sm overflow-y-auto">
        {shown.map(children)}
        {hidden > 0 ? (
          <button type="button" className="rounded-xs px-xs text-left text-caption text-muted-foreground outline-none hover:text-foreground focus-visible:ring-1 focus-visible:ring-ring" onClick={() => setAll(true)} data-column-more={id}>
            스크롤 · {hidden}개 더 보기
          </button>
        ) : null}
      </div>
    </section>
  );
}

function ColumnTitle({ label, count, children }: { label: string; count: string; children?: ReactNode }) {
  return (
    <h2 className="flex shrink-0 items-center gap-xs px-xs text-subhead font-semibold text-foreground">
      {children ?? (
        <>
          <span>{label}</span>
          <span className="font-normal text-muted-foreground">· {count}</span>
        </>
      )}
    </h2>
  );
}

/** Done starts folded to one line per card (`title >`); its header unfolds it (D-03). */
function DoneColumn({
  label,
  count,
  cards,
  open,
  onToggle,
  focusedPaneId,
  actions,
  openCheckout,
}: {
  label: string;
  count: string;
  cards: TaskCard[];
  open: boolean;
  onToggle: () => void;
  focusedPaneId: string | null;
  actions: Actions;
  openCheckout: (card: TaskCard) => void;
}) {
  return (
    <section aria-label={label} data-overview-column="done" data-collapsed={open ? "false" : "true"} className="flex min-h-0 w-(--home-column-width) shrink-0 flex-col gap-sm">
      <ColumnTitle label={label} count={count}>
        <button type="button" aria-expanded={open} onClick={onToggle} data-overview-column-toggle="done" className="inline-flex items-center gap-xs rounded-xs outline-none hover:text-foreground focus-visible:ring-1 focus-visible:ring-ring">
          <span>{label}</span>
          <span className="font-normal text-muted-foreground">· {count}</span>
          {open ? <ChevronDownIcon aria-hidden="true" className="size-(--size-icon)" /> : <ChevronRightIcon aria-hidden="true" className="size-(--size-icon)" />}
        </button>
      </ColumnTitle>
      <div className="flex min-h-0 flex-col gap-sm overflow-y-auto">
        {open ? (
          cards.map((value) => <TaskCardView key={value.id} card={value} focusedPaneId={focusedPaneId} actions={actions} onHead={() => openCheckout(value)} />)
        ) : cards.length > 0 ? (
          <ul className="flex flex-col gap-xxs" data-overview-collapsed-names="done">
            {cards.map((value) => (
              <li key={value.id}>
                <button
                  type="button"
                  onClick={() => openCheckout(value)}
                  className="flex w-full min-w-0 items-center gap-xs rounded-sm border border-border bg-card px-sm py-xxs text-left text-caption text-subtle-foreground outline-none hover:bg-accent focus-visible:ring-1 focus-visible:ring-ring"
                >
                  <span className="min-w-0 flex-1 truncate">{value.title}</span>
                  <ChevronRightIcon aria-hidden="true" className="size-(--size-icon) shrink-0 text-muted-foreground" />
                </button>
              </li>
            ))}
          </ul>
        ) : null}
      </div>
    </section>
  );
}

/** The mark a task's id carries: an issue's circle-dot, a local file's page (D-06). */
function TaskGlyph({ task, className }: { task: Task; className?: string }) {
  const Glyph = task.source === "github" ? CircleDotIcon : FileTextIcon;
  return <Glyph aria-hidden="true" className={cn("size-(--size-icon-sm) shrink-0", className)} />;
}

/**
 * A task card (D-04): head (id and title), then the delivery facts, then at
 * most two agents and `+N`. A ready card with no agent offers Start agent on
 * hover or focus only (D-05).
 */
export function TaskCardView({ card, focusedPaneId, actions, onHead }: { card: TaskCard; focusedPaneId: string | null; actions: Actions; onHead: () => void }) {
  const { checkout, task, facts } = card;
  const halo = card.needsYou ? (card.error ? "border-destructive shadow-[0_0_var(--home-halo-radius)_var(--destructive)]" : "border-warning shadow-[0_0_var(--home-halo-radius)_var(--warning)]") : "border-border";
  const dimmed = card.stage === "done";
  const hasFacts = facts.files !== null || facts.ahead !== null || facts.pr !== null || facts.behind !== null;
  return (
    <article
      className={cn("group/card flex w-(--home-column-width) min-w-0 shrink-0 flex-col gap-xs rounded-md border bg-card p-sm", halo, dimmed && "opacity-(--opacity-secondary)")}
      data-overview-card={card.id}
      data-stage={card.stage ?? undefined}
      data-needs-you={card.needsYou ? "true" : undefined}
      data-task-key={task?.key}
    >
      <div className="flex min-w-0 items-start gap-xs">
        <p className="min-w-0 flex-1 break-words text-title font-semibold text-foreground">
          {task ? <TaskId task={task} help={card.idHelp} /> : null}
          {checkout ? (
            <button type="button" onClick={onHead} data-overview-workspace={checkout.id} className="rounded-xs text-left outline-none hover:underline focus-visible:ring-1 focus-visible:ring-ring">
              {card.title}
            </button>
          ) : (
            <span>{card.title}</span>
          )}
        </p>
        {card.sourceFailure ? (
          <Hint label={card.sourceFailure}>
            <span className="shrink-0 pt-xxs text-warning" data-source-failure="true" tabIndex={0}>
              <CircleHelpIcon aria-hidden="true" className="size-(--size-icon)" />
            </span>
          </Hint>
        ) : null}
      </div>
      {hasFacts ? (
        <div className="flex flex-wrap items-center gap-xxs" data-overview-facts="true">
          {facts.files !== null ? (
            <Badge variant="outline" className="border-warning text-warning" data-fact="files">
              {facts.files} files
            </Badge>
          ) : null}
          {facts.ahead !== null ? (
            <Badge variant="outline" data-fact="ahead">
              ↑{facts.ahead}
            </Badge>
          ) : null}
          {facts.pr ? <PrChipView pr={facts.pr} /> : null}
          {facts.behind !== null ? (
            <Badge variant="outline" className="text-warning" data-fact="behind">
              ↓{facts.behind} behind
            </Badge>
          ) : null}
        </div>
      ) : null}
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
            +{card.more}
          </span>
        </Hint>
      ) : null}
      {checkout && !task ? (
        <p className="flex items-center gap-xxs text-caption text-muted-foreground" data-no-task="true">
          <LinkIcon aria-hidden="true" className="size-(--size-icon-sm)" />
          태스크 없음
        </p>
      ) : null}
      {card.canStart && checkout ? <StartAgent path={checkout.path} actions={actions} /> : null}
    </article>
  );
}

/** A task's id, small and muted before its title; it opens the source, and its tooltip names the source and branch (D-05, D-06). */
function TaskId({ task, help }: { task: Task; help: string }) {
  const content = (
    <>
      <TaskGlyph task={task} />
      {task.id}
    </>
  );
  const className = "mr-xs inline-flex items-center gap-xxs rounded-xs align-baseline font-mono text-caption font-normal text-muted-foreground outline-none hover:text-foreground focus-visible:ring-1 focus-visible:ring-ring";
  if (!task.id) return null;
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

const PR_TONE: Record<PrChip["tone"], string> = {
  open: "text-pr-open",
  draft: "text-pr-draft",
  merged: "text-pr-merged",
  closed: "text-pr-closed",
};

const CHECKS: Record<"passing" | "failed" | "pending", { label: string; tone: string }> = {
  passing: { label: "CI 통과", tone: "text-success" },
  failed: { label: "CI 실패", tone: "text-destructive" },
  pending: { label: "CI 진행 중", tone: "text-muted-foreground" },
};

/** The result: `PR #n` in its lifecycle colour, and the CI mark once read; it opens the pull request (D-06). */
function PrChipView({ pr }: { pr: PrChip }) {
  const checks = pr.checks ? CHECKS[pr.checks] : null;
  return (
    <>
      <Hint label={`PR #${pr.number} · ${pr.tone}`}>
        <a href={pr.url} target="_blank" rel="noopener noreferrer" className="rounded-xs outline-none focus-visible:ring-1 focus-visible:ring-ring" data-pr-chip={pr.number} data-pr-tone={pr.tone}>
          <Badge variant="outline" className={PR_TONE[pr.tone]}>
            <GitPullRequestIcon aria-hidden="true" />
            PR #{pr.number}
          </Badge>
        </a>
      </Hint>
      {checks && pr.checks ? (
        <Badge variant="outline" className={checks.tone} data-pr-checks={pr.checks} aria-label={checks.label}>
          {pr.checks === "passing" ? <CheckIcon aria-hidden="true" /> : pr.checks === "failed" ? <XIcon aria-hidden="true" /> : <StatusMark symbol="●" className="size-(--size-icon-sm)" />}
          {pr.checks === "pending" ? "CI 진행 중" : "CI"}
        </Badge>
      ) : null}
    </>
  );
}

/** Start agent on a ready card nobody works on, shown on hover or focus only (D-05, B7). */
function StartAgent({ path, actions }: { path: string; actions: Actions }) {
  return (
    <div className="hidden group-focus-within/card:flex group-hover/card:flex" data-start-agent="true">
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <Button variant="secondary" size="sm" data-start-agent-trigger="true">
            <PlusIcon aria-hidden="true" />
            에이전트 시작
          </Button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="start">
          <DropdownMenuItem onSelect={() => actions.startAgent(path, "claude")}>Claude</DropdownMenuItem>
          <DropdownMenuItem onSelect={() => actions.startAgent(path, "codex")}>Codex</DropdownMenuItem>
          <DropdownMenuItem onSelect={() => actions.startAgent(path, "terminal")}>Terminal only</DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>
    </div>
  );
}

/** No source and no agent: one sentence and the way to connect one (D-13, B14). */
function EmptyTasks({ reason }: { reason: string | null }) {
  return (
    <div className="flex flex-1 flex-col items-center justify-center gap-sm p-xl text-center text-caption text-muted-foreground" data-overview-empty="true" data-tasks-empty="true">
      <InboxIcon aria-hidden="true" className="size-(--size-control-sm) text-muted-foreground" />
      <p>태스크 출처가 연결되지 않았고, 실행 중인 에이전트도 없습니다</p>
      <ConnectSource reason={reason} />
    </div>
  );
}

/**
 * `GitHub 이슈 연결`: Hide reads issues through the operator's own `gh`, so
 * connecting is `gh` being logged in for a repository with a GitHub remote.
 * The popover says which of those is missing, in `gh`'s own words.
 */
function ConnectSource({ reason }: { reason: string | null }) {
  return (
    <Popover>
      <PopoverTrigger asChild>
        <Button variant="secondary" size="sm" className="w-fit" data-connect-source="true">
          <Link2Icon aria-hidden="true" />
          GitHub 이슈 연결
        </Button>
      </PopoverTrigger>
      <PopoverContent className="flex flex-col gap-xs text-caption" data-connect-source-help="true">
        <p className="text-foreground">Hide는 이 저장소의 GitHub 이슈를 운영자의 `gh`로 읽습니다.</p>
        {reason ? <p className="break-words text-warning">{reason}</p> : null}
        <p className="text-muted-foreground">
          터미널에서 <code className="font-mono">gh auth login</code>을 실행하고, 저장소에 GitHub 원격이 있는지 확인하세요.
        </p>
      </PopoverContent>
    </Popover>
  );
}

/** The Agents board: three columns of agent cards, a delegated agent under its parent (D-12). */
export function AgentsView({ board, focusedPaneId, actions }: { board: AgentsBoard; focusedPaneId: string | null; actions: Actions }) {
  if (board.cards.length === 0) {
    return (
      <div className="flex flex-1 flex-col items-center justify-center gap-sm p-xl text-center text-caption text-muted-foreground" data-overview-empty="true" data-agents-empty="true">
        <p>실행 중인 에이전트가 없습니다</p>
      </div>
    );
  }
  return (
    <div className="flex min-h-0 flex-1 items-stretch gap-md overflow-x-auto px-lg pb-md" data-overview-columns="agents">
      {AGENT_COLUMNS.map(({ column, label }) => {
        const cards = agentColumnCards(board, column);
        return (
          <Column key={column} id={column} label={label} count={String(cards.length)} cards={cards}>
            {(value) => <AgentCardView key={`${value.place.projectId}:${value.agent.pane_id}`} card={value} selected={value.agent.pane_id === focusedPaneId} onOpen={actions.openAgent} />}
          </Column>
        );
      })}
    </div>
  );
}

/**
 * One agent (D-12): its mark, provider, title and age; the checkout it works
 * in; its task chip (or a quiet "태스크 없음") and its device. The card opens
 * the pane; the task chip opens the task, and its tooltip is the only place
 * the Agents view names a pull request.
 */
function AgentCardView({ card, selected, onOpen }: { card: AgentCard; selected: boolean; onOpen: (paneId: string) => void }) {
  const { agent, task } = card;
  const line = rowLine(agent);
  const request = line && line.mode !== "quiet" ? line : null;
  const attention = agent.group === "needs_you";
  return (
    <article
      className={cn(
        "relative flex min-w-0 shrink-0 flex-col gap-xxs rounded-md border bg-card p-sm",
        attention ? (agent.demand === "error" ? "border-destructive" : "border-warning") : "border-border",
        selected && "bg-secondary",
        agent.group === "seen" && "opacity-(--opacity-secondary)",
      )}
      style={{ marginLeft: `calc(${card.depth} * var(--size-lineage-indent))` }}
      data-overview-root={card.depth === 0 ? agent.pane_id : undefined}
      data-agent-card={agent.pane_id}
      data-depth={card.depth}
    >
      <button type="button" aria-label={rowAccessibleName(agent, card.device)} data-agent-open={agent.pane_id} className="absolute inset-0 rounded-md outline-none hover:bg-accent focus-visible:ring-1 focus-visible:ring-ring" onClick={() => onOpen(agent.pane_id)} />
      <span className="pointer-events-none relative flex min-w-0 items-center gap-xs" aria-hidden="true">
        <StatusMark symbol={agent.symbol} className={markTone(agent)} />
        <AgentMark kind={agent.agent_kind} />
        <span className={cn("min-w-0 flex-1 truncate text-body", attention ? "font-semibold text-foreground" : "text-foreground")}>{agent.identity_label}</span>
        <span className="shrink-0 font-mono text-micro text-muted-foreground">{agent.elapsed}</span>
      </span>
      {request ? (
        <span className={cn("pointer-events-none relative line-clamp-2 break-words text-caption", lineTone(request, agent.demand))} aria-hidden="true">
          {request.text}
        </span>
      ) : null}
      {card.where ? (
        <span className="pointer-events-none relative truncate text-caption text-muted-foreground" aria-hidden="true">
          {card.where}
        </span>
      ) : null}
      <span className="relative flex min-w-0 flex-wrap items-center gap-xxs">
        {task ? <AgentTaskChip task={task} help={card.taskHelp ?? task.title} /> : <span className="pointer-events-none text-caption text-muted-foreground">태스크 없음</span>}
        {card.device ? (
          <Badge variant="outline" className="pointer-events-none" data-device-chip={card.device}>
            <ServerIcon aria-hidden="true" />
            {card.device}
          </Badge>
        ) : null}
      </span>
    </article>
  );
}

/** The task an agent works on: an issue's `#N`, or a local task's title (D-12); it opens the task. */
function AgentTaskChip({ task, help }: { task: Task; help: string }) {
  const label = task.id ?? task.title;
  const chip = (
    <Badge variant="outline" className="max-w-(--size-pane-child-chip-max)">
      <TaskGlyph task={task} />
      <span className="truncate">{label}</span>
    </Badge>
  );
  return (
    <Hint label={help}>
      {task.url ? (
        <a href={task.url} target="_blank" rel="noopener noreferrer" className="rounded-xs outline-none focus-visible:ring-1 focus-visible:ring-ring" data-agent-task-chip={task.key}>
          {chip}
        </a>
      ) : (
        <span className="rounded-xs outline-none focus-visible:ring-1 focus-visible:ring-ring" data-agent-task-chip={task.key} tabIndex={0}>
          {chip}
        </span>
      )}
    </Hint>
  );
}

