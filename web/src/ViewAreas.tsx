import { createContext, useContext, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import type { Actions } from "./actions";
import { AreaEmpty } from "./AreaEmpty";
import { Button } from "./components/ui/controls";
import { DisplayEditor, DocumentKeeper } from "./Editor";
import { fileIcon } from "./fileIcons";
import { ContextMenu, MenuList, type MenuEntry } from "./Menu";
import { editorTabFor, type Checkout, type ViewAreaSnapshot, type ViewDisplaySnapshot, type ViewLayoutSnapshot, type ViewNode, type ViewSplitSnapshot } from "./snapshot";
import { useShellStore } from "./store";
import { useUiStore } from "./ui";
import { IDLE, movePointer, pressTab, relayout, releasePointer, type DragSession } from "./viewDrag";
import { focusFromKeyboard, installFocusModality, noteDrawnViews } from "./viewFocus";
import {
  RATIO_MAX,
  RATIO_MIN,
  RESIZE_STEP,
  areasOf,
  displayIdentity,
  displayMenu,
  dropTarget,
  findArea,
  focusRequestArrived,
  locateDisplay,
  placeKey,
  ratioAtOffset,
  revealedScroll,
  sameTarget,
  shownDisplays,
  shownTools,
  singleAreaGeometry,
  steppedRatio,
  viewGeometry,
  workspaceKey,
  type DividerBox,
  type DropTarget,
  type Geometry,
  type LayoutSizes,
  type Point,
  type Rect,
  type TabSlot,
  type ViewMenuId,
} from "./viewLayout";
import { workspaceViewOf } from "./workspace";

// The View areas (PRD S7 B1-B13, B20; contract 3 and 6): the front
// Workspace's tree of areas as the core published it, drawn as nested flex
// splits, each area with its own tab bar and the body of its active display.
// Everything drawn here is presentation. A divider drag moves a guide line and
// lands once on release; a tab drag previews its one target and sends one
// `view_layout` on a valid drop, nothing on anything else, and resizes nothing
// while it lasts; a window too small for the tree shows only the active area.
// The rules are `viewLayout.ts`'s and `viewDrag.ts`'s; this reads the page
// (token sizes, the pointer, the focus) and hands it to them.

function tokenPx(name: string): number {
  return Number.parseFloat(getComputedStyle(document.documentElement).getPropertyValue(name)) || 0;
}

function readLayoutSizes(): LayoutSizes {
  return {
    areaMinWidth: tokenPx("--size-workspace-area-min"),
    areaMinHeight: tokenPx("--size-view-area-min-height"),
    divider: tokenPx("--size-resize-handle"),
    tabStrip: tokenPx("--size-tab-strip"),
  };
}

/** What every part of the drawn tree reads. */
type Tree = {
  layout: ViewLayoutSnapshot;
  /** What is drawn: the whole tree, or the active area alone in a narrow window. */
  geometry: Geometry;
  sizes: LayoutSizes;
  /** The Workspace's key, since display ids are only unique within one. */
  workspaceKey: string;
  actions: Actions;
  draggingId: string | null;
  press: (displayId: string, event: React.PointerEvent<HTMLElement>) => void;
  /** True once for the click that ends a drag, which must not select anything. */
  takeClick: () => boolean;
  /** The operator chose this display: it becomes the one they work in (B20). */
  focus: (displayId: string) => void;
  startResize: (box: DividerBox, event: React.PointerEvent<HTMLElement>) => void;
  menu: (displayId: string) => MenuEntry<ViewMenuId>[];
};

const TreeContext = createContext<Tree | null>(null);

function useTree(): Tree {
  const tree = useContext(TreeContext);
  if (!tree) throw new Error("a View area drew outside its tree");
  return tree;
}

/** The front Workspace's View areas, or S6's empty state while no display is open (B10). */
export function ViewAreas({ checkout, actions }: { checkout: Checkout; actions: Actions }) {
  const view = useShellStore((s) => workspaceViewOf(s.rest));
  const opening = useShellStore((s) => (s.editor?.opening ?? []).some((row) => row.checkout_id === checkout.id));
  const placement = useUiStore((s) => s.toolsPlacement);
  if (!view) return null;
  const layout = view.layout;
  if (!layout) {
    return <AreaEmpty state="view-layout-missing" text="This Hide core publishes no View areas, so no file or diff can be shown here." />;
  }
  if (areasOf(layout.root).every((area) => area.displays.length === 0)) {
    return <ViewsEmpty opening={opening} explorerShown={shownTools(view, placement).explorer} actions={actions} />;
  }
  // One tree per Workspace: a front that moves to another Workspace ends a
  // drag, a divider drag or a menu begun on this one, whose ids (a1, d2, s1)
  // name other views there (contract 4.1, B8).
  return <ViewTree key={workspaceKey({ device_id: view.device_id, path: view.path })} layout={layout} deviceId={view.device_id} path={view.path} actions={actions} />;
}

/** Nothing open in any area: the mode stays, and the way to a file is offered. */
function ViewsEmpty({ opening, explorerShown, actions }: { opening: boolean; explorerShown: boolean; actions: Actions }) {
  if (opening) return <AreaEmpty state="view-opening" text="Opening…" />;
  return (
    <AreaEmpty state="no-view" text="No file or diff is open in this Workspace.">
      {explorerShown ? null : (
        <Button onClick={() => actions.setTool("explorer", true)} data-empty-open-explorer="true">
          Show Explorer
        </Button>
      )}
      <Button appearance="quiet" onClick={() => useUiStore.getState().openOverlay("file_palette")} data-empty-open-file="true">
        Open file <span className="text-muted-foreground">⌘P</span>
      </Button>
    </AreaEmpty>
  );
}

function ViewTree({ layout, deviceId, path, actions }: { layout: ViewLayoutSnapshot; deviceId: string; path: string; actions: Actions }) {
  const workspace = useMemo(() => ({ device_id: deviceId, path }), [deviceId, path]);
  const key = workspaceKey(workspace);
  const [body, setBody] = useState<HTMLDivElement | null>(null);
  const [size, setSize] = useState({ width: 0, height: 0 });
  useLayoutEffect(() => {
    if (!body) return undefined;
    const measure = () =>
      setSize((current) => (current.width === body.clientWidth && current.height === body.clientHeight ? current : { width: body.clientWidth, height: body.clientHeight }));
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(body);
    return () => observer.disconnect();
  }, [body]);
  const sizes = useMemo(readLayoutSizes, []);
  const measured = size.width > 0 && size.height > 0;
  const rect = useMemo<Rect>(() => ({ x: 0, y: 0, width: size.width, height: size.height }), [size.width, size.height]);
  const whole = useMemo(() => viewGeometry(layout.root, rect, sizes), [layout.root, rect, sizes]);
  const areas = areasOf(layout.root);
  const shownArea = findArea(layout.root, layout.active_area) ?? areas[0] ?? null;
  // A window too small to give every area its minimum shows only the active
  // one, with a switcher to the others; nothing is sent or stored for it, so
  // widening shows the stored tree again (B13, A7).
  const single = measured && !whole.fits && shownArea !== null;
  const geometry = useMemo(() => (single && shownArea ? singleAreaGeometry(shownArea, rect, sizes) : whole), [single, shownArea, rect, sizes, whole]);

  // Published as the draw is committed, before any input can reach it, so
  // an action reads the frame the operator sees (contract 4.1).
  useLayoutEffect(() => {
    if (!measured) return undefined;
    noteDrawnViews({ workspace, layout, geometry, sizes });
    return () => noteDrawnViews(null);
  }, [measured, workspace, layout, geometry, sizes]);

  useEffect(() => installFocusModality(), []);

  // A menu, the palette or a drop moved the keyboard's place; once the core
  // shows it there, the keyboard follows, and this focus asks the core for
  // nothing (B20). A move of the active view resolves on the frame that
  // lands it, never on the one it was asked from. The keyboard goes there
  // once that frame's editors have settled, since an editor that moved to
  // a new area is built as it mounts (twice, in a development build), and
  // not when something else took the keyboard in between.
  const request = useUiStore((s) => s.viewFocusRequest);
  useEffect(() => {
    if (!request || !body || !focusRequestArrived(request, key, layout)) return;
    useUiStore.getState().setViewFocusRequest(null);
    const areaId = layout.active_area;
    const before = document.activeElement;
    requestAnimationFrame(() => {
      const now = document.activeElement;
      if (now !== before && now !== null && now !== document.body) return;
      const area = body.querySelector<HTMLElement>(`[data-view-area-id="${CSS.escape(areaId)}"]`);
      const target = area?.querySelector<HTMLElement>("[data-editor-body] .cm-content") ?? area?.querySelector<HTMLElement>('[role="tab"][aria-selected="true"]');
      target?.focus({ preventScroll: true });
    });
  }, [request, key, layout, body]);

  // One focus per choice: a second press before the core answered asks again for nothing.
  const claimed = useRef<{ displayId: string; layout: ViewLayoutSnapshot } | null>(null);
  const focus = (displayId: string) => {
    const located = locateDisplay(layout.root, displayId);
    if (!located) return;
    if (layout.active_area === located.area.id && located.area.active === displayId) return;
    if (claimed.current?.displayId === displayId && claimed.current.layout === layout) return;
    claimed.current = { displayId, layout };
    actions.focusView(displayId);
  };

  // The tab drag (B6-B8). The session lives in a ref, since pointer events
  // outrun renders; the tree redraws only when the previewed target changes,
  // and the floating tab follows the pointer on its own.
  const [session, setSession] = useState<DragSession>(IDLE);
  const sessionRef = useRef<DragSession>(IDLE);
  const layoutRef = useRef(layout);
  layoutRef.current = layout;
  const geometryRef = useRef(geometry);
  geometryRef.current = geometry;
  const swallow = useRef(false);
  const show = (next: DragSession) => {
    sessionRef.current = next;
    setSession(next);
  };
  const swallowClick = () => {
    swallow.current = true;
    window.setTimeout(() => {
      swallow.current = false;
    }, 0);
  };
  const resolve = (displayId: string) => (client: Point): DropTarget => {
    if (!body) return { kind: "none", reason: null };
    const origin = body.getBoundingClientRect();
    return dropTarget({
      layout: layoutRef.current,
      geometry: geometryRef.current,
      sizes,
      tabs: measureTabs(body, origin),
      displayId,
      point: { x: client.x - origin.left, y: client.y - origin.top },
    });
  };

  const live = session.phase !== "idle";
  useEffect(() => {
    if (!live) return undefined;
    const threshold = tokenPx("--size-tab-drag-activation");
    const move = (event: PointerEvent) => {
      const current = sessionRef.current;
      if (current.phase === "idle") return;
      const next = movePointer(current, event.pointerId, { x: event.clientX, y: event.clientY }, threshold, resolve(current.displayId));
      if (next === current) return;
      sessionRef.current = next;
      if (current.phase !== next.phase || (current.phase === "dragging" && next.phase === "dragging" && !samePreview(current.target, next.target))) setSession(next);
    };
    const up = (event: PointerEvent) => {
      const current = sessionRef.current;
      if (current.phase === "idle" || current.pointerId !== event.pointerId) return;
      const result = releasePointer(current, event.pointerId, { x: event.clientX, y: event.clientY }, resolve(current.displayId));
      show(result.session);
      if (result.dragged) swallowClick();
      if (!result.drop) return;
      if (result.drop.kind === "bar") actions.moveView(current.displayId, result.drop.areaId, result.drop.index);
      else actions.splitView(current.displayId, result.drop.areaId, result.drop.edge);
    };
    // A drag the system takes away, or the window losing focus, lands nothing.
    const cancel = () => show(IDLE);
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
    window.addEventListener("pointercancel", cancel);
    window.addEventListener("blur", cancel);
    return () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      window.removeEventListener("pointercancel", cancel);
      window.removeEventListener("blur", cancel);
    };
    // The listeners read the session, the layout and the geometry through refs.
  }, [live]);

  // A layout that changes under a drag re-resolves its preview in place
  // (B8). A layout effect, so the tab bars are measured as now drawn; the
  // pointer's own moves re-resolve in the listeners.
  useLayoutEffect(() => {
    const current = sessionRef.current;
    if (current.phase !== "dragging") return;
    const next = relayout(current, resolve(current.displayId));
    if (next !== current) show(next);
  }, [layout, geometry]);

  // Escape ends a drag with nothing sent, and the release that follows selects nothing.
  const dragging = session.phase === "dragging";
  useEffect(() => {
    if (!dragging) return undefined;
    return useUiStore.getState().pushEscape(() => {
      show(IDLE);
      swallow.current = true;
      window.addEventListener("pointerup", swallowClick, { once: true, capture: true });
    });
  }, [dragging]);

  // The pointer says whether the place under it would land (B8).
  const cursor = session.phase === "dragging" ? (session.target.kind === "none" && session.target.reason !== null ? "forbidden" : "move") : null;
  useEffect(() => {
    if (!cursor) return undefined;
    const root = document.documentElement;
    root.dataset.viewDrag = cursor;
    return () => {
      delete root.dataset.viewDrag;
    };
  }, [cursor]);

  // A divider drag moves a guide line and lands one resize on release (B9);
  // the line is its own component, so a pointer move redraws nothing else.
  const guide = useRef<((rect: Rect | null) => void) | null>(null);
  const startResize = (box: DividerBox, event: React.PointerEvent<HTMLElement>) => {
    if (event.button !== 0 || !body) return;
    event.preventDefault();
    const target = event.currentTarget;
    target.setPointerCapture(event.pointerId);
    const origin = body.getBoundingClientRect();
    const row = box.axis === "row";
    const ratioAt = (next: PointerEvent) => ratioAtOffset(box, row ? next.clientX - origin.left - box.span.x : next.clientY - origin.top - box.span.y);
    const move = (next: PointerEvent) => guide.current?.(guideAt(box, ratioAt(next)));
    const end = () => {
      target.removeEventListener("pointermove", move);
      target.removeEventListener("pointerup", up);
      target.removeEventListener("pointercancel", end);
      target.removeEventListener("lostpointercapture", end);
      guide.current?.(null);
    };
    const up = (next: PointerEvent) => {
      end();
      const ratio = ratioAt(next);
      if (Math.abs(ratio - box.ratio) > 0.001) actions.resizeViewSplit(box.id, ratio);
    };
    target.addEventListener("pointermove", move);
    target.addEventListener("pointerup", up);
    target.addEventListener("pointercancel", end);
    target.addEventListener("lostpointercapture", end);
  };

  // One keeper per document on screen, however many displays show it.
  const documents = useMemo(
    () => [...new Set(shownDisplays(layout.root).flatMap((display) => (display.state === "open" && display.kind === "file" && display.tab_id ? [display.tab_id] : [])))],
    [layout.root],
  );

  const tree: Tree = {
    layout,
    geometry,
    sizes,
    workspaceKey: key,
    actions,
    draggingId: session.phase === "dragging" ? session.displayId : null,
    press: (displayId, event) => {
      if (event.button !== 0) return;
      event.currentTarget.setPointerCapture(event.pointerId);
      show(pressTab(displayId, event.pointerId, { x: event.clientX, y: event.clientY }));
    },
    takeClick: () => {
      const taken = swallow.current;
      swallow.current = false;
      return taken;
    },
    focus,
    startResize,
    menu: (displayId) => displayMenu(layout, geometry, sizes, displayId),
  };

  return (
    <TreeContext.Provider value={tree}>
      <div ref={setBody} className="relative flex min-h-0 min-w-0 flex-1" data-view-areas={single ? "single" : "tree"}>
        {documents.map((tabId) => (
          <DocumentKeeper key={tabId} tabId={tabId} actions={actions} />
        ))}
        {!measured || !shownArea ? null : single ? (
          <AreaView area={shownArea} index={areas.indexOf(shownArea)} count={areas.length} switcher />
        ) : (
          <NodeView node={layout.root} />
        )}
        <ResizeGuide control={guide} />
        {session.phase === "dragging" ? <DragPreview session={session} /> : null}
      </div>
    </TreeContext.Provider>
  );
}

/** Two previews show the same thing, so the tree need not redraw. */
function samePreview(a: DropTarget, b: DropTarget): boolean {
  if (a.kind === "none" && b.kind === "none") return a.reason === b.reason;
  return sameTarget(a, b);
}

/** Each area's tab rectangles, in the tree's own coordinates. */
function measureTabs(body: HTMLElement, origin: DOMRect): Record<string, TabSlot[]> {
  const slots: Record<string, TabSlot[]> = {};
  for (const bar of body.querySelectorAll<HTMLElement>("[data-view-tab-bar]")) {
    const areaId = bar.dataset.viewTabBar;
    if (!areaId) continue;
    slots[areaId] = [...bar.querySelectorAll<HTMLElement>('[role="tab"][data-display]')].map((tab) => {
      const box = tab.getBoundingClientRect();
      return { displayId: tab.dataset.display ?? "", rect: { x: box.left - origin.left, y: box.top - origin.top, width: box.width, height: box.height } };
    });
  }
  return slots;
}

/** The guide line of a divider drag: the only thing a pointer move redraws. */
function ResizeGuide({ control }: { control: React.MutableRefObject<((rect: Rect | null) => void) | null> }) {
  const [rect, setRect] = useState<Rect | null>(null);
  useLayoutEffect(() => {
    control.current = setRect;
    return () => {
      control.current = null;
    };
  }, [control]);
  if (!rect) return null;
  return <div className="pointer-events-none absolute z-20 bg-primary" style={{ left: rect.x, top: rect.y, width: rect.width, height: rect.height }} data-view-resize-guide="true" />;
}

/** Where a divider dragged to `ratio` would sit. */
function guideAt(box: DividerBox, ratio: number): Rect {
  const row = box.axis === "row";
  const thickness = row ? box.rect.width : box.rect.height;
  const first = Math.round(ratio * ((row ? box.span.width : box.span.height) - thickness));
  return row
    ? { x: box.span.x + first, y: box.span.y, width: thickness, height: box.span.height }
    : { x: box.span.x, y: box.span.y + first, width: box.span.width, height: thickness };
}

function NodeView({ node }: { node: ViewNode }) {
  const tree = useTree();
  if ("area" in node) {
    const areas = areasOf(tree.layout.root);
    return <AreaView area={node.area} index={areas.findIndex((area) => area.id === node.area.id)} count={areas.length} switcher={false} />;
  }
  const { split } = node;
  const box = tree.geometry.dividers.find((divider) => divider.id === split.id);
  const row = split.axis === "row";
  return (
    <div className={`flex min-h-0 min-w-0 flex-1 ${row ? "flex-row" : "flex-col"}`} data-view-split={split.id}>
      <div className="flex min-h-0 min-w-0 shrink-0" style={box ? (row ? { width: box.first } : { height: box.first }) : undefined}>
        <NodeView node={split.first} />
      </div>
      {box ? <Separator split={split} box={box} /> : null}
      <div className="flex min-h-0 min-w-0 flex-1">
        <NodeView node={split.second} />
      </div>
    </div>
  );
}

/** A divider: dragged with a guide line, or moved a step at a time from the keyboard (B9, B20). */
function Separator({ split, box }: { split: ViewSplitSnapshot; box: DividerBox }) {
  const tree = useTree();
  const row = split.axis === "row";
  return (
    <div
      role="separator"
      aria-orientation={row ? "vertical" : "horizontal"}
      aria-label={row ? "Resize the view areas side by side" : "Resize the view areas above and below"}
      aria-valuenow={Math.round(split.ratio * 100)}
      aria-valuemin={Math.round(RATIO_MIN * 100)}
      aria-valuemax={Math.round(RATIO_MAX * 100)}
      tabIndex={0}
      data-view-divider={split.id}
      className={`relative z-10 shrink-0 bg-border outline-none hover:bg-primary focus-visible:bg-primary ${row ? "w-[var(--size-resize-handle)] cursor-col-resize" : "h-[var(--size-resize-handle)] cursor-row-resize"}`}
      onPointerDown={(event) => tree.startResize(box, event)}
      onKeyDown={(event) => {
        const back = row ? "ArrowLeft" : "ArrowUp";
        const forward = row ? "ArrowRight" : "ArrowDown";
        if (event.key !== back && event.key !== forward) return;
        event.preventDefault();
        const next = steppedRatio(box, event.key === back ? -RESIZE_STEP : RESIZE_STEP);
        if (next !== null) tree.actions.resizeViewSplit(split.id, next);
      }}
    />
  );
}

function AreaView({ area, index, count, switcher }: { area: ViewAreaSnapshot; index: number; count: number; switcher: boolean }) {
  const tree = useTree();
  const display = area.displays.find((row) => row.id === area.active) ?? null;
  const active = tree.layout.active_area === area.id;
  // The operator's pointer, or Tab, into a display makes it the one they
  // work in; a focus the page moved itself asks for nothing (B20).
  const claim = () => {
    if (display) tree.focus(display.id);
  };
  return (
    <section
      className="flex min-h-0 min-w-0 flex-1 flex-col"
      aria-label={`View area ${index + 1} of ${count}`}
      data-view-area-id={area.id}
      data-active-area={active ? "true" : "false"}
    >
      <AreaTabBar area={area} active={active} index={index} count={count} switcher={switcher} />
      <div
        className="flex min-h-0 min-w-0 flex-1 flex-col"
        data-view-body={area.id}
        onPointerDown={claim}
        onFocus={() => {
          if (focusFromKeyboard()) claim();
        }}
      >
        {display ? <DisplayBody key={display.id} display={display} /> : <AreaEmpty state="no-view" text="No file or diff is open in this area." />}
      </div>
    </section>
  );
}

/** A display's body: its document or diff, or the state it is in (B16, contract 3). */
function DisplayBody({ display }: { display: ViewDisplaySnapshot }) {
  const tree = useTree();
  if (display.state === "open") {
    return <DisplayEditor display={display} placeKey={placeKey(tree.workspaceKey, display)} actions={tree.actions} />;
  }
  if (display.state === "opening") return <AreaEmpty state="view-opening" text={`Opening ${display.label}…`} />;
  if (display.state === "waiting") return <AreaEmpty state="view-waiting" text={display.reason ?? `Waiting to read ${display.path}.`} />;
  return (
    <AreaEmpty state="view-unavailable" text={`${display.path} is unavailable${display.reason ? `: ${display.reason}` : "."}`}>
      {/* Each button is one action on this view alone, like a tab's ×: its
          press does not also make the area active (one action, one event). */}
      <Button onPointerDown={(event) => event.stopPropagation()} onClick={() => tree.actions.closeView(display.id)} data-close-unavailable={display.id}>
        Close view
      </Button>
      <Button appearance="quiet" onPointerDown={(event) => event.stopPropagation()} onClick={() => tree.actions.retryView(display.id)} data-retry-unavailable={display.id}>
        Retry
      </Button>
    </AreaEmpty>
  );
}

/** An area's own tab bar; only the active area's shown tab carries the accent indicator (B1, B20). */
function AreaTabBar({ area, active, index, count, switcher }: { area: ViewAreaSnapshot; active: boolean; index: number; count: number; switcher: boolean }) {
  const tree = useTree();
  const shown = area.displays.find((row) => row.id === area.active) ?? null;
  // The shown view's tab stays in sight however many tabs the area holds
  // (B20, B21): the strip scrolls itself, and nothing around it, whenever
  // the shown view changes or the strip is resized.
  const strip = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    const list = strip.current;
    if (!list) return;
    const reveal = () => {
      const tab = list.querySelector<HTMLElement>('[role="tab"][aria-selected="true"]');
      if (!tab) return;
      const offset = tab.getBoundingClientRect().left - list.getBoundingClientRect().left + list.scrollLeft;
      list.scrollLeft = revealedScroll(list.scrollLeft, list.clientWidth, offset, tab.offsetWidth);
    };
    reveal();
    const observer = new ResizeObserver(reveal);
    observer.observe(list);
    return () => observer.disconnect();
  }, [area.active, area.displays.length]);
  return (
    <div className="flex h-[var(--size-tab-strip)] shrink-0 items-stretch bg-card" data-view-tab-bar={area.id}>
      <div ref={strip} role="tablist" aria-label={`View tabs, area ${index + 1} of ${count}`} className="flex min-w-0 flex-1 items-stretch overflow-x-auto">
        {area.displays.map((display) => (
          <ContextMenu
            key={display.id}
            label={`${display.label} view actions`}
            items={() => tree.menu(display.id)}
            onSelect={(id) => tree.actions.runViewMenu(id, display.id)}
            className="flex shrink-0"
            data-tab-menu={display.id}
          >
            <DisplayTab display={display} selected={display.id === area.active} areaActive={active} />
          </ContextMenu>
        ))}
      </div>
      {switcher ? <AreaSwitcher current={area.id} /> : null}
      {shown ? (
        <MenuButton
          label={`View actions: ${shown.label}`}
          items={() => tree.menu(shown.id)}
          onSelect={(id) => tree.actions.runViewMenu(id, shown.id)}
          data-view-overflow={shown.id}
        >
          ⋯
        </MenuButton>
      ) : null}
    </div>
  );
}

/** One display's tab: its kind's mark, italic while a preview, its save marks, and its whole identity (B2, B21). */
function DisplayTab({ display, selected, areaActive }: { display: ViewDisplaySnapshot; selected: boolean; areaActive: boolean }) {
  const tree = useTree();
  const dirty = useShellStore((s) => editorTabFor(s.editor, display.tab_id)?.dirty ?? false);
  const saving = useShellStore((s) => display.tab_id !== null && s.savingTabs.has(display.tab_id));
  const tabOnly = useShellStore((s) => display.tab_id !== null && s.bufferWarnings.has(display.tab_id));
  const unavailable = display.state === "unavailable";
  const identity = displayIdentity(display);
  return (
    <div
      role="tab"
      aria-selected={selected}
      aria-label={identity}
      title={identity}
      tabIndex={0}
      data-tab={display.tab_id ?? ""}
      data-display={display.id}
      data-tab-kind={display.kind}
      data-preview={display.preview ? "true" : "false"}
      data-saving={saving ? "true" : "false"}
      data-tab-only={tabOnly ? "true" : "false"}
      data-unavailable={unavailable ? "true" : "false"}
      data-view-state={display.state}
      className={`group relative flex max-w-[var(--size-tab-preferred)] min-w-[var(--size-tab-title-min)] shrink-0 cursor-default select-none items-center gap-xs px-sm text-caption outline-none focus-visible:ring-1 focus-visible:ring-inset focus-visible:ring-ring ${
        selected ? "bg-background text-foreground" : "text-subtle-foreground hover:bg-accent"
      } ${tree.draggingId === display.id ? "opacity-[var(--opacity-dimmed)]" : ""}`}
      onPointerDown={(event) => tree.press(display.id, event)}
      onClick={() => {
        if (!tree.takeClick()) tree.focus(display.id);
      }}
      onDoubleClick={() => tree.actions.keepViewOpen(display.id)}
      onKeyDown={(event) => {
        if (event.key === "Enter" || event.key === " ") {
          event.preventDefault();
          tree.focus(display.id);
        }
      }}
    >
      {displayMark(display)}
      <span className={`min-w-0 flex-1 truncate ${display.preview ? "italic" : ""} ${unavailable ? "text-muted-foreground line-through" : ""}`}>
        {display.label}
        {saving ? <span className="text-muted-foreground"> saving…</span> : dirty ? <span className="text-warning"> ●</span> : null}
        {tabOnly ? <span className="text-muted-foreground"> kept in this tab only</span> : null}
      </span>
      <button
        type="button"
        className={`rounded-xs px-xxs text-subtle-foreground hover:bg-popover hover:text-foreground focus-visible:visible group-hover:visible ${selected ? "visible" : "invisible"}`}
        aria-label={`Close view ${display.label}`}
        title={`Close view ${display.label}`}
        onPointerDown={(event) => event.stopPropagation()}
        onClick={(event) => {
          event.stopPropagation();
          tree.actions.closeView(display.id);
        }}
      >
        ×
      </button>
      {selected && areaActive ? <span className="absolute inset-x-0 bottom-0 h-[var(--size-tab-indicator)] bg-primary" /> : null}
    </div>
  );
}

/** A file's type mark or the diff's comparison mark; never colour alone (D-15). */
function displayMark(display: ViewDisplaySnapshot) {
  if (display.kind === "diff") {
    return (
      <span aria-hidden="true" data-view-mark="diff" className="shrink-0 font-mono text-warning">
        ±
      </span>
    );
  }
  const icon = fileIcon(display.label);
  return (
    <span aria-hidden="true" data-view-mark="file" className={`shrink-0 ${icon.color}`} style={{ fontFamily: "seti" }}>
      {icon.glyph}
    </span>
  );
}

/** A small control that opens a menu under itself, drawn fixed so no scrolling strip clips it. */
function MenuButton<Id extends string>({
  label,
  items,
  onSelect,
  children,
  ...data
}: { label: string; items: () => MenuEntry<Id>[]; onSelect: (id: Id) => void; children: React.ReactNode } & Record<`data-${string}`, string>) {
  const [at, setAt] = useState<{ right: number; top: number } | null>(null);
  return (
    <div className="flex shrink-0">
      <button
        type="button"
        aria-haspopup="menu"
        aria-expanded={at !== null}
        aria-label={label}
        title={label}
        className="flex min-w-[var(--size-tab-overflow-control)] items-center justify-center px-xxs text-caption text-subtle-foreground hover:bg-accent hover:text-foreground focus-visible:bg-accent"
        {...data}
        onClick={(event) => {
          if (at) return setAt(null);
          const box = event.currentTarget.getBoundingClientRect();
          setAt({ right: window.innerWidth - box.right, top: box.bottom });
        }}
      >
        {children}
      </button>
      {at ? <MenuList label={label} items={items()} onSelect={onSelect} onClose={() => setAt(null)} className="fixed" style={{ right: at.right, top: at.top }} /> : null}
    </div>
  );
}

/** A narrow window shows one area; this names it and switches to another (B13). */
function AreaSwitcher({ current }: { current: string }) {
  const tree = useTree();
  const areas = areasOf(tree.layout.root);
  const index = areas.findIndex((area) => area.id === current);
  const items = (): MenuEntry<string>[] =>
    areas.map((area, at) => {
      const shown = area.displays.find((row) => row.id === area.active);
      return { id: area.id, label: `${area.id === current ? "✓ " : ""}Area ${at + 1}${shown ? ` · ${shown.label}` : ""}`, unavailable: null };
    });
  return (
    <MenuButton
      label={`View area ${index + 1} of ${areas.length}; the window shows one at a time. Switch view area`}
      items={items}
      onSelect={(id) => {
        if (id !== current) tree.actions.focusViewArea(id);
      }}
      data-view-area-switch={current}
    >
      {index + 1}/{areas.length} ▾
    </MenuButton>
  );
}

/** The drag's one preview: an insertion line, or a split's destination and its label (B6-B8, D-14). */
function DragPreview({ session }: { session: Extract<DragSession, { phase: "dragging" }> }) {
  const tree = useTree();
  const display = locateDisplay(tree.layout.root, session.displayId)?.display ?? null;
  const target = session.target;
  return (
    <>
      {target.kind === "bar" ? (
        <div
          className="pointer-events-none absolute z-30 w-[var(--size-tab-indicator)] -translate-x-1/2 bg-primary"
          style={{ left: target.line.x, top: target.line.y, height: target.line.height }}
          data-view-drop="bar"
        />
      ) : null}
      {target.kind === "edge" ? (
        <div
          className="pointer-events-none absolute z-30"
          style={{ left: target.preview.x, top: target.preview.y, width: target.preview.width, height: target.preview.height }}
          data-view-drop={target.edge}
        >
          <div className="absolute inset-0 bg-primary opacity-[var(--opacity-selected-fill)]" />
          <div className="absolute inset-0 border border-primary" />
          <div className="absolute inset-0 flex items-center justify-center">
            <span className="rounded-sm bg-popover px-sm py-xxs text-caption text-foreground shadow-lg">{target.label}</span>
          </div>
        </div>
      ) : null}
      {display ? <FloatingTab display={display} start={session.point} reason={target.kind === "none" ? target.reason : null} /> : null}
    </>
  );
}

/** The dragged tab under the pointer; it follows every move without redrawing the tree. */
function FloatingTab({ display, start, reason }: { display: ViewDisplaySnapshot; start: Point; reason: string | null }) {
  const [point, setPoint] = useState(start);
  useEffect(() => {
    const move = (event: PointerEvent) => setPoint({ x: event.clientX, y: event.clientY });
    window.addEventListener("pointermove", move);
    return () => window.removeEventListener("pointermove", move);
  }, []);
  return (
    <div
      className="pointer-events-none fixed z-50 flex max-w-[var(--size-tab-preferred)] translate-x-sm translate-y-sm flex-col gap-xxs rounded-sm border border-border bg-secondary px-sm py-xxs text-caption text-foreground shadow-lg"
      style={{ left: point.x, top: point.y }}
      data-view-drag-tab={display.id}
    >
      <span className="flex min-w-0 items-center gap-xs">
        {displayMark(display)}
        <span className={`truncate ${display.preview ? "italic" : ""}`}>{display.label}</span>
      </span>
      {reason ? <span className="text-muted-foreground">{reason}</span> : null}
    </div>
  );
}
