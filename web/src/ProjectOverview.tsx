import { ArrowDownIcon, CircleDotIcon, FolderGit2Icon, GitMergeIcon, GitPullRequestIcon, HardDriveIcon, PlusIcon, SquareTerminalIcon } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import type { Actions } from "./actions";
import { Badge } from "./components/ui/badge";
import { Button } from "./components/ui/button";
import { Kbd } from "./components/ui/kbd";
import { Tabs, TabsList, TabsTrigger } from "./components/ui/tabs";
import { Hint } from "./components/ui/tooltip";
import { cn } from "./lib/utils";
import { FACT, FACTS_LINE, OpeningStatus, UnavailableNotice } from "./MainScreen";
import { overviewProject } from "./navigation";
import { useNewIssueShortcut } from "./IssueDialogs";
import { buildAgents, buildTasks, formatBytes, projectStats, waitingCount, type BoardProject, type BoardStats, type SourceState, type TaskCard } from "./projectBoard";
import type { Workspace } from "./snapshot";
import { useShellStore } from "./store";
import { ProjectSessions } from "./ProjectSessions";
import { AgentsView, DependenciesView, TasksListView, TasksModeToggle, TasksView, type BoardHandlers } from "./TaskBoards";
import { scopeView, useUiStore, type ProjectView } from "./ui";

// A Project's Overview (PRD web-project-overview, task-agents-views, and the
// issue-first rework of 2026-09-28): the Project scope the sidebar's project
// row opens. Under its title sits one line of facts, then the view: the
// Tasks board (work flows from an issue in Backlog to its agents In progress
// to its pull request in Review; also as a List, or as the tasks that wait on
// one another), the Agents inbox, or its Sessions. The page scrolls as one.
// Its first action is a new issue (C); New agent starts one with no issue.
// The view is the page's (`projectView`), so another Project opens on the
// same one. The boards are `TaskBoards.tsx`'s, shared with All projects.

const VIEWS: readonly { view: ProjectView; label: string }[] = [
  { view: "tasks", label: "Tasks" },
  { view: "agents", label: "Agents" },
  { view: "sessions", label: "Sessions" },
];
const VIEW_IDS = VIEWS.map((row) => row.view);

export function ProjectOverview({ projectId, actions }: { projectId: string; actions: Actions }) {
  const rest = useShellStore((s) => s.rest);
  const agents = useShellStore((s) => s.agents);
  const focusedPaneId = useShellStore((s) => s.focusedPaneId);
  const setScreen = useUiStore((s) => s.setScreen);
  const view = scopeView(useUiStore((s) => s.projectView), VIEW_IDS);
  const setView = useUiStore((s) => s.setProjectView);
  const tasksMode = useUiStore((s) => s.tasksMode);
  const setTasksMode = useUiStore((s) => s.setTasksMode);
  // The Project whose Done column is open; the facts line's merged count opens it.
  const [doneOpenFor, setDoneOpenFor] = useState<string | null>(null);
  const found = useMemo(() => overviewProject(rest, agents, projectId), [rest, agents, projectId]);
  const workspace = found?.workspace ?? null;
  const deviceAgents = found?.deviceAgents ?? null;
  const projects = useMemo<BoardProject[]>(
    () => (workspace && deviceAgents ? [{ workspace, agents: deviceAgents, device: found?.device?.kind === "remote" ? found.device.label : null }] : []),
    [workspace, deviceAgents, found?.device],
  );
  const tasks = useMemo(() => (projects.length > 0 ? buildTasks(projects, "project", Date.now()) : null), [projects]);
  const agentBoard = useMemo(() => (projects.length > 0 ? buildAgents(projects, "project") : null), [projects]);
  const stats = useMemo(() => (workspace ? projectStats(workspace) : null), [workspace]);
  // A local Git project's size is measured each time its Overview opens, and
  // its tasks are read from its source; a device project has neither here.
  const local = workspace !== null && workspace.is_git === true && !workspace.remote_target_id;
  const localId = local ? workspace.id : null;
  useEffect(() => {
    if (!localId) return;
    actions.measureProjectDisk(localId);
    actions.readProjectTasks(localId);
  }, [actions, localId]);
  const canIssue = workspace !== null && !workspace.remote_target_id && workspace.tasks?.source != null;
  const newIssue = () => {
    if (canIssue && workspace) useUiStore.getState().setWorkspaceDialog({ kind: "new_issue", workspaceId: workspace.id });
  };
  useNewIssueShortcut(canIssue ? newIssue : null);
  if (!found || !tasks || !agentBoard || !stats) {
    return (
      <section className="flex flex-1 flex-col items-center justify-center gap-sm p-xl text-caption text-muted-foreground" data-overview-missing={projectId}>
        <p>This project is no longer in the catalog.</p>
        <Button variant="secondary" onClick={() => setScreen({ kind: "main" })}>Back to Overview</Button>
      </section>
    );
  }
  const { device, availability } = found;
  const project = found.workspace;
  const doneOpen = doneOpenFor === project.id;
  const newAgent = () => {
    if (project.is_git) return useUiStore.getState().setWorkspaceDialog({ kind: "new_worktree", workspaceId: project.id });
    const folder = project.checkouts[0];
    if (folder) actions.openWorkspace(project.device_id, project.id, folder.id);
  };
  const openCheckout = (card: TaskCard) => {
    if (card.checkout) actions.openWorkspace(project.device_id, card.checkout.workspace_id, card.checkout.id);
  };
  const showDone = () => {
    setView("tasks");
    setTasksMode("board");
    setDoneOpenFor(project.id);
  };
  const handlers: BoardHandlers = {
    openCheckout,
    startIssue: (card) => {
      if (card.task) useUiStore.getState().setWorkspaceDialog({ kind: "start_issue", workspaceId: card.place.projectId, taskKey: card.task.key });
    },
    newIssue,
  };
  const waiting = waitingCount(agentBoard);
  const state = view === "tasks" ? (tasks.cards.length === 0 ? "empty" : "board") : view === "agents" ? (agentBoard.cards.length === 0 ? "empty" : "board") : "sessions";
  const sessions = view === "sessions";
  return (
    <section
      className={cn("flex min-h-0 min-w-0 flex-1 flex-col bg-background", !sessions && "overflow-y-auto")}
      aria-label={`Project ${project.label}`}
      data-overview-screen={project.id}
      data-overview-state={state}
      data-overview-view={view}
    >
      <header className="flex shrink-0 flex-col gap-xs border-b border-border px-lg py-sm">
        <div className="flex min-w-0 items-center gap-lg">
          <nav aria-label="Location" className="flex min-w-0 items-center gap-xs">
            <button type="button" className="shrink-0 rounded-xs px-xs text-caption text-subtle-foreground hover:bg-accent hover:text-foreground focus-visible:bg-accent" data-go-main="true" onClick={() => setScreen({ kind: "main" })}>
              Overview
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
          <Button variant="ghost" onClick={newAgent} disabled={!project.is_git && project.checkouts.length === 0} data-overview-new-agent="true">
            <SquareTerminalIcon aria-hidden="true" />
            New agent
          </Button>
          {canIssue ? (
            <Button onClick={newIssue} data-overview-new-issue="true">
              <PlusIcon aria-hidden="true" />
              새 이슈
              <Kbd>C</Kbd>
            </Button>
          ) : null}
        </div>
        <Stats workspace={project} stats={stats} source={tasks.source} onMerged={showDone} />
      </header>
      <OpeningStatus actions={actions} />
      {device ? <UnavailableNotice device={device} availability={availability} actions={actions} /> : null}
      {availability.state === "loading" ? (
        <p role="status" className="shrink-0 px-lg pt-sm text-caption text-muted-foreground" data-device-loading="true">
          {availability.text}
        </p>
      ) : null}
      <div className="flex shrink-0 items-center justify-between gap-md px-lg py-sm">
        <Tabs value={view} onValueChange={(value) => setView(value as ProjectView)}>
          <TabsList aria-label="Project view">
            {VIEWS.map((choice) => (
              <TabsTrigger key={choice.view} value={choice.view} data-overview-tab={choice.view}>
                {choice.label}
                {choice.view === "agents" && waiting > 0 ? (
                  <span className="text-caption text-warning" data-agents-waiting={waiting} aria-label={`${waiting}개가 내 차례`}>
                    {waiting}
                  </span>
                ) : null}
              </TabsTrigger>
            ))}
          </TabsList>
        </Tabs>
        {view === "tasks" ? <TasksModeToggle /> : null}
      </div>
      {view === "sessions" ? (
        <div className="flex min-h-0 flex-1 border-t border-border">
          {/* Keyed by the Project, so another Project starts with its own filters and asks for itself. */}
          <ProjectSessions key={`${project.device_id}:${project.id}`} workspace={project} actions={actions} />
        </div>
      ) : view === "tasks" && tasksMode === "dependencies" ? (
        <DependenciesView board={tasks} focusedPaneId={focusedPaneId} actions={actions} handlers={handlers} />
      ) : view === "tasks" && tasksMode === "list" ? (
        <TasksListView board={tasks} focusedPaneId={focusedPaneId} actions={actions} handlers={handlers} />
      ) : view === "tasks" ? (
        <TasksView board={tasks} scope="project" focusedPaneId={focusedPaneId} actions={actions} handlers={handlers} doneOpen={doneOpen} onToggleDone={() => setDoneOpenFor(doneOpen ? null : project.id)} />
      ) : (
        <AgentsView board={agentBoard} focusedPaneId={focusedPaneId} actions={actions} />
      )}
    </section>
  );
}

/** `20 open issues · GitHub`, once the source has answered. */
export function IssuesFact({ source }: { source: SourceState }) {
  if (source.openIssues === null) return null;
  return (
    <span className={FACT} data-stat="open-issues">
      <CircleDotIcon aria-hidden="true" className="size-(--size-icon)" />
      {source.openIssues} open {source.openIssues === 1 ? "issue" : "issues"}
      {source.label ? <span className="text-muted-foreground">· {source.label}</span> : null}
    </span>
  );
}

/**
 * The facts line under a project's title (B2): its open issues and their
 * source; for a Git project also its worktrees, open pull requests once
 * GitHub answered, its size on disk once measured (pending while the walk
 * runs, absent when a part could not be read), main behind origin only when
 * it is, and the merged worktrees only while there are any, which opens the
 * Done column.
 */
function Stats({ workspace, stats, source, onMerged }: { workspace: Workspace; stats: BoardStats; source: SourceState; onMerged: () => void }) {
  if (!workspace.is_git) {
    return source.openIssues === null ? null : (
      <div className={FACTS_LINE} data-overview-stats="true">
        <IssuesFact source={source} />
      </div>
    );
  }
  return (
    <div className={FACTS_LINE} data-overview-stats="true">
      <IssuesFact source={source} />
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
