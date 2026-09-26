import { FolderIcon, GitBranchIcon, Maximize2Icon, Minimize2Icon, PanelRightCloseIcon, PanelRightIcon, PinIcon, PinOffIcon, PlusIcon } from "lucide-react";
import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import type { Actions } from "./actions";
import { AreaEmpty } from "./AreaEmpty";
import { EntryContextMenu, type MenuEntry } from "./components/entry-menu";
import { Button } from "./components/ui/button";
import { Hint } from "./components/ui/tooltip";
import { FindBar } from "./Overlays";
import { PaneCanvas, RemotePaneCanvas } from "./PaneGrid";
import { RelationStatus } from "./PaneRelations";
import { remoteView } from "./remote";
import { canRetryDevice, deviceLine } from "./settings";
import { catalogWorkspaces, focusedRemoteDevice, frontCheckout, type Checkout } from "./snapshot";
import { useShellStore } from "./store";
import { focusTerminal } from "./terminals";
import { AgentTabBar } from "./TabBar";
import { Tools } from "./Tools";
import { useUiStore } from "./ui";
import { ViewAreas } from "./ViewAreas";
import { shownTools } from "./viewLayout";
import { PANEL_STATES, agentEntries, panelFrame, panelNeed, panelShareAt, panelWidth, workspaceViewOf, type PanelFrame, type PanelSizes, type PanelState, type WorkspaceView } from "./workspace";
import { hostKind } from "./host";
import { displayCommand } from "./shortcuts";

// A Workspace (PRD S6 D-01..D-05, B4-B11; S7 B12, B13; issue 170): one
// checkout's Agent area (its Herdr tabs and their panes), always the body's
// full width, and a side panel docked to the body's right edge over it,
// holding the View areas (its files, diffs and pages) with the tools beside
// them. The panel's state, pin, width and tools are the core's, per
// Workspace; this draws them and sends one event per operator choice. A
// closed panel is unmounted, never closed: its views stay in the core.
//
// The panel floats over agents that keep their size and stay live beside it,
// so opening, closing, resizing and expanding it never resizes a terminal;
// only a pinned panel narrows the agents to its left edge. A window too
// narrow for both draws the panel over the whole body, and a pinned one
// floats there, without changing what the core stores, so widening brings it
// back.

const CLOSED: PanelFrame = { shown: "closed", content: "empty", width: 0, agentsRight: 0, narrow: false, resizable: false, toolsOverlay: false };

export function WorkspaceScreen({ actions }: { actions: Actions }) {
  const checkout = useShellStore((s) => frontCheckout(s.rest));
  const view = useShellStore((s) => workspaceViewOf(s.rest));
  const stored = useUiStore((s) => s.toolsPlacement);
  const [body, setBody] = useState<HTMLDivElement | null>(null);
  const width = useWidth(body);
  const sizes = useMemo<PanelSizes>(() => ({ areaMin: tokenPx("--size-workspace-area-min"), toolColumn: tokenPx("--size-panel-ideal"), toolMin: tokenPx("--size-panel-min") }), []);
  const opening = useShellStore((s) => (s.editor?.opening ?? []).some((row) => row.checkout_id === checkout?.id));
  const views = (view?.layout?.display_count ?? 0) > 0 || opening;
  const frame = view ? panelFrame({ view, views, body: width, sizes }) : CLOSED;
  // The tool column while the panel has room for it beside a View area;
  // past that, an overlay inside the panel that stays closed until the
  // operator asks for a tool (S7 B12).
  useEffect(() => {
    useUiStore.getState().setToolsNarrow(frame.toolsOverlay);
  }, [frame.toolsOverlay]);
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
  // A panel that closes with the keyboard inside it leaves no focus; it goes
  // back to the pane the core has focused, as after a closing sheet, so no
  // focus is reported for it.
  const shown = frame.shown !== "closed";
  const wasShown = useRef(shown);
  useEffect(() => {
    if (wasShown.current && !shown) {
      const paneId = useShellStore.getState().focusedPaneId;
      if (paneId) focusTerminal(paneId);
    }
    wasShown.current = shown;
  }, [shown]);
  if (!checkout || !view) return null;
  // Until the effect above has run, a panel that just turned narrow is
  // drawn with the overlay closed.
  const placement = frame.toolsOverlay ? (stored === "open" ? "open" : "closed") : "column";
  const tools = shownTools(view, placement);
  return (
    <section
      className="flex min-h-0 min-w-0 flex-1 flex-col"
      aria-label={`Workspace ${checkout.branch ?? checkout.label}`}
      data-workspace-screen={checkout.id}
      data-panel={frame.shown}
      data-panel-docked={frame.agentsRight > 0}
    >
      <WorkspaceToolbar checkout={checkout} view={view} explorer={tools.explorer} changes={tools.changes} actions={actions} />
      <div ref={setBody} className="relative min-h-0 min-w-0 flex-1 overflow-hidden" data-workspace-body={frame.narrow ? "narrow" : "wide"}>
        {/* Its own stacking context, so nothing the agents raise (a pane
            divider) draws over the panel; and out of reach of the pointer and
            the keyboard while an expanded panel covers it whole. */}
        <div
          className="absolute inset-y-0 left-0 isolate flex min-h-0 min-w-0 flex-col"
          style={{ right: frame.agentsRight }}
          inert={frame.shown === "expanded"}
          data-agent-area="true"
        >
          <AgentArea checkout={checkout} actions={actions} />
        </div>
        {shown ? <SidePanel view={view} frame={frame} explorer={tools.explorer} changes={tools.changes} body={body} sizes={sizes} actions={actions} /> : null}
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

/**
 * The path back (B4), the side panel's toggle, and the tools (B10), each
 * drawn pressed only while the panel shows it.
 */
function WorkspaceToolbar({ checkout, view, explorer, changes, actions }: { checkout: Checkout; view: WorkspaceView; explorer: boolean; changes: boolean; actions: Actions }) {
  const project = useShellStore((s) => catalogWorkspaces(s.rest).find((row) => row.checkouts.some((candidate) => candidate.id === checkout.id)) ?? null);
  const device = useShellStore((s) => focusedRemoteDevice(s.rest));
  const setScreen = useUiStore((s) => s.setScreen);
  const name = checkout.branch ?? checkout.label;
  // What the toolbar acts on is this Workspace: its panel, its tools, its
  // path and its Project (B18). Opening the menu changes none of them.
  const menuItems = (): MenuEntry<ToolbarMenuId>[] => [
    ...PANEL_STATES.map((state) => ({ id: `panel:${state.panel}` as const, label: `${state.panel === view.panel ? "✓ " : ""}${state.label}`, unavailable: null })),
    { id: "pin", label: view.pinned ? "Unpin side panel" : "Pin side panel", unavailable: null },
    { id: "explorer", label: explorer ? "Hide Explorer" : "Show Explorer", unavailable: null, separated: true },
    { id: "changes", label: changes ? "Hide History" : "Show History", unavailable: null },
    { id: "copy_path", label: "Copy Workspace path", unavailable: null, separated: true },
    { id: "overview", label: "Open Project Overview", unavailable: project ? null : "This Workspace's Project is not in the catalog" },
  ];
  const select = (id: ToolbarMenuId) => {
    if (id.startsWith("panel:")) return actions.setPanel(id.slice("panel:".length) as PanelState);
    if (id === "pin") return actions.setPanelPinned(!view.pinned);
    if (id === "explorer") return actions.setTool("explorer", !explorer);
    if (id === "changes") return actions.setTool("changes", !changes);
    if (id === "copy_path") return void navigator.clipboard?.writeText(checkout.path).catch(() => undefined);
    if (id === "overview" && project) setScreen({ kind: "overview", projectId: project.id });
  };
  return (
    <EntryContextMenu label={`Workspace ${name}`} items={menuItems} onSelect={select} className="shrink-0" data-workspace-menu="true">
      <div className="flex h-[var(--size-tab-strip)] shrink-0 items-center gap-sm border-b border-border bg-sidebar px-sm text-caption" data-workspace-toolbar="true">
        <nav aria-label="Location" className="flex min-w-0 flex-1 items-center gap-xs">
          <button type="button" className="shrink-0 rounded-xs px-xs text-subtle-foreground hover:bg-accent hover:text-foreground focus-visible:bg-accent" data-go-main="true" onClick={() => setScreen({ kind: "main" })}>
            Main
          </button>
          <span aria-hidden="true" className="text-muted-foreground">/</span>
          {project ? (
            <Hint label={`${project.label} · ${project.path}${device ? ` · ${device.label}` : ""}`}>
              <button
                type="button"
                className="min-w-0 max-w-[var(--size-recent-location-max)] shrink truncate rounded-xs px-xs text-subtle-foreground hover:bg-accent hover:text-foreground focus-visible:bg-accent"
                data-go-overview={project.id}
                onClick={() => setScreen({ kind: "overview", projectId: project.id })}
              >
                {project.label}
              </button>
            </Hint>
          ) : null}
          <span aria-hidden="true" className="text-muted-foreground">/</span>
          <Hint label={`${name} · ${checkout.path}`}>
            <span className="min-w-0 truncate text-foreground" aria-current="page">
              {name}
            </span>
          </Hint>
          {device ? (
            <Hint label={`On ${device.label}`}>
              <span className="shrink-0 rounded-xs bg-secondary px-xs text-micro text-subtle-foreground" data-workspace-device={device.id}>
                {device.label}
              </span>
            </Hint>
          ) : null}
        </nav>
        <PanelToggle view={view} actions={actions} />
        <div className="flex items-center gap-xxs" role="group" aria-label="Workspace tools">
          <ToolToggle tool="explorer" label="Explorer" on={explorer} actions={actions} />
          <ToolToggle tool="changes" label="History" on={changes} actions={actions} />
        </div>
      </div>
    </EntryContextMenu>
  );
}

type ToolbarMenuId = `panel:${PanelState}` | "pin" | "explorer" | "changes" | "copy_path" | "overview";

/**
 * The side panel's one toolbar control (issue 170): pressed while it shows,
 * and while it is closed with views open, their count as a small badge, so
 * the views it keeps are never out of mind. Neither direction resizes a
 * terminal unless the panel is pinned.
 */
function PanelToggle({ view, actions }: { view: WorkspaceView; actions: Actions }) {
  const on = view.panel !== "closed";
  const count = on ? 0 : (view.layout?.display_count ?? 0);
  const label = on ? "Hide side panel" : count > 0 ? `Show side panel, ${count} ${count === 1 ? "view" : "views"} open` : "Show side panel";
  return (
    <Hint label={label} shortcut={displayCommand("toggle_right_panel", hostKind())}>
      <Button variant={on ? "secondary" : "ghost"} size="icon-sm" className="relative" aria-pressed={on} aria-label={label} data-panel-toggle={on ? "on" : "off"} onClick={() => actions.setPanel(on ? "closed" : "open")}>
        <PanelRightIcon />
        {count > 0 ? (
          <span aria-hidden="true" className="absolute -right-xxs -top-xxs flex min-w-(--size-icon) items-center justify-center rounded-full bg-primary px-xxs text-micro leading-none text-primary-foreground" data-panel-badge={count}>
            {count}
          </span>
        ) : null}
      </Button>
    </Hint>
  );
}

// Explorer and History toggle on and off independently - both may show at
// once - so this is not a single-choice ToggleGroup; each stays its own
// icon button, and the pair keeps its `role="group"` wrapper (below). One
// pressed while the panel is closed opens the panel with it.
function ToolToggle({ tool, label, on, actions }: { tool: "explorer" | "changes"; label: string; on: boolean; actions: Actions }) {
  return (
    <Hint label={`${on ? "Hide" : "Show"} ${label}`}>
      <Button
        variant={on ? "secondary" : "ghost"}
        size="icon-sm"
        aria-pressed={on}
        data-tool-toggle={tool}
        onClick={() => actions.setTool(tool, !on)}
      >
        {tool === "explorer" ? <FolderIcon /> : <GitBranchIcon />}
      </Button>
    </Hint>
  );
}

/**
 * The side panel (issue 170): docked to the body's right edge, its left
 * border and resize edge marking it as over the agents. One strip runs
 * across its top: the View tabs above each top area, then New tab, Pin,
 * Expand and the toggle at its right end, above the tool column when it
 * shows. The left edge drags a guide line and sends one `views_over_share`
 * on release, so the panel itself does not move during a drag; a browser
 * display's slot inside the panel reports its rect like any other, and
 * closing the panel unmounts the slots, which hides their pages without
 * closing them.
 */
function SidePanel({ view, frame, explorer, changes, body, sizes, actions }: { view: WorkspaceView; frame: PanelFrame; explorer: boolean; changes: boolean; body: HTMLElement | null; sizes: PanelSizes; actions: Actions }) {
  const [guide, setGuide] = useState<number | null>(null);
  const share = view.views_over_share;
  const need = panelNeed(view.explorer || view.changes, sizes);
  const column = !frame.toolsOverlay && (explorer || changes);
  const panelActions = <PanelActions view={view} frame={frame} actions={actions} />;
  const drag = (event: React.PointerEvent<HTMLDivElement>) => {
    if (event.button !== 0 || !body) return;
    event.preventDefault();
    const left = body.getBoundingClientRect().left;
    const total = body.clientWidth;
    const target = event.currentTarget;
    target.setPointerCapture(event.pointerId);
    const shareOf = (next: PointerEvent) => panelShareAt(next.clientX - left, total, need, sizes.areaMin);
    const move = (next: PointerEvent) => setGuide(total - panelWidth(shareOf(next), total, need, sizes.areaMin));
    const end = () => {
      target.removeEventListener("pointermove", move);
      target.removeEventListener("pointerup", up);
      target.removeEventListener("pointercancel", end);
      target.removeEventListener("lostpointercapture", end);
      setGuide(null);
    };
    const up = (next: PointerEvent) => {
      end();
      const landed = shareOf(next);
      if (Math.abs(landed - share) > 0.001) actions.setWorkspaceView({ views_over_share: landed });
    };
    // A drag the system cancels lands nothing and leaves no guide behind.
    target.addEventListener("pointermove", move);
    target.addEventListener("pointerup", up);
    target.addEventListener("pointercancel", end);
    target.addEventListener("lostpointercapture", end);
  };
  return (
    <>
      <aside
        className="absolute inset-y-0 right-0 z-10 flex min-h-0 min-w-0 border-l border-border bg-background"
        style={{ width: frame.width }}
        aria-label="Side panel"
        data-side-panel={frame.shown}
        data-panel-content={frame.content}
      >
        {frame.resizable ? (
          <div
            role="separator"
            aria-orientation="vertical"
            aria-label="Resize the side panel"
            aria-valuenow={Math.round(share * 100)}
            aria-valuemin={20}
            aria-valuemax={80}
            tabIndex={0}
            data-panel-edge="true"
            className="absolute inset-y-0 left-0 z-20 w-[var(--size-resize-handle)] cursor-col-resize outline-none hover:bg-primary focus-visible:bg-primary"
            onPointerDown={drag}
            onKeyDown={(event) => {
              if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
              event.preventDefault();
              const total = body?.clientWidth ?? 0;
              if (total <= 0) return;
              // Left widens the panel, as its edge moves left.
              const step = event.key === "ArrowLeft" ? 0.05 : -0.05;
              const next = panelWidth(share + step, total, need, sizes.areaMin) / total;
              if (Math.abs(next - share) > 0.001) actions.setWorkspaceView({ views_over_share: next });
            }}
          />
        ) : null}
        {frame.content === "views" ? (
          <div className="flex min-h-0 min-w-0 flex-1 flex-col" data-view-area="true">
            <ViewAreas actions={actions} trailing={column ? null : panelActions} />
          </div>
        ) : frame.content === "empty" ? (
          <div className="flex min-h-0 min-w-0 flex-1 flex-col">
            <div className="flex h-[var(--size-tab-strip)] shrink-0 items-center justify-end bg-card">{panelActions}</div>
            <AreaEmpty state="no-view" text="No file or diff is open in this Workspace.">
              <Button variant="secondary" onClick={() => actions.setTool("explorer", true)} data-empty-show-explorer="true">
                Show Explorer
              </Button>
              <Button variant="ghost" onClick={() => actions.openFilePalette()} data-empty-open-file="true">
                Open file <span className="text-muted-foreground">{displayCommand("open_file", hostKind())}</span>
              </Button>
            </AreaEmpty>
          </div>
        ) : null}
        <Tools explorer={explorer} changes={changes} overlay={frame.toolsOverlay} alone={frame.content === "tools"} header={column ? panelActions : null} actions={actions} />
      </aside>
      {guide !== null ? <div className="pointer-events-none absolute inset-y-0 z-30 w-[var(--size-resize-handle)] bg-primary" style={{ left: guide }} data-panel-guide="true" /> : null}
    </>
  );
}

/**
 * The panel's own actions at the strip's right end: New tab (the file
 * palette), Pin, Expand while a view is open, and the toggle that closes it.
 */
function PanelActions({ view, frame, actions }: { view: WorkspaceView; frame: PanelFrame; actions: Actions }) {
  const expanded = view.panel === "expanded";
  const pinLabel = view.pinned ? "Unpin: float over the agents" : "Pin beside the agents";
  return (
    <div className="flex shrink-0 items-center gap-xxs px-xs" role="group" aria-label="Side panel actions" data-panel-actions="true">
      <Hint label="New tab: open a file" shortcut={displayCommand("open_file", hostKind())}>
        <Button variant="ghost" size="icon-sm" aria-label="New tab: open a file" data-panel-new-tab="true" onClick={() => actions.openFilePalette()}>
          <PlusIcon />
        </Button>
      </Hint>
      <Hint label={pinLabel}>
        <Button variant={view.pinned ? "secondary" : "ghost"} size="icon-sm" aria-pressed={view.pinned} aria-label={pinLabel} data-panel-pin={view.pinned ? "on" : "off"} onClick={() => actions.setPanelPinned(!view.pinned)}>
          {view.pinned ? <PinOffIcon /> : <PinIcon />}
        </Button>
      </Hint>
      {frame.content === "views" && !frame.narrow ? (
        <Hint label={expanded ? "Restore panel width" : "Expand panel"}>
          <Button
            variant="ghost"
            size="icon-sm"
            aria-pressed={expanded}
            aria-label={expanded ? "Restore panel width" : "Expand panel"}
            data-panel-expand={expanded ? "on" : "off"}
            onClick={() => actions.setPanel(expanded ? "open" : "expanded")}
          >
            {expanded ? <Minimize2Icon /> : <Maximize2Icon />}
          </Button>
        </Hint>
      ) : null}
      <Hint label="Hide side panel" shortcut={displayCommand("toggle_right_panel", hostKind())}>
        <Button variant="ghost" size="icon-sm" aria-label="Hide side panel" data-panel-close="true" onClick={() => actions.setPanel("closed")}>
          <PanelRightCloseIcon />
        </Button>
      </Hint>
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
          <Button variant="secondary" onClick={() => actions.createTab()} data-empty-new-tab="true">
            New tab <span className="text-muted-foreground">{displayCommand("new_tab", hostKind())}</span>
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
      <div className="flex min-h-0 flex-1 flex-col items-start justify-center gap-sm p-xl text-body text-subtle-foreground" data-remote-device-surface={device.id} data-remote-state={status?.state ?? "none"}>
        <h2 className="text-title font-semibold text-foreground">
          {device.label} <span className="font-mono text-caption text-muted-foreground">{device.ssh_alias}</span>
        </h2>
        <p>{connected ? `${device.label} is connected, but no Herdr workspace is open there.` : `${device.label} is ${line.text}${status?.message ? `: ${status.message}` : "."}`}</p>
        <div className="flex gap-sm">
          {canRetryDevice(device, status ?? undefined) ? (
            <Button variant="secondary" onClick={() => actions.retryDevice(device.id)} data-remote-retry={device.id}>
              Retry
            </Button>
          ) : null}
          <Button variant="ghost" onClick={() => actions.focusDevice("local")} data-use-local-device="true">
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
        <div role="status" className="flex items-center gap-md border-b border-border bg-card px-md py-xs text-caption text-warning" data-remote-stale={device.id}>
          {status?.message ? (
            <Hint label={status.message} reveals>
              <span className="min-w-0 flex-1 truncate">
                {device.label} is not connected: {status.message}. Showing the last state it reported; nothing is sent until it reconnects.
              </span>
            </Hint>
          ) : (
            <span className="min-w-0 flex-1 truncate">
              {device.label} is not connected. Showing the last state it reported; nothing is sent until it reconnects.
            </span>
          )}
          {canRetryDevice(device, status ?? undefined) ? (
            <Button variant="secondary" onClick={() => actions.retryDevice(device.id)} data-remote-retry={device.id}>
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
