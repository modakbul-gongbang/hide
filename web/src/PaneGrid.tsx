import { memo, useCallback, useRef, useState } from "react";
import { PaneView } from "./PaneView";
import { resizeStep } from "./resize";
import {
  dividerPaneId,
  focusedCheckout,
  layoutForTab,
  visibleTab,
  type LayoutNode,
  type PaneLayout,
  type PaneRow,
  type Tab,
  type TerminalPane,
} from "./snapshot";
import { useShellStore } from "./store";
import type { DispatchFn } from "./ws";

// The center draws the visible tab's projection tree as nested CSS grids
// (PRD S2 B4): every split is a two-track grid sized by Herdr's ratio and
// every leaf is a pane with its own xterm instance. Zoom keeps the other
// panes mounted underneath so their streams keep flowing; the focused pane
// is lifted over them (`toggle_zoom` decides, the shell only draws).

type PaneProps = {
  panes: Map<string, PaneRow>;
  transports: Map<string, TerminalPane>;
  scales: Record<string, number>;
  focusedPaneId: string;
  zoomedPaneId: string | null;
  dispatch: DispatchFn;
  onClosePane: (paneId: string) => void;
};

export function PaneCanvas({
  dispatch,
  onClosePane,
}: {
  dispatch: DispatchFn;
  onClosePane: (paneId: string) => void;
}) {
  const checkout = useShellStore((s) => focusedCheckout(s.rest));
  const tab = visibleTab(checkout);
  const layout = useShellStore((s) => layoutForTab(s.rest, tab?.id ?? null));
  const transportRows = useShellStore((s) => s.rest?.terminal?.panes);
  const scales = useShellStore((s) => s.rest?.ui_state?.pane_text_scales);
  if (!checkout || !tab || !layout) {
    return (
      <div className="flex min-h-0 flex-1 items-center justify-center text-caption text-muted" data-canvas="empty">
        {checkout ? "no tab" : "no checkout"}
      </div>
    );
  }
  return (
    <TabCanvas
      key={tab.id ?? "tab"}
      tab={tab}
      layout={layout}
      transportRows={transportRows ?? EMPTY_TRANSPORTS}
      scales={scales ?? EMPTY_SCALES}
      dispatch={dispatch}
      onClosePane={onClosePane}
    />
  );
}

const EMPTY_TRANSPORTS: TerminalPane[] = [];
const EMPTY_SCALES: Record<string, number> = {};

const TabCanvas = memo(function TabCanvas({
  tab,
  layout,
  transportRows,
  scales,
  dispatch,
  onClosePane,
}: {
  tab: Tab;
  layout: PaneLayout;
  transportRows: TerminalPane[];
  scales: Record<string, number>;
  dispatch: DispatchFn;
  onClosePane: (paneId: string) => void;
}) {
  const panes = new Map(tab.panes.map((pane) => [pane.id, pane]));
  const transports = new Map(transportRows.map((row) => [row.pane_id, row]));
  const props: PaneProps = {
    panes,
    transports,
    scales,
    focusedPaneId: layout.focused_pane_id,
    zoomedPaneId: layout.zoomed ? layout.focused_pane_id : null,
    dispatch,
    onClosePane,
  };
  return (
    <div className="relative min-h-0 min-w-0 flex-1" data-canvas={tab.id ?? ""} data-zoomed={layout.zoomed ? "true" : "false"}>
      <LayoutView node={layout.root} {...props} />
    </div>
  );
});

function LayoutView({ node, ...props }: { node: LayoutNode } & PaneProps) {
  if (node.type === "pane") {
    const pane = props.panes.get(node.pane_id);
    const zoomed = props.zoomedPaneId === node.pane_id;
    return (
      <div className={zoomed ? "absolute inset-0 z-10" : "relative h-full w-full min-h-0 min-w-0"} data-layout-pane={node.pane_id}>
        {pane ? (
          <PaneView
            pane={pane}
            transport={props.transports.get(node.pane_id)}
            focused={props.focusedPaneId === node.pane_id}
            scale={props.scales[node.pane_id] ?? 1}
            dispatch={props.dispatch}
            onClose={props.onClosePane}
          />
        ) : (
          // The layout names a pane the tab rows do not carry yet; the next
          // snapshot resolves it, and nothing is drawn that could be wrong.
          <div className="h-full bg-background" data-pane-missing={node.pane_id} />
        )}
      </div>
    );
  }
  return <SplitView node={node} {...props} />;
}

function SplitView({ node, ...props }: { node: Extract<LayoutNode, { type: "split" }> } & PaneProps) {
  const vertical = node.direction === "right";
  const first = `${node.ratio * 100}%`;
  const second = `${(1 - node.ratio) * 100}%`;
  return (
    <div
      className="grid h-full min-h-0 w-full min-w-0"
      style={vertical ? { gridTemplateColumns: `${first} ${second}` } : { gridTemplateRows: `${first} ${second}` }}
      data-split={node.direction}
    >
      <div className="relative min-h-0 min-w-0">
        <LayoutView node={node.first} {...props} />
      </div>
      <div className="relative min-h-0 min-w-0">
        <LayoutView node={node.second} {...props} />
      </div>
      <Divider node={node} dispatch={props.dispatch} />
    </div>
  );
}

/**
 * The handle on a split boundary. While dragging only the guide line moves;
 * releasing sends one `resize_pane` for the first subtree's last pane, or
 * nothing when the change falls outside the core's range (PRD S2 B6).
 */
function Divider({ node, dispatch }: { node: Extract<LayoutNode, { type: "split" }>; dispatch: DispatchFn }) {
  const vertical = node.direction === "right";
  const [travel, setTravel] = useState<number | null>(null);
  const origin = useRef(0);
  const spanRef = useRef(0);

  const onPointerDown = useCallback(
    (event: React.PointerEvent<HTMLDivElement>) => {
      const parent = event.currentTarget.parentElement;
      if (!parent) return;
      event.preventDefault();
      event.currentTarget.setPointerCapture(event.pointerId);
      const rect = parent.getBoundingClientRect();
      spanRef.current = vertical ? rect.width : rect.height;
      origin.current = vertical ? event.clientX : event.clientY;
      setTravel(0);
    },
    [vertical],
  );
  const onPointerMove = useCallback(
    (event: React.PointerEvent<HTMLDivElement>) => {
      if (travel === null) return;
      setTravel((vertical ? event.clientX : event.clientY) - origin.current);
    },
    [travel, vertical],
  );
  const onPointerUp = useCallback(
    (event: React.PointerEvent<HTMLDivElement>) => {
      if (travel === null) return;
      event.currentTarget.releasePointerCapture(event.pointerId);
      const step = resizeStep(travel, spanRef.current, vertical);
      setTravel(null);
      if (!step) return;
      dispatch({
        schema_version: 2,
        kind: "resize_pane",
        payload: { pane_id: dividerPaneId(node.first), direction: step.direction, amount: step.amount },
      });
    },
    [travel, vertical, dispatch, node.first],
  );
  const onPointerCancel = useCallback(() => setTravel(null), []);

  const at = `${node.ratio * 100}%`;
  const grab = vertical
    ? { left: `calc(${at} - var(--size-resize-grab) / 2)`, top: 0, bottom: 0, width: "var(--size-resize-grab)" }
    : { top: `calc(${at} - var(--size-resize-grab) / 2)`, left: 0, right: 0, height: "var(--size-resize-grab)" };
  const guide =
    travel === null
      ? null
      : vertical
        ? { left: `calc(${at} + ${travel}px - var(--size-resize-handle) / 2)`, top: 0, bottom: 0, width: "var(--size-resize-handle)" }
        : { top: `calc(${at} + ${travel}px - var(--size-resize-handle) / 2)`, left: 0, right: 0, height: "var(--size-resize-handle)" };
  return (
    <>
      <div
        role="separator"
        aria-orientation={vertical ? "vertical" : "horizontal"}
        aria-label={vertical ? "Resize pane width" : "Resize pane height"}
        data-divider={dividerPaneId(node.first)}
        className={`absolute z-20 ${vertical ? "cursor-col-resize" : "cursor-row-resize"}`}
        style={grab}
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
        onPointerUp={onPointerUp}
        onPointerCancel={onPointerCancel}
      />
      {guide ? <div className="pointer-events-none absolute z-20 bg-accent" style={guide} data-resize-guide="true" /> : null}
    </>
  );
}
