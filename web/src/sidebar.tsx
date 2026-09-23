import { memo } from "react";
import type { Actions } from "./actions";
import { NewWorkspace } from "./NewWorkspace";
import { activeCheckouts, activityLabel, inactiveCheckouts, projectRows, pullRequestBadge, type ProjectRow } from "./projects";
import type { AgentRow, Checkout, InactiveProjectGroup, Workspace } from "./snapshot";
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
  const status = herdrRowLabel(herdrState);
  if (!visible) return null;

  return (
    <nav className="flex h-full w-[var(--size-sidebar-ideal)] shrink-0 flex-col bg-sidebar text-primary" data-sidebar={mode}>
      <div className="flex h-[var(--size-tab-strip)] shrink-0 items-center gap-sm px-md text-caption">
        {SIDEBAR_MODES.map((candidate) => (
          <button
            key={candidate}
            type="button"
            data-sidebar-mode={candidate}
            aria-pressed={mode === candidate}
            className={mode === candidate ? "text-primary" : "text-muted hover:text-secondary"}
            onClick={() => actions.showSidebarMode(candidate)}
          >
            {candidate === "agents" ? "Agents" : "Projects"}
          </button>
        ))}
        <span className="flex-1" />
        <span className="text-muted">⌘E</span>
      </div>
      {status ? (
        <div className="border-b border-divider px-md py-sm text-caption text-muted">{status}</div>
      ) : null}
      {mode === "agents" ? <AgentList actions={actions} /> : <ProjectList actions={actions} />}
      <NewWorkspace actions={actions} />
      <button
        type="button"
        data-new-workspace-button="true"
        className="shrink-0 border-t border-divider px-md py-sm text-left text-caption text-secondary hover:text-primary"
        onClick={() => actions.openNewWorkspace()}
      >
        + 새 워크스페이스 <span className="text-muted">⌥⇧N</span>
      </button>
    </nav>
  );
}

function AgentList({ actions }: { actions: Actions }) {
  const agents = useShellStore((s) => s.agents);
  const focusedPaneId = useShellStore((s) => s.focusedPaneId);
  return (
    <ul className="min-h-0 flex-1 overflow-auto">
      {agents.map((agent) => (
        <AgentRowView
          key={agent.id}
          agent={agent}
          selected={agent.pane_id === focusedPaneId}
          onSelect={() =>
            actions.dispatch({
              schema_version: 2,
              kind: "focus_pane",
              payload: { pane_id: agent.pane_id, origin: "operator" },
            })
          }
        />
      ))}
    </ul>
  );
}

const AgentRowView = memo(function AgentRowView({
  agent,
  selected,
  onSelect,
}: {
  agent: AgentRow;
  selected: boolean;
  onSelect: () => void;
}) {
  const attention = agent.group === "needs_you" || agent.unread;
  return (
    <li>
      <button
        type="button"
        onClick={onSelect}
        data-pane={agent.pane_id}
        data-attention={attention ? "true" : "false"}
        className={`flex w-full flex-col items-start px-md py-xs text-left ${
          selected ? "bg-elevated" : ""
        } ${agent.emphasized || attention ? "text-primary" : "text-secondary"}`}
      >
        <span className="flex w-full items-baseline gap-xs text-body">
          <span className="w-[var(--size-agent-mark)] font-mono text-caption">{agent.symbol}</span>
          <span className="flex-1 truncate">{agent.identity_label}</span>
          <span className="text-micro text-muted">{agent.elapsed}</span>
        </span>
        <span className="pl-[var(--size-agent-mark)] text-caption text-secondary">
          {agent.unknown ? "unknown" : `${agent.agent_kind}${agent.detail ? ` / ${agent.detail}` : ""}`}
        </span>
      </button>
    </li>
  );
});

const NO_WORKSPACES: Workspace[] = [];
const NO_GROUPS: InactiveProjectGroup[] = [];

function ProjectList({ actions }: { actions: Actions }) {
  const workspaces = useShellStore((s) => s.rest?.navigator?.workspaces ?? NO_WORKSPACES);
  const groups = useShellStore((s) => s.rest?.navigator?.inactive_projects ?? NO_GROUPS);
  const focusedCheckoutId = useShellStore((s) => s.rest?.navigator?.focused_checkout_id ?? null);
  const agents = useShellStore((s) => s.agents);
  const rows = projectRows(workspaces, groups);
  return (
    <ul className="min-h-0 flex-1 overflow-auto" data-project-list="true">
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
      <li className="px-md pt-sm pb-xxs text-micro uppercase text-muted" data-section={row.title}>
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
          className="flex w-full items-center gap-xs px-md py-xs text-left text-caption text-muted hover:text-secondary"
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
      <button
        type="button"
        data-project-row={workspace.id}
        className="flex w-full flex-col items-start px-md py-xs text-left hover:bg-elevated"
        onClick={() => actions.focusProject(workspace.id)}
      >
        <span className="flex w-full items-baseline gap-xs text-body text-primary">
          <span className="min-w-0 flex-1 truncate">{workspace.label}</span>
          {workspace.pinned ? (
            <span className="text-micro uppercase text-muted" data-pinned="true">
              pinned
            </span>
          ) : null}
        </span>
        <span className="text-caption text-muted">{activityLabel(workspace, agents, Date.now())}</span>
      </button>
      <ul>
        {active.map((checkout) => (
          <CheckoutRowView key={checkout.id} checkout={checkout} focused={checkout.id === focusedCheckoutId} actions={actions} />
        ))}
        {inactive.length > 0 ? (
          <li>
            <button
              type="button"
              data-inactive-checkouts={workspace.path}
              aria-expanded={workspace.inactive_checkouts.expanded}
              className="flex w-full items-center gap-xs px-md py-xxs pl-[var(--size-lineage-indent)] text-left text-caption text-muted hover:text-secondary"
              onClick={() => actions.toggleInactiveCheckouts(workspace.path)}
            >
              <span className="font-mono">{workspace.inactive_checkouts.expanded ? "▾" : "▸"}</span>
              Inactive · {inactive.length}
            </button>
          </li>
        ) : null}
        {workspace.inactive_checkouts.expanded
          ? inactive.map((checkout) => (
              <CheckoutRowView key={checkout.id} checkout={checkout} focused={checkout.id === focusedCheckoutId} actions={actions} />
            ))
          : null}
      </ul>
    </li>
  );
}

const CheckoutRowView = memo(function CheckoutRowView({
  checkout,
  focused,
  actions,
}: {
  checkout: Checkout;
  focused: boolean;
  actions: Actions;
}) {
  const badge = checkout.pull_request ? pullRequestBadge(checkout.pull_request) : null;
  return (
    <li>
      <button
        type="button"
        data-checkout={checkout.id}
        aria-current={focused ? "true" : undefined}
        className={`flex w-full flex-col items-start px-md py-xs pl-[var(--size-lineage-indent)] text-left ${
          focused ? "bg-elevated text-primary" : "text-secondary hover:bg-elevated"
        }`}
        onClick={() => actions.focusCheckout(checkout.workspace_id, checkout.id)}
      >
        <span className="flex w-full items-baseline gap-xs text-body">
          <span className="w-[var(--size-checkout-icon)] font-mono text-caption text-muted" aria-hidden="true">
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
          <span className="w-full truncate pl-[var(--size-checkout-icon)] text-caption text-muted" data-purpose={checkout.purpose.origin}>
            {checkout.purpose.text}
          </span>
        ) : null}
      </button>
    </li>
  );
});
