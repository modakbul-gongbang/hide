import { ArrowDownIcon, FolderGit2Icon, GitMergeIcon, HardDriveIcon, PlusIcon, SquareTerminalIcon } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import type { Actions } from "./actions";
import { Badge } from "./components/ui/badge";
import { Button } from "./components/ui/button";
import { Kbd } from "./components/ui/kbd";
import { Hint } from "./components/ui/tooltip";
import { cn } from "./lib/utils";
import { FACT, FACTS_LINE, OpeningStatus, UnavailableNotice } from "./MainScreen";
import { overviewProject } from "./navigation";
import { useNewIssueShortcut } from "./IssueDialogs";
import { AgentsLens, AgentsModeToggle, LensTiles, lensHandlers } from "./OverviewLenses";
import { agentsTile, buildLanes, buildLineages, issuesTile, lastIssueRead, scopeAgents, sessionsTile } from "./overviewLens";
import { buildTasks, formatBytes, projectStats, type BoardProject, type BoardStats, type TaskCard } from "./projectBoard";
import { IssuesView, panelCard, type IssuesPage } from "./IssuesView";
import type { Workspace } from "./snapshot";
import { useShellStore } from "./store";
import { ProjectSessions } from "./ProjectSessions";
import { IssueFilterControl, TasksModeToggle } from "./TaskBoards";
import { toggledFold, useUiStore, type OverviewLens } from "./ui";

// A Project's Overview (PRD web-project-overview, task-agents-views, the
// issue-first rework and overview-lenses-tiles-agents): the Project scope the
// sidebar's project row opens. Under its title sits one line of repository
// facts with the chosen tile's mode control at its right end, then the lens
// tiles, Agents, Issues and Sessions, where the tab row was. Every way in
// opens Agents › checkouts with the lane in front selected; the lens rides
// on the screen, so only Recent Panels brings back one as it was left
// (`OverviewLens`). Issues is the issue-first Tasks board under its new name;
// the boards are `TaskBoards.tsx`'s and the lenses `OverviewLenses.tsx`'s.

export function ProjectOverview({ projectId, lens, actions }: { projectId: string; lens: OverviewLens; actions: Actions }) {
  const rest = useShellStore((s) => s.rest);
  const agents = useShellStore((s) => s.agents);
  const focusedPaneId = useShellStore((s) => s.focusedPaneId);
  const sessions = useShellStore((s) => s.projectSessions);
  const setScreen = useUiStore((s) => s.setScreen);
  const setLens = useUiStore((s) => s.setLens);
  // The Project whose Done column is open.
  const [doneOpenFor, setDoneOpenFor] = useState<string | null>(null);
  const found = useMemo(() => overviewProject(rest, agents, projectId), [rest, agents, projectId]);
  const workspace = found?.workspace ?? null;
  const deviceAgents = found?.deviceAgents ?? null;
  const projects = useMemo<BoardProject[]>(
    () => (workspace && deviceAgents ? [{ workspace, agents: deviceAgents, device: found?.device?.kind === "remote" ? found.device.label : null }] : []),
    [workspace, deviceAgents, found?.device],
  );
  const now = Date.now();
  const tasks = useMemo(() => (projects.length > 0 ? buildTasks(projects, "project", Date.now()) : null), [projects]);
  const lensAgents = useMemo(() => scopeAgents(projects), [projects]);
  const lanes = useMemo(() => buildLanes(projects, lensAgents, "project"), [projects, lensAgents]);
  const lineages = useMemo(() => buildLineages(lensAgents), [lensAgents]);
  const stats = useMemo(() => (workspace ? projectStats(workspace) : null), [workspace]);
  const tiles = useMemo(
    () => (tasks && workspace && found ? [agentsTile(lensAgents, found.availability), issuesTile(tasks, Date.now(), lastIssueRead(workspace)), sessionsTile(sessions, workspace.id, Date.now())] : []),
    [tasks, workspace, found, lensAgents, sessions],
  );
  // A local Git project's size is measured each time its Overview opens, and
  // its tasks are read from its source; every Project's session history is
  // read once, for the Sessions tile (D-16).
  const local = workspace !== null && workspace.is_git === true && !workspace.remote_target_id;
  const localId = local ? workspace.id : null;
  useEffect(() => {
    if (!localId) return;
    actions.measureProjectDisk(localId);
    actions.readProjectTasks(localId);
  }, [actions, localId]);
  const sessionsId = workspace?.id ?? null;
  const sessionsDevice = workspace?.device_id ?? null;
  useEffect(() => {
    if (sessionsId && sessionsDevice) actions.refreshProjectSessions(sessionsId, sessionsDevice);
  }, [actions, sessionsId, sessionsDevice]);
  // An issue chip asked for its card: bring it into view once the Issues tab draws it.
  useEffect(() => {
    if (lens.tab !== "issues" || !lens.focusTask) return;
    document.querySelector(`[data-overview-screen] [data-task-key="${CSS.escape(lens.focusTask)}"]`)?.scrollIntoView({ block: "nearest" });
  }, [lens.tab, lens.focusTask]);
  const canIssue = workspace !== null && !workspace.remote_target_id && workspace.tasks?.source != null;
  const newIssue = () => {
    if (canIssue && workspace) useUiStore.getState().setWorkspaceDialog({ kind: "new_issue", workspaceId: workspace.id });
  };
  useNewIssueShortcut(canIssue ? newIssue : null);
  if (!found || !tasks || !stats) {
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
  // `N merged → 정리` opens what is only there to be removed (B20).
  const showCleanup = () => setLens({ tab: "agents", agentsMode: "checkouts", folds: lens.folds.includes("cleanup") ? lens.folds : [...lens.folds, "cleanup"] });
  const page: IssuesPage = {
    openCheckout,
    startIssue: (card) => useUiStore.getState().setWorkspaceDialog({ kind: "start_issue", workspaceId: card.place.projectId, taskKey: card.task.key }),
    newIssue,
    showCheckouts: () => setLens({ tab: "agents", agentsMode: "checkouts", panel: null }),
  };
  // An issue chip opens the Issues tile at its card with the issue's panel open.
  const lensActions = lensHandlers(actions, {
    openIssue: (_owner, task) => setLens({ tab: "issues", focusTask: task.key, panel: task.key }),
    toggleFold: (fold) => setLens({ folds: toggledFold(lens.folds, fold) }),
  });
  const view = lens.tab;
  // With the issue panel open the board and the panel scroll on their own, under a header that stays (D-43).
  const split = view === "issues" && panelCard(tasks, lens.panel) !== null;
  const state = view === "issues" ? (tasks.cards.length === 0 ? "empty" : "board") : view === "agents" ? (lensAgents.length === 0 ? "empty" : "board") : "sessions";
  const sessionsView = view === "sessions";
  return (
    <section
      className={cn("flex min-h-0 min-w-0 flex-1 flex-col bg-background", !sessionsView && !split && "overflow-y-auto")}
      aria-label={`Project ${project.label}`}
      data-overview-screen={project.id}
      data-overview-state={state}
      data-overview-view={view}
      data-agents-mode={view === "agents" ? lens.agentsMode : undefined}
    >
      <header className="flex shrink-0 flex-col gap-sm border-b border-border px-lg py-sm">
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
        <div className="flex min-w-0 flex-wrap items-center justify-between gap-md">
          <Stats workspace={project} stats={stats} onMerged={showCleanup} />
          {view === "agents" ? (
            <AgentsModeToggle mode={lens.agentsMode} onChange={(agentsMode) => setLens({ agentsMode })} />
          ) : view === "issues" ? (
            <span className="flex items-center gap-xs" data-issues-controls="true">
              <IssueFilterControl filter={lens.filter} onChange={(filter) => setLens({ filter })} />
              <TasksModeToggle mode={lens.tasksMode} onChange={(tasksMode) => setLens({ tasksMode })} />
            </span>
          ) : null}
        </div>
        <LensTiles tiles={tiles} selected={view} onSelect={(tab) => setLens({ tab, focusTask: null, panel: null })} />
      </header>
      <OpeningStatus actions={actions} />
      {device ? <UnavailableNotice device={device} availability={availability} actions={actions} /> : null}
      {availability.state === "loading" ? (
        <p role="status" className="shrink-0 px-lg pt-sm text-caption text-muted-foreground" data-device-loading="true">
          {availability.text}
        </p>
      ) : null}
      <div className="h-(--spacing-md) shrink-0" />
      {view === "sessions" ? (
        <div className="flex min-h-0 flex-1 border-t border-border">
          {/* Keyed by the Project, so another Project starts with its own filters and asks for itself. */}
          <ProjectSessions key={`${project.device_id}:${project.id}`} workspace={project} actions={actions} />
        </div>
      ) : view === "agents" ? (
        <AgentsLens mode={lens.agentsMode} lanes={lanes} lineages={lineages} scope="project" selectedLane={lens.lane} folds={lens.folds} handlers={lensActions} now={now} />
      ) : (
        <IssuesView
          board={tasks}
          scope="project"
          mode={lens.tasksMode}
          filter={lens.filter}
          onFilterChange={(filter) => setLens({ filter })}
          panel={lens.panel}
          onPanel={(panel) => setLens({ panel })}
          focusTask={lens.focusTask}
          focusedPaneId={focusedPaneId}
          doneOpen={doneOpen}
          onToggleDone={() => setDoneOpenFor(doneOpen ? null : project.id)}
          actions={actions}
          page={page}
        />
      )}
    </section>
  );
}

/**
 * The facts line under a project's title (B8): only the repository's own
 * facts, for a Git project its worktrees, its size on disk once measured
 * (pending while the walk runs, absent when a part could not be read), main
 * behind origin only when it is, and the merged worktrees only while there
 * are any, which opens `정리할 것` on Agents › checkouts. The issue and pull
 * request counts are the tiles'.
 */
function Stats({ workspace, stats, onMerged }: { workspace: Workspace; stats: BoardStats; onMerged: () => void }) {
  if (!workspace.is_git) return <span />;
  return (
    <div className={FACTS_LINE} data-overview-stats="true">
      <span className={FACT} data-stat="worktrees">
        <FolderGit2Icon aria-hidden="true" className="size-(--size-icon)" />
        {stats.worktrees} {stats.worktrees === 1 ? "worktree" : "worktrees"}
      </span>
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
        <Hint label="머지된 워크트리를 정리할 것에서 보기">
          <button type="button" className={cn(FACT, "rounded-xs text-pr-merged outline-none hover:underline focus-visible:ring-1 focus-visible:ring-ring")} data-stat="merged" onClick={onMerged}>
            <GitMergeIcon aria-hidden="true" className="size-(--size-icon)" />
            {stats.merged} merged → 정리
          </button>
        </Hint>
      ) : null}
    </div>
  );
}
