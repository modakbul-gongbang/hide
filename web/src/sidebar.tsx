import { CheckIcon, ChevronDownIcon, ChevronRightIcon, CornerUpLeftIcon, FolderGit2Icon, FolderIcon, HouseIcon, LaptopIcon, LayoutDashboardIcon, Loader2Icon, PlusIcon, RefreshCwIcon, ServerIcon, SettingsIcon } from "lucide-react";
import { memo, useEffect, useMemo, useRef, useState, type CSSProperties, type ReactNode } from "react";
import type { Actions } from "./actions";
import { Button } from "./components/ui/button";
import { EntryContextMenu } from "./components/entry-menu";
import { SidebarHeader } from "./components/sidebar-header";
import { Hint } from "./components/ui/tooltip";
import { CHECKOUT_KIND_ICON } from "./components/checkout-icon";
import { CheckoutCardHint, pullRequestOpenExternal } from "./components/pr-card";
import { DeviceRail } from "./components/device-rail";
import { frontDeviceId, frontTitle, homeProjectCount, railVisible, sidebarBody } from "./devices";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from "./components/ui/dropdown-menu";
import { badgeWords, unfoldedRows } from "./agentRow";
import { Badge } from "./components/ui/badge";
import { cn } from "./lib/utils";
import { agentNumber, listedAgentOrder, numberOf, numberedAgents, projectListNumbers } from "./numbering";
import { checkoutAgentRows, type BoardRow } from "./projectBoard";
import { FoldLane, SidebarAgentRow, type AgentRowMenu } from "./components/sidebar-agent-row";
import { StatusBadge } from "./components/status-badge";
import { WeeklyUsage } from "./components/weekly-usage";
import { agentPlaces, agentSections, allAgents, allLineageWorkspaces, overviewScreen, projectPaneIds, type ListedAgent } from "./navigation";
import { foldedLineage, type FoldedLineage } from "./lineageSummary";
import {
  activeCheckouts,
  checkoutCard,
  checkoutHasSecondLine,
  checkoutNameParts,
  checkoutPresentation,
  checkoutRowExpansion,
  projectRowExpansion,
  overviewRowSelected,
  folderCheckout,
  inactiveCheckouts,
  projectMarks,
  projectRows,
  shownPullRequest,
  type CheckoutPresentation,
  type ProjectRow,
} from "./projects";
import { hostBridge, hostKind } from "./host";
import { displayCommand, hostRegistry, type Digit } from "./shortcuts";
import { contextAgents, contextHome, contextWorkspaces, deviceCatalogLine, remoteContext, remoteView } from "./remote";
import { agentMenu, checkoutMenu, checkoutRemoving, FOLDER_CHECKOUT_ITEMS, folderMenu, primaryCheckout, projectMenu, remotePurposeProblem, type MenuHost, type MenuItem } from "./workspaceManage";
import { focusedRemoteDevice, type AgentRow, type Checkout, type InactiveProjectGroup, type SnapshotRest, type Workspace } from "./snapshot";
import { useShellStore } from "./store";
import { draggedSidebarWidth, sidebarWidthToSend } from "./sidebarWidth";
import { useUiStore } from "./ui";

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
  // The rail exists while any remote device is registered; the Inbox is its selection beside the devices (PRD home-device-rail D-10, D-11).
  const rail = useShellStore((s) => railVisible(s.rest));
  const inboxSelected = useUiStore((s) => s.inbox);
  const body = useShellStore((s) => sidebarBody(s.rest, inboxSelected, mode));
  const inbox = body === "inbox";
  const devices = useShellStore((s) => s.rest?.navigator?.devices);
  const frontId = useShellStore((s) => s.rest?.navigator?.focused_device_id);
  const title = useMemo(() => frontTitle(devices, frontId, inbox), [devices, frontId, inbox]);
  useHomeStart();
  // The switch has no chord of its own until the operator binds one (issue 170).
  const switchChord = useShellStore((s) => displayCommand("toggle_sidebar_view", hostKind(), hostRegistry(s.rest?.ui_state, hostKind()).registry));
  const storedWidth = useShellStore((s) => s.rest?.ui_state?.sidebar_width ?? null);
  // A drag draws the nav alone at the width under the pointer, over the
  // center, so nothing beside it (a terminal above all) reflows per move; the
  // box beside the center takes the width once the core carries it.
  const [preview, setPreview] = useState<number | null>(null);
  const dragging = useRef(false);
  useEffect(() => {
    if (!dragging.current) setPreview(null);
  }, [storedWidth]);
  if (!visible) return null;
  const land = (current: number, landed: number) => {
    const send = sidebarWidthToSend(storedWidth ?? current, landed);
    if (send === null) setPreview(null);
    else actions.setSidebarWidth(send);
  };
  const home = <HomeSection actions={actions} />;
  // With the rail, a device's sidebar is its Needs You, its Home and its Projects, with no tabs; the Inbox lists every device's agents;
  // a device that is not connected shows its name and the way to reconnect, and no stale tree (B2, B4, B8).
  const list =
    body === "inbox" || body === "agents" ? (
      <AgentList actions={actions} />
    ) : body === "disconnected" ? (
      <DisconnectedDevice actions={actions} />
    ) : (
      <ProjectList actions={actions} home={rail ? home : null} />
    );

  return (
    // The width is a CSS variable, never the nav's own inline width, which
    // the e2e overflow check sets to try other widths (PRD sidebar-typography D-13).
    <div
      className="relative h-full w-(--sidebar-total) shrink-0"
      // The stored width is the content column's; the rail is an extra fixed column to its left, only while it shows (PRD home-device-rail D-09).
      style={
        {
          "--sidebar-width": storedWidth === null ? "var(--size-sidebar-ideal)" : `${storedWidth}px`,
          "--sidebar-rail": rail ? "var(--size-rail)" : "var(--spacing-none)",
          "--sidebar-total": "calc(var(--sidebar-width) + var(--sidebar-rail))",
        } as CSSProperties
      }
      data-sidebar-box="true"
    >
      {/* The sidebar keeps its own text sizes whatever Appearance's interface
          font is (D-05): `interface-scale-exempt` is the generated rule in
          tokens.css that declares the sizes again at scale 1. */}
      <nav
        className={cn("interface-scale-exempt absolute inset-y-0 left-0 flex w-(--sidebar-total) flex-col bg-sidebar text-foreground", preview !== null && "z-30")}
        style={
          preview === null
            ? undefined
            : ({ "--sidebar-width": `${preview}px`, "--sidebar-total": "calc(var(--sidebar-width) + var(--sidebar-rail))" } as CSSProperties)
        }
        data-sidebar={mode}
        data-sidebar-rail={rail ? (inbox ? "inbox" : "device") : "none"}
      >
        <SidebarHeader
          rail={rail}
          title={title}
          addProject={!inbox}
          mode={mode}
          home={home}
          switchChord={switchChord || null}
          searchChord={displayCommand("search", hostKind()) || null}
          newWorkspaceChord={displayCommand("new_workspace", hostKind()) || null}
          onMode={actions.showSidebarMode}
          onSearch={() => actions.openSearch()}
          onNewWorkspace={hostBridge() ? () => actions.openAddProject() : null}
        />
        {status ? (
          <div className="border-b border-border px-md py-sm text-caption text-muted-foreground">{status}</div>
        ) : null}
        <div className="flex min-h-0 flex-1">
          {rail ? <DeviceRail actions={actions} /> : null}
          <div className="flex min-h-0 min-w-0 flex-1 flex-col" data-sidebar-content="true">
            {list}
            <div className="flex shrink-0 items-center gap-xs border-t border-border px-md py-xs">
              {rail ? null : <FooterDeviceButton actions={actions} />}
              <span className="flex-1" />
              <WeeklyUsage actions={actions} />
              <Hint label={`Settings (${displayCommand("settings", hostKind())})`}>
                <Button variant="ghost" size="icon-sm" data-open-settings="true" onClick={() => actions.openSettings()}>
                  <SettingsIcon />
                </Button>
              </Hint>
            </div>
          </div>
        </div>
        <SidebarEdge
          onBegin={(start) => {
            dragging.current = true;
            setPreview(start);
          }}
          onPreview={setPreview}
          onLand={(start, landed) => {
            dragging.current = false;
            land(start, landed);
          }}
          onCancel={() => {
            dragging.current = false;
            setPreview(null);
          }}
          onReset={(current) => land(current, tokenPx("--size-sidebar-ideal"))}
        />
      </nav>
    </div>
  );
}

/**
 * The Home row's `+` starts a terminal tab in the device's Home and answers
 * with its pane under the request id it sent; that pane is opened once it is
 * listed, and a refusal (a `~/hide` that is not Hide's, a busy core) is shown
 * under that Home row, where the operator opened it (PRD home-device-rail D-13, B21).
 */
function useHomeStart() {
  const request = useUiStore((s) => s.homeStart);
  const operation = useShellStore((s) => s.rest?.task_operation);
  const error = useShellStore((s) => s.rest?.status?.last_error);
  useEffect(() => {
    if (!request || request.refusal !== null) return;
    const ui = useUiStore.getState();
    const id = request.requestId;
    if (operation?.request_id === id && operation.pane_id && operation.phase !== "failed") {
      ui.setHomeStart(null);
      ui.setFocusWhenListed(operation.pane_id);
      return;
    }
    const refused = error?.request_id === id ? error : operation?.request_id === id && operation.phase === "failed" ? { message: operation.message ?? "Home did not open." } : null;
    if (refused) ui.setHomeStart({ ...request, refusal: refused.message });
  }, [request, operation, error]);
}

function tokenPx(name: string): number {
  const value = Number.parseFloat(getComputedStyle(document.documentElement).getPropertyValue(name));
  if (!Number.isFinite(value)) throw new Error(`token ${name} is not a length`);
  return value;
}

/**
 * The sidebar's right edge (PRD sidebar-typography B10-B12): the pane
 * divider's part, a `--size-resize-grab` hit area over the edge and a
 * `--size-resize-handle` line in the accent under the pointer and while
 * dragging. The edge follows the pointer between `--size-sidebar-min` and
 * `--size-sidebar-max` and lands once on release; a double-click returns it
 * to `--size-sidebar-ideal`. Widths are measured on the content column (the nav less the rail), so the start is
 * what the operator sees.
 */
function SidebarEdge({
  onBegin,
  onPreview,
  onLand,
  onCancel,
  onReset,
}: {
  onBegin: (start: number) => void;
  onPreview: (width: number) => void;
  onLand: (start: number, landed: number) => void;
  onCancel: () => void;
  onReset: (current: number) => void;
}) {
  const drag = useRef<{ start: number; x: number; landed: number } | null>(null);
  const [active, setActive] = useState(false);
  // The width the operator sets is the content column's: the nav less the rail column while one shows.
  const navWidth = (edge: HTMLElement) => {
    const nav = edge.parentElement!;
    return Math.round(nav.getBoundingClientRect().width - (nav.querySelector("[data-device-rail]")?.getBoundingClientRect().width ?? 0));
  };
  return (
    <div
      role="separator"
      aria-orientation="vertical"
      aria-label="Resize sidebar"
      data-sidebar-edge="true"
      className="group/edge absolute inset-y-0 z-30 flex w-(--size-resize-grab) cursor-col-resize justify-center"
      style={{ right: "calc(var(--size-resize-grab) / -2)" }}
      onPointerDown={(event) => {
        if (event.button !== 0) return;
        event.currentTarget.setPointerCapture(event.pointerId);
        const start = navWidth(event.currentTarget);
        drag.current = { start, x: event.clientX, landed: start };
        setActive(true);
        onBegin(start);
      }}
      onPointerMove={(event) => {
        const current = drag.current;
        if (!current) return;
        current.landed = draggedSidebarWidth(current.start, event.clientX - current.x, {
          min: tokenPx("--size-sidebar-min"),
          max: tokenPx("--size-sidebar-max"),
        });
        onPreview(current.landed);
      }}
      onPointerUp={(event) => {
        const current = drag.current;
        if (!current) return;
        event.currentTarget.releasePointerCapture(event.pointerId);
        drag.current = null;
        setActive(false);
        onLand(current.start, current.landed);
      }}
      onPointerCancel={() => {
        drag.current = null;
        setActive(false);
        onCancel();
      }}
      onDoubleClick={(event) => onReset(navWidth(event.currentTarget))}
    >
      <span aria-hidden="true" className={cn("h-full w-(--size-resize-handle) bg-primary opacity-0 group-hover/edge:opacity-100", active && "opacity-100")} />
    </div>
  );
}

/**
 * The Home row (PRD home-device-rail D-13, B16, B17), at every device count:
 * `Home` and how many projects the device has registered, its Home not among
 * them. It opens the device's all-projects Overview and is marked while that
 * screen is in front. The agents running in the Home are its child rows, and
 * a `+` waiting for the pointer starts a new tab there. It is drawn before the
 * device has a Home folder, since the count comes from its registrations and
 * the folder is made by the first start.
 */
const HomeSection = memo(function HomeSection({ actions }: { actions: Actions }) {
  const deviceId = useShellStore((s) => frontDeviceId(s.rest));
  const count = useShellStore((s) => (s.rest === null ? null : homeProjectCount(s.rest, deviceId)));
  // A Home Overview names its device, or follows the one in front when it names none.
  const screenDevice = useUiStore((s) => (s.screen?.kind === "main" ? (s.screen.deviceId ?? "") : null));
  const selected = screenDevice !== null && (screenDevice === "" || screenDevice === deviceId);
  const workspaceScreen = useUiStore((s) => s.screen?.kind === "workspace");
  const home = useShellStore((s) => contextHome(s.rest));
  const agents = useShellStore((s) => contextAgents(s.rest, s.agents));
  const localWorkspaces = useShellStore((s) => s.rest?.navigator?.workspaces);
  const remoteStatuses = useShellStore((s) => s.rest?.status?.remote);
  const focusedPaneId = useShellStore((s) => s.focusedPaneId);
  const lineageWorkspaces = useMemo(() => allLineageWorkspaces(localWorkspaces, remoteStatuses), [localWorkspaces, remoteStatuses]);
  const roots = useMemo(() => {
    if (!home) return NO_HOME_AGENTS;
    const panes = projectPaneIds(home);
    return agents.filter((agent) => panes.has(agent.pane_id) && !agent.delegated);
  }, [home, agents]);
  const presentations = useMemo(() => new Map(roots.map((agent) => [agent.pane_id, foldedLineage(agent, agents, lineageWorkspaces)])), [roots, agents, lineageWorkspaces]);
  const menu = useAgentRowMenu(actions);
  const refusal = useUiStore((s) => (s.homeStart?.deviceId === deviceId ? s.homeStart.refusal : null));
  return (
    <li data-home={deviceId}>
      <div className="group relative flex items-stretch">
        <button
          type="button"
          data-home-destination="true"
          aria-current={selected ? "page" : undefined}
          className={cn(
            "flex min-h-(--size-project-row) w-full items-center gap-sm rounded-sm pr-xs pl-sm text-left outline-none focus-visible:ring-1 focus-visible:ring-inset focus-visible:ring-ring",
            selected ? "bg-secondary" : "hover:bg-accent",
          )}
          onClick={() => {
            const ui = useUiStore.getState();
            if (refusal !== null) ui.setHomeStart(null);
            ui.setScreen({ kind: "main", deviceId });
          }}
        >
          <HouseIcon aria-hidden="true" className="size-(--size-checkout-icon) shrink-0 text-subtle-foreground" />
          <span className="min-w-0 flex-1 truncate text-subhead font-semibold text-foreground">Home</span>
          <span className="flex shrink-0 items-center gap-xs">
            {count === null ? null : (
              <span className="text-body text-muted-foreground" data-home-count="true">
                {count} {count === 1 ? "project" : "projects"}
              </span>
            )}
            <FoldLane />
          </span>
        </button>
        <Hint label="New tab in Home">
          <button
            type="button"
            aria-label="New tab in Home"
            data-home-new-tab="true"
            className={cn(
              "absolute inset-y-0 right-xs my-auto flex h-(--size-icon-button-toolbar) w-(--size-lineage-chevron) items-center justify-center rounded-xs text-subtle-foreground outline-none hover:text-foreground focus-visible:ring-1 focus-visible:ring-ring",
              ROW_REVEALED,
            )}
            onClick={() => actions.startHomeTab(deviceId)}
          >
            <PlusIcon aria-hidden="true" className="size-(--size-icon-sm)" />
          </button>
        </Hint>
      </div>
      {refusal === null ? null : (
        <p role="alert" data-home-refusal="true" className="pr-sm pb-xs text-caption break-words text-destructive" style={{ paddingLeft: CHECKOUT_COLUMN }}>
          {refusal}
        </p>
      )}
      {roots.length > 0 ? (
        <ul aria-label="Home agents" data-home-agents="true">
          {roots.map((agent) => {
            const presentation = presentations.get(agent.pane_id)!;
            return (
              <SidebarAgentRow
                key={agent.id}
                agent={agent}
                device={null}
                place={null}
                depth={0}
                descendants={presentation.badgeDescendants}
                childRows={presentation.badgeChildren}
                selected={workspaceScreen && agent.pane_id === focusedPaneId}
                onOpen={actions.openAgent}
                onToggleTree={null}
                inset={CHECKOUT_COLUMN}
                foldedLineage={presentation}
                number={null}
                menu={menu}
              />
            );
          })}
        </ul>
      ) : null}
    </li>
  );
});

const NO_HOME_AGENTS: AgentRow[] = [];

/** A selected device the core cannot read: its name, why, and the one way to try again, with the last tree left out (PRD home-device-rail B8, B9). */
function DisconnectedDevice({ actions }: { actions: Actions }) {
  const deviceId = useShellStore((s) => frontDeviceId(s.rest));
  const label = useShellStore((s) => s.rest?.navigator?.devices?.find((device) => device.id === frontDeviceId(s.rest))?.label ?? frontDeviceId(s.rest));
  return (
    <div className="flex min-h-0 flex-1 flex-col items-center justify-center gap-sm px-md text-center" data-device-disconnected={deviceId}>
      <ServerIcon aria-hidden="true" className="size-(--size-icon-lg) text-subtle-foreground" />
      <span className="max-w-full truncate text-subhead font-semibold text-foreground" data-device-disconnected-name="true">
        {label}
      </span>
      <span role="status" className="text-caption text-muted-foreground">
        연결 안 됨
      </span>
      <Button variant="secondary" size="sm" data-device-reconnect={deviceId} onClick={() => actions.retryDevice(deviceId)}>
        <RefreshCwIcon aria-hidden="true" />
        다시 연결
      </Button>
    </div>
  );
}

/**
 * With no remote device the footer's left button opens This Mac and Add
 * device (PRD home-device-rail B11); the first registration removes it and
 * shows the rail, whose `+` does the same Add device.
 */
function FooterDeviceButton({ actions }: { actions: Actions }) {
  return (
    <DropdownMenu>
      <Hint label="Devices">
        <DropdownMenuTrigger asChild>
          <Button variant="ghost" size="icon-sm" aria-label="Devices" data-footer-device="true">
            <LaptopIcon />
          </Button>
        </DropdownMenuTrigger>
      </Hint>
      <DropdownMenuContent align="start" side="top" aria-label="Devices" data-footer-device-menu="true">
        <DropdownMenuItem data-footer-device-this="true">
          <LaptopIcon aria-hidden="true" />
          <span className="min-w-0 flex-1 truncate">This Mac · 이 기기</span>
          <CheckIcon aria-hidden="true" className="size-(--size-icon-sm) shrink-0 text-foreground" />
        </DropdownMenuItem>
        <DropdownMenuItem data-footer-device-add="true" onSelect={() => actions.openAddDevice()}>
          <PlusIcon aria-hidden="true" />
          기기 추가…
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
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
  const lineageWorkspaces = useMemo(() => allLineageWorkspaces(workspaces, remote), [workspaces, remote]);
  const tree = useMemo(() => agentTree(listed, lineageWorkspaces), [listed, lineageWorkspaces]);
  const placeOf = useMemo(() => agentPlaces(workspaces, remote, devices), [workspaces, remote, devices]);
  const focusedPaneId = useShellStore((s) => s.focusedPaneId);
  // An ⌥ hold numbers the drawn rows top to bottom (PRD
  // electron-digit-shortcuts-hints B5); the numbers exist only while it
  // shows, so the memoized rows are untouched by an unrevealed hold.
  const numbered = useUiStore((s) => s.hint === "agents");
  const numbers = useMemo(() => (numbered ? numberedAgents(tree.sections.flatMap((section) => section.rows)) : null), [numbered, tree]);
  const menu = useAgentRowMenu(actions);
  // Before the first snapshot nothing is known, so an empty list would be a claim.
  if (!loaded) return <ListLoading />;
  if (tree.sections.length === 0) {
    return <div className="min-h-0 flex-1 px-md py-sm text-caption text-muted-foreground" data-agents-empty="true">No agents are running</div>;
  }
  return (
    <ul className="min-h-0 flex-1 overflow-auto px-xs" data-agent-list="true">
      {tree.sections.map((section) => (
        <li key={section.group} data-agent-group={section.group}>
          <div className="px-sm pb-xxs pt-sm text-micro font-medium text-muted-foreground" id={`agent-group-${section.group}`}>
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
                childRows={tree.presentation(row.agent).badgeChildren}
                selected={row.agent.pane_id === focusedPaneId}
                onOpen={actions.openAgent}
                onToggleTree={actions.toggleAgentTree}
                inset="var(--spacing-xs)"
                foldedLineage={tree.presentation(row.agent)}
                number={numbers ? numberOf(numbers, row.agent.pane_id) : null}
                menu={menu}
              />
            ))}
          </ul>
        </li>
      ))}
    </ul>
  );
}

/**
 * The sections and rows each draws from the core's one global lineage.
 * Remote pane ids are already scoped, while machine identities can connect
 * a child to a parent on another device.
 */
function agentTree(listed: ListedAgent[], workspaces: Workspace[]) {
  const index = new Map(listed.map((row) => [row.agent.pane_id, row]));
  const agents = listed.map((row) => row.agent);
  const presentations = new Map(agents.map((agent) => [agent.pane_id, foldedLineage(agent, agents, workspaces)]));
  const allDescendants = (agent: AgentRow) => {
    const seen = new Set<string>();
    const queue = [...(agent.lineage_child_pane_ids ?? [])];
    while (queue.length > 0) {
      const paneId = queue.shift()!;
      if (seen.has(paneId) || paneId === agent.pane_id) continue;
      const child = index.get(paneId)?.agent;
      if (!child) continue;
      seen.add(paneId);
      queue.push(...(child.lineage_child_pane_ids ?? []));
    }
    return seen.size;
  };
  const sections = agentSections(listed.map((row) => row.agent))
    .map((section) => {
      const roots = section.agents.filter((agent) => !agent.delegated);
      const rows: { agent: AgentRow; device: string | null; depth: number; descendants: number }[] = [];
      const visit = (agent: AgentRow, depth: number, seen: Set<string>) => {
        if (seen.has(agent.pane_id)) return;
        seen.add(agent.pane_id);
        const listedAgent = index.get(agent.pane_id);
        const presentation = presentations.get(agent.pane_id)!;
        rows.push({ agent, device: listedAgent?.device ?? null, depth, descendants: presentation.badgeDescendants });
        if (agent.lineage_collapsed !== false) return;
        for (const paneId of agent.lineage_child_pane_ids ?? []) {
          const child = index.get(paneId)?.agent;
          if (child) visit(child, depth + 1, seen);
        }
      };
      for (const root of roots) visit(root, 0, new Set());
      const count = roots.reduce((total, root) => total + 1 + allDescendants(root), 0);
      return { group: section.group, label: section.label, rows, count };
    })
    .filter((section) => section.rows.length > 0);
  return {
    sections,
    presentation: (agent: AgentRow) => presentations.get(agent.pane_id)!,
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
 * The scope picker below the Home row: the projects of the context on
 * screen. A selected SSH device lists its Herdr workspaces, one checkout
 * each; the inactive folds and pins are this machine's and are not drawn
 * there (docs/UI_BEHAVIOR.md: the remote context carries no pins). The row of
 * the scope the center shows is marked: a project on its Overview, or the
 * focused checkout and its agent while a Workspace is in front.
 */
function ProjectList({ actions, home }: { actions: Actions; home: ReactNode }) {
  const loaded = useShellStore((s) => s.rest !== null);
  const remote = useShellStore((s) => focusedRemoteDevice(s.rest) !== null);
  const workspaces = useShellStore((s) => contextWorkspaces(s.rest));
  const groups = useShellStore((s) => (remote ? NO_GROUPS : (s.rest?.navigator?.inactive_projects ?? NO_GROUPS)));
  const focusedCheckoutId = useShellStore((s) =>
    remote ? (remoteView(remoteContext(s.rest)?.session ?? null)?.checkout.id ?? null) : (s.rest?.navigator?.focused_checkout_id ?? null),
  );
  const localWorkspaces = useShellStore((s) => s.rest?.navigator?.workspaces);
  const remoteStatuses = useShellStore((s) => s.rest?.status?.remote);
  const devices = useShellStore((s) => s.rest?.navigator?.devices);
  const localAgents = useShellStore((s) => s.agents);
  const listedAgents = useMemo(() => allAgents(remoteStatuses, devices, localAgents), [remoteStatuses, devices, localAgents]);
  const agents = useMemo(() => listedAgents.map((row) => row.agent), [listedAgents]);
  const lineageWorkspaces = useMemo(() => allLineageWorkspaces(localWorkspaces, remoteStatuses), [localWorkspaces, remoteStatuses]);
  const openCheckouts = useShellStore((s) => s.rest?.ui_state?.expanded_checkout_ids ?? NO_IDS);
  const focusedPaneId = useShellStore((s) => s.focusedPaneId);
  const screenKind = useUiStore((s) => s.screen?.kind ?? null);
  const overviewProjectId = useUiStore((s) => (s.screen?.kind === "overview" ? s.screen.projectId : null));
  const catalogState = useShellStore((s) => catalogLineOf(s.rest)?.state ?? null);
  const catalogText = useShellStore((s) => catalogLineOf(s.rest)?.text ?? null);
  const catalogLine = catalogState && catalogText ? { state: catalogState, text: catalogText } : null;
  const homeWorkspace = useShellStore((s) => contextHome(s.rest));
  const rows = useMemo(() => projectRows(workspaces, groups, listedAgents, homeWorkspace), [workspaces, groups, listedAgents, homeWorkspace]);
  // The device's Needs You and Done come first, then its Home, then the projects (PRD home-device-rail B6).
  // With the rail only Needs You is kept above the Home row; Done stays with the tab-less one-device sidebar (D-12).
  const raised = useMemo(() => rows.filter((row) => row.kind === "raised" && (home === null || row.group === "needs_you")), [rows, home]);
  const tree = useMemo(() => rows.filter((row) => row.kind !== "raised"), [rows]);
  const agentRowMenu = useAgentRowMenu(actions);
  const presentations = useMemo(() => new Map(agents.map((agent) => [agent.pane_id, foldedLineage(agent, agents, lineageWorkspaces)])), [agents, lineageWorkspaces]);
  const placeOf = useMemo(() => agentPlaces(localWorkspaces, remoteStatuses, devices), [localWorkspaces, remoteStatuses, devices]);
  // An ⌥ hold shows each agent's number once, on its raised row or else its
  // own checkout's row; ⌥n selects by the Agents list, so the number is that list's.
  const numbered = useUiStore((s) => s.hint === "agents");
  const numberOfPane = useMemo(
    () => (numbered ? projectListNumbers(numberedAgents(listedAgentOrder(listedAgents)), rows, workspaces) : null),
    [numbered, listedAgents, rows, workspaces],
  );
  // The folds are this machine's choices, like the inactive groups, so a
  // selected SSH device's tree is drawn open with every checkout's line two.
  const context: ListContext = {
    agents,
    presentationOf: (agent) => presentations.get(agent.pane_id)!,
    placeOf,
    numberOf: numberOfPane,
    focusedCheckoutId,
    focusedPaneId,
    workspaceScreen: screenKind === "workspace",
    overviewProjectId,
    openCheckouts,
    disclosure: !remote,
    actions,
    agentRowMenu,
  };
  if (!loaded) return <ListLoading />;
  return (
    <ul className="min-h-0 flex-1 overflow-auto px-xs" data-project-list="true">
      {catalogLine ? (
        <li role="status" className="px-md py-xs text-caption text-muted-foreground" data-device-catalog={catalogLine.state}>
          {catalogLine.text}
        </li>
      ) : null}
      {raised.map((row) => (
        <ProjectRowView key={rowKey(row)} row={row} context={context} />
      ))}
      {home}
      {tree.map((row) => (
        <ProjectRowView key={rowKey(row)} row={row} context={context} />
      ))}
      {!remote && workspaces.length === 0 ? (
        <li className="flex flex-col items-start gap-sm px-sm py-sm text-caption text-muted-foreground" data-projects-empty="true">
          No project on this machine yet.
          {hostBridge() ? (
            <Button variant="secondary" size="sm" onClick={() => actions.openAddProject()} data-projects-empty-add="true">
              Add project
            </Button>
          ) : null}
        </li>
      ) : null}
    </ul>
  );
}

const NO_IDS: string[] = [];

/** What every row of the Projects list reads besides its own project. */
type ListContext = {
  agents: AgentRow[];
  presentationOf: (agent: AgentRow) => FoldedLineage;
  /** The Agents list's context line for a raised row, which no project row above names. */
  placeOf: (device: string | null, paneId: string) => string | null;
  /** The number an ⌥ hold shows on an agent's raised row (checkout null) or tree row, while one shows. */
  numberOf: ((paneId: string, checkoutId: string | null) => Digit | null) | null;
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
  agentRowMenu: AgentRowMenu;
};

function rowKey(row: ProjectRow): string {
  switch (row.kind) {
    case "header":
      return `header:${row.title}`;
    case "raised":
      return `raised:${row.group}`;
    case "workspace":
      return `workspace:${row.workspace.id}`;
    case "inactive_projects":
      return `inactive-projects:${row.group.device_id}`;
  }
}

const ProjectRowView = memo(function ProjectRowView({ row, context }: { row: ProjectRow; context: ListContext }) {
  if (row.kind === "header") {
    return (
      <li className="px-sm pt-sm pb-xxs text-micro font-medium text-muted-foreground" data-section={row.title}>
        {row.title} · {row.count}
      </li>
    );
  }
  if (row.kind === "raised") return <RaisedSection row={row} context={context} />;
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
 * A raised Needs You or Done section above the project tree
 * (docs/status-model.md): the Agents list's row for each agent, drawn
 * whatever its project, checkout or parent has folded. A raised row never
 * unfolds, so it always wears its descendant badge; opening it does what the
 * same row does in Agents.
 */
function RaisedSection({ row, context }: { row: Extract<ProjectRow, { kind: "raised" }>; context: ListContext }) {
  return (
    <li data-raised-group={row.group}>
      <div className="px-sm pt-sm pb-xxs text-micro font-medium text-muted-foreground" data-section={row.title} id={`raised-group-${row.group}`}>
        {row.title} · {row.agents.length}
      </div>
      <ul aria-labelledby={`raised-group-${row.group}`}>
        {row.agents.map(({ agent, device }) => {
          const presentation = context.presentationOf(agent);
          return (
            <SidebarAgentRow
              key={`${device ?? "local"}:${agent.id}`}
              agent={agent}
              // The device in front is the whole list, so no row names it (PRD home-device-rail B2); the Inbox is where chips are.
              device={null}
              place={context.placeOf(device, agent.pane_id)}
              depth={0}
              descendants={presentation.badgeDescendants}
              childRows={presentation.badgeChildren}
              selected={context.workspaceScreen && agent.pane_id === context.focusedPaneId}
              onOpen={context.actions.openAgent}
              onToggleTree={null}
              inset={PROJECT_COLUMN}
              foldedLineage={presentation}
              number={context.numberOf?.(agent.pane_id, null) ?? null}
              menu={context.agentRowMenu}
            />
          );
        })}
      </ul>
    </li>
  );
}

// The Projects columns (PRD sidebar-readability D-2): a project's glyph at the
// row's inset, a checkout's glyph one lineage step in, and an opened
// checkout's agent marks on the checkout name's column. Every row ends the
// same way, agent rows included: its time, then its fold slot, so the times
// and the chevrons stand in one column each. A row's menu opens on a
// right-click, or the menu key or ⇧F10 on the focused row; nothing on the
// row stands for it.
/** Where a project's glyph starts. */
const PROJECT_COLUMN = "var(--spacing-sm)";
/** Where a project's name, and a plain folder's agent marks, start. */
const PROJECT_NAME_COLUMN = "calc(var(--spacing-sm) + var(--size-checkout-icon) + var(--spacing-sm))";
/** Where a checkout's kind glyph, and its Inactive fold, start. */
const CHECKOUT_COLUMN = "calc(var(--spacing-sm) + var(--size-lineage-indent))";
/** Where a checkout's name, its line two and its agents' marks start. */
const CHECKOUT_NAME_COLUMN = "calc(var(--spacing-sm) + var(--size-lineage-indent) + var(--size-checkout-icon) + var(--spacing-sm))";

/**
 * A row control that waits for the pointer, inside a row `EntryContextMenu`
 * wraps: its slot is always kept, and it shows under the pointer, with focus
 * inside the row, while the row's menu is open, and always on an input with no
 * hover (PRD sidebar-readability D-3, B2, B3). `REVEALED_CONTROL` in
 * sidebar-agent-row.tsx is the same rule for an agent row, which has no menu.
 */
const ROW_REVEALED =
  "opacity-0 group-hover:opacity-100 group-focus-within:opacity-100 group-data-[state=open]:opacity-100 focus-visible:opacity-100 hoverless:opacity-100";

/** A row's time and fold slot, a step apart as on an agent row, so every row's time ends on one column. */
function RowEnd({ children }: { children: ReactNode }) {
  return <span className="flex shrink-0 items-center gap-xs">{children}</span>;
}

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
    </button>
  );
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
  const selected = overviewRowSelected(workspace, context.overviewProjectId);
  const marks = projectMarks(workspace);
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
      <EntryContextMenu
        label={`${workspace.label} actions`}
        items={() => projectMenu(workspace, menuHost())}
        onSelect={(item) => runProjectItem(actions, workspace, item)}
        className="group flex items-stretch"
        data-project-menu={workspace.id}
      >
        <div
          className="flex min-h-(--size-project-row) w-full items-center gap-xs rounded-sm pr-xs hover:bg-accent"
          style={{ paddingLeft: PROJECT_COLUMN }}
        >
          <button
            type="button"
            data-project-row={workspace.id}
            aria-label={[workspace.label, badgeWords(marks)].filter(Boolean).join(", ")}
            className="flex min-w-0 flex-1 self-stretch items-center gap-sm rounded-xs text-left outline-none focus-visible:ring-1 focus-visible:ring-inset focus-visible:ring-ring"
            onClick={() => actions.openProject(workspace, projectRowExpansion(workspace, context.overviewProjectId, disclosure))}
          >
            <ProjectIcon aria-hidden="true" className="size-(--size-checkout-icon) shrink-0 text-subtle-foreground" />
            <span data-row-name="true" className="min-w-0 flex-1 truncate text-subhead font-semibold text-foreground">{workspace.label}</span>
            {/* The project's badge stays while its checkouts are open: it is the project's own summary. */}
            <StatusBadge counts={marks} data-project-status={workspace.id} />
          </button>
          {disclosure ? (
            <FoldToggle
              open={expanded}
              label={expanded ? `Fold ${workspace.label}` : `Unfold ${workspace.label}`}
              onToggle={() => actions.toggleProjectCheckouts(workspace)}
              data-project-toggle={workspace.id}
            />
          ) : (
            <FoldLane />
          )}
        </div>
      </EntryContextMenu>
      {expanded ? (
        <ul>
          {workspace.is_git ? <OverviewRow workspace={workspace} selected={selected} /> : null}
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

function OverviewRow({ workspace, selected }: { workspace: Workspace; selected: boolean }) {
  return (
    <li>
      <button
        type="button"
        data-project-overview={workspace.id}
        aria-current={selected ? "page" : undefined}
        className={cn(
          "flex min-h-(--size-checkout-row) w-full items-center gap-sm rounded-sm pr-xs text-left text-body text-foreground outline-none focus-visible:ring-1 focus-visible:ring-inset focus-visible:ring-ring",
          selected ? "bg-secondary font-medium" : "hover:bg-accent",
        )}
        style={{ paddingLeft: CHECKOUT_COLUMN }}
        onClick={() => useUiStore.getState().setScreen(overviewScreen(useShellStore.getState().rest, workspace.id))}
      >
        <LayoutDashboardIcon aria-hidden="true" className="size-(--size-checkout-icon) shrink-0 text-subtle-foreground" />
        <span className="min-w-0 flex-1 truncate">Overview</span>
        <FoldLane />
      </button>
    </li>
  );
}

const NO_BOARD_ROWS: BoardRow[] = [];


/**
 * What a checkout's row draws besides line one, shared by the checkout row and
 * a folder's one row: its agent rows open only where agents run and the
 * operator opened them. Line two is the purpose and exists only while there
 * is one (PRD sidebar-typography D-04); opening the agents never grows or
 * shrinks the row.
 */
function checkoutDisclosure(checkout: Checkout, agentRows: BoardRow[], view: CheckoutPresentation, context: ListContext) {
  const foldable = context.disclosure && agentRows.length > 0;
  const open = foldable && context.openCheckouts.includes(checkout.id);
  const purpose = checkout.purpose?.text ?? null;
  const parents = [...new Set(agentRows
    .filter((row) => row.depth === 0)
    .map((row) => context.agents.find((candidate) => candidate.pane_id === row.agent.lineage_parent_pane_id))
    .filter((parent): parent is AgentRow => parent !== undefined)
    .map((parent) => parent.identity_label))];
  const raisedFrom = parents.length > 0 ? `${parents[0]}${parents.length > 1 ? ` +${parents.length - 1}` : ""}에서` : null;
  return { foldable, open, purpose, secondLine: checkoutHasSecondLine(view, purpose, raisedFrom), raisedFrom };
}

/**
 * A checkout: the row opens it, and a right-click opens its menu. Line one
 * ends in its agents' status badge while their rows are folded, then the fold
 * slot, whose chevron opens those rows while agents run in it. Line two is
 * the purpose, and the last-commit age ends the row's last line on the time
 * column: line two's when the row has a purpose, line one's otherwise.
 * Opened, its rows and it share a small group fill, and the rows' own marks
 * stand for the badge. While its worktree is being deleted the row and its
 * agent rows are dimmed, a spinner stands where the badge was, and neither
 * the row nor its menu opens anything.
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
  const removing = useShellStore((s) => checkoutRemoving(s.rest?.worktree_removal, workspace.device_id, checkout.path));
  const view = checkoutPresentation(workspace, checkout, Date.now());
  const name = checkout.branch ?? checkout.label;
  const { foldable, open, purpose, secondLine, raisedFrom } = checkoutDisclosure(checkout, agentRows, view, context);
  const marks = checkout.agent_summary?.marks;
  return (
    <li
      data-checkout-row={checkout.id}
      data-checkout-open={open ? "true" : undefined}
      data-checkout-removing={removing ? "true" : undefined}
      className={cn(open && "rounded-sm bg-muted py-xs", removing && "opacity-(--opacity-dimmed)")}
    >
      <EntryContextMenu
        label={`${name} actions`}
        items={() => checkoutMenu(workspace, checkout, menuHost(), purposeProblem)}
        onSelect={(item) => runCheckoutItem(actions, workspace, checkout, item)}
        disabled={removing}
        className="group flex items-stretch"
        data-checkout-menu={checkout.id}
      >
        <div
          className={cn(
            "relative flex w-full flex-col justify-center gap-xxs rounded-sm pr-xs",
            secondLine ? "min-h-(--size-checkout-row-detailed)" : "min-h-(--size-checkout-row)",
            focused ? "bg-secondary" : !removing && "hover:bg-accent",
            view.settled && !removing && "opacity-(--opacity-dimmed)",
          )}
          style={{ paddingLeft: CHECKOUT_COLUMN }}
        >
          <CheckoutOpenButton
            workspace={workspace}
            checkout={checkout}
            view={view}
            focused={focused}
            label={[name, removing ? "deleting" : open ? null : badgeWords(marks), view.age, purpose].filter(Boolean).join(", ")}
            actions={actions}
            expanded={checkoutRowExpansion(foldable, context.workspaceScreen && focused, open)}
            disabled={removing}
          />
          {/* The row button covers the whole row; a control drawn over it is positioned, so it stacks above.
              Line one is 20 whatever it carries: the 24-high fold toggle overhangs it rather than moving line two. */}
          <span className="pointer-events-none flex h-(--size-sidebar-line) min-w-0 items-center gap-sm">
            <KindGlyph view={view} deviceId={workspace.device_id} actions={actions} />
            <CheckoutName name={name} focused={focused} />
            <CheckoutBadge checkout={checkout} />
            <span className="flex-1" />
            <RowEnd>
              {removing ? (
                <Loader2Icon aria-hidden="true" data-checkout-removing-mark="true" className="size-(--size-icon-sm) shrink-0 animate-spin text-muted-foreground" />
              ) : open ? null : (
                <StatusBadge counts={marks} data-checkout-status={checkout.id} />
              )}
              {!secondLine && view.age ? <CheckoutAge age={view.age} /> : null}
              {foldable ? (
                <FoldToggle
                  open={open}
                  label={open ? `Hide the agents in ${name}` : `Show the agents in ${name}`}
                  onToggle={() => actions.toggleCheckoutAgents(checkout.id)}
                  data-checkout-toggle={checkout.id}
                />
              ) : (
                <FoldLane />
              )}
            </RowEnd>
          </span>
          {secondLine ? <PurposeLine purpose={purpose} origin={checkout.purpose?.origin} age={view.age} raisedFrom={raisedFrom} /> : null}
        </div>
      </EntryContextMenu>
      {open ? <OpenAgentRows checkoutId={checkout.id} deviceId={workspace.device_id} agentRows={agentRows} inset={CHECKOUT_NAME_COLUMN} context={context} /> : null}
    </li>
  );
});

/**
 * A plain folder (`folderCheckout`): the project and its only checkout are
 * one row. Line one is the project's name and its status badge, which stays
 * while the agent rows are open because it is the project's own summary; line
 * two is the purpose. The row opens the checkout and is marked while that
 * checkout is the Workspace in front or the project's Overview is, and its
 * menu is the project's followed by the checkout's. It ends as a checkout row
 * does, and its Overview is reached from All projects.
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
  const marks = checkout.agent_summary?.marks;
  const { foldable, open, purpose, secondLine, raisedFrom } = checkoutDisclosure(checkout, agentRows, view, context);
  return (
    <li data-project={workspace.id} data-checkout-open={open ? "true" : undefined} className={cn(inset, open && "rounded-sm bg-muted py-xs")}>
      <EntryContextMenu
        label={`${workspace.label} actions`}
        items={() => folderMenu(workspace, checkout, menuHost(), purposeProblem)}
        onSelect={(item) => (FOLDER_CHECKOUT_ITEMS.has(item) ? runCheckoutItem(actions, workspace, checkout, item) : runProjectItem(actions, workspace, item))}
        className="group flex items-stretch"
        data-project-menu={workspace.id}
      >
        <div
          className={cn(
            "relative flex w-full flex-col justify-center gap-xxs rounded-sm pr-xs",
            secondLine ? "min-h-(--size-checkout-row-detailed)" : "min-h-(--size-project-row)",
            focused ? "bg-secondary" : "hover:bg-accent",
          )}
          style={{ paddingLeft: PROJECT_COLUMN }}
        >
          <CheckoutOpenButton
            workspace={workspace}
            checkout={checkout}
            view={view}
            focused={focused}
            label={[workspace.label, badgeWords(marks), purpose].filter(Boolean).join(", ")}
            actions={actions}
            expanded={checkoutRowExpansion(foldable, context.workspaceScreen && focused, open)}
          />
          <span className="pointer-events-none flex h-(--size-sidebar-line) min-w-0 items-center gap-sm">
            <FolderIcon aria-hidden="true" className={cn("size-(--size-checkout-icon) shrink-0", view.kindTone)} />
            <span aria-hidden="true" data-row-name="true" className="min-w-0 truncate text-subhead font-semibold text-foreground">
              {workspace.label}
            </span>
            <CheckoutBadge checkout={checkout} />
            <span className="flex-1" />
            <RowEnd>
              <StatusBadge counts={marks} data-project-status={workspace.id} />
              {foldable ? (
                <FoldToggle
                  open={open}
                  label={open ? `Hide the agents in ${workspace.label}` : `Show the agents in ${workspace.label}`}
                  onToggle={() => actions.toggleCheckoutAgents(checkout.id)}
                  data-checkout-toggle={checkout.id}
                />
              ) : (
                <FoldLane />
              )}
            </RowEnd>
          </span>
          {secondLine ? <PurposeLine purpose={purpose} origin={checkout.purpose?.origin} age={null} raisedFrom={raisedFrom} /> : null}
        </div>
      </EntryContextMenu>
      {open ? <OpenAgentRows checkoutId={checkout.id} deviceId={workspace.device_id} agentRows={agentRows} inset={PROJECT_NAME_COLUMN} context={context} /> : null}
    </li>
  );
});

/**
 * The checkout's kind glyph. On a row whose glyph is a pull request's
 * lifecycle it is a button that opens that pull request (PRD
 * checkout-pr-glyph-card D-02): a ring in the pull request's color under the
 * pointer says so, ⌘ asks for the default browser, and the press never
 * reaches the row button under it, so the checkout neither opens nor
 * unfolds. Every other kind is the plain icon the row button covers.
 */
function KindGlyph({ view, deviceId, actions }: { view: CheckoutPresentation; deviceId: string; actions: Actions }) {
  const KindIcon = CHECKOUT_KIND_ICON[view.kind];
  const pr = view.kind.startsWith("pr_") ? view.pullRequest : null;
  if (!pr) return <KindIcon aria-hidden="true" className={cn("size-(--size-checkout-icon) shrink-0", view.kindTone)} />;
  return (
    <button
      type="button"
      data-checkout-pr-glyph={pr.number}
      aria-label={`Open pull request #${pr.number}`}
      className={cn(
        "pointer-events-auto relative flex size-(--size-checkout-icon) shrink-0 cursor-pointer items-center justify-center rounded-xs outline-none hover:ring-1 hover:ring-current focus-visible:ring-1 focus-visible:ring-ring",
        view.kindTone,
      )}
      onClick={(event) => {
        event.stopPropagation();
        actions.openPullRequest(pr.url, deviceId, pullRequestOpenExternal(event));
      }}
    >
      <KindIcon aria-hidden="true" className="size-(--size-checkout-icon)" />
    </button>
  );
}

/**
 * The button under a checkout's whole row: it opens the checkout, and the
 * card it opens on hover and focus carries the pull request, agents, branch
 * and path (`checkoutCard`); a screen reader reads the same facts as the
 * sentence `view.detail`.
 */
function CheckoutOpenButton({
  workspace,
  checkout,
  view,
  focused,
  label,
  actions,
  expanded,
  disabled = false,
}: {
  workspace: Workspace;
  checkout: Checkout;
  view: CheckoutPresentation;
  focused: boolean;
  label: string;
  actions: Actions;
  expanded?: boolean;
  disabled?: boolean;
}) {
  // Read on every render, like `view.detail`: a memo keyed on the checkout would keep the Commit age from the last change to this row.
  const card = checkoutCard(workspace, checkout, Date.now());
  return (
    <CheckoutCardHint
      card={card}
      description={view.detail}
      onOpenPullRequest={(url, external) => actions.openPullRequest(url, workspace.device_id, external)}
      onShowPullRequestRow={workspace.is_git && !workspace.remote_target_id ? (number) => actions.openPullRequestRow(workspace.id, number) : undefined}
    >
      <button
        type="button"
        data-checkout={checkout.id}
        data-checkout-kind={view.kind}
        aria-current={focused ? "true" : undefined}
        aria-label={label}
        disabled={disabled}
        className="absolute inset-0 rounded-sm outline-none focus-visible:ring-1 focus-visible:ring-inset focus-visible:ring-ring disabled:cursor-default"
        onClick={() => actions.openWorkspace(workspace.device_id, checkout.workspace_id, checkout.id, expanded)}
      />
    </CheckoutCardHint>
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

/**
 * A checkout's name at 12/400, 500 while its Workspace is in front, with its
 * path prefix (`prd/`) muted so the part that tells checkouts apart reads
 * first (PRD sidebar-typography D-02, D-06).
 */
function CheckoutName({ name, focused }: { name: string; focused: boolean }) {
  const { prefix, rest } = checkoutNameParts(name);
  return (
    <span aria-hidden="true" data-row-name="true" className={cn("min-w-0 truncate text-body text-foreground", focused && "font-medium")}>
      {prefix ? (
        <span className="text-muted-foreground" data-name-prefix="true">
          {prefix}
        </span>
      ) : null}
      {rest}
    </span>
  );
}

/** A checkout's last-commit age, drawn on the time column of whichever line ends the row. */
function CheckoutAge({ age }: { age: string }) {
  return (
    <span aria-hidden="true" data-checkout-age={age} className="font-mono text-caption text-muted-foreground">
      {age}
    </span>
  );
}

/**
 * A checkout's line two: its purpose, and the last-commit age ending on the
 * time column over an empty fold slot, as line one ends. A folder's line two
 * has no age: a plain folder has no commit.
 */
function PurposeLine({ purpose, origin, age, raisedFrom }: { purpose: string | null; origin: string | undefined; age: string | null; raisedFrom: string | null }) {
  return (
    // The row is already inset to its glyph, so line two starts one glyph and gap further, under the name.
    <span aria-hidden="true" className="pointer-events-none flex min-h-(--size-sidebar-line-detail) min-w-0 items-center gap-sm pl-(--size-checkout-metadata-inset) text-caption leading-(--size-sidebar-line-detail)">
      <span className="flex min-w-0 flex-1 items-center gap-xs truncate text-muted-foreground" data-purpose={purpose === null ? undefined : origin}>
        {raisedFrom ? (
          <span className="inline-flex min-w-0 shrink items-center gap-xxs text-subtle-foreground" data-checkout-parent={raisedFrom}>
            <CornerUpLeftIcon aria-hidden="true" className="size-(--size-icon-sm) shrink-0" />
            <span className="truncate">{raisedFrom}</span>
          </span>
        ) : null}
        {purpose ? <span className="min-w-0 truncate">{purpose}</span> : null}
      </span>
      {age ? (
        <RowEnd>
          <CheckoutAge age={age} />
          <FoldLane />
        </RowEnd>
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
function OpenAgentRows({ checkoutId, deviceId, agentRows, inset, context }: { checkoutId: string; deviceId: string; agentRows: BoardRow[]; inset: string; context: ListContext }) {
  const rows = context.disclosure ? unfoldedRows(agentRows) : agentRows;
  return (
    <ul data-checkout-agents-open={checkoutId}>
      {rows.map((row) => {
        const presentation = context.presentationOf(row.agent);
        const device = row.agent.device_id !== deviceId ? (row.agent.device_label ?? null) : null;
        return (
          <SidebarAgentRow
            key={row.agent.pane_id}
            agent={row.agent}
            device={device}
            place={null}
            depth={row.depth}
            descendants={context.disclosure ? presentation.badgeDescendants : 0}
            childRows={context.disclosure ? presentation.badgeChildren : NO_AGENT_ROWS}
            selected={context.workspaceScreen && row.agent.pane_id === context.focusedPaneId}
            onOpen={context.actions.openAgent}
            onToggleTree={context.disclosure ? context.actions.toggleAgentTree : null}
            inset={inset}
            branchShown={row.depth > 0}
            foldedLineage={presentation}
            number={context.numberOf?.(row.agent.pane_id, checkoutId) ?? null}
            menu={context.agentRowMenu}
          />
        );
      })}
    </ul>
  );
}

/** A tree drawn with nothing folded lists no folded children. */
const NO_AGENT_ROWS: AgentRow[] = [];

/** What a row's menu reads from the host when it opens: Finder, and the new-tab chord the registry binds here. */
function menuHost(): MenuHost {
  const host = hostKind();
  return { finder: host === "electron", newTabChord: displayCommand("new_tab", host, hostRegistry(useShellStore.getState().rest?.ui_state, host).registry) };
}

function runProjectItem(actions: Actions, workspace: Workspace, item: MenuItem["id"]) {
  switch (item) {
    case "open_overview":
      return useUiStore.getState().setScreen(overviewScreen(useShellStore.getState().rest, workspace.id));
    case "new_worktree":
      return useUiStore.getState().setWorkspaceDialog({ kind: "new_worktree", workspaceId: workspace.id });
    case "new_tab_primary": {
      const primary = primaryCheckout(workspace);
      return primary ? actions.newTabIn(workspace.device_id, primary) : undefined;
    }
    case "reveal_finder":
      return actions.revealInFinder(workspace.path);
    case "copy_path":
      return actions.copyText(workspace.path, "path");
    case "pin":
    case "unpin":
      return actions.setPinned(workspace.id, item === "pin");
    case "remove_project":
      return useUiStore.getState().setWorkspaceDialog({ kind: "remove_project", workspaceId: workspace.id });
  }
}

function runCheckoutItem(actions: Actions, workspace: Workspace, checkout: Checkout, item: MenuItem["id"]) {
  switch (item) {
    case "open_checkout":
      return actions.openWorkspace(workspace.device_id, checkout.workspace_id, checkout.id);
    case "new_tab_here":
      return actions.newTabIn(workspace.device_id, checkout);
    case "open_pull_request": {
      const pr = shownPullRequest(checkout);
      return pr ? actions.openPullRequest(pr.url, workspace.device_id, false) : undefined;
    }
    case "set_purpose":
      return useUiStore.getState().setWorkspaceDialog({ kind: "purpose", workspaceId: workspace.id, checkoutId: checkout.id });
    case "set_primary":
      return actions.setPrimaryCheckout(workspace.id, checkout.id);
    case "copy_branch":
      return checkout.branch ? actions.copyText(checkout.branch, "branch name") : undefined;
    case "copy_path":
      return actions.copyText(checkout.path, "path");
    case "reveal_finder":
      return actions.revealInFinder(checkout.path);
    case "delete_worktree":
      return useUiStore.getState().setWorkspaceDialog({ kind: "delete_worktree", workspaceId: workspace.id, checkoutId: checkout.id });
  }
}

/**
 * One agent row menu for a whole list: Show carries the ⌥n that selects the
 * same row in the Agents list, read when the menu opens.
 */
function useAgentRowMenu(actions: Actions): AgentRowMenu {
  return useMemo(
    () => ({
      items: (agent) => {
        const state = useShellStore.getState();
        const number = agentNumber(state, agent.pane_id);
        const host = hostKind();
        const chord = number === null ? "" : displayCommand(`select_agent_${number}`, host, hostRegistry(state.rest?.ui_state, host).registry);
        return agentMenu(agent, chord);
      },
      onSelect: (agent, item) => {
        switch (item) {
          case "show_agent":
            return actions.openAgent(agent.pane_id);
          case "copy_title":
            return actions.copyText(agent.identity_label, "title");
          case "copy_session_id":
            return agent.session_id ? actions.copyText(agent.session_id, "session id") : undefined;
          case "close_tab":
            return actions.closeTabOfPane(agent.pane_id);
        }
      },
    }),
    [actions],
  );
}
