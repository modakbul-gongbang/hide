import { GlobeIcon, XIcon } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import type { Actions } from "./actions";
import { createAreaTree, type AreaAdapter, type AreaTabInteraction } from "./AreaTree";
import { AreaEmpty } from "./AreaEmpty";
import { BrowserDisplay } from "./BrowserDisplay";
import { Button } from "./components/ui/button";
import { Hint } from "./components/ui/tooltip";
import { hostKind } from "./host";
import { DisplayEditor, DocumentKeeper } from "./Editor";
import { fileIcon } from "./fileIcons";
import { editorTabFor, type ViewDisplaySnapshot, type ViewLayoutSnapshot } from "./snapshot";
import { useShellStore } from "./store";
import { useUiStore } from "./ui";
import { displayCommand } from "./shortcuts";
import { noteDrawnViews } from "./viewFocus";
import { displayIdentity, displayMenu, focusRequestArrived, placeKey, shownDisplays, workspaceKey, showsSameDocument, VIEW_WORDS, type ViewMenuId, type ViewWorkspace } from "./viewLayout";
import { locateDisplay } from "./areaLayout";
import { workspaceViewOf } from "./workspace";

/**
 * The front Workspace's View areas, in the side panel. With no display they
 * are drawn only while a file of this checkout is opening; otherwise the
 * panel holds its tools or closes (`panelFrame`). `trailing` is what
 * the panel's strip carries at its right end when no tool column is there to
 * carry it.
 */
export function ViewAreas({ actions, trailing = null }: { actions: Actions; trailing?: React.ReactNode }) {
  const view = useShellStore((s) => workspaceViewOf(s.rest));
  if (!view) return null;
  const layout = view.layout;
  if (!layout) {
    return <AreaEmpty state="view-layout-missing" text="This Hide core publishes no View areas, so no file or diff can be shown here." />;
  }
  if (layout.display_count === 0) {
    return (
      <>
        {trailing ? <div className="flex h-[var(--size-tab-strip)] shrink-0 items-center justify-end border-b border-border">{trailing}</div> : null}
        <AreaEmpty state="view-opening" text="Opening…" />
      </>
    );
  }
  // One tree per Workspace: a front that moves to another Workspace ends a
  // drag, a divider drag or a menu begun on this one, whose ids (a1, d2, s1)
  // name other views there (contract 4.1, B8).
  return <ViewTree key={workspaceKey({ device_id: view.device_id, path: view.path })} layout={layout} deviceId={view.device_id} path={view.path} trailing={trailing} actions={actions} />;
}

const SharedViewTree = createAreaTree<ViewDisplaySnapshot>("view");
function ViewTree({ layout, deviceId, path, trailing, actions }: { layout: ViewLayoutSnapshot; deviceId: string; path: string; trailing: React.ReactNode; actions: Actions }) {
  const workspace = useMemo(() => ({ device_id: deviceId, path }), [deviceId, path]);
  const key = workspaceKey(workspace);
  const [body, setBody] = useState<HTMLDivElement | null>(null);
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

  // One keeper per document on screen, however many displays show it.
  const documents = useMemo(
    () => [...new Set(shownDisplays(layout.root).flatMap((display) => (display.state === "open" && display.kind === "file" && display.tab_id ? [display.tab_id] : [])))],
    [layout.root],
  );


  const adapter: AreaAdapter<ViewDisplaySnapshot> = {
    words: VIEW_WORDS,
    label: (display) => display.label,
    sameContent: showsSameDocument,
    tab: (display, interaction) => <DisplayTab display={display} interaction={interaction} actions={actions} />,
    body: (display) => <DisplayBody key={display.id} display={display} workspace={workspace} actions={actions} />,
    empty: () => <AreaEmpty state="no-view" text="No file, diff or page is open in this area." />,
    floating: (display) => <>{displayMark(display)}<span className={`truncate ${display.preview ? "italic" : ""}`}>{display.label}</span></>,
    menu: (id, geometry, sizes) => [{ id: "new_tab", label: "New tab", unavailable: null }, ...displayMenu(layout, geometry, sizes, id)],
    runMenu: (id, displayId) => id === "new_tab" ? actions.openBrowser("", workspace, locateDisplay(layout.root, displayId)?.area.id) : actions.runViewMenu(id as ViewMenuId, displayId),
    focus: actions.focusView,
    focusArea: actions.focusViewArea,
    move: actions.moveView,
    split: actions.splitView,
    resize: actions.resizeViewSplit,
    newTab: (areaId) => actions.openBrowser("", workspace, areaId),
    newTabLabel: "New tab",
    newTabShortcut: displayCommand("new_tab", hostKind()),
    tabListLabel: "View tabs",
    actionsLabel: "View actions",
    onDraw: (frame) => noteDrawnViews(frame ? { ...frame, workspace } : null),
    onBody: setBody,
  };
  return <SharedViewTree layout={layout} adapter={adapter} trailing={trailing}>
    {documents.map((tabId) => <DocumentKeeper key={tabId} tabId={tabId} actions={actions} />)}
  </SharedViewTree>;
}

/** A display's body: its document or diff, or the state it is in (B16, contract 3). */
function DisplayBody({ display, workspace, actions }: { display: ViewDisplaySnapshot; workspace: ViewWorkspace; actions: Actions }) {
  if (display.kind === "browser") return <BrowserDisplay display={display} workspace={workspace} actions={actions} />;
  if (display.state === "open") {
    return <DisplayEditor display={display} placeKey={placeKey(workspaceKey(workspace), display)} actions={actions} />;
  }
  if (display.state === "opening") return <AreaEmpty state="view-opening" text={`Opening ${display.label}…`} />;
  if (display.state === "waiting") return <AreaEmpty state="view-waiting" text={display.reason ?? `Waiting to read ${display.path}.`} />;
  return (
    <AreaEmpty state="view-unavailable" text={`${display.path} is unavailable${display.reason ? `: ${display.reason}` : "."}`}>
      {/* Each button is one action on this view alone, like a tab's ×: its
          press does not also make the area active (one action, one event). */}
      <Button variant="secondary" onPointerDown={(event) => event.stopPropagation()} onClick={() => actions.closeView(display.id)} data-close-unavailable={display.id}>
        Close view
      </Button>
      <Button variant="ghost" onPointerDown={(event) => event.stopPropagation()} onClick={() => actions.retryView(display.id)} data-retry-unavailable={display.id}>
        Retry
      </Button>
    </AreaEmpty>
  );
}

/** One display's tab: its kind's mark, italic while a preview, its save marks, and its whole identity (B2, B21). */
function DisplayTab({ display, interaction, actions }: { display: ViewDisplaySnapshot; interaction: AreaTabInteraction; actions: Actions }) {
  const { selected, areaActive } = interaction;
  const dirty = useShellStore((s) => editorTabFor(s.editor, display.tab_id)?.dirty ?? false);
  const saving = useShellStore((s) => display.tab_id !== null && s.savingTabs.has(display.tab_id));
  const tabOnly = useShellStore((s) => display.tab_id !== null && s.bufferWarnings.has(display.tab_id));
  const unavailable = display.state === "unavailable";
  const identity = displayIdentity(display);
  return (
    <Hint label={identity} reveals>
    <div
      role="tab"
      aria-selected={selected}
      aria-label={identity}
      tabIndex={0}
      data-tab={display.tab_id ?? ""}
      data-display={display.id}
      data-tab-kind={display.kind}
      data-preview={display.preview ? "true" : "false"}
      data-saving={saving ? "true" : "false"}
      data-tab-only={tabOnly ? "true" : "false"}
      data-unavailable={unavailable ? "true" : "false"}
      data-view-state={display.state}
      className={`group relative flex min-w-0 flex-1 cursor-default select-none items-center gap-xs px-sm text-caption outline-none focus-visible:ring-1 focus-visible:ring-inset focus-visible:ring-ring ${
        selected ? "text-foreground" : "text-subtle-foreground hover:bg-accent"
      } ${interaction.dragging ? "opacity-[var(--opacity-dimmed)]" : ""}`}
      onPointerDown={interaction.press}
      onClick={() => {
        interaction.select();
      }}
      onDoubleClick={() => actions.keepViewOpen(display.id)}
      onKeyDown={(event) => {
        if (event.key === "Enter" || event.key === " ") {
          event.preventDefault();
          interaction.select();
        }
      }}
    >
      {displayMark(display)}
      <span className={`min-w-0 flex-1 truncate ${display.preview ? "italic" : ""} ${unavailable ? "text-muted-foreground line-through" : ""}`}>
        {display.label}
        {saving ? <span className="text-muted-foreground"> saving…</span> : dirty ? <span className="text-warning"> ●</span> : null}
        {tabOnly ? <span className="text-muted-foreground"> kept in this tab only</span> : null}
      </span>
      <Hint label={`Close view ${display.label}`}>
        <Button
          variant="ghost"
          size="icon-sm"
          className={`shrink-0 hover:bg-popover hover:text-foreground focus-visible:visible group-hover:visible ${selected ? "visible" : "invisible"}`}
          aria-label={`Close view ${display.label}`}
          onPointerDown={(event) => event.stopPropagation()}
          onClick={(event) => {
            event.stopPropagation();
            actions.closeView(display.id);
          }}
        >
          <XIcon />
        </Button>
      </Hint>
      {selected && areaActive ? <span className="absolute inset-x-0 bottom-0 h-[var(--size-tab-indicator)] bg-primary" /> : null}
    </div>
    </Hint>
  );
}

/** A file's type mark or the diff's comparison mark; never colour alone (D-15). */
export function displayMark(display: Pick<ViewDisplaySnapshot, "kind" | "label">) {
  if (display.kind === "browser") return <GlobeIcon aria-hidden="true" data-view-mark="browser" className="size-(--size-icon) shrink-0 text-muted-foreground" />;
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
