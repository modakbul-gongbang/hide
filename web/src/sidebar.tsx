import { SettingsIcon } from "lucide-react";
import { memo, useMemo } from "react";
import type { Actions } from "./actions";
import { Button } from "./components/ui/button";
import { RowMenu } from "./components/entry-menu";
import { Hint } from "./components/ui/tooltip";
import { DevicePicker } from "./DevicePicker";
import { NewWorkspace } from "./NewWorkspace";
import { directChildren, sectionCount, sectionTree } from "./agentRow";
import { AgentRowItem } from "./components/agent-row";
import { WeeklyUsage } from "./components/weekly-usage";
import { agentSections, allAgents, liveDescendantCounts, type ListedAgent } from "./navigation";
import { activeCheckouts, activityLabel, inactiveCheckouts, projectRows, pullRequestBadge, type ProjectRow } from "./projects";
import { hostKind } from "./host";
import { displayCommand } from "./shortcuts";
import { contextAgents, contextWorkspaces, deviceCatalogLine, remoteContext, remoteView } from "./remote";
import { checkoutMenu, projectMenu, remotePurposeProblem, type MenuItem } from "./workspaceManage";
import { focusedRemoteDevice, type AgentRow, type Checkout, type InactiveProjectGroup, type SnapshotRest, type Workspace } from "./snapshot";
import { useShellStore } from "./store";
import { SIDEBAR_MODES, useUiStore } from "./ui";

function herdrRowLabel(state: string | null): string | null {
  if (state === "unconfigured" || state === "socket_missing") return "Herdr 소켓 없음";
  if (state === "not_connected" || state === "unreachable" || state === "stale") {
    return "Herdr 무응답";
  }
  return null;
}

export function Sidebar({ actions }: { actions: Actions }) {
  const herdrState = useShellStore((s) => s.herdrState);
  const mode = useUiStore((s) => s.sidebarMode);
  const visible = useShellStore((s) => s.rest?.ui_state?.left_sidebar_visible ?? true);
  // The row speaks for this machine's Herdr, so it is not shown over a
  // selected SSH device's lists; that device's state is on the canvas.
  const remote = useShellStore((s) => focusedRemoteDevice(s.rest) !== null);
  const status = remote ? null : herdrRowLabel(herdrState);
  if (!visible) return null;

  return (
    <nav className="flex h-full w-[var(--size-sidebar-ideal)] shrink-0 flex-col bg-sidebar text-foreground" data-sidebar={mode}>
      <div className="flex h-[var(--size-tab-strip)] shrink-0 items-center gap-sm px-md text-caption">
        {SIDEBAR_MODES.map((candidate) => (
          <button
            key={candidate}
            type="button"
            data-sidebar-mode={candidate}
            aria-pressed={mode === candidate}
            className={mode === candidate ? "text-foreground" : "text-muted-foreground hover:text-subtle-foreground"}
            onClick={() => actions.showSidebarMode(candidate)}
          >
            {candidate === "agents" ? "Agents" : "Projects"}
          </button>
        ))}
        <span className="flex-1" />
        <span className="text-muted-foreground">⌘E</span>
        <Hint label={`Settings (${displayCommand("settings", hostKind())})`}>
          <Button variant="ghost" size="icon-sm" data-open-settings="true" onClick={() => actions.openSettings()}>
            <SettingsIcon />
          </Button>
        </Hint>
      </div>
      {status ? (
        <div className="border-b border-border px-md py-sm text-caption text-muted-foreground">{status}</div>
      ) : null}
      {mode === "agents" ? <AgentList actions={actions} /> : <ProjectList actions={actions} />}
      <NewWorkspace actions={actions} />
      <button
        type="button"
        data-new-workspace-button="true"
        className="shrink-0 border-t border-border px-md py-sm text-left text-caption text-subtle-foreground hover:text-foreground"
        onClick={() => actions.openNewWorkspace()}
      >
        + 새 워크스페이스 <span className="text-muted-foreground">{displayCommand("new_workspace", hostKind())}</span>
      </button>
      <div className="flex shrink-0 items-center gap-xs border-t border-border px-md py-xs">
        <DevicePicker actions={actions} />
        <span className="flex-1" />
        <WeeklyUsage actions={actions} />
      </div>
    </nav>
  );
}

/**
 * Every current agent, this machine's and each connected device's, under
 * Needs You, Done, Working and Seen (S6 B13); a device's row names its
 * device. Each section draws its roots, and a root's descendants follow it
 * while the operator has it unfolded; folded, the root's badge speaks for
 * them and opens their list (PRD sidebar-agent-status D-02, D-03). Showing
 * the list changes nothing: no focus moves and nothing is marked read.
 */
function AgentList({ actions }: { actions: Actions }) {
  // Only what the list reads, so a terminal frame or an editor change does
  // not rebuild it.
  const remote = useShellStore((s) => s.rest?.status?.remote);
  const devices = useShellStore((s) => s.rest?.navigator?.devices);
  const localAgents = useShellStore((s) => s.agents);
  const listed = useMemo(() => allAgents(remote, devices, localAgents), [remote, devices, localAgents]);
  const tree = useMemo(() => agentTree(listed), [listed]);
  const focusedPaneId = useShellStore((s) => s.focusedPaneId);
  if (tree.sections.length === 0) {
    return <div className="min-h-0 flex-1 px-md py-sm text-caption text-muted-foreground" data-agents-empty="true">No agents are running</div>;
  }
  return (
    <ul className="min-h-0 flex-1 overflow-auto" data-agent-list="true">
      {tree.sections.map((section) => (
        <li key={section.group} data-agent-group={section.group}>
          <div className="px-md pb-xxs pt-sm text-micro uppercase text-muted-foreground" id={`agent-group-${section.group}`}>
            {section.label} · {section.count}
          </div>
          <ul aria-labelledby={`agent-group-${section.group}`}>
            {section.rows.map((row) => (
              <AgentRowItem
                key={`${row.device ?? "local"}:${row.agent.id}`}
                agent={row.agent}
                device={row.device}
                depth={row.depth}
                descendants={row.descendants}
                childRows={tree.children(row.device, row.agent)}
                selected={row.agent.pane_id === focusedPaneId}
                onOpen={actions.openAgent}
                onToggleTree={actions.toggleAgentTree}
              />
            ))}
          </ul>
        </li>
      ))}
    </ul>
  );
}

/**
 * The sections and the rows each draws, from one index per device: pane ids
 * are scoped to the device that reported them, so a lineage never crosses
 * devices.
 */
function agentTree(listed: ListedAgent[]) {
  const byDevice = new Map<string | null, AgentRow[]>();
  for (const { agent, device } of listed) {
    const rows = byDevice.get(device) ?? [];
    rows.push(agent);
    byDevice.set(device, rows);
  }
  const index = new Map([...byDevice].map(([device, rows]) => [device, new Map(rows.map((row) => [row.pane_id, row]))]));
  const counts = new Map([...byDevice].map(([device, rows]) => [device, liveDescendantCounts(rows)]));
  const deviceOf = new Map(listed.map((row) => [row.agent, row.device]));
  const lookup = (device: string | null, paneId: string) => index.get(device)?.get(paneId);
  const descendantsOf = (device: string | null, paneId: string) => counts.get(device)?.get(paneId) ?? 0;
  const sections = agentSections(listed.map((row) => row.agent))
    .map((section) => {
      const roots = section.agents.filter((agent) => !agent.delegated);
      const rows = sectionTree(roots.map((agent) => ({ agent, device: deviceOf.get(agent) ?? null })), lookup, descendantsOf);
      return { group: section.group, label: section.label, rows, count: sectionCount(rows) };
    })
    .filter((section) => section.rows.length > 0);
  return {
    sections,
    children: (device: string | null, agent: AgentRow) => directChildren(agent, (paneId) => lookup(device, paneId)),
  };
}

const NO_GROUPS: InactiveProjectGroup[] = [];

function catalogLineOf(rest: SnapshotRest | null) {
  const context = remoteContext(rest);
  return context ? deviceCatalogLine(context) : null;
}

/**
 * The projects of the context on screen. A selected SSH device lists its
 * Herdr workspaces, one checkout each, with the one its host has focused
 * marked; the inactive folds and pins are this machine's and are not drawn
 * there (docs/UI_BEHAVIOR.md: the remote context carries no pins).
 */
function ProjectList({ actions }: { actions: Actions }) {
  const remote = useShellStore((s) => focusedRemoteDevice(s.rest) !== null);
  const workspaces = useShellStore((s) => contextWorkspaces(s.rest));
  const groups = useShellStore((s) => (remote ? NO_GROUPS : (s.rest?.navigator?.inactive_projects ?? NO_GROUPS)));
  const focusedCheckoutId = useShellStore((s) =>
    remote ? (remoteView(remoteContext(s.rest)?.session ?? null)?.checkout.id ?? null) : (s.rest?.navigator?.focused_checkout_id ?? null),
  );
  const agents = useShellStore((s) => contextAgents(s.rest, s.agents));
  const catalogState = useShellStore((s) => catalogLineOf(s.rest)?.state ?? null);
  const catalogText = useShellStore((s) => catalogLineOf(s.rest)?.text ?? null);
  const catalogLine = catalogState && catalogText ? { state: catalogState, text: catalogText } : null;
  const rows = projectRows(workspaces, groups);
  return (
    <ul className="min-h-0 flex-1 overflow-auto" data-project-list="true">
      {catalogLine ? (
        <li role="status" className="px-md py-xs text-caption text-muted-foreground" data-device-catalog={catalogLine.state}>
          {catalogLine.text}
        </li>
      ) : null}
      {rows.map((row) => (
        <ProjectRowView key={rowKey(row)} row={row} agents={agents} focusedCheckoutId={focusedCheckoutId} actions={actions} />
      ))}
    </ul>
  );
}

function rowKey(row: ProjectRow): string {
  switch (row.kind) {
    case "header":
      return `header:${row.title}`;
    case "workspace":
      return `workspace:${row.workspace.id}`;
    case "inactive_projects":
      return `inactive-projects:${row.group.device_id}`;
  }
}

const ProjectRowView = memo(function ProjectRowView({
  row,
  agents,
  focusedCheckoutId,
  actions,
}: {
  row: ProjectRow;
  agents: AgentRow[];
  focusedCheckoutId: string | null;
  actions: Actions;
}) {
  if (row.kind === "header") {
    return (
      <li className="px-md pt-sm pb-xxs text-micro uppercase text-muted-foreground" data-section={row.title}>
        {row.title} · {row.count}
      </li>
    );
  }
  if (row.kind === "inactive_projects") {
    return (
      <li>
        <button
          type="button"
          data-inactive-projects={row.group.device_id}
          aria-expanded={row.group.expanded}
          className="flex w-full items-center gap-xs px-md py-xs text-left text-caption text-muted-foreground hover:text-subtle-foreground"
          onClick={() => actions.toggleInactiveProjects(row.group.device_id)}
        >
          <span className="font-mono">{row.group.expanded ? "▾" : "▸"}</span>
          Inactive projects · {row.count}
        </button>
      </li>
    );
  }
  return <WorkspaceRows workspace={row.workspace} level={row.level} agents={agents} focusedCheckoutId={focusedCheckoutId} actions={actions} />;
});

function WorkspaceRows({
  workspace,
  level,
  agents,
  focusedCheckoutId,
  actions,
}: {
  workspace: Workspace;
  level: "root" | "child";
  agents: AgentRow[];
  focusedCheckoutId: string | null;
  actions: Actions;
}) {
  const active = activeCheckouts(workspace);
  const inactive = inactiveCheckouts(workspace);
  const inset = level === "child" ? "pl-[var(--size-lineage-indent)]" : "";
  return (
    <li data-project={workspace.id} className={inset}>
      <RowMenu
        label={`${workspace.label} actions`}
        items={projectMenu(workspace)}
        onSelect={(item) => runProjectItem(actions, workspace, item)}
        data-project-menu={workspace.id}
      >
        <button
          type="button"
          data-project-row={workspace.id}
          className="flex w-full flex-col items-start px-md py-xs text-left hover:bg-accent"
          onClick={() => useUiStore.getState().setScreen({ kind: "overview", projectId: workspace.id })}
        >
          <span className="flex w-full items-baseline gap-xs text-body text-foreground">
            <span className="min-w-0 flex-1 truncate">{workspace.label}</span>
            {workspace.pinned ? (
              <span className="text-micro uppercase text-muted-foreground" data-pinned="true">
                pinned
              </span>
            ) : null}
          </span>
          <span className="text-caption text-muted-foreground">{activityLabel(workspace, agents, Date.now())}</span>
        </button>
      </RowMenu>
      <ul>
        {active.map((checkout) => (
          <CheckoutRowView key={checkout.id} workspace={workspace} checkout={checkout} focused={checkout.id === focusedCheckoutId} actions={actions} />
        ))}
        {inactive.length > 0 ? (
          <li>
            <button
              type="button"
              data-inactive-checkouts={workspace.path}
              aria-expanded={workspace.inactive_checkouts.expanded}
              className="flex w-full items-center gap-xs px-md py-xxs pl-[var(--size-lineage-indent)] text-left text-caption text-muted-foreground hover:text-subtle-foreground"
              onClick={() => actions.toggleInactiveCheckouts(workspace.path)}
            >
              <span className="font-mono">{workspace.inactive_checkouts.expanded ? "▾" : "▸"}</span>
              Inactive · {inactive.length}
            </button>
          </li>
        ) : null}
        {workspace.inactive_checkouts.expanded
          ? inactive.map((checkout) => (
              <CheckoutRowView key={checkout.id} workspace={workspace} checkout={checkout} focused={checkout.id === focusedCheckoutId} actions={actions} />
            ))
          : null}
      </ul>
    </li>
  );
}

const CheckoutRowView = memo(function CheckoutRowView({
  workspace,
  checkout,
  focused,
  actions,
}: {
  workspace: Workspace;
  checkout: Checkout;
  focused: boolean;
  actions: Actions;
}) {
  const badge = checkout.pull_request ? pullRequestBadge(checkout.pull_request) : null;
  const purposeProblem = useShellStore((s) => remotePurposeProblem(workspace, s.rest?.status?.remote));
  return (
    <li>
      <RowMenu
        label={`${checkout.branch ?? checkout.label} actions`}
        items={checkoutMenu(checkout, purposeProblem)}
        onSelect={(item) => runCheckoutItem(workspace, checkout, item)}
        data-checkout-menu={checkout.id}
      >
        <button
          type="button"
          data-checkout={checkout.id}
          aria-current={focused ? "true" : undefined}
          className={`flex w-full flex-col items-start px-md py-xs pl-[var(--size-lineage-indent)] text-left ${
            focused ? "bg-secondary text-foreground" : "text-subtle-foreground hover:bg-accent"
          }`}
          onClick={() => actions.openWorkspace(workspace.device_id, checkout.workspace_id, checkout.id)}
        >
          <span className="flex w-full items-baseline gap-xs text-body">
            <span className="w-[var(--size-checkout-icon)] font-mono text-caption text-muted-foreground" aria-hidden="true">
              {checkout.is_worktree ? "⑂" : "◆"}
            </span>
            <span className="min-w-0 flex-1 truncate">{checkout.branch ?? checkout.label}</span>
            {badge ? (
              <span className={`text-micro ${badge.color}`} data-pr-badge={badge.label}>
                #{checkout.pull_request?.number} {badge.label}
              </span>
            ) : null}
          </span>
          {checkout.purpose?.text ? (
            <span className="w-full truncate pl-[var(--size-checkout-icon)] text-caption text-muted-foreground" data-purpose={checkout.purpose.origin}>
              {checkout.purpose.text}
            </span>
          ) : null}
        </button>
      </RowMenu>
    </li>
  );
});

function runProjectItem(actions: Actions, workspace: Workspace, item: MenuItem["id"]) {
  if (item === "pin" || item === "unpin") return actions.setPinned(workspace.id, item === "pin");
  if (item === "new_worktree") useUiStore.getState().setWorkspaceDialog({ kind: "new_worktree", workspaceId: workspace.id });
  if (item === "remove_project") useUiStore.getState().setWorkspaceDialog({ kind: "remove_project", workspaceId: workspace.id });
}

function runCheckoutItem(workspace: Workspace, checkout: Checkout, item: MenuItem["id"]) {
  if (item === "set_purpose") useUiStore.getState().setWorkspaceDialog({ kind: "purpose", workspaceId: workspace.id, checkoutId: checkout.id });
  if (item === "delete_worktree") useUiStore.getState().setWorkspaceDialog({ kind: "delete_worktree", workspaceId: workspace.id, checkoutId: checkout.id });
}
