import { FolderIcon, GitMergeIcon, GitPullRequestIcon, PlusIcon } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import type { Actions } from "./actions";
import { Button } from "./components/ui/button";
import { Tabs, TabsList, TabsTrigger } from "./components/ui/tabs";
import { Hint } from "./components/ui/tooltip";
import { cn } from "./lib/utils";
import { AGENT_GROUPS, boardProjects, mainSections, type DeviceAvailability, type DeviceSection, type GroupCounts, type ProjectEntry } from "./navigation";
import { allProjectsStats, buildAgents, buildTasks, buildWaiting, type AllProjectsStats, type TaskCard } from "./projectBoard";
import type { Device } from "./snapshot";
import { useShellStore } from "./store";
import { AgentsView, DependenciesView, TasksModeToggle, TasksView } from "./TaskBoards";
import { WaitingBand } from "./WaitingBand";
import { scopeView, useUiStore, type ProjectView } from "./ui";
import { hostKind } from "./host";
import { displayCommand } from "./shortcuts";

// All projects (PRD S6 D-02, B1-B4, B21; `screen.kind === "main"`), the scope
// the sidebar's top row opens. Its facts line totals what every Project can
// give, and the waiting band lists every agent that waits on the operator;
// its views are every Project's tasks on one board, every agent, and
// the registered Projects by device (PRD task-agents-views D-01, D-10); a
// Project opens its Overview (`ProjectOverview.tsx`), which also uses the
// facts line style and the opening and device notices below.
// Everything drawn is a value the snapshot carries; a device that cannot
// answer says why on its own section, with Retry where retrying can help.

const VIEWS: readonly { view: ProjectView; label: string }[] = [
  { view: "tasks", label: "Tasks" },
  { view: "agents", label: "Agents" },
  { view: "projects", label: "Projects" },
];
const VIEW_IDS = VIEWS.map((row) => row.view);

export function MainScreen({ actions }: { actions: Actions }) {
  const rest = useShellStore((s) => s.rest);
  const agents = useShellStore((s) => s.agents);
  const focusedPaneId = useShellStore((s) => s.focusedPaneId);
  const view = scopeView(useUiStore((s) => s.projectView), VIEW_IDS);
  const setView = useUiStore((s) => s.setProjectView);
  const tasksMode = useUiStore((s) => s.tasksMode);
  const [doneOpen, setDoneOpen] = useState(false);
  const sections = useMemo(() => mainSections(rest, agents), [rest, agents]);
  const stats = useMemo(() => allProjectsStats(sections.flatMap((section) => section.projects.map((project) => project.workspace))), [sections]);
  const projects = useMemo(() => boardProjects(rest, agents), [rest, agents]);
  const tasks = useMemo(() => buildTasks(projects, "all", Date.now()), [projects]);
  const agentBoard = useMemo(() => buildAgents(projects, "all"), [projects]);
  const waiting = useMemo(() => buildWaiting(projects, "all"), [projects]);
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
  const unavailable = sections.filter((section) => section.availability.state !== "ready");
  return (
    <section className="flex min-h-0 min-w-0 flex-1 flex-col bg-background" aria-label="All projects" data-main-screen="true" data-main-view={view}>
      <header className="flex shrink-0 flex-col gap-xs border-b border-border px-lg py-sm">
        <div className="flex min-w-0 items-center gap-lg">
          <h1 className="min-w-0 flex-1 truncate text-headline font-semibold text-foreground">All projects</h1>
          <Button variant="ghost" onClick={() => actions.openNewWorkspace()} data-main-add-project="true">
            <PlusIcon aria-hidden="true" />
            Add project <span className="text-muted-foreground">{displayCommand("new_workspace", hostKind())}</span>
          </Button>
        </div>
        <Facts stats={stats} />
        <WaitingBand rows={waiting} onOpen={actions.openAgent} />
      </header>
      <OpeningStatus actions={actions} />
      <div className="flex shrink-0 items-center justify-between gap-md px-lg py-sm">
        <Tabs value={view} onValueChange={(value) => setView(value as ProjectView)}>
          <TabsList aria-label="All projects view">
            {VIEWS.map((choice) => (
              <TabsTrigger key={choice.view} value={choice.view} data-main-tab={choice.view}>
                {choice.label}
              </TabsTrigger>
            ))}
          </TabsList>
        </Tabs>
        {view === "tasks" ? <TasksModeToggle /> : null}
      </div>
      {boards ? (
        // A device that cannot answer keeps its last rows off these boards and says why here.
        unavailable.map((section) => (
          <div key={section.device.id} className="shrink-0 px-lg">
            <UnavailableNotice device={section.device} availability={section.availability} actions={actions} />
          </div>
        ))
      ) : null}
      {view === "tasks" && tasksMode === "dependencies" ? (
        <DependenciesView board={tasks} scope="all" focusedPaneId={focusedPaneId} actions={actions} openCheckout={openCheckout} />
      ) : view === "tasks" ? (
        <TasksView board={tasks} focusedPaneId={focusedPaneId} actions={actions} openCheckout={openCheckout} doneOpen={doneOpen} onToggleDone={() => setDoneOpen((open) => !open)} />
      ) : view === "agents" ? (
        <AgentsView board={agentBoard} focusedPaneId={focusedPaneId} actions={actions} />
      ) : total === 0 && sections.every((section) => section.availability.state === "ready") ? (
        <div className="flex flex-1 flex-col items-center justify-center gap-sm p-xl text-center text-caption text-muted-foreground" data-main-empty="true">
          <p>No project is registered yet.</p>
          <Button variant="secondary" onClick={() => actions.openNewWorkspace()} data-main-empty-add="true">
            Add project
          </Button>
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

/** The Project count, and each total only once every Project gave its part (design #10). */
function Facts({ stats }: { stats: AllProjectsStats }) {
  return (
    <div className={FACTS_LINE} data-main-stats="true">
      <span className={FACT} data-stat="projects">
        <FolderIcon aria-hidden="true" className="size-(--size-icon)" />
        {stats.projects} {stats.projects === 1 ? "project" : "projects"}
      </span>
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
        onClick={() => setScreen({ kind: "overview", projectId: project.id })}
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
