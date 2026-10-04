import { ArrowLeftRightIcon, FilesIcon, PanelRightIcon, ServerIcon } from "lucide-react";
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
import { workspaceKey } from "./viewLayout";
import { bodyStep, columnFrame, dividerLanding, slotsForStep, workspaceViewOf, type BodyStep, type ColumnFrame, type ColumnSizes, type SideColumn, type WorkspaceView } from "./workspace";
import { keyboardOwner, noteKeyboardOwner } from "./viewFocus";
import { commandLabel } from "./shortcutLabels";
import "./columnCalls";

// A Workspace (PRD S6 D-01..D-05; S7 B12, B13; PRD three-column-panel D-01,
// D-02, D-04, D-07, D-12): one checkout's toolbar across the whole body, and
// under it three docked columns, left to right Agent Views (its Herdr tabs
// and their panes), File Views (its files, diffs and pages) and Tools (the
// Explorer or History). No column is drawn over another. Whether File Views
// and Tools are on, the tool and the two widths are the core's, per
// Workspace; this draws them and sends one event per operator choice. A
// column that is off is unmounted, never closed: its views stay in the core.
//
// Agent Views takes what the other columns leave, so turning a column on or
// off, a divider landing, or the body crossing a step resizes the agents'
// terminals once (D-02); a drag moves only a guide until it lands. A body
// too narrow for every column that is on hides columns without storing it
// (`columnFrame`), so widening brings back what the core stores.

/** Where every device keeps its Home (PRD home-device-rail D-03). */
const HOME_FOLDER = "~/hide";
/** One arrow key on a focused divider moves it this many pixels. */
const DIVIDER_STEP = 32;

export function WorkspaceScreen({ actions }: { actions: Actions }) {
  const checkout = useShellStore((s) => frontCheckout(s.rest));
  const view = useShellStore((s) => workspaceViewOf(s.rest));
  const slots = useUiStore((s) => s.columnSlots);
  const [body, setBody] = useState<HTMLElement | null>(null);
  const width = useWidth(body);
  const sizes = useMemo<ColumnSizes>(
    () => ({
      agentMin: tokenPx("--size-agent-views-min"),
      viewsMin: tokenPx("--size-file-views-min"),
      toolsMin: tokenPx("--size-panel-min"),
      viewsIdeal: tokenPx("--size-file-views-ideal"),
      toolsIdeal: tokenPx("--size-panel-ideal"),
      divider: tokenPx("--spacing-sm"),
    }),
    [],
  );
  const step = bodyStep(width, sizes);
  const previousStep = useRef<BodyStep | null>(null);
  // Draw the new step's default in its first frame, before retiring the old
  // call. This avoids fitting terminals to an intermediate wrong column.
  const stepSlots = slotsForStep(slots, previousStep.current, step);
  useLayoutEffect(() => {
    if (width <= 0) return;
    if (previousStep.current !== null && previousStep.current !== step) useUiStore.getState().resetColumnSlots();
    previousStep.current = step;
  }, [width, step]);
  const opening = useShellStore((s) => (s.editor?.opening ?? []).some((row) => row.checkout_id === checkout?.id));
  const hasViews = (view?.layout?.display_count ?? 0) > 0 || opening;
  const frame = columnFrame({
    views: view?.views === true && hasViews,
    tools: view?.tools === true,
    viewsWidth: view?.views_width ?? null,
    toolsWidth: view?.tools_width ?? null,
    body: width,
    sizes,
    slots: stepSlots,
  });
  const checkoutId = checkout?.id ?? null;
  const viewsShown = frame.views !== null;
  const toolsShown = frame.tools !== null;
  const agentsCovered = frame.agents === null;
  // The toolbar's icons and the column chords read what is drawn.
  useLayoutEffect(() => {
    useUiStore.getState().setShownColumns({ views: viewsShown, tools: toolsShown, step: frame.step });
  }, [viewsShown, toolsShown, frame.step]);
  // Agent Views under a narrow body's one column keeps its size and takes
  // no keyboard, so its keyboard target is retired before a chord can act on
  // a pane out of sight.
  useLayoutEffect(() => {
    const owner = keyboardOwner();
    if (agentsCovered && (owner.kind === "pane" || owner.kind === "agent") && owner.workspace === checkoutId) noteKeyboardOwner({ kind: "none" });
  }, [agentsCovered, checkoutId]);
  // A column that leaves with the keyboard inside it hands the keyboard to
  // File Views' active area while that shows, else to the pane the core has
  // focused, as after a closing sheet, so no focus is reported for it (B24).
  const shown = useRef({ views: viewsShown, tools: toolsShown });
  const activeArea = view?.layout?.active_area ?? null;
  const key = view ? workspaceKey(view) : null;
  useEffect(() => {
    const before = shown.current;
    shown.current = { views: viewsShown, tools: toolsShown };
    const owner = keyboardOwner();
    const left = (before.views && !viewsShown && owner.kind === "view") || (before.tools && !toolsShown && owner.kind === "tool");
    if (!left) return;
    if (viewsShown && key && activeArea) return useUiStore.getState().setViewFocusRequest({ workspace: key, areaId: activeArea });
    const paneId = useShellStore.getState().focusedPaneId;
    if (paneId) focusTerminal(paneId);
  }, [viewsShown, toolsShown, key, activeArea]);
  if (!checkout || !view) return null;
  return (
    <section
      className="flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden"
      aria-label={`Workspace ${checkout.branch ?? checkout.label}`}
      data-workspace-screen={checkout.id}
      data-file-views={viewsShown ? "shown" : view.views ? "hidden" : "off"}
      data-tools={toolsShown ? "shown" : view.tools ? "hidden" : "off"}
      data-workspace-body={frame.step}
    >
      <WorkspaceToolbar checkout={checkout} view={view} viewsShown={viewsShown} toolsShown={toolsShown} actions={actions} />
      <ColumnRow view={view} frame={frame} body={body} setBody={setBody} sizes={sizes} actions={actions} checkout={checkout} />
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
 * The body under the toolbar: Agent Views, then File Views and Tools while
 * they show, each beside a divider on its left. Agent Views is always
 * mounted; under a narrow body's other column it keeps the body's size out
 * of sight, so moving between the one column and Agent Views resizes no
 * terminal.
 */
function ColumnRow({ view, frame, body, setBody, sizes, actions, checkout }: { view: WorkspaceView; frame: ColumnFrame; body: HTMLElement | null; setBody: (element: HTMLElement | null) => void; sizes: ColumnSizes; actions: Actions; checkout: Checkout }) {
  const [guide, setGuide] = useState<number | null>(null);
  const narrow = frame.step === "narrow";
  const covered = frame.agents === null;
  const divider = (column: SideColumn) => <ColumnDivider workspace={workspaceKey(view)} acknowledged={view.width_request_id ?? null} column={column} frame={frame} body={body} sizes={sizes} setGuide={setGuide} actions={actions} />;
  return (
    <div ref={setBody} className="relative flex min-h-0 min-w-0 flex-1" data-column-row="true">
      {/* Its own stacking context, so nothing the agents raise (a pane
          divider) draws over a column; out of sight and out of reach of the
          pointer and the keyboard while a narrow body shows another column. */}
      <div
        className={`isolate flex min-h-0 min-w-0 flex-1 flex-col ${covered ? "invisible absolute inset-0" : ""}`}
        inert={covered}
        data-agent-area="true"
        data-column="agents"
      >
        <AgentArea checkout={checkout} actions={actions} />
      </div>
      {frame.views !== null ? (
        <>
          {narrow ? null : divider("views")}
          <div
            className={`flex min-h-0 min-w-0 shrink-0 flex-col bg-card ${narrow ? "absolute inset-0" : ""}`}
            style={narrow ? undefined : { width: frame.views }}
            aria-label="File Views"
            role="region"
            data-column="views"
            data-view-area="true"
          >
            <ViewAreas actions={actions} />
          </div>
        </>
      ) : null}
      {frame.tools !== null ? (
        <>
          {narrow ? null : divider("tools")}
          <div
            className={`flex min-h-0 min-w-0 shrink-0 flex-col bg-card ${narrow ? "absolute inset-0" : ""}`}
            style={narrow ? undefined : { width: frame.tools }}
            data-column="tools"
          >
            <Tools tool={view.tool} actions={actions} />
          </div>
        </>
      ) : null}
      {guide !== null ? (
        <div className="pointer-events-none absolute inset-y-0 z-30 flex w-sm justify-center" style={{ left: guide }} data-column-guide="true">
          <DividerGrip />
        </div>
      ) : null}
    </div>
  );
}

/**
 * The divider on a column's left (D-12; `Component / Side panel grip`):
 * nothing at rest, and on hover, keyboard focus and while dragging a hairline
 * with a ⇆ pill at its middle. A drag moves a guide and lands once on
 * release, so no terminal or page resizes during it and every page shows its
 * still; a focused divider moves one step per arrow key, one event per press.
 */
function ColumnDivider({ workspace, acknowledged, column, frame, body, sizes, setGuide, actions }: { workspace: string; acknowledged: string | null; column: SideColumn; frame: ColumnFrame; body: HTMLElement | null; sizes: ColumnSizes; setGuide: (x: number | null) => void; actions: Actions }) {
  const cancelDrag = useRef<(() => void) | null>(null);
  const [keySession] = useState(() => crypto.randomUUID());
  const keyIntent = useRef<{ frame: ColumnFrame; published: ColumnFrame; body: number; request: string; observed: string | null } | null>(null);
  // Keep one absolute target until its own acknowledgement, not an older
  // echo from this key session. Other callers and changed geometry rebase it.
  useLayoutEffect(() => {
    const pending = keyIntent.current;
    if (pending && (acknowledged === pending.request ||
      ((acknowledged !== pending.observed || frame.views !== pending.published.views || frame.tools !== pending.published.tools) && !acknowledged?.startsWith(`${keySession}:`)) ||
      pending.body !== body?.clientWidth || pending.frame.step !== frame.step ||
      (pending.frame.views === null) !== (frame.views === null) || (pending.frame.tools === null) !== (frame.tools === null))) {
      keyIntent.current = null;
    }
  }, [acknowledged, keySession, body, frame]);
  useLayoutEffect(() => () => { keyIntent.current = null; }, [workspace, body]);
  // A captured pointer belongs to the Workspace and geometry it started on.
  // Switching either, or unmounting, retires the guide and its page still.
  useLayoutEffect(() => () => cancelDrag.current?.(), [workspace, body, frame.agents, frame.views, frame.tools]);
  const width = (column === "views" ? frame.views : frame.tools) ?? 0;
  const name = column === "views" ? "File Views" : "Tools";
  const land = (x: number, basis = frame, request?: string) => {
    const total = body?.clientWidth ?? 0;
    if (total <= 0) return;
    const patch = dividerLanding({ divider: column, x, body: total, frame: basis, sizes });
    const changed = (patch.views_width !== undefined && patch.views_width !== basis.views) || (patch.tools_width !== undefined && patch.tools_width !== basis.tools);
    if (!changed) return;
    if (request) {
      const views = patch.views_width ?? basis.views;
      const tools = patch.tools_width ?? basis.tools;
      const dividers = (views !== null ? sizes.divider : 0) + (tools !== null ? sizes.divider : 0);
      keyIntent.current = { frame: { ...basis, views, tools, agents: total - dividers - (views ?? 0) - (tools ?? 0) }, published: frame, body: total, request, observed: acknowledged };
    }
    if (actions.setWorkspaceView({ ...patch, ...(request ? { width_request_id: request } : {}) }) === false) keyIntent.current = null;
  };
  // Where the divider's left edge stands now, from the body's left edge.
  const position = (basis = frame) => {
    const total = body?.clientWidth ?? 0;
    const right = column === "views" ? total - (basis.tools !== null ? basis.tools + sizes.divider : 0) : total;
    const currentWidth = (column === "views" ? basis.views : basis.tools) ?? 0;
    return right - currentWidth - sizes.divider;
  };
  const drag = (event: React.PointerEvent<HTMLDivElement>) => {
    if (event.button !== 0 || !body) return;
    event.preventDefault();
    cancelDrag.current?.();
    keyIntent.current = null;
    const left = body.getBoundingClientRect().left;
    const target = event.currentTarget;
    target.setPointerCapture(event.pointerId);
    const at = (next: PointerEvent) => next.clientX - left - sizes.divider / 2;
    // The guide shows where the divider would land, clamped like the landing.
    const guideAt = (x: number) => {
      const patch = dividerLanding({ divider: column, x, body: body.clientWidth, frame, sizes });
      const landed = column === "views" ? patch.views_width ?? width : patch.tools_width ?? width;
      return position() + width - landed;
    };
    const move = (next: PointerEvent) => setGuide(guideAt(at(next)));
    // The drag marks the root, so a page the guide crosses gives way to its still.
    const release = holdShellDrag("col-resize");
    const end = () => {
      if (cancelDrag.current !== end) return;
      cancelDrag.current = null;
      release();
      target.removeEventListener("pointermove", move);
      target.removeEventListener("pointerup", up);
      target.removeEventListener("pointercancel", end);
      target.removeEventListener("lostpointercapture", end);
      if (target.hasPointerCapture(event.pointerId)) target.releasePointerCapture(event.pointerId);
      setGuide(null);
    };
    const up = (next: PointerEvent) => {
      if (cancelDrag.current !== end) return;
      end();
      land(at(next));
    };
    cancelDrag.current = end;
    // A drag the system cancels lands nothing and leaves no guide behind.
    target.addEventListener("pointermove", move);
    target.addEventListener("pointerup", up);
    target.addEventListener("pointercancel", end);
    target.addEventListener("lostpointercapture", end);
  };
  // Tools trades with File Views while both show; its accessible range
  // must use that neighbour's spare width, like dividerLanding does.
  const spare = column === "tools" && frame.views !== null ? frame.views - sizes.viewsMin : (frame.agents ?? 0) - sizes.agentMin;
  const min = column === "views" ? sizes.viewsMin : sizes.toolsMin;
  return (
    <div
      role="separator"
      aria-orientation="vertical"
      aria-label={`Resize ${name}`}
      aria-valuenow={width}
      aria-valuemin={min}
      aria-valuemax={width + Math.max(0, spare)}
      tabIndex={0}
      data-column-divider={column}
      className="group relative z-10 flex w-sm shrink-0 cursor-col-resize justify-center bg-background outline-none"
      onPointerDown={drag}
      onBlur={() => { keyIntent.current = null; }}
      onKeyDown={(event) => {
        if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
        event.preventDefault();
        // Left widens the column, as its divider moves left. Calculate from
        // the last requested target while the core is still confirming it.
        const pending = keyIntent.current;
        const basis = pending && pending.body === body?.clientWidth ? pending.frame : frame;
        const request = `${keySession}:${crypto.randomUUID()}`;
        land(position(basis) + (event.key === "ArrowLeft" ? -DIVIDER_STEP : DIVIDER_STEP), basis, request);
      }}
    >
      <DividerGrip className="opacity-0 group-hover:opacity-100 group-focus-visible:opacity-100" />
    </div>
  );
}

/** The divider's grip: a hairline with a ⇆ pill at its middle, drawn on hover, keyboard focus and while dragging. */
function DividerGrip({ className = "" }: { className?: string }) {
  return (
    <span aria-hidden="true" className={`relative h-full w-(--size-hairline) ${className}`} data-column-grip="true">
      <span className="absolute inset-0 bg-muted-foreground opacity-[var(--opacity-secondary)]" />
      <span className="absolute left-1/2 top-1/2 flex size-(--size-icon-button-toolbar) -translate-x-1/2 -translate-y-1/2 items-center justify-center rounded-md border border-border bg-card text-muted-foreground">
        <ArrowLeftRightIcon className="size-(--size-icon-sm)" />
      </span>
    </span>
  );
}

/**
 * The toolbar across the whole body (B6): the path back on the left, and at
 * the right end, icons only, Open server, File Views and Tools (D-04).
 */
function WorkspaceToolbar({ checkout, view, viewsShown, toolsShown, actions }: { checkout: Checkout; view: WorkspaceView; viewsShown: boolean; toolsShown: boolean; actions: Actions }) {
  const project = useShellStore((s) => catalogWorkspaces(s.rest).find((row) => row.checkouts.some((candidate) => candidate.id === checkout.id)) ?? null);
  const device = useShellStore((s) => focusedRemoteDevice(s.rest));
  const setScreen = useUiStore((s) => s.setScreen);
  // The chords as bound, so a rebound one shows as bound (B7).
  const viewsChord = useShellStore((s) => commandLabel("toggle_right_panel", s.rest?.ui_state));
  const toolsChord = useShellStore((s) => commandLabel("toggle_explorer", s.rest?.ui_state));
  const name = checkout.branch ?? checkout.label;
  // What the toolbar acts on is this Workspace: its columns, its path and
  // its Project (B15). Opening the menu changes none of them.
  const menuItems = (): MenuEntry<ToolbarMenuId>[] => [
    { id: "views", label: viewsShown ? "Hide File Views" : "Show File Views", unavailable: null },
    { id: "tools", label: toolsShown ? "Hide Tools" : "Show Tools", unavailable: null },
    { id: "copy_path", label: "Copy Workspace path", unavailable: null, separated: true },
    { id: "overview", label: "Open Project Overview", unavailable: project ? null : "This Workspace's Project is not in the catalog" },
  ];
  const select = (id: ToolbarMenuId) => {
    if (id === "views") return actions.toggleFileViews();
    if (id === "tools") return actions.toggleTools();
    if (id === "copy_path") return void navigator.clipboard?.writeText(checkout.path).catch(() => undefined);
    if (id === "overview" && project) setScreen(overviewScreen(useShellStore.getState().rest, project.id));
  };
  const count = viewsShown ? 0 : (view.layout?.display_count ?? 0);
  const viewsOpen = count > 0 ? `${count} ${count === 1 ? "view" : "views"} open` : undefined;
  return (
    <EntryContextMenu label={`Workspace ${name}`} items={menuItems} onSelect={select} className="shrink-0" data-workspace-menu="true">
      <div className="flex h-[var(--size-tab-strip)] shrink-0 items-center gap-sm border-b border-border bg-sidebar pl-sm pr-sm text-caption" data-workspace-toolbar="true">
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
        <div className="flex shrink-0 items-center gap-xxs" role="group" aria-label="Workspace columns" data-column-toggles="true">
          <RunningServers key={checkout.id} checkout={checkout} view={view} actions={actions} />
          {/* One name per icon, its state in aria-pressed; the tooltip carries the name and the chord (B7, B8). */}
          <Hint label="File Views" shortcut={viewsChord}>
            <Button
              variant={viewsShown ? "secondary" : "ghost"}
              size="icon-sm"
              className="relative"
              aria-pressed={viewsShown}
              aria-label="File Views"
              aria-description={viewsOpen}
              data-column-toggle="views"
              onClick={() => actions.toggleFileViews()}
            >
              <FilesIcon />
              {count > 0 ? (
                <span aria-hidden="true" className="absolute -right-xxs -top-xxs flex h-(--size-icon) min-w-(--size-icon) items-center justify-center rounded-full bg-primary px-xxs text-micro text-primary-foreground" data-column-badge={count}>
                  {count}
                </span>
              ) : null}
            </Button>
          </Hint>
          <Hint label="Tools" shortcut={toolsChord}>
            <Button variant={toolsShown ? "secondary" : "ghost"} size="icon-sm" aria-pressed={toolsShown} aria-label="Tools" data-column-toggle="tools" onClick={() => actions.toggleTools()}>
              <PanelRightIcon />
            </Button>
          </Hint>
        </div>
      </div>
    </EntryContextMenu>
  );
}

type ToolbarMenuId = "views" | "tools" | "copy_path" | "overview";

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
