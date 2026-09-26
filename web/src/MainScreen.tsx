import { FolderIcon, GitMergeIcon, GitPullRequestIcon, PlusIcon } from "lucide-react";
import { useMemo } from "react";
import type { Actions } from "./actions";
import { Button } from "./components/ui/button";
import { Hint } from "./components/ui/tooltip";
import { cn } from "./lib/utils";
import { AGENT_GROUPS, mainSections, type DeviceAvailability, type DeviceSection, type GroupCounts, type ProjectEntry } from "./navigation";
import { allProjectsStats, type AllProjectsStats } from "./projectBoard";
import type { Device } from "./snapshot";
import { useShellStore } from "./store";
import { useUiStore } from "./ui";
import { hostKind } from "./host";
import { displayCommand } from "./shortcuts";

// All projects (PRD S6 D-02, B1-B4, B21; `screen.kind === "main"`), the scope
// the sidebar's top row opens. Its facts line totals what every Project can
// give, and it lists every registered Project by device; a Project opens its
// Overview (`ProjectOverview.tsx`), which also uses the facts line style and
// the opening and device notices below.
// Everything drawn is a value the snapshot carries; a device that cannot
// answer says why on its own section, with Retry where retrying can help.

export function MainScreen({ actions }: { actions: Actions }) {
  const rest = useShellStore((s) => s.rest);
  const agents = useShellStore((s) => s.agents);
  const sections = useMemo(() => mainSections(rest, agents), [rest, agents]);
  const stats = useMemo(() => allProjectsStats(sections.flatMap((section) => section.projects.map((project) => project.workspace))), [sections]);
  const total = stats.projects;
  return (
    <section className="flex min-h-0 min-w-0 flex-1 flex-col overflow-auto bg-background" aria-label="All projects" data-main-screen="true">
      <header className="flex shrink-0 flex-col gap-xs border-b border-border px-lg py-sm">
        <div className="flex min-w-0 items-center gap-lg">
          <h1 className="min-w-0 flex-1 truncate text-headline font-semibold text-foreground">All projects</h1>
          <Button variant="ghost" onClick={() => actions.openNewWorkspace()} data-main-add-project="true">
            <PlusIcon aria-hidden="true" />
            Add project <span className="text-muted-foreground">{displayCommand("new_workspace", hostKind())}</span>
          </Button>
        </div>
        <Facts stats={stats} />
      </header>
      <OpeningStatus actions={actions} />
      {total === 0 && sections.every((section) => section.availability.state === "ready") ? (
        <div className="flex flex-1 flex-col items-center justify-center gap-sm p-xl text-center text-caption text-muted-foreground" data-main-empty="true">
          <p>No project is registered yet.</p>
          <Button variant="secondary" onClick={() => actions.openNewWorkspace()} data-main-empty-add="true">
            Add project
          </Button>
        </div>
      ) : (
        <div className="flex flex-col gap-lg p-md">
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
