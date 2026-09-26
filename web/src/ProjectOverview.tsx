import { ArrowDownIcon, ChevronDownIcon, ChevronRightIcon, FolderGit2Icon, FolderIcon, GitBranchIcon, GitCommitHorizontalIcon, GitMergeIcon, GitPullRequestIcon, HardDriveIcon, HouseIcon, PlusIcon } from "lucide-react";
import { useEffect, useMemo, useState, type ReactNode } from "react";
import type { Actions } from "./actions";
import { AgentRowItem } from "./components/agent-row";
import { Badge } from "./components/ui/badge";
import { Button } from "./components/ui/button";
import { Tabs, TabsList, TabsTrigger } from "./components/ui/tabs";
import { Hint } from "./components/ui/tooltip";
import { cn } from "./lib/utils";
import { FACT, FACTS_LINE, OpeningStatus, UnavailableNotice } from "./MainScreen";
import { overviewProject } from "./navigation";
import { AGENT_COLUMNS, STAGES, agentColumnCards, buildBoard, formatBytes, stageCards, type Board, type BoardCard, type Stage } from "./projectBoard";
import type { AgentRow, Checkout, Workspace } from "./snapshot";
import { useShellStore } from "./store";
import { ProjectSessions } from "./ProjectSessions";
import { useUiStore, type ProjectView } from "./ui";

// A Project's Overview (PRD web-project-overview): the Project scope the
// sidebar's project row opens. Under its title sits one line of facts, then
// the view: the Tasks board of its checkouts in Git columns under an ad hoc
// strip, the Agents board of its lineage roots, or its Sessions. The view is
// the page's (`projectView`), so another Project opens on the same one.
// Built from `buildBoard`; this file only draws it and routes the clicks.

const VIEWS: readonly { view: ProjectView; label: string }[] = [
  { view: "tasks", label: "Tasks" },
  { view: "agents", label: "Agents" },
  { view: "sessions", label: "Sessions" },
];

export function ProjectOverview({ projectId, actions }: { projectId: string; actions: Actions }) {
  const rest = useShellStore((s) => s.rest);
  const agents = useShellStore((s) => s.agents);
  const focusedPaneId = useShellStore((s) => s.focusedPaneId);
  const setScreen = useUiStore((s) => s.setScreen);
  const view = useUiStore((s) => s.projectView);
  const setView = useUiStore((s) => s.setProjectView);
  // The Project whose Merged column is open; the facts line's merged count opens it.
  const [mergedOpenFor, setMergedOpenFor] = useState<string | null>(null);
  const found = useMemo(() => overviewProject(rest, agents, projectId), [rest, agents, projectId]);
  const workspace = found?.workspace ?? null;
  const deviceAgents = found?.deviceAgents ?? null;
  const board = useMemo(() => (workspace && deviceAgents ? buildBoard(workspace, deviceAgents, Date.now()) : null), [workspace, deviceAgents]);
  // A local Git project's size is measured each time its Overview opens; a
  // remote project has none this machine can walk.
  const measurable = workspace !== null && workspace.is_git === true && !workspace.remote_target_id;
  const measuredId = measurable ? workspace.id : null;
  useEffect(() => {
    if (measuredId) actions.measureProjectDisk(measuredId);
  }, [actions, measuredId]);
  if (!found || !board) {
    return (
      <section className="flex flex-1 flex-col items-center justify-center gap-sm p-xl text-caption text-muted-foreground" data-overview-missing={projectId}>
        <p>This project is no longer in the catalog.</p>
        <Button variant="secondary" onClick={() => setScreen({ kind: "main" })}>Back to All projects</Button>
      </section>
    );
  }
  const { device, availability } = found;
  const project = found.workspace;
  const mergedOpen = mergedOpenFor === project.id;
  const newAgent = () => {
    if (project.is_git) return useUiStore.getState().setWorkspaceDialog({ kind: "new_worktree", workspaceId: project.id });
    const folder = project.checkouts[0];
    if (folder) actions.openWorkspace(project.device_id, project.id, folder.id);
  };
  const openCheckout = (checkout: Checkout) => actions.openWorkspace(project.device_id, checkout.workspace_id, checkout.id);
  const showMerged = () => {
    setView("tasks");
    setMergedOpenFor(project.id);
  };
  return (
    <section className="flex min-h-0 min-w-0 flex-1 flex-col bg-background" aria-label={`Project ${project.label}`} data-overview-screen={project.id} data-overview-state={board.state} data-overview-view={view}>
      <header className="flex shrink-0 flex-col gap-xs border-b border-border px-lg py-sm">
        <div className="flex min-w-0 items-center gap-lg">
          <nav aria-label="Location" className="flex min-w-0 items-center gap-xs">
            <button type="button" className="shrink-0 rounded-xs px-xs text-caption text-subtle-foreground hover:bg-accent hover:text-foreground focus-visible:bg-accent" data-go-main="true" onClick={() => setScreen({ kind: "main" })}>
              All projects
            </button>
            <span aria-hidden="true" className="text-caption text-muted-foreground">/</span>
            <Hint label={project.path} reveals>
              <h1 className="min-w-0 truncate text-headline font-semibold text-foreground" aria-current="page">
                {project.label}
              </h1>
            </Hint>
            {device && device.kind === "remote" ? <Badge variant="secondary">{device.label}</Badge> : null}
          </nav>
          <span className="flex-1" />
          <Button onClick={newAgent} disabled={!project.is_git && project.checkouts.length === 0} data-overview-new-agent="true">
            <PlusIcon aria-hidden="true" />
            New agent
          </Button>
        </div>
        <Stats workspace={project} board={board} onMerged={showMerged} />
      </header>
      <OpeningStatus actions={actions} />
      {device ? <UnavailableNotice device={device} availability={availability} actions={actions} /> : null}
      {availability.state === "loading" ? (
        <p role="status" className="shrink-0 px-lg pt-sm text-caption text-muted-foreground" data-device-loading="true">
          {availability.text}
        </p>
      ) : null}
      <div className="flex shrink-0 px-lg py-sm">
        <Tabs value={view} onValueChange={(value) => setView(value as ProjectView)}>
          <TabsList aria-label="Project view">
            {VIEWS.map((choice) => (
              <TabsTrigger key={choice.view} value={choice.view} data-overview-tab={choice.view}>
                {choice.label}
              </TabsTrigger>
            ))}
          </TabsList>
        </Tabs>
      </div>
      {view === "sessions" ? (
        <div className="flex min-h-0 flex-1 border-t border-border">
          {/* Keyed by the Project, so another Project starts with its own filters and asks for itself. */}
          <ProjectSessions key={`${project.device_id}:${project.id}`} workspace={project} actions={actions} />
        </div>
      ) : board.state === "empty" ? (
        <div className="flex flex-1 flex-col items-center justify-center gap-sm p-xl text-center text-caption text-muted-foreground" data-overview-empty="true">
          <p>No agent is working in this project yet.</p>
          <Button variant="secondary" onClick={newAgent} disabled={!project.is_git && project.checkouts.length === 0} data-overview-empty-new-agent="true">
            <PlusIcon aria-hidden="true" />
            New agent
          </Button>
        </div>
      ) : (
        <div className="min-h-0 flex-1 overflow-auto" data-overview-board="true">
          <div className="flex w-max min-w-full flex-col gap-lg px-lg pb-lg">
            {view === "tasks" ? (
              <TasksBoard
                board={board}
                focusedPaneId={focusedPaneId}
                actions={actions}
                openCheckout={openCheckout}
                mergedOpen={mergedOpen}
                onToggleMerged={() => setMergedOpenFor(mergedOpen ? null : project.id)}
              />
            ) : (
              <AgentsBoard board={board} focusedPaneId={focusedPaneId} actions={actions} />
            )}
          </div>
        </div>
      )}
    </section>
  );
}

/**
 * The facts line under a Git project's title (B2): its worktrees, open pull
 * requests once GitHub answered, its size on disk once measured (pending while
 * the walk runs, absent when a part could not be read), main behind origin
 * only when it is, and the merged worktrees only while there are any, which
 * opens the Merged column where each one is removed. A plain folder has no line.
 */
function Stats({ workspace, board, onMerged }: { workspace: Workspace; board: Board; onMerged: () => void }) {
  const { stats } = board;
  if (!workspace.is_git) return null;
  return (
    <div className={FACTS_LINE} data-overview-stats="true">
      <span className={FACT} data-stat="worktrees">
        <FolderGit2Icon aria-hidden="true" className="size-(--size-icon)" />
        {stats.worktrees} {stats.worktrees === 1 ? "worktree" : "worktrees"}
      </span>
      {stats.openPullRequests === null ? null : (
        <span className={FACT} data-stat="open-prs">
          <GitPullRequestIcon aria-hidden="true" className="size-(--size-icon)" />
          {stats.openPullRequests} open {stats.openPullRequests === 1 ? "PR" : "PRs"}
        </span>
      )}
      {stats.disk === "measuring" ? (
        <Hint label="Measuring allocated disk…">
          <span className={cn(FACT, "text-muted-foreground")} data-stat="disk" data-disk-measuring="true">
            <HardDriveIcon aria-hidden="true" className="size-(--size-icon)" />… GB
          </span>
        </Hint>
      ) : stats.disk === null ? null : (
        <Hint label="Allocated on disk, shared Git data counted once">
          <span className={FACT} data-stat="disk">
            <HardDriveIcon aria-hidden="true" className="size-(--size-icon)" />
            {formatBytes(stats.disk)}
          </span>
        </Hint>
      )}
      {stats.behind ? (
        <span className={cn(FACT, "text-warning")} data-stat="behind">
          <ArrowDownIcon aria-hidden="true" className="size-(--size-icon)" />
          {stats.behind.branch} ↓{stats.behind.count} behind origin
        </span>
      ) : null}
      {stats.merged > 0 ? (
        <Hint label="Show the merged worktrees">
          <button type="button" className={cn(FACT, "rounded-xs text-pr-merged outline-none hover:underline focus-visible:ring-1 focus-visible:ring-ring")} data-stat="merged" onClick={onMerged}>
            <GitMergeIcon aria-hidden="true" className="size-(--size-icon)" />
            {stats.merged} merged → 정리
          </button>
        </Hint>
      ) : null}
    </div>
  );
}

function TasksBoard({
  board,
  focusedPaneId,
  actions,
  openCheckout,
  mergedOpen,
  onToggleMerged,
}: {
  board: Board;
  focusedPaneId: string | null;
  actions: Actions;
  openCheckout: (checkout: Checkout) => void;
  mergedOpen: boolean;
  onToggleMerged: () => void;
}) {
  return (
    <>
      {board.adHoc.length > 0 ? (
        <section className="flex items-start gap-lg" aria-label="즉석" data-overview-adhoc="true">
          <div className="flex w-(--home-collapsed-width) shrink-0 flex-col gap-xxs pt-xs">
            <h2 className="text-subhead font-semibold text-subtle-foreground">즉석</h2>
            <p className="text-caption text-muted-foreground">{board.state === "board" ? "main · 폴더" : "폴더"}</p>
          </div>
          <div className="flex items-start gap-md">
            {board.adHoc.map((value) => (
              <CheckoutCard key={value.id} card={value} focusedPaneId={focusedPaneId} actions={actions} onHeader={() => value.checkout && openCheckout(value.checkout)} />
            ))}
          </div>
        </section>
      ) : null}
      {board.state === "board" ? (
        <>
          {board.adHoc.length > 0 ? <div className="h-(--size-hairline) bg-border" aria-hidden="true" /> : null}
          <div className="flex items-start gap-md" data-overview-columns="tasks">
            {STAGES.map(({ stage, label }) => {
              const cards = stageCards(board, stage);
              const collapsible = stage === "merged";
              return (
                <Column
                  key={stage}
                  id={stage}
                  label={label}
                  count={`${cards.length}${stage === "ready" && board.overflow ? "+" : ""}`}
                  collapsed={collapsible && !mergedOpen}
                  onToggle={collapsible ? onToggleMerged : null}
                  names={cards.map((value) => value.checkout?.branch ?? value.checkout?.label ?? value.title ?? "")}
                  mono
                >
                  {cards.map((value) =>
                    value.backlog ? (
                      <BacklogCard key={value.id} card={value} />
                    ) : (
                      <CheckoutCard key={value.id} card={value} focusedPaneId={focusedPaneId} actions={actions} dimmed={stage === "merged"} onHeader={() => value.checkout && openCheckout(value.checkout)} />
                    ),
                  )}
                </Column>
              );
            })}
          </div>
        </>
      ) : null}
    </>
  );
}

function AgentsBoard({ board, focusedPaneId, actions }: { board: Board; focusedPaneId: string | null; actions: Actions }) {
  const [seenOpen, setSeenOpen] = useState(false);
  return (
    <div className="flex items-start gap-md" data-overview-columns="agents">
      {AGENT_COLUMNS.map(({ column, label }) => {
        const cards = agentColumnCards(board, column);
        const collapsible = column === "seen";
        return (
          <Column
            key={column}
            id={column}
            label={label}
            count={String(cards.length)}
            collapsed={collapsible && !seenOpen}
            onToggle={collapsible ? () => setSeenOpen((open) => !open) : null}
            names={cards.map((value) => value.rows[0]?.agent.identity_label ?? "")}
            mono={false}
          >
            {cards.map((value) => (
              <CheckoutCard
                key={value.id}
                card={value}
                focusedPaneId={focusedPaneId}
                actions={actions}
                dimmed={column === "seen"}
                agentsView
              />
            ))}
          </Column>
        );
      })}
    </div>
  );
}

/** A column of cards; a collapsible one starts closed and lists only its names while closed. */
function Column({
  id,
  label,
  count,
  collapsed,
  onToggle,
  names,
  mono,
  children,
}: {
  id: string;
  label: string;
  count: string;
  collapsed: boolean;
  onToggle: (() => void) | null;
  names: string[];
  /** Branch names are read in mono; agent titles are prose. */
  mono: boolean;
  children: ReactNode;
}) {
  const title = (
    <>
      <span>{label}</span>
      <span className="font-normal text-muted-foreground">{count}</span>
    </>
  );
  return (
    <section
      aria-label={label}
      data-overview-column={id}
      data-collapsed={collapsed ? "true" : "false"}
      className={cn("flex shrink-0 flex-col gap-sm", collapsed ? "w-(--home-collapsed-width)" : "w-(--home-column-width)")}
    >
      <h2 className="flex items-center gap-xs px-xs text-subhead font-semibold text-foreground">
        {onToggle ? (
          <button type="button" aria-expanded={!collapsed} onClick={onToggle} data-overview-column-toggle={id} className="inline-flex items-center gap-xs rounded-xs outline-none hover:text-foreground focus-visible:ring-1 focus-visible:ring-ring">
            {title}
            {collapsed ? <ChevronRightIcon aria-hidden="true" className="size-(--size-icon)" /> : <ChevronDownIcon aria-hidden="true" className="size-(--size-icon)" />}
          </button>
        ) : (
          title
        )}
      </h2>
      {collapsed ? (
        names.length > 0 ? (
          <ul className="flex flex-col gap-xxs rounded-md bg-card p-sm" data-overview-collapsed-names={id}>
            {names.map((name, index) => (
              <li key={`${name}:${index}`} className={cn("truncate text-caption text-muted-foreground", mono && "font-mono")}>
                {name}
              </li>
            ))}
          </ul>
        ) : null
      ) : (
        children
      )}
    </section>
  );
}

function checkoutIcon(checkout: Checkout) {
  if (checkout.is_worktree) return GitBranchIcon;
  if (checkout.worktree) return HouseIcon;
  return FolderIcon;
}

/** A card draws its whole lineage, so no row lists folded children. */
const NO_ROWS: AgentRow[] = [];

/**
 * A checkout's card: its header opens the checkout and each agent row its
 * pane (B8). An Agents card is its lineage, root first, with the checkout
 * named in the footer, so it has no header of its own.
 */
function CheckoutCard({
  card,
  focusedPaneId,
  actions,
  onHeader,
  dimmed = false,
  agentsView = false,
}: {
  card: BoardCard;
  focusedPaneId: string | null;
  actions: Actions;
  onHeader?: () => void;
  dimmed?: boolean;
  agentsView?: boolean;
}) {
  const checkout = card.checkout;
  const name = checkout?.branch ?? checkout?.label ?? "";
  const Icon = checkout ? checkoutIcon(checkout) : null;
  const halo = card.needsYou ? (card.error ? "border-destructive shadow-[0_0_var(--home-halo-radius)_var(--destructive)]" : "border-warning shadow-[0_0_var(--home-halo-radius)_var(--warning)]") : "border-border";
  return (
    <article
      className={cn("flex w-(--home-column-width) min-w-0 flex-col gap-xs rounded-md border bg-card p-sm", halo, dimmed && "opacity-(--opacity-secondary)")}
      data-overview-card={card.id}
      data-stage={card.stage ?? undefined}
      data-needs-you={card.needsYou ? "true" : undefined}
      data-overview-root={agentsView ? card.rows[0]?.agent.pane_id : undefined}
    >
      {agentsView ? null : (
        <div className="flex min-w-0 items-center gap-xs">
          <Hint label={`${name} · ${checkout?.path ?? ""}${checkout && !checkout.exists ? " · missing" : ""}`}>
            <button
              type="button"
              onClick={onHeader}
              data-overview-workspace={checkout?.id}
              className="flex min-w-0 flex-1 items-center gap-xs rounded-xs text-left outline-none hover:text-foreground focus-visible:ring-1 focus-visible:ring-ring"
            >
              {Icon ? <Icon aria-hidden="true" className="size-(--size-icon) shrink-0 text-muted-foreground" /> : null}
              <span className="min-w-0 truncate text-title font-semibold text-foreground">{name}</span>
              {checkout && !checkout.exists ? <span className="shrink-0 text-micro uppercase text-destructive">missing</span> : null}
            </button>
          </Hint>
          <IssueChips card={card} />
        </div>
      )}
      {!agentsView && card.title ? (
        <Hint label={card.title} reveals>
          <p className="line-clamp-2 break-words text-caption text-subtle-foreground" data-overview-card-title="true">
            {card.title}
          </p>
        </Hint>
      ) : null}
      {card.rows.length > 0 ? (
        <ul className="flex flex-col" role="list" data-overview-rows="true">
          {card.rows.map((row) => (
            <AgentRowItem
              key={row.agent.pane_id}
              agent={row.agent}
              device={null}
              depth={row.depth}
              descendants={0}
              childRows={NO_ROWS}
              selected={row.agent.pane_id === focusedPaneId}
              onOpen={actions.openAgent}
              onToggleTree={null}
            />
          ))}
        </ul>
      ) : null}
      <Footer card={card} agentsView={agentsView} />
    </article>
  );
}

function IssueChips({ card }: { card: BoardCard }) {
  const issue = card.issue?.issue;
  if (!issue) return null;
  const closed = issue.state === "CLOSED";
  return (
    <>
      <Hint label={card.issueHelp ?? ""}>
        <a href={issue.url} target="_blank" rel="noopener noreferrer" data-issue-chip={issue.reference.number} className="shrink-0 rounded-xs outline-none focus-visible:ring-1 focus-visible:ring-ring">
          <Badge variant="secondary" className={closed ? "text-pr-merged" : undefined}>
            #{issue.reference.number}
            {closed ? " 닫힘" : ""}
          </Badge>
        </a>
      </Hint>
      {card.mismatch ? (
        <Hint label={card.mismatchHelp ?? ""}>
          <Badge variant="outline" className="shrink-0 text-warning" data-issue-mismatch="true">
            ≠ {card.mismatch}
          </Badge>
        </Hint>
      ) : null}
    </>
  );
}

const CHECKS: Record<"passing" | "failed" | "pending", { label: string; tone: string }> = {
  passing: { label: "CI 통과", tone: "text-success" },
  failed: { label: "CI 실패", tone: "text-destructive" },
  pending: { label: "CI 대기", tone: "text-warning" },
};

/** The card's one delivery fact (B6); an Agents card also names its checkout. */
function Footer({ card, agentsView }: { card: BoardCard; agentsView: boolean }) {
  const fact = card.delivery;
  const branch = agentsView ? (card.checkout?.branch ?? card.checkout?.label ?? null) : null;
  if (!fact && !branch) return null;
  const stage: Stage | null = card.stage;
  const Icon = stage === "review" ? GitPullRequestIcon : stage === "merged" ? GitMergeIcon : GitCommitHorizontalIcon;
  const checks = fact?.checks ? CHECKS[fact.checks] : null;
  const tone = stage === "review" ? (checks?.tone ?? "text-pr-open") : stage === "merged" ? "text-pr-merged" : "text-muted-foreground";
  return (
    <div className={cn("flex min-w-0 items-center gap-xs pt-xxs font-mono text-caption", tone)} data-overview-delivery={card.stage ?? "none"}>
      {branch ? (
        <Hint label={branch} reveals>
          <span className="min-w-0 truncate text-muted-foreground">{branch}</span>
        </Hint>
      ) : null}
      {fact ? (
        <span className="inline-flex min-w-0 items-center gap-xxs">
          <Icon aria-hidden="true" className="size-(--size-icon) shrink-0" />
          <span className="truncate">
            {fact.label}
            {checks ? ` · ${checks.label}` : ""}
          </span>
        </span>
      ) : null}
    </div>
  );
}

/** An open issue no checkout works on; opening it goes to GitHub (B5). */
function BacklogCard({ card }: { card: BoardCard }) {
  const issue = card.issue?.issue;
  if (!issue) return null;
  return (
    <Hint label={card.issueHelp ?? issue.title}>
      <a
        href={issue.url}
        target="_blank"
        rel="noopener noreferrer"
        data-overview-card={card.id}
        data-backlog="true"
        className="flex w-(--home-column-width) min-w-0 flex-col gap-xxs rounded-md border border-dashed border-border bg-sidebar p-sm text-left outline-none hover:bg-accent focus-visible:ring-1 focus-visible:ring-ring"
      >
        <span className="font-mono text-caption text-muted-foreground">#{issue.reference.number}</span>
        <span className="line-clamp-2 break-words text-body text-subtle-foreground">{issue.title}</span>
      </a>
    </Hint>
  );
}
