import { ArrowDownIcon, FolderGit2Icon, GitMergeIcon, HardDriveIcon, RefreshCwIcon } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import type { Actions } from "./actions";
import { useInterfaceTranslation } from "./i18n/client";
import { requireInterfaceLanguage } from "./i18n/locale";
import { Button } from "./components/ui/button";
import { Hint } from "./components/ui/tooltip";
import { cn } from "./lib/utils";
import { DiskFact, LowFreeFact, openDiskCleanup } from "./DiskEntrance";
import { FACT, FACTS_LINE, OpeningStatus, UnavailableNotice } from "./MainScreen";
import { overviewProject } from "./navigation";
import { useNewIssueShortcut } from "./IssueDialogs";
import { AgentGraph, GraphFilterControls } from "./GraphView";
import { foldId } from "./agentGraph";
import { OverviewTitleRow } from "./OverviewTitleRow";
import { LensTabs, lensHandlers } from "./OverviewLenses";
import { agentsTile, issuesTile, lastIssueRead, prsTile, scopeAgents, sessionsTile } from "./overviewLens";
import { RequestView } from "./RequestView";
import { requestRows, requestsTile } from "./requestList";
import { boardLabels, buildPullRequests, buildTasks, projectStats, type BoardProject, type BoardStats, type TaskCard } from "./projectBoard";
import { PullRequestsView } from "./PullRequestsView";
import { IssuesView, panelCard, type IssuesPage } from "./IssuesView";
import type { Workspace } from "./snapshot";
import { useShellStore } from "./store";
import { ProjectSessions } from "./ProjectSessions";
import { IssueFilterControl, TasksModeToggle } from "./TaskBoards";
import { toggledFold, useUiStore, type OverviewLens } from "./ui";

// A Project's Overview (PRD web-project-overview, task-agents-views, the
// issue-first rework and overview-lenses-tiles-agents): the Project scope the
// shared Overview's scope tab opens. Under its title sits repository
// facts, then one row of lens tabs, Agents, Requests, Issues, PRs and
// Sessions, with the chosen tab's controls at its right end, as All projects'
// tab row has them. Every way in opens the Agents graph with the box in
// front selected; the lens rides
// on the screen, so only Recent Panels brings back one as it was left
// (`OverviewLens`). Issues is the issue-first Tasks board under its new name;
// the boards are `TaskBoards.tsx`'s and the lenses `OverviewLenses.tsx`'s.

export function ProjectOverview({ projectId, lens, actions }: { projectId: string; lens: OverviewLens; actions: Actions }) {
  const { t, i18n } = useInterfaceTranslation();
  const rest = useShellStore((s) => s.rest);
  const agents = useShellStore((s) => s.agents);
  const focusedPaneId = useShellStore((s) => s.focusedPaneId);
  const sessions = useShellStore((s) => s.projectSessions);
  const setProject = useUiStore((s) => s.setOverviewProject);
  const setLens = useUiStore((s) => s.setLens);
  const onRequestLens = useCallback((requests: Partial<OverviewLens["requests"]>) => setLens({ requests: { ...lens.requests, ...requests } }), [setLens, lens.requests]);
  const lensActions = useMemo(() => lensHandlers(actions, {
    openIssue: (_owner, task) => setLens({ tab: "issues", focusTask: task.key, panel: task.key }),
    toggleFold: (fold) => setLens({ folds: toggledFold(lens.folds, fold) }),
  }), [actions, setLens, lens.folds]);
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
  const rows = useMemo(() => requestRows(lensAgents, deviceAgents ?? []), [lensAgents, deviceAgents]);
  const stats = useMemo(() => (workspace ? projectStats(workspace) : null), [workspace]);
  const pullRequests = useMemo(() => (projects[0] ? buildPullRequests(projects[0], Date.now()) : null), [projects]);
  const tiles = useMemo(
    () =>
      tasks && workspace && found && pullRequests
        ? [agentsTile(lensAgents, found.availability, t), requestsTile(rows, found.availability, t), issuesTile(tasks, Date.now(), lastIssueRead(workspace), requireInterfaceLanguage(i18n.language), t), prsTile(pullRequests, t), sessionsTile(sessions, workspace.id, Date.now(), t)]
        : [],
    [tasks, workspace, found, rows, lensAgents, sessions, pullRequests, t, i18n.language],
  );
  // A local Git project's Git facts and pull requests are read whenever this
  // screen opens. The previous answer stays visible during the worker reads.
  const local = workspace !== null && workspace.is_git === true && !workspace.remote_target_id;
  const localId = local ? workspace.id : null;
  useEffect(() => {
    if (!localId) return;
    actions.measureProjectDisk(localId);
    actions.refreshProjectOverview(localId);
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
  if (!found || !tasks || !stats || !pullRequests) {
    return (
      <section className="flex flex-1 flex-col items-center justify-center gap-sm p-xl text-caption text-muted-foreground" data-overview-missing={projectId}>
        <p>{t("overview.projectMissing")}</p>
        <Button variant="secondary" onClick={() => setProject(null)}>{t("overview.allProjects")}</Button>
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
  // `N merged → Clean up` opens the disk cleanup sheet on what is finished (PRD disk-layers B3).
  const showCleanup = () => openDiskCleanup(project.id, "done");
  const page: IssuesPage = {
    openCheckout,
    startIssue: (card) => useUiStore.getState().setWorkspaceDialog({ kind: "start_issue", workspaceId: card.place.projectId, taskKey: card.task.key }),
    newIssue,
    // `N worktrees without an issue` opens the graph with its `Worktrees without agents` line unfolded (agents-graph-view B23).
    showCheckouts: () => setLens({ tab: "agents", panel: null, folds: lens.folds.includes(foldId("empty", project.id)) ? lens.folds : [...lens.folds, foldId("empty", project.id)] }),
  };
  // An issue chip opens the Issues tile at its card with the issue's panel open.
  const view = lens.tab;
  // With the issue panel open the board and the panel scroll on their own, under a header that stays (D-43).
  const split = view === "issues" && panelCard(tasks, lens.panel) !== null;
  const state =
    view === "issues" ? (tasks.cards.length === 0 ? "empty" : "board") : view === "agents" || view === "requests" ? (lensAgents.length === 0 ? "empty" : "board") : view === "prs" ? (pullRequests.groups.length === 0 ? "empty" : "board") : "sessions";
  const sessionsView = view === "sessions";
  return (
    <section
      className={cn("flex min-h-0 min-w-0 flex-1 flex-col bg-background", !sessionsView && !split && "overflow-y-auto")}
      aria-label={t("overview.projectName", { project: project.label })}
      data-overview-screen={project.id}
      data-overview-state={state}
      data-overview-view={view}
    >
      <header className="flex shrink-0 flex-col gap-xs border-b border-border px-lg py-sm">
        <OverviewTitleRow
          name={project.label}
          path={project.path}
          remoteDevice={device && device.kind === "remote" ? device.label : null}
          onAllProjects={() => setProject(null)}
          newAgent={{ run: newAgent, disabled: !project.is_git && project.checkouts.length === 0 }}
          newIssue={canIssue ? newIssue : null}
        />
        <Stats workspace={project} stats={stats} refreshing={!!rest?.git_worktrees_loading || project.checkouts.some((checkout) => checkout.github?.loading)} onMerged={showCleanup} />
      </header>
      <OpeningStatus actions={actions} />
      <div className="flex shrink-0 flex-wrap items-center justify-between gap-md px-lg py-sm" data-overview-view-row="true">
        <LensTabs tiles={tiles} selected={view} onSelect={(tab) => setLens({ tab, focusTask: null, panel: null, prs: { ...lens.prs, focus: null } })} />
        {view === "agents" ? (
          <GraphFilterControls agents={lensAgents} filter={lens.graph} onChange={(graph) => setLens({ graph })} />
        ) : view === "issues" ? (
          <span className="flex items-center gap-xs" data-issues-controls="true">
            <IssueFilterControl filter={lens.filter} labels={tasks ? boardLabels(tasks, lens.filter.labels) : []} onChange={(filter) => setLens({ filter })} />
            <TasksModeToggle mode={lens.tasksMode} onChange={(tasksMode) => setLens({ tasksMode })} />
          </span>
        ) : null}
      </div>
      {device ? <UnavailableNotice device={device} availability={availability} actions={actions} /> : null}
      {availability.state === "loading" ? (
        <p role="status" className="shrink-0 px-lg pt-sm text-caption text-muted-foreground" data-device-loading="true">
          {availability.text}
        </p>
      ) : null}
      {view === "sessions" ? (
        <div className="flex min-h-0 flex-1 border-t border-border">
          {/* Keyed by the Project, so another Project starts with its own filters and asks for itself. */}
          <ProjectSessions key={`${project.device_id}:${project.id}`} workspace={project} actions={actions} />
        </div>
      ) : view === "prs" ? (
        <PullRequestsView board={pullRequests} project={project} lens={lens.prs} onLens={(prs) => setLens({ prs: { ...lens.prs, ...prs } })} handlers={lensActions} now={now} />
      ) : view === "requests" ? (
        availability.state === "ready" ? (
          <RequestView rows={rows} scope="project" lens={lens.requests} onLens={onRequestLens} handlers={lensActions} actions={actions} onNewAgent={rows.length === 0 ? newAgent : undefined} />
        ) : (
          <div className="flex-1" data-requests-unavailable={availability.state} />
        )
      ) : view === "agents" ? (
        // A device that does not answer shows its reason above and no graph: the last picture is not left standing (B32).
        availability.state === "ready" ? (
          <AgentGraph projects={projects} agents={lensAgents} scope="project" selectedBox={lens.box} filter={lens.graph} onFilter={(graph) => setLens({ graph })} folds={lens.folds} handlers={lensActions} now={now} />
        ) : (
          <div className="flex-1" data-graph-unavailable={availability.state} />
        )
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
 * are any. The size opens the disk cleanup sheet with the layers on hover, a
 * warning cell joins it while the volume is short of space, and `N merged`
 * opens the same sheet on what is finished. The issue and pull request counts
 * are the tiles'.
 */
function Stats({ workspace, stats, refreshing, onMerged }: { workspace: Workspace; stats: BoardStats; refreshing: boolean; onMerged: () => void }) {
  const { t } = useInterfaceTranslation();
  if (!workspace.is_git) return <span />;
  return (
    <div className={FACTS_LINE} data-overview-stats="true">
      <span className={cn("inline-flex size-(--size-icon) items-center justify-center text-muted-foreground", !refreshing && "invisible")} role={refreshing ? "status" : undefined} aria-label={refreshing ? t("overview.refreshingGit") : undefined} data-overview-refreshing={refreshing ? "true" : undefined}>
        <RefreshCwIcon aria-hidden="true" className={cn("size-(--size-icon-sm)", refreshing && "animate-spin")} />
      </span>
      <span className={FACT} data-stat="worktrees">
        <FolderGit2Icon aria-hidden="true" className="size-(--size-icon)" />
        {t("overview.worktrees", { count: stats.worktrees })}
      </span>
      {stats.disk === "measuring" ? (
        <Hint label={t("overview.measuringDisk")}>
          <span className={cn(FACT, "text-muted-foreground")} data-stat="disk" data-disk-measuring="true">
            <HardDriveIcon aria-hidden="true" className="size-(--size-icon)" />… GB
          </span>
        </Hint>
      ) : stats.disk === null ? null : (
        <DiskFact workspace={workspace} bytes={stats.disk} />
      )}
      <LowFreeFact workspace={workspace} />
      {stats.behind ? (
        <span className={cn(FACT, "text-warning")} data-stat="behind">
          <ArrowDownIcon aria-hidden="true" className="size-(--size-icon)" />
          {t("overview.behindOrigin", { branch: stats.behind.branch, count: stats.behind.count })}
        </span>
      ) : null}
      {stats.merged > 0 ? (
        <Hint label={t("overview.showMergedCleanup")}>
          <button type="button" className={cn(FACT, "rounded-xs text-pr-merged outline-none hover:underline focus-visible:ring-1 focus-visible:ring-ring")} data-stat="merged" onClick={onMerged}>
            <GitMergeIcon aria-hidden="true" className="size-(--size-icon)" />
            {t("overview.mergedCleanup", { count: stats.merged })}
          </button>
        </Hint>
      ) : null}
    </div>
  );
}
