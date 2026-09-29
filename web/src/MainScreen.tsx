import { CircleDotIcon, FolderIcon, GitMergeIcon, GitPullRequestIcon, PlusIcon } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import type { Actions } from "./actions";
import { Button } from "./components/ui/button";
import { Kbd } from "./components/ui/kbd";
import { Tabs, TabsList, TabsTrigger } from "./components/ui/tabs";
import { Hint } from "./components/ui/tooltip";
import { cn } from "./lib/utils";
import { AGENT_GROUPS, boardProjects, mainSections, overviewScreen, type DeviceAvailability, type DeviceSection, type GroupCounts, type ProjectEntry } from "./navigation";
import { useNewIssueShortcut } from "./IssueDialogs";
import { AgentsLens, AgentsModeToggle, lensHandlers } from "./OverviewLenses";
import { agentsTile, buildLanes, buildLineages, scopeAgents } from "./overviewLens";
import { allProjectsStats, buildTasks, NO_FILTER, type AllProjectsStats, type IssueFilter, type SourceState, type TaskCard } from "./projectBoard";
import { frontDeviceId } from "./devices";
import { frontCheckout, type Device } from "./snapshot";
import { useShellStore } from "./store";
import { IssueFilterControl, TasksModeToggle } from "./TaskBoards";
import { IssuesView, panelCard, type IssuesPage } from "./IssuesView";
import { toggledFold, useUiStore, type LensFold, type MainView } from "./ui";
import { hostBridge, hostKind } from "./host";
import { displayCommand } from "./shortcuts";

// The Overview of one device's projects (PRD S6 D-02, B1-B4, B21; `screen.kind
// === "main"`, opened by the sidebar's Home row, PRD home-device-rail D-13): the
// device in front, or the one the screen names, and its header says which. Its facts line totals what every Project can
// give; its views are every Project's tasks on one board (every project has
// an issue source, so every project's issues are there), every agent as one
// inbox, and the registered Projects by device (PRD task-agents-views D-01,
// D-10, reworked issue-first on 2026-09-28); a
// Project opens its Overview (`ProjectOverview.tsx`), which also uses the
// facts line style and the opening and device notices below.
// Everything drawn is a value the snapshot carries; a device that cannot
// answer says why on its own section, with Retry where retrying can help.

const VIEWS: readonly { view: MainView; label: string }[] = [
  { view: "tasks", label: "Tasks" },
  { view: "agents", label: "Agents" },
  { view: "projects", label: "Projects" },
];

export function MainScreen({ actions }: { actions: Actions }) {
  const rest = useShellStore((s) => s.rest);
  const agents = useShellStore((s) => s.agents);
  const focusedPaneId = useShellStore((s) => s.focusedPaneId);
  const view = useUiStore((s) => s.mainView);
  const setView = useUiStore((s) => s.setMainView);
  const tasksMode = useUiStore((s) => s.tasksMode);
  const setTasksMode = useUiStore((s) => s.setTasksMode);
  const agentsMode = useUiStore((s) => s.agentsMode);
  const setAgentsMode = useUiStore((s) => s.setAgentsMode);
  const [doneOpen, setDoneOpen] = useState(false);
  const [folds, setFolds] = useState<readonly LensFold[]>([]);
  const [focusTask, setFocusTask] = useState<string | null>(null);
  // The Tasks view's issue panel and filter, this screen's own page state (PRD overview-lenses-issues).
  const [panel, setPanel] = useState<string | null>(null);
  const [filter, setFilter] = useState<IssueFilter>(NO_FILTER);
  // The device this Overview is of: the one its screen names, else the one in front.
  const named = useUiStore((s) => (s.screen?.kind === "main" ? s.screen.deviceId : undefined));
  // A device removed since the screen named it leaves the one in front.
  const deviceId = named !== undefined && rest?.navigator?.devices?.some((device) => device.id === named) ? named : frontDeviceId(rest);
  const deviceName = rest?.navigator?.devices?.find((device) => device.id === deviceId)?.label ?? deviceId;
  const sections = useMemo(() => mainSections(rest, agents, deviceId), [rest, agents, deviceId]);
  const stats = useMemo(() => allProjectsStats(sections.flatMap((section) => section.projects.map((project) => project.workspace))), [sections]);
  const projects = useMemo(() => boardProjects(rest, agents, deviceId), [rest, agents, deviceId]);
  const tasks = useMemo(() => buildTasks(projects, "all", Date.now()), [projects]);
  const lensAgents = useMemo(() => scopeAgents(projects), [projects]);
  const lanes = useMemo(() => buildLanes(projects, lensAgents, "all"), [projects, lensAgents]);
  const lineages = useMemo(() => buildLineages(lensAgents), [lensAgents]);
  // Every local Git project's tasks are read once the boards are on screen.
  const localGit = useMemo(() => projects.filter(({ workspace }) => workspace.is_git && !workspace.remote_target_id).map(({ workspace }) => workspace.id).join("\n"), [projects]);
  const boards = view !== "projects";
  useEffect(() => {
    if (!boards || !localGit) return;
    for (const id of localGit.split("\n")) actions.readProjectTasks(id);
  }, [actions, boards, localGit]);
  const total = stats.projects;
  const openCheckout = (card: TaskCard) => {
    const project = projects.find(({ workspace }) => workspace.id === card.place.projectId);
    if (project && card.checkout) actions.openWorkspace(project.workspace.device_id, card.checkout.workspace_id, card.checkout.id);
  };
  // A new issue goes to the Project in front, else the first one that has a
  // source; the dialog can move it to another.
  const issueProject = useMemo(() => {
    const front = frontCheckout(rest);
    const local = projects.filter(({ workspace }) => !workspace.remote_target_id && workspace.tasks?.source);
    return (front ? local.find(({ workspace }) => workspace.checkouts.some((checkout) => checkout.id === front.id)) : undefined)?.workspace.id ?? local[0]?.workspace.id ?? null;
  }, [rest, projects]);
  const newIssue = () => {
    if (issueProject) useUiStore.getState().setWorkspaceDialog({ kind: "new_issue", workspaceId: issueProject });
  };
  useNewIssueShortcut(issueProject ? newIssue : null);
  const page: IssuesPage = {
    openCheckout,
    startIssue: (card) => useUiStore.getState().setWorkspaceDialog({ kind: "start_issue", workspaceId: card.place.projectId, taskKey: card.task.key }),
    newIssue,
    showCheckouts: () => {
      setView("agents");
      setAgentsMode("checkouts");
      setPanel(null);
    },
  };
  // The Agents tab keeps the count of agents whose turn it is, the Agents tile's badge.
  const waiting = agentsTile(lensAgents, { state: "ready" }).badge?.count ?? 0;
  const lensActions = lensHandlers(actions, {
    openIssue: (_owner, task) => {
      setView("tasks");
      setFocusTask(task.key);
      setPanel(task.key);
    },
    toggleFold: (fold) => setFolds((open) => toggledFold(open, fold)),
  });
  // With the issue panel open the board and the panel scroll on their own (D-43).
  const scrolls = view !== "projects" && !(view === "tasks" && panelCard(tasks, panel) !== null);
  const unavailable = sections.filter((section) => section.availability.state !== "ready");
  return (
    <section className={cn("flex min-h-0 min-w-0 flex-1 flex-col bg-background", scrolls && "overflow-y-auto")} aria-label="Overview" data-main-screen="true" data-main-view={view}>
      <header className="flex shrink-0 flex-col gap-xs border-b border-border px-lg py-sm">
        <div className="flex min-w-0 items-center gap-lg">
          <h1 className="flex min-w-0 flex-1 items-baseline gap-sm text-headline font-semibold text-foreground">
            <span className="shrink-0">Home</span>
            <span className="min-w-0 truncate text-body font-normal text-muted-foreground" data-main-device-name={deviceId}>
              {deviceName}
            </span>
          </h1>
          {hostBridge() ? (
            <Button variant="ghost" onClick={() => actions.openAddProject()} data-main-add-project="true">
              <PlusIcon aria-hidden="true" />
              Add project <span className="text-muted-foreground">{displayCommand("new_workspace", hostKind())}</span>
            </Button>
          ) : null}
          {issueProject ? (
            <Button onClick={newIssue} data-main-new-issue="true">
              <PlusIcon aria-hidden="true" />
              새 이슈
              <Kbd>C</Kbd>
            </Button>
          ) : null}
        </div>
        <Facts stats={stats} source={tasks.source} />
      </header>
      <OpeningStatus actions={actions} />
      <div className="flex shrink-0 items-center justify-between gap-md px-lg py-sm">
        <Tabs
          value={view}
          onValueChange={(value) => {
            setView(value as MainView);
            setFocusTask(null);
            setPanel(null);
          }}
        >
          <TabsList aria-label="Overview view">
            {VIEWS.map((choice) => (
              <TabsTrigger key={choice.view} value={choice.view} data-main-tab={choice.view}>
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
        {view === "tasks" ? (
          <span className="flex items-center gap-xs" data-issues-controls="true">
            <IssueFilterControl filter={filter} onChange={setFilter} />
            <TasksModeToggle mode={tasksMode} onChange={setTasksMode} />
          </span>
        ) : view === "agents" ? <AgentsModeToggle mode={agentsMode} onChange={setAgentsMode} /> : null}
      </div>
      {boards ? (
        // A device that cannot answer keeps its last rows off these boards and says why here.
        unavailable.map((section) => (
          <div key={section.device.id} className="shrink-0 px-lg">
            <UnavailableNotice device={section.device} availability={section.availability} actions={actions} />
          </div>
        ))
      ) : null}
      {view === "tasks" ? (
        <IssuesView
          board={tasks}
          scope="all"
          mode={tasksMode}
          filter={filter}
          onFilterChange={setFilter}
          panel={panel}
          onPanel={setPanel}
          focusTask={focusTask}
          focusedPaneId={focusedPaneId}
          doneOpen={doneOpen}
          onToggleDone={() => setDoneOpen((open) => !open)}
          actions={actions}
          page={page}
        />
      ) : view === "agents" ? (
        <AgentsLens mode={agentsMode} lanes={lanes} lineages={lineages} scope="all" selectedLane={null} folds={folds} handlers={lensActions} now={Date.now()} />
      ) : total === 0 && sections.every((section) => section.availability.state === "ready") ? (
        <div className="flex flex-1 flex-col items-center justify-center gap-sm p-xl text-center text-caption text-muted-foreground" data-main-empty="true">
          <p>No project is registered yet.</p>
          {hostBridge() ? (
            <Button variant="secondary" onClick={() => actions.openAddProject()} data-main-empty-add="true">
              Add project
            </Button>
          ) : null}
        </div>
      ) : (
        <div className="flex min-h-0 flex-1 flex-col gap-lg overflow-auto p-md" data-main-projects="true">
          {sections.map((section) => (
            <DeviceProjects key={section.device.id} section={section} actions={actions} />
          ))}
        </div>
      )}
    </section>
  );
}

/** One fact of a scope's facts line, a glyph and its number. */
export const FACT = "inline-flex items-center gap-xxs";
export const FACTS_LINE = "flex flex-wrap items-center gap-md font-mono text-caption text-subtle-foreground";

/** `20 open issues · GitHub`, once the source has answered. */
function IssuesFact({ source }: { source: SourceState }) {
  if (source.openIssues === null) return null;
  return (
    <span className={FACT} data-stat="open-issues">
      <CircleDotIcon aria-hidden="true" className="size-(--size-icon)" />
      {source.openIssues} open {source.openIssues === 1 ? "issue" : "issues"}
      {source.label ? <span className="text-muted-foreground">· {source.label}</span> : null}
    </span>
  );
}

/** The Project count, and each total only once every Project gave its part (design #10). */
function Facts({ stats, source }: { stats: AllProjectsStats; source: SourceState }) {
  return (
    <div className={FACTS_LINE} data-main-stats="true">
      <span className={FACT} data-stat="projects">
        <FolderIcon aria-hidden="true" className="size-(--size-icon)" />
        {stats.projects} {stats.projects === 1 ? "project" : "projects"}
      </span>
      <IssuesFact source={source} />
      {stats.openPullRequests === null ? null : (
        <span className={FACT} data-stat="open-prs">
          <GitPullRequestIcon aria-hidden="true" className="size-(--size-icon)" />
          {stats.openPullRequests} open {stats.openPullRequests === 1 ? "PR" : "PRs"}
        </span>
      )}
      {stats.merged ? (
        <span className={cn(FACT, "text-pr-merged")} data-stat="merged">
          <GitMergeIcon aria-hidden="true" className="size-(--size-icon)" />
          {stats.merged} merged
        </span>
      ) : null}
    </div>
  );
}

function DeviceProjects({ section, actions }: { section: DeviceSection; actions: Actions }) {
  const { device, availability } = section;
  return (
    <section aria-label={`Projects on ${device.label}`} data-main-device={device.id} data-device-availability={availability.state}>
      <h2 className="flex items-center gap-sm pb-xs text-micro uppercase text-muted-foreground">
        <span>{device.label}</span>
        {availability.state === "loading" ? (
          <span role="status" className="normal-case" data-device-loading="true">
            {availability.text}
          </span>
        ) : null}
      </h2>
      <UnavailableNotice device={device} availability={availability} actions={actions} />
      {section.projects.length === 0 ? (
        <p className="px-sm py-xs text-caption text-muted-foreground">{availability.state === "ready" ? "No projects on this device." : "Its projects show once it answers."}</p>
      ) : (
        <ul className="flex flex-col" role="list">
          {section.projects.map((project) => (
            <ProjectRow key={project.id} project={project} actions={actions} />
          ))}
        </ul>
      )}
    </section>
  );
}

/**
 * A Workspace or agent asked for from here that is not in front yet: a quiet
 * line while it is on its way, and the refusal with Dismiss when it was not
 * brought forward, so the operator stays where they were (B2, B21).
 */
export function OpeningStatus({ actions }: { actions: Actions }) {
  const opening = useUiStore((s) => s.opening);
  if (!opening) return null;
  if (!opening.failure) {
    return (
      <p role="status" className="shrink-0 px-md pt-sm text-caption text-muted-foreground" data-opening="pending">
        Opening…
      </p>
    );
  }
  return (
    <div role="alert" className="mx-md mt-sm flex shrink-0 items-center gap-sm rounded-sm bg-card px-sm py-xs text-caption text-warning" data-opening="failed">
      <span className="min-w-0 flex-1 break-words">Not opened: {opening.failure}</span>
      <Button variant="secondary" onClick={() => actions.dismissOpening()} data-opening-dismiss="true">
        Dismiss
      </Button>
    </div>
  );
}

/** Why a device cannot answer, with Retry where retrying can help (B3). */
export function UnavailableNotice({ device, availability, actions }: { device: Device; availability: DeviceAvailability; actions: Actions }) {
  if (availability.state !== "unavailable") return null;
  const retry = availability.retry;
  return (
    <div role="status" className="mb-xs flex items-center gap-sm rounded-sm bg-card px-sm py-xs text-caption text-warning" data-device-unavailable={device.id}>
      <span className="min-w-0 flex-1 break-words">{availability.text}</span>
      {retry ? (
        <Button variant="secondary" onClick={() => (retry === "helper" ? actions.retryDeviceHost(device.id) : actions.retryDevice(device.id))} data-device-retry={device.id}>
          Retry
        </Button>
      ) : null}
    </div>
  );
}

function ProjectRow({ project }: { project: ProjectEntry; actions: Actions }) {
  const setScreen = useUiStore((s) => s.setScreen);
  const reachable = project.workspace !== null;
  return (
    <li>
      <Hint label={reachable ? `${project.label} · ${project.path}` : `${project.label} · ${project.path} · its device has not answered`}>
      <button
        type="button"
        disabled={!reachable}
        data-main-project={project.id}
        className="flex w-full items-center gap-md rounded-sm px-sm py-xs text-left outline-none hover:bg-accent focus-visible:bg-accent disabled:cursor-default disabled:hover:bg-transparent"
        onClick={() => setScreen(overviewScreen(useShellStore.getState().rest, project.id))}
      >
        <span className="flex min-w-0 flex-1 flex-col">
          <span className="flex items-baseline gap-xs text-body text-foreground">
            <span className="min-w-0 truncate">{project.label}</span>
            {project.pinned ? <span className="text-micro uppercase text-muted-foreground">pinned</span> : null}
          </span>
          <span className="truncate text-caption text-muted-foreground">{project.path}</span>
        </span>
        <span className="shrink-0 text-caption text-subtle-foreground" data-workspace-count={project.workspaceCount ?? "unknown"}>
          {project.workspaceCount === null ? "…" : `${project.workspaceCount} ${project.workspaceCount === 1 ? "workspace" : "workspaces"}`}
        </span>
        <Counts counts={project.counts} />
      </button>
      </Hint>
    </li>
  );
}

/** One small cell per non-empty group, in the canonical order; unknown counts read `…`, never zero. */
function Counts({ counts }: { counts: GroupCounts | null }) {
  if (!counts) return <span className="w-[var(--size-recent-location-max)] shrink-0 text-right text-caption text-muted-foreground" data-agent-counts="unknown">…</span>;
  const shown = AGENT_GROUPS.filter(({ group }) => counts[group] > 0);
  return (
    <span className="flex w-[var(--size-recent-location-max)] shrink-0 justify-end gap-sm text-caption" data-agent-counts={shown.map(({ group }) => `${group}:${counts[group]}`).join(" ")}>
      {shown.length === 0 ? <span className="text-muted-foreground">No agents</span> : null}
      {shown.map(({ group, label }) => (
        <Hint key={group} label={`${counts[group]} ${label}`} reveals>
        <span className={group === "needs_you" ? "text-warning" : group === "done" ? "text-success" : group === "working" ? "text-agent-working" : "text-muted-foreground"}>
          {counts[group]} {label}
        </span>
        </Hint>
      ))}
    </span>
  );
}
