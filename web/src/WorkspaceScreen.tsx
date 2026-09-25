import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import type { Actions } from "./actions";
import { AreaEmpty } from "./AreaEmpty";
import { Button } from "./components/ui/controls";
import { LayoutIcon, ToolIcon } from "./icons";
import { ContextMenu, MenuList, type MenuEntry } from "./Menu";
import { FindBar } from "./Overlays";
import { PaneCanvas, RemotePaneCanvas } from "./PaneGrid";
import { RelationStatus } from "./PaneRelations";
import { remoteView } from "./remote";
import { canRetryDevice, deviceLine } from "./settings";
import { catalogWorkspaces, focusedRemoteDevice, frontCheckout, type Checkout } from "./snapshot";
import { useShellStore } from "./store";
import { AgentTabBar } from "./TabBar";
import { Tools } from "./Tools";
import { useUiStore, type WorkingRegion } from "./ui";
import { ViewAreas } from "./ViewAreas";
import { narrowWorkspace, shownTools } from "./viewLayout";
import { LAYOUTS, agentEntries, agentWidth, drawnMode, layoutLabel, shareAt, workspaceViewOf, type ViewMode } from "./workspace";
import { hostKind } from "./host";
import { displayCommand } from "./shortcuts";

// A Workspace (PRD S6 D-01..D-05, B4-B11; S7 B12, B13): one checkout's Agent
// area (its Herdr tabs and their panes) and View areas (its files and diffs),
// side by side or one at a time, with its tools beside them. The layout, the
// tools and the boundary are the core's, per Workspace; this draws them and
// sends one event per operator choice. A hidden area is unmounted, never
// closed: its terminals park and stay fed, and its tabs stay in the core.
//
// A window too narrow for what the core stored is drawn narrower without
// changing it: the tools float over the work area once the operator asks for
// one, and Together shows one working region at a time with a switch to the
// other. None of that is sent or stored, so widening shows the stored layout
// again.

const WIDE = { toolsOverlay: false, singleRegion: false };

export function WorkspaceScreen({ actions }: { actions: Actions }) {
  const checkout = useShellStore((s) => frontCheckout(s.rest));
  const view = useShellStore((s) => workspaceViewOf(s.rest));
  const stored = useUiStore((s) => s.toolsPlacement);
  const [body, setBody] = useState<HTMLDivElement | null>(null);
  const width = useWidth(body);
  const sizes = useMemo(() => ({ areaMin: tokenPx("--size-workspace-area-min"), panelMin: tokenPx("--size-panel-min"), divider: tokenPx("--size-resize-handle") }), []);
  const opening = useShellStore((s) => (s.editor?.opening ?? []).some((row) => row.checkout_id === checkout?.id));
  // With nothing open in the View areas the agents take their space; the
  // stored layout, the toolbar's choice and the tools stay as they are.
  const mode = view ? drawnMode(view, opening) : "agents";
  const narrow = view ? narrowWorkspace({ bodyWidth: width, mode, ...sizes }) : WIDE;
  // The column while there is room for it; past that, an overlay that stays
  // closed until the operator asks for a tool (S7 B12).
  useEffect(() => {
    useUiStore.getState().setToolsNarrow(narrow.toolsOverlay);
  }, [narrow.toolsOverlay]);
  // An overlay opened in one Workspace is not left open over the next one,
  // nor over this screen when the operator comes back to it; one left with
  // no tool to show closes too, so a tool shown later does not float over
  // the work by itself.
  const checkoutId = checkout?.id ?? null;
  useEffect(() => () => useUiStore.getState().closeTools(), [checkoutId]);
  const toolless = view ? !view.explorer && !view.changes : true;
  useEffect(() => {
    if (toolless) useUiStore.getState().closeTools();
  }, [toolless]);
  if (!checkout || !view) return null;
  // Until the effect above has run, a window that just turned narrow is
  // drawn with the overlay closed.
  const placement = narrow.toolsOverlay ? (stored === "open" ? "open" : "closed") : "column";
  const { explorer, changes } = shownTools(view, placement);
  return (
    <section className="flex min-h-0 min-w-0 flex-1 flex-col" aria-label={`Workspace ${checkout.branch ?? checkout.label}`} data-workspace-screen={checkout.id} data-layout={view.mode}>
      <WorkspaceToolbar checkout={checkout} mode={view.mode} explorer={explorer} changes={changes} singleRegion={narrow.singleRegion} actions={actions} />
      <div ref={setBody} className="relative flex min-h-0 flex-1" data-workspace-body={narrow.toolsOverlay ? "narrow" : "wide"}>
        <Areas checkout={checkout} mode={mode} share={view.agent_share} single={narrow.singleRegion} actions={actions} />
        <Tools explorer={explorer} changes={changes} overlay={narrow.toolsOverlay} actions={actions} />
      </div>
    </section>
  );
}

function tokenPx(name: string): number {
  return Number.parseFloat(getComputedStyle(document.documentElement).getPropertyValue(name)) || 0;
}

/** An element's width, followed as it changes; 0 until it is measured. */
function useWidth(element: HTMLElement | null): number {
  const [width, setWidth] = useState(0);
  useLayoutEffect(() => {
    if (!element) return undefined;
    setWidth(element.clientWidth);
    const observer = new ResizeObserver(() => setWidth(element.clientWidth));
    observer.observe(element);
    return () => observer.disconnect();
  }, [element]);
  return width;
}

/** The path back (B4), the layout choice (B5), the tools (B10), and the working region of a narrow Together (S7 B13). */
function WorkspaceToolbar({ checkout, mode, explorer, changes, singleRegion, actions }: { checkout: Checkout; mode: ViewMode; explorer: boolean; changes: boolean; singleRegion: boolean; actions: Actions }) {
  const project = useShellStore((s) => catalogWorkspaces(s.rest).find((row) => row.checkouts.some((candidate) => candidate.id === checkout.id)) ?? null);
  const device = useShellStore((s) => focusedRemoteDevice(s.rest));
  const setScreen = useUiStore((s) => s.setScreen);
  const name = checkout.branch ?? checkout.label;
  // What the toolbar acts on is this Workspace: its layout, its tools, its
  // path and its Project (B18). Opening the menu changes none of them.
  const menuItems = (): MenuEntry<ToolbarMenuId>[] => [
    ...LAYOUTS.map((layout) => ({ id: `layout:${layout.mode}` as const, label: `${layout.mode === mode ? "✓ " : ""}${layout.label}`, unavailable: null })),
    { id: "explorer", label: explorer ? "Hide Explorer" : "Show Explorer", unavailable: null, separated: true },
    { id: "changes", label: changes ? "Hide History" : "Show History", unavailable: null },
    { id: "copy_path", label: "Copy Workspace path", unavailable: null, separated: true },
    { id: "overview", label: "Open Project Overview", unavailable: project ? null : "This Workspace's Project is not in the catalog" },
  ];
  const select = (id: ToolbarMenuId) => {
    if (id.startsWith("layout:")) return actions.setLayout(id.slice("layout:".length) as ViewMode);
    if (id === "explorer") return actions.setTool("explorer", !explorer);
    if (id === "changes") return actions.setTool("changes", !changes);
    if (id === "copy_path") return void navigator.clipboard?.writeText(checkout.path).catch(() => undefined);
    if (id === "overview" && project) setScreen({ kind: "overview", projectId: project.id });
  };
  return (
    <ContextMenu label={`Workspace ${name}`} items={menuItems} onSelect={select} className="shrink-0" data-workspace-menu="true">
      <div className="flex h-[var(--size-tab-strip)] shrink-0 items-center gap-sm border-b border-divider bg-sidebar px-sm text-caption" data-workspace-toolbar="true">
        <nav aria-label="Location" className="flex min-w-0 flex-1 items-center gap-xs">
          <button type="button" className="shrink-0 rounded-xs px-xs text-secondary hover:bg-elevated hover:text-primary focus-visible:bg-elevated" data-go-main="true" onClick={() => setScreen({ kind: "main" })}>
            Main
          </button>
          <span aria-hidden="true" className="text-muted">/</span>
          {project ? (
            <button
              type="button"
              className="min-w-0 max-w-[var(--size-recent-location-max)] shrink truncate rounded-xs px-xs text-secondary hover:bg-elevated hover:text-primary focus-visible:bg-elevated"
              title={`${project.label} · ${project.path}${device ? ` · ${device.label}` : ""}`}
              data-go-overview={project.id}
              onClick={() => setScreen({ kind: "overview", projectId: project.id })}
            >
              {project.label}
            </button>
          ) : null}
          <span aria-hidden="true" className="text-muted">/</span>
          <span className="min-w-0 truncate text-primary" title={`${name} · ${checkout.path}`} aria-current="page">
            {name}
          </span>
          {device ? (
            <span className="shrink-0 rounded-xs bg-elevated px-xs text-micro text-secondary" title={`On ${device.label}`} data-workspace-device={device.id}>
              {device.label}
            </span>
          ) : null}
        </nav>
        {singleRegion ? <RegionSwitch /> : null}
        <LayoutSwitch mode={mode} actions={actions} />
        <div className="flex items-center gap-xxs" role="group" aria-label="Workspace tools">
          <ToolToggle tool="explorer" label="Explorer" on={explorer} actions={actions} />
          <ToolToggle tool="changes" label="History" on={changes} actions={actions} />
        </div>
      </div>
    </ContextMenu>
  );
}

type ToolbarMenuId = `layout:${ViewMode}` | "explorer" | "changes" | "copy_path" | "overview";

/** Three icons with the choice marked, and the same choices by name in a menu (D-03). */
function LayoutSwitch({ mode, actions }: { mode: ViewMode; actions: Actions }) {
  const [menu, setMenu] = useState(false);
  const items: MenuEntry<ViewMode>[] = LAYOUTS.map((layout) => ({
    id: layout.mode,
    label: `${layout.mode === mode ? "✓ " : ""}${layout.label}`,
    unavailable: null,
  }));
  return (
    <div className="relative flex items-center gap-xxs">
      <div role="radiogroup" aria-label="Layout" className="flex items-center rounded-sm bg-panel p-xxs">
        {LAYOUTS.map((layout) => (
          <button
            key={layout.mode}
            type="button"
            role="radio"
            aria-checked={layout.mode === mode}
            aria-label={layout.label}
            title={layout.label}
            data-layout-choice={layout.mode}
            className={`flex h-[var(--size-icon-button-toolbar)] w-[var(--size-icon-button-toolbar)] items-center justify-center rounded-xs outline-none focus-visible:ring-1 focus-visible:ring-accent ${
              layout.mode === mode ? "bg-elevated text-primary" : "text-muted hover:text-secondary"
            }`}
            onClick={() => actions.setLayout(layout.mode)}
          >
            <LayoutIcon mode={layout.mode} />
          </button>
        ))}
      </div>
      <button
        type="button"
        aria-haspopup="menu"
        aria-expanded={menu}
        aria-label={`Layout menu: ${layoutLabel(mode)}`}
        title={`Layout menu: ${layoutLabel(mode)}`}
        data-layout-menu="true"
        className="rounded-xs px-xs text-muted hover:bg-elevated hover:text-primary focus-visible:bg-elevated"
        onClick={() => setMenu(!menu)}
      >
        ▾
      </button>
      {menu ? (
        <MenuList label="Layout" items={items} onSelect={(id) => actions.setLayout(id)} onClose={() => setMenu(false)} className="absolute right-0 top-full" />
      ) : null}
    </div>
  );
}

const REGIONS: readonly { region: WorkingRegion; label: string }[] = [
  { region: "agents", label: "Agents" },
  { region: "views", label: "Views" },
];

/**
 * The explicit way between the two working regions while a Together window
 * is too narrow to show both (S7 B13). It is this page's presentation only:
 * choosing a region sends nothing.
 */
function RegionSwitch() {
  const region = useUiStore((s) => s.workingRegion);
  return (
    <div role="radiogroup" aria-label="Working region, one at a time in this narrow window" className="flex items-center rounded-sm bg-panel p-xxs" data-region-switch={region}>
      {REGIONS.map((choice) => (
        <button
          key={choice.region}
          type="button"
          role="radio"
          aria-checked={choice.region === region}
          title={`Show ${choice.label}`}
          data-region-choice={choice.region}
          className={`rounded-xs px-xs text-caption outline-none focus-visible:ring-1 focus-visible:ring-accent ${
            choice.region === region ? "bg-elevated text-primary" : "text-muted hover:text-secondary"
          }`}
          onClick={() => useUiStore.getState().setWorkingRegion(choice.region)}
        >
          {choice.label}
        </button>
      ))}
    </div>
  );
}

function ToolToggle({ tool, label, on, actions }: { tool: "explorer" | "changes"; label: string; on: boolean; actions: Actions }) {
  return (
    <button
      type="button"
      aria-pressed={on}
      aria-label={`${on ? "Hide" : "Show"} ${label}`}
      title={`${on ? "Hide" : "Show"} ${label}`}
      data-tool-toggle={tool}
      className={`flex h-[var(--size-icon-button-toolbar)] w-[var(--size-icon-button-toolbar)] items-center justify-center rounded-xs outline-none focus-visible:ring-1 focus-visible:ring-accent ${
        on ? "bg-elevated text-primary" : "text-muted hover:text-secondary"
      }`}
      onClick={() => actions.setTool(tool, !on)}
    >
      <ToolIcon tool={tool} />
    </button>
  );
}

/** The minimum width either area keeps while both show (B6), read from its token. */
function areaMinimum(): number {
  return tokenPx("--size-workspace-area-min");
}

/**
 * The areas the layout shows. While both show, a boundary between them moves
 * a guide line as it is dragged and sends one `workspace_view` share on
 * release, so a drag never resizes the terminals on every pointer event.
 * `single` is a Together window too narrow for both: it shows the working
 * region the operator was last in (S7 B13).
 */
function Areas({ checkout, mode, share, single, actions }: { checkout: Checkout; mode: ViewMode; share: number; single: boolean; actions: Actions }) {
  const region = useUiStore((s) => s.workingRegion);
  const body = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState(0);
  const [guide, setGuide] = useState<number | null>(null);
  useLayoutEffect(() => {
    const element = body.current;
    if (!element) return undefined;
    setWidth(element.clientWidth);
    const observer = new ResizeObserver(() => setWidth(element.clientWidth));
    observer.observe(element);
    return () => observer.disconnect();
  }, []);
  const minimum = useMemo(() => areaMinimum(), []);
  const agents = mode !== "views" && (!single || region === "agents");
  const views = mode !== "agents" && (!single || region === "views");
  // The region the operator works in is the one a narrow Together keeps.
  const workIn = (next: WorkingRegion) => () => useUiStore.getState().setWorkingRegion(next);
  const agentPx = agents && views && width > 0 ? agentWidth(share, width, minimum) : null;
  const drag = (event: React.PointerEvent<HTMLDivElement>) => {
    const element = body.current;
    if (!element) return;
    event.preventDefault();
    const left = element.getBoundingClientRect().left;
    const total = element.clientWidth;
    const target = event.currentTarget;
    target.setPointerCapture(event.pointerId);
    const move = (next: PointerEvent) => setGuide(shareAt(next.clientX - left, total, minimum) * total);
    const end = () => {
      target.removeEventListener("pointermove", move);
      target.removeEventListener("pointerup", up);
      target.removeEventListener("pointercancel", end);
      target.removeEventListener("lostpointercapture", end);
      setGuide(null);
    };
    const up = (next: PointerEvent) => {
      end();
      const nextShare = shareAt(next.clientX - left, total, minimum);
      if (Math.abs(nextShare - share) > 0.001) actions.setWorkspaceView({ agent_share: nextShare });
    };
    // A drag the system cancels lands nothing and leaves no guide behind.
    target.addEventListener("pointermove", move);
    target.addEventListener("pointerup", up);
    target.addEventListener("pointercancel", end);
    target.addEventListener("lostpointercapture", end);
  };
  return (
    // While both areas show they ask for two minimums, and the tools column
    // gives way first, down to its own minimum (B6): its shrink weight is far
    // above the areas'. Below that the areas split what is left evenly.
    <div
      ref={body}
      className={`relative flex min-h-0 min-w-0 grow ${agents && views ? "shrink basis-[calc(2_*_var(--size-workspace-area-min)_+_var(--size-resize-handle))]" : "flex-1"}`}
      data-areas={mode}
    >
      {agents ? (
        <div
          className={`flex min-h-0 min-w-0 flex-col ${agentPx === null ? "flex-1" : "shrink-0"}`}
          style={agentPx === null ? undefined : { width: agentPx }}
          data-agent-area="true"
          onPointerDownCapture={workIn("agents")}
          onFocusCapture={workIn("agents")}
        >
          <AgentArea checkout={checkout} actions={actions} />
        </div>
      ) : null}
      {agents && views ? (
        <div
          role="separator"
          aria-orientation="vertical"
          aria-label="Resize the Agent and View areas"
          aria-valuenow={Math.round(share * 100)}
          aria-valuemin={20}
          aria-valuemax={80}
          tabIndex={0}
          data-area-divider="true"
          className="relative z-10 w-[var(--size-resize-handle)] shrink-0 cursor-col-resize bg-divider outline-none hover:bg-accent focus-visible:bg-accent"
          onPointerDown={drag}
          onKeyDown={(event) => {
            if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
            event.preventDefault();
            const step = event.key === "ArrowLeft" ? -0.05 : 0.05;
            const next = width > 0 ? shareAt((share + step) * width, width, minimum) : share;
            if (next !== share) actions.setWorkspaceView({ agent_share: next });
          }}
        />
      ) : null}
      {views ? (
        <div className="flex min-h-0 min-w-0 flex-1 flex-col" data-view-area="true" onPointerDownCapture={workIn("views")} onFocusCapture={workIn("views")}>
          <ViewAreas actions={actions} />
        </div>
      ) : null}
      {guide !== null ? <div className="pointer-events-none absolute inset-y-0 w-[var(--size-resize-handle)] bg-accent" style={{ left: guide }} data-area-guide="true" /> : null}
    </div>
  );
}

/** This machine's tabs, or the selected SSH device's own tabs and panes in their place (S5 B19). */
function AgentArea({ checkout, actions }: { checkout: Checkout; actions: Actions }) {
  const remote = useShellStore((s) => focusedRemoteDevice(s.rest) !== null);
  if (remote) return <RemoteAgentArea actions={actions} />;
  return <LocalAgentArea checkout={checkout} actions={actions} />;
}

function LocalAgentArea({ checkout, actions }: { checkout: Checkout; actions: Actions }) {
  const agents = useShellStore((s) => s.agents);
  const hasTabs = agentEntries(checkout).length > 0;
  return (
    <>
      <AgentTabBar checkout={checkout} activeTabId={checkout.active_tab_id} agents={agents} actions={actions} />
      <RelationStatus actions={actions} />
      <FindBar actions={actions} />
      {hasTabs ? (
        <PaneCanvas actions={actions} />
      ) : (
        <AreaEmpty state="no-agent-tab" text="No agent tab is open in this Workspace.">
          <Button onClick={() => actions.createTab()} data-empty-new-tab="true">
            New tab <span className="text-muted">{displayCommand("new_tab", hostKind())}</span>
          </Button>
        </AreaEmpty>
      )}
    </>
  );
}

/**
 * A selected SSH device's Agent area: that host's visible tab, drawn from the
 * session the core projected over SSH, or the connection's own state when
 * there is no session. A lost connection keeps the last session on screen
 * and names the device (B21); nothing here reads this machine's tabs.
 */
function RemoteAgentArea({ actions }: { actions: Actions }) {
  const device = useShellStore((s) => focusedRemoteDevice(s.rest));
  const status = useShellStore((s) => (device ? (s.rest?.status?.remote?.find((row) => row.target_id === device.id) ?? null) : null));
  const session = status?.session ?? null;
  const view = useMemo(() => remoteView(session), [session]);
  if (!device) return null;
  const connected = status?.state === "connected";
  if (!view) {
    const line = deviceLine(device, status ?? undefined);
    return (
      <div className="flex min-h-0 flex-1 flex-col items-start justify-center gap-sm p-xl text-body text-secondary" data-remote-device-surface={device.id} data-remote-state={status?.state ?? "none"}>
        <h2 className="text-title font-semibold text-primary">
          {device.label} <span className="font-mono text-caption text-muted">{device.ssh_alias}</span>
        </h2>
        <p>{connected ? `${device.label} is connected, but no Herdr workspace is open there.` : `${device.label} is ${line.text}${status?.message ? `: ${status.message}` : "."}`}</p>
        <div className="flex gap-sm">
          {canRetryDevice(device, status ?? undefined) ? (
            <Button onClick={() => actions.retryDevice(device.id)} data-remote-retry={device.id}>
              Retry
            </Button>
          ) : null}
          <Button appearance="quiet" onClick={() => actions.focusDevice("local")} data-use-local-device="true">
            Show this machine
          </Button>
        </div>
      </div>
    );
  }
  return (
    <>
      {connected ? null : (
        // A lost connection keeps the last session on screen, but the host
        // takes no command until it is back (`remote.control.not_connected`).
        <div role="status" className="flex items-center gap-md border-b border-divider bg-panel px-md py-xs text-caption text-warning" data-remote-stale={device.id}>
          <span className="min-w-0 flex-1 truncate" title={status?.message ?? undefined}>
            {device.label} is not connected{status?.message ? `: ${status.message}` : ""}. Showing the last state it reported; nothing is sent until it reconnects.
          </span>
          {canRetryDevice(device, status ?? undefined) ? (
            <Button onClick={() => actions.retryDevice(device.id)} data-remote-retry={device.id}>
              Retry
            </Button>
          ) : null}
        </div>
      )}
      <AgentTabBar checkout={view.checkout} activeTabId={view.tab?.id ?? null} agents={session?.agents ?? null} device actions={actions} />
      <RelationStatus actions={actions} />
      <FindBar actions={actions} />
      <RemotePaneCanvas view={view} connected={connected} actions={actions} />
    </>
  );
}
