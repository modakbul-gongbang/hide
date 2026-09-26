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
  LayoutGridIcon,
  SettingsIcon,
} from "lucide-react";
import { memo, useMemo, type ReactNode } from "react";
import type { Actions } from "./actions";
import { Button } from "./components/ui/button";
import { RowMenu } from "./components/entry-menu";
import { SearchField } from "./components/search-field";
import { Hint } from "./components/ui/tooltip";
import { DevicePicker } from "./DevicePicker";
import { NewWorkspace } from "./NewWorkspace";
import { directChildren, markTone, sectionCount, sectionTree, unfoldedRows } from "./agentRow";
import { AgentMark } from "./AgentMark";
import { Badge } from "./components/ui/badge";
import { cn } from "./lib/utils";
import { checkoutAgentRows, type BoardRow } from "./projectBoard";
import { SidebarAgentRow } from "./components/sidebar-agent-row";
import { StatusMark } from "./components/status-mark";
import { WeeklyUsage } from "./components/weekly-usage";
import { agentPlaces, agentSections, allAgents, allProjectsCount, liveDescendantCounts, type ListedAgent } from "./navigation";
import {
  activeCheckouts,
  activityLabel,
  checkoutPresentation,
  folderCheckout,
  inactiveCheckouts,
  projectRows,
  type CheckoutKind,
  type CheckoutPresentation,
  type ProjectRow,
} from "./projects";
import { hostKind } from "./host";
import { displayCommand } from "./shortcuts";
import { contextAgents, contextWorkspaces, deviceCatalogLine, remoteContext, remoteView } from "./remote";
import { checkoutMenu, folderMenu, projectMenu, remotePurposeProblem, type MenuItem } from "./workspaceManage";
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
  const loaded = useShellStore((s) => s.rest !== null);
  const remote = useShellStore((s) => s.rest?.status?.remote);
  const devices = useShellStore((s) => s.rest?.navigator?.devices);
  const workspaces = useShellStore((s) => s.rest?.navigator?.workspaces);
  const localAgents = useShellStore((s) => s.agents);
  const listed = useMemo(() => allAgents(remote, devices, localAgents), [remote, devices, localAgents]);
  const tree = useMemo(() => agentTree(listed), [listed]);
  const placeOf = useMemo(() => agentPlaces(workspaces, remote, devices), [workspaces, remote, devices]);
  const focusedPaneId = useShellStore((s) => s.focusedPaneId);
  // Before the first snapshot nothing is known, so an empty list would be a claim.
  if (!loaded) return <ListLoading />;
  if (tree.sections.length === 0) {
    return <div className="min-h-0 flex-1 px-md py-sm text-caption text-muted-foreground" data-agents-empty="true">No agents are running</div>;
  }
  return (
    <ul className="min-h-0 flex-1 overflow-auto px-xs" data-agent-list="true">
      {tree.sections.map((section) => (
        <li key={section.group} data-agent-group={section.group}>
          <div className="px-sm pb-xxs pt-sm text-micro font-semibold uppercase text-muted-foreground" id={`agent-group-${section.group}`}>
            {section.label} · {section.count}
          </div>
          <ul aria-labelledby={`agent-group-${section.group}`}>
            {section.rows.map((row) => (
              <SidebarAgentRow
                key={`${row.device ?? "local"}:${row.agent.id}`}
                agent={row.agent}
                device={row.device}
                place={row.depth === 0 ? placeOf(row.device, row.agent.pane_id) : null}
                depth={row.depth}
                descendants={row.descendants}
                childRows={tree.children(row.device, row.agent)}
                selected={row.agent.pane_id === focusedPaneId}
                onOpen={actions.openAgent}
                onToggleTree={actions.toggleAgentTree}
                inset="var(--spacing-xs)"
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

/** The first snapshot has not arrived: neither an empty list nor a zero is known yet. */
function ListLoading() {
  return (
    <div role="status" className="min-h-0 flex-1 px-md py-sm text-caption text-muted-foreground" data-sidebar-loading="true">
      Connecting…
    </div>
  );
}

const NO_GROUPS: InactiveProjectGroup[] = [];

function catalogLineOf(rest: SnapshotRest | null) {
  const context = remoteContext(rest);
  return context ? deviceCatalogLine(context) : null;
}

/**
 * The scope picker: All projects on top, then the projects of the context on
 * screen. A selected SSH device lists its Herdr workspaces, one checkout
 * each; the inactive folds and pins are this machine's and are not drawn
 * there (docs/UI_BEHAVIOR.md: the remote context carries no pins). The row of
 * the scope the center shows is marked: All projects, a project on its
 * Overview, or the focused checkout and its agent while a Workspace is in front.
 */
function ProjectList({ actions }: { actions: Actions }) {
  const loaded = useShellStore((s) => s.rest !== null);
  const remote = useShellStore((s) => focusedRemoteDevice(s.rest) !== null);
  const workspaces = useShellStore((s) => contextWorkspaces(s.rest));
  const groups = useShellStore((s) => (remote ? NO_GROUPS : (s.rest?.navigator?.inactive_projects ?? NO_GROUPS)));
  const focusedCheckoutId = useShellStore((s) =>
    remote ? (remoteView(remoteContext(s.rest)?.session ?? null)?.checkout.id ?? null) : (s.rest?.navigator?.focused_checkout_id ?? null),
  );
  const agents = useShellStore((s) => contextAgents(s.rest, s.agents));
  const openCheckouts = useShellStore((s) => s.rest?.ui_state?.expanded_checkout_ids ?? NO_IDS);
  const focusedPaneId = useShellStore((s) => s.focusedPaneId);
  const screenKind = useUiStore((s) => s.screen?.kind ?? null);
  const overviewProjectId = useUiStore((s) => (s.screen?.kind === "overview" ? s.screen.projectId : null));
  const catalogState = useShellStore((s) => catalogLineOf(s.rest)?.state ?? null);
  const catalogText = useShellStore((s) => catalogLineOf(s.rest)?.text ?? null);
  const catalogLine = catalogState && catalogText ? { state: catalogState, text: catalogText } : null;
  const rows = projectRows(workspaces, groups);
  const descendants = useMemo(() => liveDescendantCounts(agents), [agents]);
  const byPane = useMemo(() => new Map(agents.map((agent) => [agent.pane_id, agent])), [agents]);
  // The folds are this machine's choices, like the inactive groups, so a
  // selected SSH device's tree is drawn open with every checkout's line two.
  const context: ListContext = {
    agents,
    descendantsOf: (paneId) => descendants.get(paneId) ?? 0,
    childrenOf: (agent) => directChildren(agent, (paneId) => byPane.get(paneId)),
    focusedCheckoutId,
    focusedPaneId,
    workspaceScreen: screenKind === "workspace",
    overviewProjectId,
    openCheckouts,
    disclosure: !remote,
    actions,
  };
  if (!loaded) return <ListLoading />;
  return (
    <ul className="min-h-0 flex-1 overflow-auto px-xs" data-project-list="true">
      <AllProjectsRow selected={screenKind === "main"} />
      {catalogLine ? (
        <li role="status" className="px-md py-xs text-caption text-muted-foreground" data-device-catalog={catalogLine.state}>
          {catalogLine.text}
        </li>
      ) : null}
      {rows.map((row) => (
        <ProjectRowView key={rowKey(row)} row={row} context={context} />
      ))}
      {!remote && workspaces.length === 0 ? (
        <li className="flex flex-col items-start gap-sm px-sm py-sm text-caption text-muted-foreground" data-projects-empty="true">
          No project on this machine yet.
          <Button variant="secondary" size="sm" onClick={() => actions.openNewWorkspace()} data-projects-empty-add="true">
            Add project
          </Button>
        </li>
      ) : null}
    </ul>
  );
}

const NO_IDS: string[] = [];

/** What every row of the Projects list reads besides its own project. */
type ListContext = {
  agents: AgentRow[];
  /** Live descendants of a pane among the context device's agents. */
  descendantsOf: (paneId: string) => number;
  /** A pane's direct children still listed, for its chevron and badge. */
  childrenOf: (agent: AgentRow) => AgentRow[];
  focusedCheckoutId: string | null;
  focusedPaneId: string | null;
  /** A Workspace is in front, so the focused checkout and agent are the scope shown. */
  workspaceScreen: boolean;
  /** The project whose Overview is in front. */
  overviewProjectId: string | null;
  /** Checkouts whose agent rows the operator opened; every other checkout names its agents on line two. */
  openCheckouts: string[];
  /** False over a selected SSH device, whose tree is drawn with nothing folded. */
  disclosure: boolean;
  actions: Actions;
};

/** The top row: every project on every device, the scope All projects shows. */
function AllProjectsRow({ selected }: { selected: boolean }) {
  const count = useShellStore((s) => allProjectsCount(s.rest));
  return (
    <li>
      <button
        type="button"
        data-all-projects="true"
        aria-current={selected ? "page" : undefined}
        className={cn(
          "flex min-h-(--size-control-regular) w-full items-center gap-sm rounded-sm pr-xs pl-sm text-left outline-none focus-visible:ring-1 focus-visible:ring-inset focus-visible:ring-ring",
          selected ? "bg-secondary" : "hover:bg-accent",
        )}
        onClick={() => useUiStore.getState().setScreen({ kind: "main" })}
      >
        <LayoutGridIcon aria-hidden="true" className="size-(--size-checkout-icon) shrink-0 text-subtle-foreground" />
        <span className="min-w-0 flex-1 truncate text-title font-semibold text-foreground">All projects</span>
        <span className="shrink-0 text-body text-muted-foreground">
          {count} {count === 1 ? "project" : "projects"}
        </span>
        <Lane width="chevron" />
        <Lane width="slot" />
      </button>
    </li>
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

const ProjectRowView = memo(function ProjectRowView({ row, context }: { row: ProjectRow; context: ListContext }) {
  if (row.kind === "header") {
    return (
      <li className="px-sm pt-sm pb-xxs text-micro font-semibold uppercase text-muted-foreground" data-section={row.title}>
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

// The Projects columns (PRD sidebar-readability D-2): a project's glyph at the
// row's inset, a checkout's glyph one lineage step in, and an opened
// checkout's agent marks on the checkout name's column. Every control is on
// the right, in the same two slots on every row: the fold, then the `⋯`.
/** Where a project's glyph starts. */
const PROJECT_COLUMN = "var(--spacing-sm)";
/** Where a project's name, and a plain folder's agent marks, start. */
const PROJECT_NAME_COLUMN = "calc(var(--spacing-sm) + var(--size-checkout-icon) + var(--spacing-sm))";
/** Where a checkout's kind glyph, and its Inactive fold, start. */
const CHECKOUT_COLUMN = "calc(var(--spacing-sm) + var(--size-lineage-indent))";
/** Where a checkout's name, its line two and its agents' marks start. */
const CHECKOUT_NAME_COLUMN = "calc(var(--spacing-sm) + var(--size-lineage-indent) + var(--size-checkout-icon) + var(--spacing-sm))";

/**
 * A row control that waits for the pointer, inside a row `RowMenu` wraps: its
 * slot is always kept, and it shows under the pointer, with focus inside
 * the row, while the row's menu is open, and always on an input with no hover
 * (PRD sidebar-readability D-3, B2, B3). `REVEALED_CONTROL` in
 * sidebar-agent-row.tsx is the same rule for an agent row, which has no menu.
 */
const ROW_REVEALED =
  "opacity-0 group-hover:opacity-100 group-focus-within:opacity-100 group-has-[[data-menu-open]]:opacity-100 group-data-[state=open]:opacity-100 focus-visible:opacity-100 hoverless:opacity-100";

/** One disclosure for both inactive folds, the project level's and the checkout level's; its chevron stands in the fold slot. */
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
      className="flex min-h-(--size-control-lg) w-full items-center gap-sm rounded-sm pr-xs text-left text-caption font-medium text-subtle-foreground outline-none hover:bg-accent hover:text-foreground focus-visible:ring-1 focus-visible:ring-inset focus-visible:ring-ring"
      style={{ paddingLeft: level === "project" ? PROJECT_COLUMN : CHECKOUT_COLUMN }}
      onClick={onToggle}
      {...data}
    >
      <span className="min-w-0 flex-1 truncate">
        {label} {count}
      </span>
      <span className="flex w-(--size-lineage-chevron) shrink-0 justify-center text-muted-foreground">
        <Chevron aria-hidden="true" className="size-(--size-icon-sm)" />
      </span>
      <Lane width="slot" />
    </button>
  );
}

/** A slot for a row control that is not there, so every row's time and controls line up. */
function Lane({ width }: { width: "chevron" | "slot" }) {
  return <span aria-hidden="true" className={cn("shrink-0", width === "chevron" ? "w-(--size-lineage-chevron)" : "w-(--size-icon-button-toolbar)")} />;
}

/** The fold on a project or checkout row: a folded one is always shown, an open one waits for the pointer (D-3). */
function FoldToggle({ open, label, onToggle, ...data }: { open: boolean; label: string; onToggle: () => void } & Record<`data-${string}`, string>) {
  const Chevron = open ? ChevronDownIcon : ChevronRightIcon;
  return (
    <button
      type="button"
      aria-expanded={open}
      aria-label={label}
      className={cn(
        "pointer-events-auto relative flex h-(--size-icon-button-toolbar) w-(--size-lineage-chevron) shrink-0 items-center justify-center rounded-xs text-muted-foreground outline-none hover:text-foreground focus-visible:ring-1 focus-visible:ring-ring",
        open && ROW_REVEALED,
      )}
      onClick={onToggle}
      {...data}
    >
      <Chevron aria-hidden="true" className="size-(--size-icon-sm)" />
    </button>
  );
}

/** The `⋯` slot every project and checkout row keeps at its end. */
function MenuSlot({ trigger }: { trigger: ReactNode }) {
  return <span className="pointer-events-auto relative flex h-(--size-icon-button-toolbar) w-(--size-icon-button-toolbar) shrink-0">{trigger}</span>;
}

function WorkspaceRows({ workspace, level, context }: { workspace: Workspace; level: "root" | "child"; context: ListContext }) {
  const { agents, actions, disclosure } = context;
  const active = activeCheckouts(workspace);
  const inactive = inactiveCheckouts(workspace);
  const rowsByCheckout = useMemo(() => checkoutAgentRows(workspace, agents), [workspace, agents]);
  const expanded = !disclosure || workspace.expanded !== false;
  const inset = level === "child" ? "pl-(--size-lineage-indent)" : "";
  const folder = folderCheckout(workspace);
  if (folder) {
    return (
      <FolderRowView
        workspace={workspace}
        checkout={folder}
        agentRows={rowsByCheckout.get(folder.id) ?? NO_BOARD_ROWS}
        focused={(context.workspaceScreen && folder.id === context.focusedCheckoutId) || context.overviewProjectId === workspace.id}
        inset={inset}
        context={context}
      />
    );
  }
  const ProjectIcon = workspace.is_git ? FolderGit2Icon : FolderIcon;
  const selected = context.overviewProjectId === workspace.id;
  const checkoutRow = (checkout: Checkout) => (
    <CheckoutRowView
      key={checkout.id}
      workspace={workspace}
      checkout={checkout}
      agentRows={rowsByCheckout.get(checkout.id) ?? NO_BOARD_ROWS}
      focused={context.workspaceScreen && checkout.id === context.focusedCheckoutId}
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
          <div
            className={cn("flex min-h-(--size-control-regular) w-full items-center gap-sm rounded-sm pr-xs", selected ? "bg-secondary" : "hover:bg-accent")}
            style={{ paddingLeft: PROJECT_COLUMN }}
          >
            <button
              type="button"
              data-project-row={workspace.id}
              aria-current={selected ? "page" : undefined}
              className="flex min-w-0 flex-1 self-stretch items-center gap-sm rounded-xs text-left outline-none focus-visible:ring-1 focus-visible:ring-inset focus-visible:ring-ring"
              onClick={() => useUiStore.getState().setScreen({ kind: "overview", projectId: workspace.id })}
            >
              <ProjectIcon aria-hidden="true" className="size-(--size-checkout-icon) shrink-0 text-subtle-foreground" />
              <span className="min-w-0 flex-1 truncate text-title font-semibold text-foreground">{workspace.label}</span>
              <span className="shrink-0 text-caption text-muted-foreground" data-project-activity="true">
                {activityLabel(workspace, agents, Date.now())}
              </span>
            </button>
            {disclosure ? (
              <FoldToggle
                open={expanded}
                label={expanded ? `Fold ${workspace.label}` : `Unfold ${workspace.label}`}
                onToggle={() => actions.toggleProjectCheckouts(workspace)}
                data-project-toggle={workspace.id}
              />
            ) : (
              <Lane width="chevron" />
            )}
            <MenuSlot trigger={trigger} />
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
 * What a checkout's row draws besides line one, shared by the checkout row and
 * a folder's one row: its agent rows open only where agents run and the
 * operator opened them, and line two stands in for them while they are closed.
 */
function checkoutDisclosure(checkout: Checkout, agentRows: BoardRow[], view: CheckoutPresentation, context: ListContext) {
  const foldable = context.disclosure && agentRows.length > 0;
  const open = foldable && context.openCheckouts.includes(checkout.id);
  const purpose = checkout.purpose?.text ?? null;
  const secondLine = !open && view.secondLineReady && (view.agentCount > 0 || purpose !== null);
  return { foldable, open, purpose, secondLine };
}

/**
 * A checkout: the row opens it. On its right, in fixed slots, are the
 * last-commit age (always, where one is known), the chevron that opens its
 * agent rows (there only while agents run in it) and the `⋯` menu. Closed,
 * line two names its agents (the representative's mark and provider, +N for
 * the rest) and the purpose; opened, its rows and it share a small group fill.
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
  const { actions } = context;
  const purposeProblem = useShellStore((s) => remotePurposeProblem(workspace, s.rest?.status?.remote));
  const view = checkoutPresentation(workspace, checkout, Date.now());
  const name = checkout.branch ?? checkout.label;
  const { foldable, open, purpose, secondLine } = checkoutDisclosure(checkout, agentRows, view, context);
  const KindIcon = KIND_ICON[view.kind];
  return (
    <li data-checkout-row={checkout.id} data-checkout-open={open ? "true" : undefined} className={cn(open && "rounded-sm bg-muted")}>
      <RowMenu
        label={`${name} actions`}
        items={checkoutMenu(checkout, purposeProblem)}
        onSelect={(item) => runCheckoutItem(workspace, checkout, item)}
        data-checkout-menu={checkout.id}
      >
        {(trigger) => (
          <div
            className={cn(
              "relative flex w-full flex-col justify-center gap-xxs rounded-sm pr-xs",
              secondLine ? "min-h-(--size-checkout-row-detailed)" : "min-h-(--size-checkout-row)",
              focused ? "bg-secondary" : "hover:bg-accent",
              view.settled && "opacity-(--opacity-dimmed)",
            )}
            style={{ paddingLeft: CHECKOUT_COLUMN }}
          >
            <CheckoutOpenButton
              workspace={workspace}
              checkout={checkout}
              view={view}
              focused={focused}
              label={[name, view.age, purpose].filter(Boolean).join(", ")}
              actions={actions}
            />
            {/* The row button covers the whole row; a control drawn over it is positioned, so it stacks above. */}
            <span className="pointer-events-none flex min-w-0 items-center gap-sm">
              <KindIcon aria-hidden="true" className={cn("size-(--size-checkout-icon) shrink-0", view.kindTone)} />
              <span aria-hidden="true" className={cn("min-w-0 truncate text-subhead text-foreground", focused ? "font-semibold" : "font-medium")}>
                {name}
              </span>
              <CheckoutBadge checkout={checkout} />
              <span className="flex-1" />
              {view.age ? (
                <span aria-hidden="true" data-checkout-age={view.age} className="shrink-0 font-mono text-caption text-muted-foreground">
                  {view.age}
                </span>
              ) : null}
              {foldable ? (
                <FoldToggle
                  open={open}
                  label={open ? `Hide the agents in ${name}` : `Show the agents in ${name}`}
                  onToggle={() => actions.toggleCheckoutAgents(checkout.id)}
                  data-checkout-toggle={checkout.id}
                />
              ) : (
                <Lane width="chevron" />
              )}
              <MenuSlot trigger={trigger} />
            </span>
            {secondLine ? <CheckoutSummaryLine checkout={checkout} agentCount={view.agentCount} agents={context.agents} /> : null}
          </div>
        )}
      </RowMenu>
      {open ? <OpenAgentRows checkoutId={checkout.id} agentRows={agentRows} inset={CHECKOUT_NAME_COLUMN} context={context} /> : null}
    </li>
  );
});

/**
 * A plain folder (`folderCheckout`): the project and its only checkout are
 * one row. Line one is the project's name and activity, line two the
 * checkout's agents and purpose; the row opens the checkout and is marked
 * while that checkout is the Workspace in front or the project's Overview is,
 * and its menu is the project's followed by the checkout's. It keeps the
 * checkout row's right slots, and its Overview is reached from All projects.
 */
const FolderRowView = memo(function FolderRowView({
  workspace,
  checkout,
  agentRows,
  focused,
  inset,
  context,
}: {
  workspace: Workspace;
  checkout: Checkout;
  agentRows: BoardRow[];
  focused: boolean;
  inset: string;
  context: ListContext;
}) {
  const { actions } = context;
  const purposeProblem = useShellStore((s) => remotePurposeProblem(workspace, s.rest?.status?.remote));
  const view = checkoutPresentation(workspace, checkout, Date.now());
  const activity = activityLabel(workspace, context.agents, Date.now());
  const { foldable, open, purpose, secondLine } = checkoutDisclosure(checkout, agentRows, view, context);
  return (
    <li data-project={workspace.id} data-checkout-open={open ? "true" : undefined} className={cn(inset, open && "rounded-sm bg-muted")}>
      <RowMenu
        label={`${workspace.label} actions`}
        items={folderMenu(workspace, checkout, purposeProblem)}
        onSelect={(item) => (item === "set_purpose" || item === "delete_worktree" ? runCheckoutItem(workspace, checkout, item) : runProjectItem(actions, workspace, item))}
        data-project-menu={workspace.id}
      >
        {(trigger) => (
          <div
            className={cn(
              "relative flex w-full flex-col justify-center gap-xxs rounded-sm pr-xs",
              secondLine ? "min-h-(--size-checkout-row-detailed)" : "min-h-(--size-control-regular)",
              focused ? "bg-secondary" : "hover:bg-accent",
            )}
            style={{ paddingLeft: PROJECT_COLUMN }}
          >
            <CheckoutOpenButton
              workspace={workspace}
              checkout={checkout}
              view={view}
              focused={focused}
              label={[workspace.label, activity, purpose].filter(Boolean).join(", ")}
              actions={actions}
            />
            <span className="pointer-events-none flex min-w-0 items-center gap-sm">
              <FolderIcon aria-hidden="true" className={cn("size-(--size-checkout-icon) shrink-0", view.kindTone)} />
              <span aria-hidden="true" className="min-w-0 truncate text-title font-semibold text-foreground">
                {workspace.label}
              </span>
              <CheckoutBadge checkout={checkout} />
              <span className="flex-1" />
              {/* The activity stands where a checkout's age does. */}
              <span aria-hidden="true" className="shrink-0 text-caption text-muted-foreground" data-project-activity="true">
                {activity}
              </span>
              {foldable ? (
                <FoldToggle
                  open={open}
                  label={open ? `Hide the agents in ${workspace.label}` : `Show the agents in ${workspace.label}`}
                  onToggle={() => actions.toggleCheckoutAgents(checkout.id)}
                  data-checkout-toggle={checkout.id}
                />
              ) : (
                <Lane width="chevron" />
              )}
              <MenuSlot trigger={trigger} />
            </span>
            {secondLine ? <CheckoutSummaryLine checkout={checkout} agentCount={view.agentCount} agents={context.agents} /> : null}
          </div>
        )}
      </RowMenu>
      {open ? <OpenAgentRows checkoutId={checkout.id} agentRows={agentRows} inset={PROJECT_NAME_COLUMN} context={context} /> : null}
    </li>
  );
});

/** The button under a checkout's whole row: it opens the checkout, and its tooltip carries the pull request, agents, branch and path. */
function CheckoutOpenButton({
  workspace,
  checkout,
  view,
  focused,
  label,
  actions,
}: {
  workspace: Workspace;
  checkout: Checkout;
  view: CheckoutPresentation;
  focused: boolean;
  label: string;
  actions: Actions;
}) {
  return (
    <Hint label={view.detail}>
      <button
        type="button"
        data-checkout={checkout.id}
        data-checkout-kind={view.kind}
        aria-current={focused ? "true" : undefined}
        aria-label={label}
        className="absolute inset-0 rounded-sm outline-none focus-visible:ring-1 focus-visible:ring-inset focus-visible:ring-ring"
        onClick={() => actions.openWorkspace(workspace.device_id, checkout.workspace_id, checkout.id)}
      />
    </Hint>
  );
}

function CheckoutBadge({ checkout }: { checkout: Checkout }) {
  if (!checkout.exists) {
    return (
      <Badge variant="secondary" className="shrink-0 text-destructive" data-checkout-missing="true">
        missing
      </Badge>
    );
  }
  if (checkout.temporary) {
    return (
      <Badge variant="secondary" className="shrink-0 text-warning">
        temporary
      </Badge>
    );
  }
  return null;
}

/** A closed checkout's line two: the representative agent's mark and provider, +N for the rest, and the purpose. */
function CheckoutSummaryLine({ checkout, agentCount, agents }: { checkout: Checkout; agentCount: number; agents: AgentRow[] }) {
  const representative = agents.find((agent) => agent.pane_id === checkout.agent_summary?.representative_pane_id) ?? null;
  const purpose = checkout.purpose?.text ?? null;
  return (
    // The row is already inset to its glyph, so line two starts one glyph and gap further, under the name.
    <span aria-hidden="true" className="pointer-events-none flex min-w-0 items-center gap-xs pr-md pl-(--size-checkout-metadata-inset) text-caption">
      {agentCount > 0 ? (
        <span className="flex shrink-0 items-center gap-xs" data-checkout-agents={agentCount}>
          {representative ? <StatusMark symbol={representative.symbol} className={markTone(representative)} /> : null}
          <AgentMark kind={representative?.agent_kind} />
          {agentCount > 1 ? <span className="font-mono text-subtle-foreground">+{agentCount - 1}</span> : null}
        </span>
      ) : null}
      {purpose ? (
        <span className="min-w-0 truncate text-muted-foreground" data-purpose={checkout.purpose?.origin}>
          {purpose}
        </span>
      ) : null}
    </span>
  );
}

/**
 * An opened checkout's agent rows, their marks on the name column. A parent
 * folds its descendants with the core's own lineage choice, the one the
 * Agents list folds by, and its badge speaks for them while folded (PRD
 * sidebar-readability D-6, B12). A selected SSH device's tree is drawn with
 * nothing folded, as its checkouts are.
 */
function OpenAgentRows({ checkoutId, agentRows, inset, context }: { checkoutId: string; agentRows: BoardRow[]; inset: string; context: ListContext }) {
  const rows = context.disclosure ? unfoldedRows(agentRows) : agentRows;
  return (
    <ul data-checkout-agents-open={checkoutId} className="pb-xs">
      {rows.map((row) => (
        <SidebarAgentRow
          key={row.agent.pane_id}
          agent={row.agent}
          device={null}
          place={null}
          depth={row.depth}
          descendants={context.disclosure ? context.descendantsOf(row.agent.pane_id) : 0}
          childRows={context.disclosure ? context.childrenOf(row.agent) : NO_AGENT_ROWS}
          selected={context.workspaceScreen && row.agent.pane_id === context.focusedPaneId}
          onOpen={context.actions.openAgent}
          onToggleTree={context.disclosure ? context.actions.toggleAgentTree : null}
          inset={inset}
          branchShown={row.depth > 0}
        />
      ))}
    </ul>
  );
}

/** A tree drawn with nothing folded lists no folded children. */
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
