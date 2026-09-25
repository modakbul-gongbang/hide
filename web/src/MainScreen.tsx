import { useMemo } from "react";
import type { Actions } from "./actions";
import { AgentMark } from "./AgentMark";
import { Button } from "./components/ui/controls";
import { AGENT_GROUPS, agentSections, checkoutAgents, groupCounts, mainSections, overviewProject, type DeviceAvailability, type DeviceSection, type GroupCounts, type ProjectEntry } from "./navigation";
import { pullRequestBadge } from "./projects";
import type { AgentRow, Checkout, Device, Workspace } from "./snapshot";
import { useShellStore } from "./store";
import { useUiStore } from "./ui";

// Main and Project Overview (PRD S6 D-02, B1-B4, B21). Main lists every
// registered Project by device; a Project opens its Overview, which lists its
// Workspaces and the agents running in it, and either opens the Workspace.
// Everything drawn is a value the snapshot carries; a device that cannot
// answer says why on its own section, with Retry where retrying can help.

export function MainScreen({ actions }: { actions: Actions }) {
  const rest = useShellStore((s) => s.rest);
  const agents = useShellStore((s) => s.agents);
  const sections = useMemo(() => mainSections(rest, agents), [rest, agents]);
  const total = sections.reduce((sum, section) => sum + section.projects.length, 0);
  return (
    <section className="flex min-h-0 min-w-0 flex-1 flex-col overflow-auto bg-background" aria-label="Main" data-main-screen="true">
      <header className="flex h-[var(--size-tab-strip)] shrink-0 items-center gap-sm border-b border-divider bg-sidebar px-md">
        <h1 className="flex-1 text-subhead font-semibold text-primary">Projects</h1>
        <Button appearance="quiet" onClick={() => actions.openNewWorkspace()} data-main-add-project="true">
          Add project <span className="text-muted">⌥⇧N</span>
        </Button>
      </header>
      <OpeningStatus actions={actions} />
      {total === 0 && sections.every((section) => section.availability.state === "ready") ? (
        <div className="flex flex-1 flex-col items-center justify-center gap-sm p-xl text-center text-caption text-muted" data-main-empty="true">
          <p>No project is registered yet.</p>
          <Button onClick={() => actions.openNewWorkspace()} data-main-empty-add="true">
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

function DeviceProjects({ section, actions }: { section: DeviceSection; actions: Actions }) {
  const { device, availability } = section;
  return (
    <section aria-label={`Projects on ${device.label}`} data-main-device={device.id} data-device-availability={availability.state}>
      <h2 className="flex items-center gap-sm pb-xs text-micro uppercase text-muted">
        <span>{device.label}</span>
        {availability.state === "loading" ? (
          <span role="status" className="normal-case" data-device-loading="true">
            {availability.text}
          </span>
        ) : null}
      </h2>
      <UnavailableNotice device={device} availability={availability} actions={actions} />
      {section.projects.length === 0 ? (
        <p className="px-sm py-xs text-caption text-muted">{availability.state === "ready" ? "No projects on this device." : "Its projects show once it answers."}</p>
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
function OpeningStatus({ actions }: { actions: Actions }) {
  const opening = useUiStore((s) => s.opening);
  if (!opening) return null;
  if (!opening.failure) {
    return (
      <p role="status" className="shrink-0 px-md pt-sm text-caption text-muted" data-opening="pending">
        Opening…
      </p>
    );
  }
  return (
    <div role="alert" className="mx-md mt-sm flex shrink-0 items-center gap-sm rounded-sm bg-panel px-sm py-xs text-caption text-warning" data-opening="failed">
      <span className="min-w-0 flex-1 break-words">Not opened: {opening.failure}</span>
      <Button onClick={() => actions.dismissOpening()} data-opening-dismiss="true">
        Dismiss
      </Button>
    </div>
  );
}

/** Why a device cannot answer, with Retry where retrying can help (B3). */
function UnavailableNotice({ device, availability, actions }: { device: Device; availability: DeviceAvailability; actions: Actions }) {
  if (availability.state !== "unavailable") return null;
  const retry = availability.retry;
  return (
    <div role="status" className="mb-xs flex items-center gap-sm rounded-sm bg-panel px-sm py-xs text-caption text-warning" data-device-unavailable={device.id}>
      <span className="min-w-0 flex-1 break-words">{availability.text}</span>
      {retry ? (
        <Button onClick={() => (retry === "helper" ? actions.retryDeviceHost(device.id) : actions.retryDevice(device.id))} data-device-retry={device.id}>
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
      <button
        type="button"
        disabled={!reachable}
        title={reachable ? `${project.label} · ${project.path}` : `${project.label} · ${project.path} · its device has not answered`}
        data-main-project={project.id}
        className="flex w-full items-center gap-md rounded-sm px-sm py-xs text-left outline-none hover:bg-elevated focus-visible:bg-elevated disabled:cursor-default disabled:hover:bg-transparent"
        onClick={() => setScreen({ kind: "overview", projectId: project.id })}
      >
        <span className="flex min-w-0 flex-1 flex-col">
          <span className="flex items-baseline gap-xs text-body text-primary">
            <span className="min-w-0 truncate">{project.label}</span>
            {project.pinned ? <span className="text-micro uppercase text-muted">pinned</span> : null}
          </span>
          <span className="truncate text-caption text-muted">{project.path}</span>
        </span>
        <span className="shrink-0 text-caption text-secondary" data-workspace-count={project.workspaceCount ?? "unknown"}>
          {project.workspaceCount === null ? "…" : `${project.workspaceCount} ${project.workspaceCount === 1 ? "workspace" : "workspaces"}`}
        </span>
        <Counts counts={project.counts} />
      </button>
    </li>
  );
}

/** One small cell per non-empty group, in the canonical order; unknown counts read `…`, never zero. */
function Counts({ counts }: { counts: GroupCounts | null }) {
  if (!counts) return <span className="w-[var(--size-recent-location-max)] shrink-0 text-right text-caption text-muted" data-agent-counts="unknown">…</span>;
  const shown = AGENT_GROUPS.filter(({ group }) => counts[group] > 0);
  return (
    <span className="flex w-[var(--size-recent-location-max)] shrink-0 justify-end gap-sm text-caption" data-agent-counts={shown.map(({ group }) => `${group}:${counts[group]}`).join(" ")}>
      {shown.length === 0 ? <span className="text-muted">No agents</span> : null}
      {shown.map(({ group, label }) => (
        <span key={group} className={group === "needs_you" ? "text-warning" : group === "done" ? "text-success" : group === "working" ? "text-agent-working" : "text-muted"} title={`${counts[group]} ${label}`}>
          {counts[group]} {label}
        </span>
      ))}
    </span>
  );
}

export function OverviewScreen({ projectId, actions }: { projectId: string; actions: Actions }) {
  const rest = useShellStore((s) => s.rest);
  const agents = useShellStore((s) => s.agents);
  const setScreen = useUiStore((s) => s.setScreen);
  const found = useMemo(() => overviewProject(rest, agents, projectId), [rest, agents, projectId]);
  if (!found) {
    return (
      <section className="flex flex-1 flex-col items-center justify-center gap-sm p-xl text-caption text-muted" data-overview-missing={projectId}>
        <p>This project is no longer in the catalog.</p>
        <Button onClick={() => setScreen({ kind: "main" })}>Back to Main</Button>
      </section>
    );
  }
  const { workspace, device, availability } = found;
  return (
    <section className="flex min-h-0 min-w-0 flex-1 flex-col overflow-auto bg-background" aria-label={`Project ${workspace.label}`} data-overview-screen={workspace.id}>
      <header className="flex h-[var(--size-tab-strip)] shrink-0 items-center gap-xs border-b border-divider bg-sidebar px-sm text-caption">
        <button type="button" className="rounded-xs px-xs text-secondary hover:bg-elevated hover:text-primary focus-visible:bg-elevated" data-go-main="true" onClick={() => setScreen({ kind: "main" })}>
          Main
        </button>
        <span aria-hidden="true" className="text-muted">/</span>
        <h1 className="min-w-0 truncate text-subhead font-semibold text-primary" title={workspace.path} aria-current="page">
          {workspace.label}
        </h1>
        {device ? <span className="shrink-0 rounded-xs bg-elevated px-xs text-micro text-secondary">{device.label}</span> : null}
        <span className="flex-1" />
        {workspace.is_git ? (
          <Button appearance="quiet" onClick={() => useUiStore.getState().setWorkspaceDialog({ kind: "new_worktree", workspaceId: workspace.id })} data-overview-new-worktree="true">
            New worktree
          </Button>
        ) : null}
      </header>
      <OpeningStatus actions={actions} />
      <div className="flex flex-col gap-lg p-md">
        {device ? <UnavailableNotice device={device} availability={availability} actions={actions} /> : null}
        {availability.state === "loading" ? (
          <p role="status" className="px-sm text-caption text-muted" data-device-loading="true">
            {availability.text}
          </p>
        ) : null}
        <WorkspaceList workspace={workspace} agents={found.agents} actions={actions} />
        <ProjectAgents agents={found.agents} workspace={workspace} actions={actions} />
      </div>
    </section>
  );
}

function WorkspaceList({ workspace, agents, actions }: { workspace: Workspace; agents: AgentRow[] | null; actions: Actions }) {
  return (
    <section aria-label="Workspaces" data-overview-workspaces="true">
      <h2 className="pb-xs text-micro uppercase text-muted">Workspaces · {workspace.checkouts.length}</h2>
      {workspace.checkouts.length === 0 ? (
        <div className="flex items-center gap-sm px-sm py-xs text-caption text-muted" data-overview-no-workspaces="true">
          <span>No Workspace in this project yet.</span>
          {workspace.is_git ? (
            <Button onClick={() => useUiStore.getState().setWorkspaceDialog({ kind: "new_worktree", workspaceId: workspace.id })}>New worktree</Button>
          ) : null}
        </div>
      ) : (
        <ul role="list">
          {workspace.checkouts.map((checkout) => (
            <WorkspaceRow key={checkout.id} workspace={workspace} checkout={checkout} counts={agents ? groupCounts(checkoutAgents(checkout, agents)) : null} actions={actions} />
          ))}
        </ul>
      )}
    </section>
  );
}

function WorkspaceRow({ workspace, checkout, counts, actions }: { workspace: Workspace; checkout: Checkout; counts: GroupCounts | null; actions: Actions }) {
  const name = checkout.branch ?? checkout.label;
  const badge = checkout.pull_request ? pullRequestBadge(checkout.pull_request) : null;
  return (
    <li>
      <button
        type="button"
        title={`${name} · ${checkout.path}${checkout.purpose?.text ? ` · ${checkout.purpose.text}` : ""}`}
        data-overview-workspace={checkout.id}
        className="flex w-full items-center gap-md rounded-sm px-sm py-xs text-left outline-none hover:bg-elevated focus-visible:bg-elevated"
        onClick={() => actions.openWorkspace(workspace.device_id, workspace.id, checkout.id)}
      >
        <span className="w-[var(--size-checkout-icon)] shrink-0 font-mono text-caption text-muted" aria-hidden="true">
          {checkout.is_worktree ? "⑂" : "◆"}
        </span>
        <span className="flex min-w-0 flex-1 flex-col">
          <span className="flex items-baseline gap-xs text-body text-primary">
            <span className="min-w-0 truncate">{name}</span>
            {checkout.exists ? null : <span className="text-micro uppercase text-danger">missing</span>}
            {badge ? (
              <span className={`text-micro ${badge.color}`}>
                #{checkout.pull_request?.number} {badge.label}
              </span>
            ) : null}
          </span>
          <span className="truncate text-caption text-muted">{checkout.purpose?.text ?? checkout.path}</span>
        </span>
        <Counts counts={counts} />
      </button>
    </li>
  );
}

function ProjectAgents({ agents, workspace, actions }: { agents: AgentRow[] | null; workspace: Workspace; actions: Actions }) {
  const byPane = new Map<string, Checkout>();
  for (const checkout of workspace.checkouts) for (const tab of checkout.tabs) for (const pane of tab.panes) byPane.set(pane.id, checkout);
  if (!agents) {
    return (
      <section aria-label="Agents in this project" data-overview-agents="unknown">
        <h2 className="pb-xs text-micro uppercase text-muted">Agents · …</h2>
        <p className="px-sm py-xs text-caption text-muted">Its agents show once this device's Herdr answers.</p>
      </section>
    );
  }
  return (
    <section aria-label="Agents in this project" data-overview-agents="true">
      <h2 className="pb-xs text-micro uppercase text-muted">Agents · {agents.length}</h2>
      {agents.length === 0 ? (
        <p className="px-sm py-xs text-caption text-muted">No agent is running in this project.</p>
      ) : (
        agentSections(agents).map(({ group, label, agents: rows }) => {
          return (
            <div key={group} data-overview-agent-group={group}>
              <h3 className="px-sm pt-xs text-micro uppercase text-muted">
                {label} · {rows.length}
              </h3>
              <ul role="list">
                {rows.map((agent) => {
                  const checkout = byPane.get(agent.pane_id);
                  return (
                    <li key={agent.id}>
                      <button
                        type="button"
                        title={`${agent.identity_label} · ${agent.agent_kind} · ${agent.status_label}${agent.detail ? ` · ${agent.detail}` : ""}`}
                        data-overview-agent={agent.pane_id}
                        className={`flex w-full items-center gap-xs rounded-sm px-sm py-xs text-left outline-none hover:bg-elevated focus-visible:bg-elevated ${agent.emphasized ? "text-primary" : "text-secondary"}`}
                        onClick={() => actions.openAgent(agent.pane_id)}
                      >
                        <span className="w-[var(--size-agent-mark)] shrink-0 font-mono text-caption">{agent.symbol}</span>
                        <AgentMark kind={agent.agent_kind} />
                        <span className="min-w-0 flex-1 truncate text-body">{agent.identity_label}</span>
                        <span className="max-w-[var(--size-recent-location-max)] shrink-0 truncate text-caption text-muted">{checkout ? (checkout.branch ?? checkout.label) : ""}</span>
                      </button>
                    </li>
                  );
                })}
              </ul>
            </div>
          );
        })
      )}
    </section>
  );
}
