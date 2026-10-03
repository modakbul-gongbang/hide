import { ArrowLeftRightIcon, Maximize2Icon, Minimize2Icon, PanelRightDashedIcon, PanelRightIcon, PinIcon, PinOffIcon, ServerIcon } from "lucide-react";
import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import type { Actions } from "./actions";
import { EntryContextMenu, type MenuEntry } from "./components/entry-menu";
import { Button } from "./components/ui/button";
import { Hint } from "./components/ui/tooltip";
import { RemotePaneCanvas } from "./PaneGrid";
import { remoteView } from "./remote";
import { holdShellDrag } from "./shellDrag";
import { canRetryDevice, deviceLine } from "./settings";
import { catalogWorkspaces, focusedRemoteDevice, frontCheckout, type Checkout } from "./snapshot";
import { overviewScreen } from "./navigation";
import { useShellStore } from "./store";
import { focusTerminal } from "./terminals";
import { AgentAreas } from "./AgentAreas";
import { Tools } from "./Tools";
import { RunningServers } from "./RunningServers";
import { useUiStore } from "./ui";
import { ViewAreas } from "./ViewAreas";
import { shownTool } from "./viewLayout";
import { PANEL_STATES, panelCoversToSend, panelFrame, panelShareAt, panelWidth, workspaceViewOf, type PanelFrame, type PanelSizes, type PanelState, type Tool, type WorkspaceView } from "./workspace";
import { keyboardOwner, noteKeyboardOwner } from "./viewFocus";
import { commandLabel } from "./shortcutLabels";

// A Workspace (PRD S6 D-01..D-05, B4-B11; S7 B12, B13; issue 170): one
// checkout's agent column (its toolbar, then its Herdr tabs and their panes),
// always the Workspace's full width, and a side panel docked to its right
// edge over it at the Workspace's full height, holding the View areas (its
// files, diffs and pages) with the tool column beside them. The panel's
// state, pin, width and tool are the core's, per Workspace; this draws them
// and sends one event per operator choice. A closed panel is unmounted, never
// closed: its views stay in the core.
//
// Every control sits once, on the container it changes (issue 170, "Side
// panel hierarchy, revised"): the toolbar holds the path back, the server globe and, while
// the panel is closed, its toggle; the panel's first row, at the toolbar's
// height, holds each area's tabs and New tab, then the tool column's toggle,
// Expand, Pin and the panel toggle; its second row, level with the agents'
// tab strip, holds the document header and the tool tabs.
//
// The panel floats over agents that keep their size and stay live beside it,
// so opening, closing, resizing and expanding it never resizes a terminal;
// only a pinned panel narrows the agent column to its left edge. A window too
// narrow for both draws the panel over the whole Workspace, and a pinned one
// floats there, without changing what the core stores, so widening brings it
// back.

/** Where every device keeps its Home (PRD home-device-rail D-03). */
const HOME_FOLDER = "~/hide";
const CLOSED: PanelFrame = { shown: "closed", content: "tools", width: 0, agentsRight: 0, narrow: false, resize: null, need: 0, toolsOverlay: false };

export function WorkspaceScreen({ actions }: { actions: Actions }) {
  const checkout = useShellStore((s) => frontCheckout(s.rest));
  const view = useShellStore((s) => workspaceViewOf(s.rest));
  const stored = useUiStore((s) => s.toolsPlacement);
  const [body, setBody] = useState<HTMLElement | null>(null);
  const width = useWidth(body);
  const sizes = useMemo<PanelSizes>(
    () => ({ areaMin: tokenPx("--size-workspace-area-min"), toolColumn: tokenPx("--size-panel-ideal"), gap: tokenPx("--spacing-sm"), hairline: tokenPx("--size-hairline") }),
    [],
  );
  const opening = useShellStore((s) => (s.editor?.opening ?? []).some((row) => row.checkout_id === checkout?.id));
  const views = (view?.layout?.display_count ?? 0) > 0 || opening;
  const frame = view ? panelFrame({ view, views, body: width, sizes }) : CLOSED;
  // The tool column while the panel has room for it beside a View area;
  // past that, an overlay inside the panel that stays closed until the
  // operator asks for the tools (S7 B12).
  // It runs as the panel shows too, so a tool asked for with a closed panel
  // opens the overlay of a panel that turns out narrow.
  const panelShown = frame.shown !== "closed";
  useEffect(() => {
    if (panelShown) useUiStore.getState().setToolsNarrow(frame.toolsOverlay);
  }, [frame.toolsOverlay, panelShown]);
  // An overlay opened in one Workspace is not left open over the next one,
  // nor over this screen when the operator comes back to it; one left with
  // no tool to show closes too, so a tool shown later does not float over
  // the work by itself.
  const checkoutId = checkout?.id ?? null;
  // Making the Agent area inert retires its keyboard target before another
  // chord can close a pane hidden by the panel.
  useLayoutEffect(() => {
    const owner = keyboardOwner();
    if (frame.shown === "expanded" && owner.kind === "pane" && owner.workspace === checkoutId) {
      noteKeyboardOwner({ kind: "none" });
    }
  }, [frame.shown, checkoutId]);
  useEffect(() => () => useUiStore.getState().closeTools(), [checkoutId]);
  // Only the page sees the window, so it tells the core whether this
  // Workspace's panel takes the whole body when it shows, a fact of the
  // window and what the Workspace holds, not of the panel being open: it is
  // sent once per crossing and never per resize, so an agent chosen from
  // elsewhere uncovers it even when pinned (issue 170). Nothing is reported
  // before the body is measured.
  const covers = view ? panelFrame({ view: { ...view, panel: view.panel === "closed" ? "open" : view.panel }, views, body: width, sizes }).narrow : false;
  const measured = width > 0;
  const coreCovers = view?.covered === true;
  const deviceId = view?.device_id ?? null;
  const viewPath = view?.path ?? null;
  // The page's last report holds only while it draws the same Workspace
  // over one live connection: the core forgets its value whenever the front
  // moves, and a report sent while the socket was down never arrived. When
  // this screen is not drawn (All projects, an Overview) nothing is sent: the
  // core keeps what the page last saw until the front moves.
  const live = useShellStore((s) => s.connection === "live");
  const lastSent = useRef<{ key: string; covers: boolean } | null>(null);
  useEffect(() => {
    if (!live) {
      lastSent.current = null;
      return;
    }
    if (!measured || deviceId === null || viewPath === null) return;
    const key = `${deviceId}\u0000${viewPath}`;
    if (lastSent.current?.key !== key) lastSent.current = null;
    if (!panelCoversToSend(covers, coreCovers, lastSent.current?.covers ?? null)) return;
    if (actions.reportPanelCovers({ device_id: deviceId, path: viewPath }, covers) === false) return;
    lastSent.current = { key, covers };
  }, [actions, live, measured, deviceId, viewPath, covers, coreCovers]);
  const toolless = view ? !view.tools : true;
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
  const tool = shownTool(view, placement);
  return (
    <section
      ref={setBody}
      className="relative min-h-0 min-w-0 flex-1 overflow-hidden"
      aria-label={`Workspace ${checkout.branch ?? checkout.label}`}
      data-workspace-screen={checkout.id}
      data-panel={frame.shown}
      data-panel-docked={frame.agentsRight > 0}
      data-workspace-body={frame.narrow ? "narrow" : "wide"}
    >
      {/* Its own stacking context, so nothing the agents raise (a pane
          divider) draws over the panel; and out of reach of the pointer and
          the keyboard while an expanded panel covers it whole. */}
      <div
        className="absolute inset-y-0 left-0 isolate flex min-h-0 min-w-0 flex-col"
        style={{ right: frame.agentsRight }}
        inert={frame.shown === "expanded"}
        data-agent-area="true"
      >
        <WorkspaceToolbar checkout={checkout} view={view} panelShown={shown} coveredRight={shown ? Math.max(0, frame.width - frame.agentsRight) : 0} actions={actions} />
        <AgentArea checkout={checkout} actions={actions} />
      </div>
      {shown ? <SidePanel view={view} frame={frame} tool={tool} placement={placement} body={body} sizes={sizes} actions={actions} /> : null}
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
 * The path back (B4), over the agent column only, and the side panel's
 * toggle at its right end while the panel is closed; an open panel carries
 * the toggle in the same spot.
 */
function WorkspaceToolbar({ checkout, view, panelShown, coveredRight, actions }: { checkout: Checkout; view: WorkspaceView; panelShown: boolean; coveredRight: number; actions: Actions }) {
  const project = useShellStore((s) => catalogWorkspaces(s.rest).find((row) => row.checkouts.some((candidate) => candidate.id === checkout.id)) ?? null);
  const device = useShellStore((s) => focusedRemoteDevice(s.rest));
  const setScreen = useUiStore((s) => s.setScreen);
  const name = checkout.branch ?? checkout.label;
  // What the toolbar acts on is this Workspace: its panel, its path and its
  // Project (B18). Opening the menu changes none of them.
  const menuItems = (): MenuEntry<ToolbarMenuId>[] => [
    ...PANEL_STATES.map((state) => ({ id: `panel:${state.panel}` as const, label: `${state.panel === view.panel ? "✓ " : ""}${state.label}`, unavailable: null })),
    { id: "pin", label: view.pinned ? "Unpin side panel" : "Pin side panel", unavailable: null },
    { id: "copy_path", label: "Copy Workspace path", unavailable: null, separated: true },
    { id: "overview", label: "Open Project Overview", unavailable: project ? null : "This Workspace's Project is not in the catalog" },
  ];
  const select = (id: ToolbarMenuId) => {
    if (id.startsWith("panel:")) return actions.setPanel(id.slice("panel:".length) as PanelState);
    if (id === "pin") return actions.setPanelPinned(!view.pinned);
    if (id === "copy_path") return void navigator.clipboard?.writeText(checkout.path).catch(() => undefined);
    if (id === "overview" && project) setScreen(overviewScreen(useShellStore.getState().rest, project.id));
  };
  return (
    <EntryContextMenu label={`Workspace ${name}`} items={menuItems} onSelect={select} className="shrink-0" data-workspace-menu="true">
      <div className="flex h-[var(--size-tab-strip)] shrink-0 items-center gap-sm border-b border-border bg-sidebar pl-sm pr-sm text-caption" data-workspace-toolbar="true" style={{ marginRight: coveredRight }}>
        {device ? (
          // The front thing is a remote device's: its color and name lead the toolbar, and the pane border wears the same color (PRD home-device-rail D-15).
          <span className="-ml-sm flex h-full max-w-(--size-recent-location-max) shrink-0 items-center gap-xs bg-device-remote px-sm font-medium text-primary-foreground" data-device-band={device.id}>
            <ServerIcon aria-hidden="true" className="size-(--size-icon-sm) shrink-0" />
            <span className="min-w-0 truncate">{device.label}</span>
          </span>
        ) : null}
        <nav aria-label="Location" className="flex min-w-0 flex-1 items-center gap-xs">
          <button type="button" className="shrink-0 rounded-xs px-xs text-subtle-foreground hover:bg-accent hover:text-foreground focus-visible:bg-accent" data-go-main="true" onClick={() => setScreen({ kind: "main" })}>
            Home
          </button>
          {project && !project.is_home ? (
            <>
              <span aria-hidden="true" className="text-muted-foreground">/</span>
              <Hint label={`${project.label} · ${project.path}${device ? ` · ${device.label}` : ""}`}>
                <button
                  type="button"
                  className="min-w-0 max-w-[var(--size-recent-location-max)] shrink truncate rounded-xs px-xs text-subtle-foreground hover:bg-accent hover:text-foreground focus-visible:bg-accent"
                  data-go-overview={project.id}
                  onClick={() => setScreen(overviewScreen(useShellStore.getState().rest, project.id))}
                >
                  {project.label}
                </button>
              </Hint>
            </>
          ) : null}
          <span aria-hidden="true" className="text-muted-foreground">/</span>
          {/* Home is no project (B16): its tab is `Home / ~/hide`, the folder every device keeps it in (D-03). */}
          <Hint label={`${project?.is_home ? HOME_FOLDER : name} · ${checkout.path}`}>
            <span className="min-w-0 truncate text-foreground" aria-current="page" data-workspace-location={project?.is_home ? "home" : "checkout"}>
              {project?.is_home ? HOME_FOLDER : name}
            </span>
          </Hint>
        </nav>
        <RunningServers key={checkout.id} checkout={checkout} view={view} actions={actions} />
        {panelShown ? null : <PanelToggle view={view} actions={actions} />}
      </div>
    </EntryContextMenu>
  );
}

type ToolbarMenuId = `panel:${PanelState}` | "pin" | "copy_path" | "overview";

/**
 * The side panel's toggle (issue 170), at the Workspace's top right in either
 * state: pressed while the panel shows, and while it is closed with views
 * open, their count as a small badge, so the views it keeps are never out of
 * mind. Neither direction resizes a terminal unless the panel is pinned.
 */
function PanelToggle({ view, actions }: { view: WorkspaceView; actions: Actions }) {
  const on = view.panel !== "closed";
  const count = on ? 0 : (view.layout?.display_count ?? 0);
  const views = count > 0 ? `${count} ${count === 1 ? "view" : "views"} open` : undefined;
  // One name for the control, its state in aria-pressed; the tooltip says what a press does.
  return (
    <Hint label={on ? "Hide side panel" : views ? `Show side panel, ${views}` : "Show side panel"} shortcut={commandLabel("toggle_right_panel")}>
      <Button
        variant={on ? "secondary" : "ghost"}
        size="icon-sm"
        className="relative"
        aria-pressed={on}
        aria-label="Side panel"
        aria-description={views}
        data-panel-toggle={on ? "on" : "off"}
        onClick={() => actions.setPanel(on ? "closed" : "open")}
      >
        <PanelRightIcon />
        {count > 0 ? (
          <span aria-hidden="true" className="absolute -right-xxs -top-xxs flex h-(--size-icon) min-w-(--size-icon) items-center justify-center rounded-full bg-primary px-xxs text-micro text-primary-foreground" data-panel-badge={count}>
            {count}
          </span>
        ) : null}
      </Button>
    </Hint>
  );
}

/**
 * The side panel (issue 170): a --card surface at the Workspace's full
 * height, docked to its right edge, with a --spacing-sm gap on its left that
 * is the resize grip. Its first row carries the View tabs above each top
 * area and the panel's actions at its right end, over the tool column when
 * that shows. Hovering the gap shows the grip, a hairline with a ⇆ pill; a
 * drag moves the grip as a guide and sends the panel's one stored width on
 * release (the tools-only panel keeps its own), so the panel itself does not
 * move during a drag; a browser display's slot inside the panel reports its rect like any
 * other, and closing the panel unmounts the slots, which hides their pages
 * without closing them.
 */
function SidePanel({ view, frame, tool, placement, body, sizes, actions }: { view: WorkspaceView; frame: PanelFrame; tool: Tool | null; placement: "column" | "open" | "closed"; body: HTMLElement | null; sizes: PanelSizes; actions: Actions }) {
  const [guide, setGuide] = useState<number | null>(null);
  const resize = frame.resize;
  const need = frame.need;
  // The share the panel is drawn at, which a key steps from and assistive
  // technology reads.
  const share = body && body.clientWidth > 0 ? frame.width / body.clientWidth : view.views_over_share;
  const land = (next: number) => {
    if (resize && Math.abs(next - share) > 0.001) actions.setWorkspaceView({ [resize]: next });
  };
  const column = !frame.toolsOverlay && tool !== null;
  // The column toggle reads pressed while the tools show: the column, or a
  // narrow panel's overlay while it is open.
  const toolsShown = frame.toolsOverlay ? view.tools && placement === "open" : view.tools;
  const panelActions = <PanelActions view={view} frame={frame} toolsShown={toolsShown} actions={actions} />;
  const drag = (event: React.PointerEvent<HTMLDivElement>) => {
    if (event.button !== 0 || !body) return;
    event.preventDefault();
    const left = body.getBoundingClientRect().left;
    const total = body.clientWidth;
    const target = event.currentTarget;
    target.setPointerCapture(event.pointerId);
    const shareOf = (next: PointerEvent) => panelShareAt(next.clientX - left, total, need, sizes.areaMin);
    const move = (next: PointerEvent) => setGuide(total - panelWidth(shareOf(next), total, need, sizes.areaMin));
    // The drag marks the root, so a page the guide crosses gives way to its still.
    const release = holdShellDrag("col-resize");
    const end = () => {
      release();
      target.removeEventListener("pointermove", move);
      target.removeEventListener("pointerup", up);
      target.removeEventListener("pointercancel", end);
      target.removeEventListener("lostpointercapture", end);
      setGuide(null);
    };
    const up = (next: PointerEvent) => {
      end();
      land(shareOf(next));
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
        className="absolute inset-y-0 right-0 z-10 flex min-h-0 min-w-0 bg-background"
        style={{ width: frame.width }}
        aria-label="Side panel"
        data-side-panel={frame.shown}
        data-panel-content={frame.content}
      >
        {resize ? (
          <div
            role="separator"
            aria-orientation="vertical"
            aria-label="Resize the side panel"
            aria-valuenow={Math.round(share * 100)}
            aria-valuemin={20}
            aria-valuemax={80}
            tabIndex={0}
            data-panel-edge="true"
            className="group relative z-10 flex w-sm shrink-0 cursor-col-resize justify-center outline-none"
            onPointerDown={drag}
            onKeyDown={(event) => {
              if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
              event.preventDefault();
              const total = body?.clientWidth ?? 0;
              if (total <= 0) return;
              // Left widens the panel, as its edge moves left.
              const step = event.key === "ArrowLeft" ? 0.05 : -0.05;
              land(panelWidth(share + step, total, need, sizes.areaMin) / total);
            }}
          >
            {/* While a drag moves the guide, the grip travels with it. */}
            <PanelGrip className={guide === null ? "opacity-0 group-hover:opacity-100 group-focus-visible:opacity-100" : "opacity-0"} />
          </div>
        ) : (
          <div aria-hidden="true" className="w-sm shrink-0" />
        )}
        <div className="relative flex min-h-0 min-w-0 flex-1 overflow-hidden rounded-tl-lg border border-border bg-card" data-panel-card="true">
          {frame.content === "views" ? (
            <>
              <div className="flex min-h-0 min-w-0 flex-1 flex-col" data-view-area="true">
                <ViewAreas actions={actions} trailing={column ? null : panelActions} />
              </div>
              <Tools tool={tool} overlay={frame.toolsOverlay} header={column ? panelActions : null} actions={actions} />
            </>
          ) : (
            <Tools tool={view.tool} overlay={false} alone header={panelActions} actions={actions} />
          )}
        </div>
      </aside>
      {guide !== null ? (
        <div className="pointer-events-none absolute inset-y-0 z-30 flex w-sm justify-center" style={{ left: guide }} data-panel-guide="true">
          <PanelGrip />
        </div>
      ) : null}
    </>
  );
}

/** The resize grip (issue 170; `Component / Side panel grip`): a hairline with a ⇆ pill at its middle, centred on the gap so the pill overlaps the card's edge, drawn on hover, keyboard focus and while dragging. */
function PanelGrip({ className = "" }: { className?: string }) {
  return (
    <span aria-hidden="true" className={`relative h-full w-(--size-hairline) ${className}`} data-panel-grip="true">
      <span className="absolute inset-0 bg-muted-foreground opacity-[var(--opacity-secondary)]" />
      <span className="absolute left-1/2 top-1/2 flex size-(--size-icon-button-toolbar) -translate-x-1/2 -translate-y-1/2 items-center justify-center rounded-md border border-border bg-card text-muted-foreground">
        <ArrowLeftRightIcon className="size-(--size-icon-sm)" />
      </span>
    </span>
  );
}

/**
 * The panel's actions at the right end of its first row: the tool column's
 * toggle while a view leaves room for it, Expand while a
 * view is open, Pin, and the panel toggle. A narrow window has no Expand or
 * Pin, since the panel already covers the Workspace there.
 */
function PanelActions({ view, frame, toolsShown, actions }: { view: WorkspaceView; frame: PanelFrame; toolsShown: boolean; actions: Actions }) {
  const checkout = useShellStore((s) => frontCheckout(s.rest));
  const expanded = view.panel === "expanded";
  const pinLabel = view.pinned ? "Unpin: float over the agents" : "Pin beside the agents";
  return (
    <div className="flex shrink-0 items-center gap-xxs pl-xs pr-sm" role="group" aria-label="Side panel actions" data-panel-actions="true">
      {frame.shown === "expanded" && checkout ? <RunningServers checkout={checkout} view={view} actions={actions} /> : null}
      {frame.content === "tools" ? null : (
        <Hint label={toolsShown ? "Hide tools" : "Show tools"}>
          <Button
            variant={toolsShown ? "secondary" : "ghost"}
            size="icon-sm"
            aria-pressed={toolsShown}
            aria-label="Tools"
            data-tools-toggle={toolsShown ? "on" : "off"}
            onClick={() => actions.setToolsShown(!toolsShown)}
          >
            <PanelRightDashedIcon />
          </Button>
        </Hint>
      )}
      {frame.content === "views" && !frame.narrow ? (
        <Hint label={expanded ? "Restore panel width" : "Expand panel"}>
          <Button
            variant="ghost"
            size="icon-sm"
            aria-pressed={expanded}
            aria-label="Expand side panel"
            data-panel-expand={expanded ? "on" : "off"}
            onClick={() => actions.setPanel(expanded ? "open" : "expanded")}
          >
            {expanded ? <Minimize2Icon /> : <Maximize2Icon />}
          </Button>
        </Hint>
      ) : null}
      {frame.narrow ? null : (
        <Hint label={pinLabel}>
          <Button variant={view.pinned ? "secondary" : "ghost"} size="icon-sm" aria-pressed={view.pinned} aria-label="Pin side panel" data-panel-pin={view.pinned ? "on" : "off"} onClick={() => actions.setPanelPinned(!view.pinned)}>
            {view.pinned ? <PinOffIcon /> : <PinIcon />}
          </Button>
        </Hint>
      )}
      <PanelToggle view={view} actions={actions} />
    </div>
  );
}

/** This machine's tabs, or the selected SSH device's own tabs and panes in their place (S5 B19). */
function AgentArea({ checkout, actions }: { checkout: Checkout; actions: Actions }) {
  const remote = useShellStore((s) => focusedRemoteDevice(s.rest) !== null);
  if (remote) return <RemoteAgentArea actions={actions} />;
  return <LocalAgentArea checkout={checkout} actions={actions} />;
}

/**
 * The strip lists only the checkout's operator tabs, but the canvas draws
 * whichever tab the core made visible: a tab holding nothing but delegated
 * children stays off the strip while the sidebar still reaches it (PRD
 * hide-orchestrator B1, B4), so a checkout whose only tab is delegated is
 * not empty. The empty state means there is no tab at all.
 */
function LocalAgentArea({ checkout, actions }: { checkout: Checkout; actions: Actions }) {
  return <AgentAreas checkout={checkout} actions={actions} />;
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
      <AgentAreas checkout={view.checkout} deviceId={device.id} actions={actions} remoteBody={<RemotePaneCanvas view={view} connected={connected} actions={actions} />} />
    </>
  );
}
