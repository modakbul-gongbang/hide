import {
  ChevronDownIcon,
  ChevronRightIcon,
  FolderGit2Icon,
  FolderIcon,
  GitBranchIcon,
  GitCommitHorizontalIcon,
  GitMergeIcon,
  GitPullRequestClosedIcon,
  GitPullRequestDraftIcon,
  GitPullRequestIcon,
  HouseIcon,
  SettingsIcon,
} from "lucide-react";
import { memo, useMemo } from "react";
import type { Actions } from "./actions";
import { Button } from "./components/ui/button";
import { RowMenu } from "./components/entry-menu";
import { SearchField } from "./components/search-field";
import { Hint } from "./components/ui/tooltip";
import { DevicePicker } from "./DevicePicker";
import { NewWorkspace } from "./NewWorkspace";
import { directChildren, markTone, sectionCount, sectionTree } from "./agentRow";
import { AgentMark } from "./AgentMark";
import { Badge } from "./components/ui/badge";
import { cn } from "./lib/utils";
import { checkoutAgentRows, type BoardRow } from "./projectBoard";
import { AgentRowItem } from "./components/agent-row";
import { WeeklyUsage } from "./components/weekly-usage";
import { agentSections, allAgents, liveDescendantCounts, type ListedAgent } from "./navigation";
import { activeCheckouts, activityLabel, checkoutPresentation, inactiveCheckouts, projectRows, type CheckoutKind, type ProjectRow } from "./projects";
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
      </div>
      <SearchField onOpen={() => actions.openSearch()} />
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
        <Hint label={`Settings (${displayCommand("settings", hostKind())})`}>
          <Button variant="ghost" size="icon-sm" data-open-settings="true" onClick={() => actions.openSettings()}>
            <SettingsIcon />
          </Button>
        </Hint>
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
          <div className="px-md pb-xxs pt-sm text-micro font-semibold uppercase text-muted-foreground" id={`agent-group-${section.group}`}>
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
  const collapsedCheckouts = useShellStore((s) => s.rest?.ui_state?.collapsed_checkout_ids ?? NO_IDS);
  const focusedPaneId = useShellStore((s) => s.focusedPaneId);
  const catalogState = useShellStore((s) => catalogLineOf(s.rest)?.state ?? null);
  const catalogText = useShellStore((s) => catalogLineOf(s.rest)?.text ?? null);
  const catalogLine = catalogState && catalogText ? { state: catalogState, text: catalogText } : null;
  const rows = projectRows(workspaces, groups);
  // The folds are this machine's choices, like the inactive groups, so a
  // selected SSH device's tree is drawn open with every checkout's line two.
  const context: ListContext = { agents, focusedCheckoutId, focusedPaneId, collapsedCheckouts, disclosure: !remote, actions };
  return (
    <ul className="min-h-0 flex-1 overflow-auto" data-project-list="true">
      {catalogLine ? (
        <li role="status" className="px-md py-xs text-caption text-muted-foreground" data-device-catalog={catalogLine.state}>
          {catalogLine.text}
        </li>
      ) : null}
      {rows.map((row) => (
        <ProjectRowView key={rowKey(row)} row={row} context={context} />
      ))}
    </ul>
  );
}

const NO_IDS: string[] = [];

/** What every row of the Projects list reads besides its own project. */
type ListContext = {
  agents: AgentRow[];
  focusedCheckoutId: string | null;
  focusedPaneId: string | null;
  collapsedCheckouts: string[];
  /** False over a selected SSH device, whose tree is drawn with nothing folded. */
  disclosure: boolean;
  actions: Actions;
};

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

const ProjectRowView = memo(function ProjectRowView({ row, context }: { row: ProjectRow; context: ListContext }) {
  if (row.kind === "header") {
    return (
      <li className="px-md pt-sm pb-xxs text-micro font-semibold uppercase text-muted-foreground" data-section={row.title}>
        {row.title} · {row.count}
      </li>
    );
  }
  if (row.kind === "inactive_projects") {
    return (
      <li>
        <FoldRow
          label="Inactive projects"
          count={row.count}
          expanded={row.group.expanded}
          level="project"
          onToggle={() => context.actions.toggleInactiveProjects(row.group.device_id)}
          data-inactive-projects={row.group.device_id}
        />
      </li>
    );
  }
  return <WorkspaceRows workspace={row.workspace} level={row.level} context={context} />;
});

/**
 * Where a checkout's name, and an opened checkout's agent marks, start: the
 * project name's column, past the disclosure lane, the icon and two gaps.
 */
const NAME_COLUMN = "calc(var(--spacing-md) + var(--size-lineage-chevron) + var(--size-checkout-icon) + 2 * var(--spacing-sm))";
/** Where a checkout's kind glyph, and its Inactive fold, start: under the project icon. */
const GLYPH_COLUMN = "calc(var(--spacing-md) + var(--size-lineage-chevron) + var(--spacing-sm))";

/** One disclosure for both inactive folds, the project level's and the checkout level's. */
function FoldRow({
  label,
  count,
  expanded,
  level,
  onToggle,
  ...data
}: {
  label: string;
  count: number;
  expanded: boolean;
  level: "project" | "checkout";
  onToggle: () => void;
} & Record<`data-${string}`, string>) {
  const Chevron = expanded ? ChevronDownIcon : ChevronRightIcon;
  return (
    <button
      type="button"
      aria-expanded={expanded}
      className="flex h-(--size-control-lg) w-full items-center gap-sm pr-md text-left text-caption font-medium text-subtle-foreground outline-none hover:bg-accent hover:text-foreground focus-visible:ring-1 focus-visible:ring-inset focus-visible:ring-ring"
      style={{ paddingLeft: level === "project" ? "var(--spacing-md)" : GLYPH_COLUMN }}
      onClick={onToggle}
      {...data}
    >
      <span className="flex w-(--size-lineage-chevron) shrink-0 justify-center text-muted-foreground">
        <Chevron aria-hidden="true" className="size-(--size-icon-sm)" />
      </span>
      {label} {count}
    </button>
  );
}

/** A lane for a row control that is not there, so every row's columns line up. */
function Lane({ width }: { width: "chevron" | "slot" }) {
  return <span aria-hidden="true" className={cn("shrink-0", width === "chevron" ? "w-(--size-lineage-chevron)" : "w-(--size-icon-button-toolbar)")} />;
}

function WorkspaceRows({ workspace, level, context }: { workspace: Workspace; level: "root" | "child"; context: ListContext }) {
  const { agents, actions, disclosure } = context;
  const active = activeCheckouts(workspace);
  const inactive = inactiveCheckouts(workspace);
  const rowsByCheckout = useMemo(() => checkoutAgentRows(workspace, agents), [workspace, agents]);
  const expanded = !disclosure || workspace.expanded !== false;
  const inset = level === "child" ? "pl-[var(--size-lineage-indent)]" : "";
  const ProjectIcon = workspace.is_git ? FolderGit2Icon : FolderIcon;
  const Chevron = expanded ? ChevronDownIcon : ChevronRightIcon;
  const checkoutRow = (checkout: Checkout) => (
    <CheckoutRowView
      key={checkout.id}
      workspace={workspace}
      checkout={checkout}
      agentRows={rowsByCheckout.get(checkout.id) ?? NO_BOARD_ROWS}
      focused={checkout.id === context.focusedCheckoutId}
      context={context}
    />
  );
  return (
    <li data-project={workspace.id} className={inset}>
      <RowMenu
        label={`${workspace.label} actions`}
        items={projectMenu(workspace)}
        onSelect={(item) => runProjectItem(actions, workspace, item)}
        data-project-menu={workspace.id}
      >
        {(trigger) => (
          <div className="flex h-(--size-control-regular) w-full items-center pr-xs pl-md hover:bg-accent">
            {disclosure ? (
              <button
                type="button"
                aria-expanded={expanded}
                aria-label={expanded ? `Fold ${workspace.label}` : `Unfold ${workspace.label}`}
                data-project-toggle={workspace.id}
                className="flex h-full w-(--size-lineage-chevron) shrink-0 items-center justify-center rounded-xs text-muted-foreground outline-none hover:text-foreground focus-visible:ring-1 focus-visible:ring-ring"
                onClick={() => actions.toggleProjectCheckouts(workspace)}
              >
                <Chevron aria-hidden="true" className="size-(--size-icon-sm)" />
              </button>
            ) : (
              <Lane width="chevron" />
            )}
            <button
              type="button"
              data-project-row={workspace.id}
              className="flex h-full min-w-0 flex-1 items-center gap-sm pl-sm text-left outline-none focus-visible:ring-1 focus-visible:ring-inset focus-visible:ring-ring"
              onClick={() => useUiStore.getState().setScreen({ kind: "overview", projectId: workspace.id })}
            >
              <ProjectIcon aria-hidden="true" className="size-(--size-checkout-icon) shrink-0 text-subtle-foreground" />
              <span className="min-w-0 flex-1 truncate text-title font-semibold text-foreground">{workspace.label}</span>
              <span className="shrink-0 text-body text-muted-foreground">{activityLabel(workspace, agents, Date.now())}</span>
            </button>
            <span className="relative ml-sm flex h-(--size-icon-button-toolbar) w-(--size-icon-button-toolbar) shrink-0">{trigger}</span>
          </div>
        )}
      </RowMenu>
      {expanded ? (
        <ul>
          {active.map(checkoutRow)}
          {inactive.length > 0 ? (
            <li>
              <FoldRow
                label="Inactive"
                count={inactive.length}
                expanded={workspace.inactive_checkouts.expanded}
                level="checkout"
                onToggle={() => actions.toggleInactiveCheckouts(workspace.path)}
                data-inactive-checkouts={workspace.path}
              />
            </li>
          ) : null}
          {workspace.inactive_checkouts.expanded ? inactive.map(checkoutRow) : null}
        </ul>
      ) : null}
    </li>
  );
}

const NO_BOARD_ROWS: BoardRow[] = [];

const KIND_ICON: Record<CheckoutKind, typeof GitBranchIcon> = {
  pr_open: GitPullRequestIcon,
  pr_draft: GitPullRequestDraftIcon,
  pr_merged: GitMergeIcon,
  pr_closed: GitPullRequestClosedIcon,
  folder: FolderIcon,
  primary: HouseIcon,
  detached: GitCommitHorizontalIcon,
  branch: GitBranchIcon,
};

/**
 * A checkout: the row opens it; its trailing chevron, there only while
 * agents run in it, opens their rows in place of line two, which names them
 * (the representative's mark and provider, +N for the rest) and the purpose.
 * The `⋯` menu takes the age's place while the row is under the pointer.
 */
const CheckoutRowView = memo(function CheckoutRowView({
  workspace,
  checkout,
  agentRows,
  focused,
  context,
}: {
  workspace: Workspace;
  checkout: Checkout;
  agentRows: BoardRow[];
  focused: boolean;
  context: ListContext;
}) {
  const { agents, actions, disclosure } = context;
  const purposeProblem = useShellStore((s) => remotePurposeProblem(workspace, s.rest?.status?.remote));
  const view = checkoutPresentation(workspace, checkout, Date.now());
  const name = checkout.branch ?? checkout.label;
  const foldable = disclosure && agentRows.length > 0;
  const open = foldable && !context.collapsedCheckouts.includes(checkout.id);
  const purpose = checkout.purpose?.text ?? null;
  const secondLine = !open && view.secondLineReady && (view.agentCount > 0 || purpose !== null);
  const representative = agents.find((agent) => agent.pane_id === checkout.agent_summary?.representative_pane_id) ?? null;
  const KindIcon = KIND_ICON[view.kind];
  const Chevron = open ? ChevronDownIcon : ChevronRightIcon;
  return (
    <li data-checkout-row={checkout.id}>
      <RowMenu
        label={`${name} actions`}
        items={checkoutMenu(checkout, purposeProblem)}
        onSelect={(item) => runCheckoutItem(workspace, checkout, item)}
        data-checkout-menu={checkout.id}
      >
        {(trigger) => (
          <div
            className={cn(
              "relative flex w-full flex-col justify-center gap-xxs pr-xs pl-md",
              secondLine ? "h-(--size-checkout-row-detailed)" : "h-(--size-checkout-row)",
              focused ? "bg-secondary" : "hover:bg-accent",
              view.settled && "opacity-(--opacity-dimmed)",
            )}
          >
            <Hint label={view.detail}>
              <button
                type="button"
                data-checkout={checkout.id}
                data-checkout-kind={view.kind}
                aria-current={focused ? "true" : undefined}
                aria-label={[name, view.age, purpose].filter(Boolean).join(", ")}
                className="absolute inset-0 outline-none focus-visible:ring-1 focus-visible:ring-inset focus-visible:ring-ring"
                onClick={() => actions.openWorkspace(workspace.device_id, checkout.workspace_id, checkout.id)}
              />
            </Hint>
            {/* The row button covers the whole row; a control drawn over it is positioned, so it stacks above. */}
            <span className="pointer-events-none flex min-w-0 items-center gap-sm">
              <Lane width="chevron" />
              <KindIcon aria-hidden="true" className={cn("size-(--size-checkout-icon) shrink-0", view.kindTone)} />
              <span aria-hidden="true" className={cn("min-w-0 truncate text-subhead text-foreground", focused ? "font-semibold" : "font-medium")}>
                {name}
              </span>
              {!checkout.exists ? (
                <Badge variant="secondary" className="shrink-0 text-destructive" data-checkout-missing="true">
                  missing
                </Badge>
              ) : checkout.temporary ? (
                <Badge variant="secondary" className="shrink-0 text-warning">
                  temporary
                </Badge>
              ) : null}
              <span className="flex-1" />
              <span className="group/menu pointer-events-auto relative flex h-(--size-icon-button-toolbar) w-(--size-icon-button-toolbar) shrink-0 items-center justify-end">
                {view.age ? (
                  <span
                    aria-hidden="true"
                    data-checkout-age={view.age}
                    className="font-mono text-caption text-muted-foreground group-hover:opacity-0 group-has-[:focus-visible]/menu:opacity-0 group-has-[[data-state=open]]/menu:opacity-0"
                  >
                    {view.age}
                  </span>
                ) : null}
                {trigger}
              </span>
              {foldable ? (
                <button
                  type="button"
                  aria-expanded={open}
                  aria-label={open ? `Hide the agents in ${name}` : `Show the agents in ${name}`}
                  data-checkout-toggle={checkout.id}
                  className="pointer-events-auto relative flex h-(--size-icon-button-toolbar) w-(--size-lineage-chevron) shrink-0 items-center justify-center rounded-xs text-subtle-foreground outline-none hover:text-foreground focus-visible:ring-1 focus-visible:ring-ring"
                  onClick={() => actions.toggleCheckoutAgents(checkout.id)}
                >
                  <Chevron aria-hidden="true" className="size-(--size-icon-sm)" />
                </button>
              ) : (
                <Lane width="chevron" />
              )}
            </span>
            {secondLine ? (
              <span aria-hidden="true" className="pointer-events-none flex min-w-0 items-center gap-xs pl-(--size-checkout-metadata-inset) pr-md text-caption">
                {view.agentCount > 0 ? (
                  <span className="flex shrink-0 items-center gap-xs" data-checkout-agents={view.agentCount}>
                    {representative ? <span className={cn("w-(--size-agent-mark) text-center font-mono", markTone(representative))}>{representative.symbol}</span> : null}
                    <AgentMark kind={representative?.agent_kind} />
                    {view.agentCount > 1 ? <span className="font-mono text-subtle-foreground">+{view.agentCount - 1}</span> : null}
                  </span>
                ) : null}
                {purpose ? (
                  <span className="min-w-0 truncate text-muted-foreground" data-purpose={checkout.purpose?.origin}>
                    {purpose}
                  </span>
                ) : null}
              </span>
            ) : null}
          </div>
        )}
      </RowMenu>
      {open ? (
        <ul data-checkout-agents-open={checkout.id}>
          {agentRows.map((row) => (
            <AgentRowItem
              key={row.agent.pane_id}
              agent={row.agent}
              device={null}
              depth={row.depth}
              descendants={0}
              childRows={NO_AGENT_ROWS}
              selected={row.agent.pane_id === context.focusedPaneId}
              onOpen={actions.openAgent}
              onToggleTree={null}
              inset={NAME_COLUMN}
            />
          ))}
        </ul>
      ) : null}
    </li>
  );
});

/** An opened checkout draws every descendant, so no row lists folded children. */
const NO_AGENT_ROWS: AgentRow[] = [];

function runProjectItem(actions: Actions, workspace: Workspace, item: MenuItem["id"]) {
  if (item === "pin" || item === "unpin") return actions.setPinned(workspace.id, item === "pin");
  if (item === "new_worktree") useUiStore.getState().setWorkspaceDialog({ kind: "new_worktree", workspaceId: workspace.id });
  if (item === "remove_project") useUiStore.getState().setWorkspaceDialog({ kind: "remove_project", workspaceId: workspace.id });
}

function runCheckoutItem(workspace: Workspace, checkout: Checkout, item: MenuItem["id"]) {
  if (item === "set_purpose") useUiStore.getState().setWorkspaceDialog({ kind: "purpose", workspaceId: workspace.id, checkoutId: checkout.id });
  if (item === "delete_worktree") useUiStore.getState().setWorkspaceDialog({ kind: "delete_worktree", workspaceId: workspace.id, checkoutId: checkout.id });
}
