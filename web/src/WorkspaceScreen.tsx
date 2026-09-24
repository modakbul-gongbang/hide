import { useLayoutEffect, useMemo, useRef, useState } from "react";
import type { Actions } from "./actions";
import { Button } from "./components/ui/controls";
import { EditorSurface } from "./Editor";
import { LayoutIcon, ToolIcon } from "./icons";
import { MenuList, type MenuEntry } from "./Menu";
import { FindBar } from "./Overlays";
import { PaneCanvas, RemotePaneCanvas } from "./PaneGrid";
import { remoteView } from "./remote";
import { canRetryDevice, deviceLine } from "./settings";
import { catalogWorkspaces, focusedRemoteDevice, frontCheckout, type Checkout } from "./snapshot";
import { useShellStore } from "./store";
import { AgentTabBar, ViewTabBar } from "./TabBar";
import { Tools } from "./Tools";
import { useUiStore } from "./ui";
import { LAYOUTS, activeViewTab, agentEntries, agentWidth, layoutLabel, shareAt, workspaceViewOf, type ViewMode } from "./workspace";

// A Workspace (PRD S6 D-01..D-05, B4-B11): one checkout's Agent area (its
// Herdr tabs and their panes) and View area (its files and diffs), side by
// side or one at a time, with its tools beside them. The layout, the tools
// and the boundary are the core's, per Workspace; this draws them and sends
// one event per operator choice. A hidden area is unmounted, never closed:
// its terminals park and stay fed, and its tabs stay in the core.

export function WorkspaceScreen({ actions }: { actions: Actions }) {
  const checkout = useShellStore((s) => frontCheckout(s.rest));
  const view = useShellStore((s) => workspaceViewOf(s.rest));
  if (!checkout || !view) return null;
  return (
    <section className="flex min-h-0 min-w-0 flex-1 flex-col" aria-label={`Workspace ${checkout.branch ?? checkout.label}`} data-workspace-screen={checkout.id} data-layout={view.mode}>
      <WorkspaceToolbar checkout={checkout} mode={view.mode} explorer={view.explorer} changes={view.changes} actions={actions} />
      <div className="flex min-h-0 flex-1">
        <Areas checkout={checkout} mode={view.mode} share={view.agent_share} actions={actions} />
        <Tools actions={actions} />
      </div>
    </section>
  );
}

/** The path back (B4), the layout choice (B5) and the tools (B10). */
function WorkspaceToolbar({ checkout, mode, explorer, changes, actions }: { checkout: Checkout; mode: ViewMode; explorer: boolean; changes: boolean; actions: Actions }) {
  const project = useShellStore((s) => catalogWorkspaces(s.rest).find((row) => row.checkouts.some((candidate) => candidate.id === checkout.id)) ?? null);
  const device = useShellStore((s) => focusedRemoteDevice(s.rest));
  const setScreen = useUiStore((s) => s.setScreen);
  const name = checkout.branch ?? checkout.label;
  return (
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
      <LayoutSwitch mode={mode} actions={actions} />
      <div className="flex items-center gap-xxs" role="group" aria-label="Workspace tools">
        <ToolToggle tool="explorer" label="Explorer" on={explorer} actions={actions} />
        <ToolToggle tool="changes" label="History" on={changes} actions={actions} />
      </div>
    </div>
  );
}

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
            title={`${layout.label} — ${layout.description}`}
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
        aria-label={`Layout: ${layoutLabel(mode)}`}
        title="Layout menu"
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
  return Number.parseFloat(getComputedStyle(document.documentElement).getPropertyValue("--size-workspace-area-min")) || 320;
}

/**
 * The areas the layout shows. While both show, a boundary between them moves
 * a guide line as it is dragged and sends one `workspace_view` share on
 * release, so a drag never resizes the terminals on every pointer event.
 */
function Areas({ checkout, mode, share, actions }: { checkout: Checkout; mode: ViewMode; share: number; actions: Actions }) {
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
  const agents = mode !== "views";
  const views = mode !== "agents";
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
    const up = (next: PointerEvent) => {
      target.removeEventListener("pointermove", move);
      target.removeEventListener("pointerup", up);
      setGuide(null);
      const nextShare = shareAt(next.clientX - left, total, minimum);
      if (Math.abs(nextShare - share) > 0.001) actions.setWorkspaceView({ agent_share: nextShare });
    };
    target.addEventListener("pointermove", move);
    target.addEventListener("pointerup", up);
  };
  return (
    <div ref={body} className="relative flex min-h-0 min-w-0 flex-1" data-areas={mode}>
      {agents ? (
        <div className={`flex min-h-0 min-w-0 flex-col ${agentPx === null ? "flex-1" : "shrink-0"}`} style={agentPx === null ? undefined : { width: agentPx }} data-agent-area="true">
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
        <div className="flex min-h-0 min-w-0 flex-1 flex-col" data-view-area="true">
          <ViewArea checkout={checkout} actions={actions} />
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
      <FindBar actions={actions} />
      {hasTabs ? (
        <PaneCanvas dispatch={actions.dispatch} onClosePane={(paneId) => actions.closePane(paneId)} />
      ) : (
        <AreaEmpty state="no-agent-tab" text="No agent tab is open in this Workspace.">
          <Button onClick={() => actions.createTab()} data-empty-new-tab="true">
            New tab <span className="text-muted">⌥T</span>
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
      <RemotePaneCanvas view={view} connected={connected} dispatch={actions.dispatch} onClosePane={(paneId) => actions.closePane(paneId)} />
    </>
  );
}

/** The Workspace's files and diffs (B9, B11, B20). */
function ViewArea({ checkout, actions }: { checkout: Checkout; actions: Actions }) {
  const tab = useShellStore((s) => activeViewTab(s.editor, checkout));
  const opening = useShellStore((s) => (s.editor?.opening ?? []).some((row) => row.checkout_id === checkout.id));
  const explorer = useShellStore((s) => workspaceViewOf(s.rest)?.explorer ?? false);
  return (
    <>
      <ViewTabBar checkout={checkout} actions={actions} />
      {tab?.unavailable_reason ? (
        <AreaEmpty state="view-unavailable" text={`${tab.path} is unavailable: ${tab.unavailable_reason}`}>
          <Button onClick={() => actions.closeFileTab(tab.id)} data-close-unavailable={tab.id}>
            Close view
          </Button>
        </AreaEmpty>
      ) : tab ? (
        <EditorSurface actions={actions} />
      ) : opening ? (
        <AreaEmpty state="view-opening" text="Opening…" />
      ) : (
        <AreaEmpty state="no-view" text="No file or diff is open in this Workspace.">
          {explorer ? null : (
            <Button onClick={() => actions.setTool("explorer", true)} data-empty-open-explorer="true">
              Show Explorer
            </Button>
          )}
          <Button appearance="quiet" onClick={() => useUiStore.getState().openOverlay("file_palette")} data-empty-open-file="true">
            Open file <span className="text-muted">⌘P</span>
          </Button>
        </AreaEmpty>
      )}
    </>
  );
}

function AreaEmpty({ state, text, children }: { state: string; text: string; children?: React.ReactNode }) {
  return (
    <div className="flex min-h-0 flex-1 flex-col items-center justify-center gap-sm bg-background p-lg text-center text-caption text-muted" data-area-empty={state}>
      <p className="max-w-full break-words">{text}</p>
      {children ? <div className="flex flex-wrap justify-center gap-sm">{children}</div> : null}
    </div>
  );
}
