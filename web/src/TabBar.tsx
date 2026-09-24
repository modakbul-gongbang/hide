import { memo, useRef, useState } from "react";
import type { Actions } from "./actions";
import { fileIcon } from "./fileIcons";
import type { RemoteView } from "./remote";
import { editorFor, focusedCheckout, remoteEditorTab, type AsyncOperation, type Checkout, type EditorSnapshot, type EditorTabSnapshot, type StripTab } from "./snapshot";
import { useShellStore } from "./store";

// The tab bar draws the focused checkout's strip (PRD S2 B3, S3 B3/B4). Every
// entry the core put in the strip is drawn, Herdr tabs and editor tabs alike,
// in the order it gave: the web joins no lists of its own. A preview editor
// entry is titled in italic and a dirty one carries a dot. A drag that lands
// sends one `reorder_tab` with the strip index of the tab it landed on, and the
// bar redraws in the core's order; nothing moves until the snapshot says so.

export function TabBar({ actions }: { actions: Actions }) {
  const checkout = useShellStore((s) => focusedCheckout(s.rest));
  const editor = useShellStore((s) => s.editor);
  const savingTabs = useShellStore((s) => s.savingTabs);
  const bufferWarnings = useShellStore((s) => s.bufferWarnings);
  const operations = useShellStore((s) => s.rest?.status?.async_operations);
  if (!checkout) return <div className="h-[var(--size-tab-strip)] shrink-0 bg-panel" data-tab-bar="empty" />;
  return (
    <Strip
      checkout={checkout}
      activeId={activeStripId(checkout, editor)}
      showingEditor={editorFor(editor) !== null}
      editor={editor}
      savingTabs={savingTabs}
      bufferWarnings={bufferWarnings}
      operations={operations ?? NONE}
      actions={actions}
    />
  );
}

const NONE: AsyncOperation[] = [];

/**
 * A tab drag. The drag itself lives in a ref: pointer events arrive faster
 * than a continuous-priority render lands, so the release reads the ref and
 * the state only drives the highlight. A drop sends one `reorder_tab` with
 * the strip index of the tab it landed on.
 */
function useTabDrag(actions: Actions) {
  const drag = useRef<{ id: string; x: number; active: boolean } | null>(null);
  const [dragging, setDragging] = useState<string | null>(null);
  const [over, setOver] = useState<string | null>(null);
  const handlers = (entry: StripTab, index: number) => ({
    dragging: dragging === entry.id,
    over: over === entry.id && dragging !== entry.id,
    onPointerDown: (x: number) => {
      drag.current = { id: entry.id, x, active: false };
    },
    onPointerMove: (x: number) => {
      const start = drag.current;
      if (!start || start.active) return;
      const threshold = Number.parseFloat(
        getComputedStyle(document.documentElement).getPropertyValue("--size-tab-drag-activation"),
      );
      if (Math.abs(x - start.x) >= threshold) {
        start.active = true;
        setDragging(start.id);
      }
    },
    onPointerEnter: () => {
      if (drag.current?.active) setOver(entry.id);
    },
    onPointerUp: () => {
      const start = drag.current;
      drag.current = null;
      setDragging(null);
      setOver(null);
      if (start?.active && start.id !== entry.id) actions.reorderTab(start.id, index);
    },
  });
  return handlers;
}

/**
 * A selected SSH device's strip: the Herdr tabs of the workspace its host has
 * focused and the device files opened in that checkout, in the order the core
 * placed them (`place_device_strips`). A terminal tab is selected, added and
 * closed on that host (`remote_control`); a file tab is the core's own. A drag
 * is the same as this machine's: a Herdr tab moves on the device's Herdr.
 */
export function RemoteTabBar({ view, actions }: { view: RemoteView; actions: Actions }) {
  const operations = useShellStore((s) => s.rest?.status?.async_operations);
  const editor = useShellStore((s) => s.editor);
  const savingTabs = useShellStore((s) => s.savingTabs);
  const bufferWarnings = useShellStore((s) => s.bufferWarnings);
  const showing = remoteEditorTab(editor, view.checkout);
  return (
    <Strip
      checkout={view.checkout}
      activeId={showing ? showing.id : (view.tab?.id ?? null)}
      showingEditor={showing !== null}
      device
      editor={editor}
      savingTabs={savingTabs}
      bufferWarnings={bufferWarnings}
      operations={operations ?? NONE}
      actions={actions}
    />
  );
}

/** Phases of a close the core is still confirming with Herdr (`operations.rs`). */
const IN_FLIGHT = new Set(["transmitting", "awaiting_topology", "unknown"]);

/** What a file or diff tab stands for, read by assistive technology. */
function editorIdentity(entry: StripTab, tab: EditorTabSnapshot): string {
  const kind = entry.kind === "diff" ? `${tab.diff_committed ? "Committed on branch" : "Uncommitted"} diff` : "File";
  return `${kind}: ${tab.path}${entry.preview ? " · Preview" : ""}`;
}

/** "closing…" while the core is still confirming a close with Herdr. */
export function closingSuffix(targetId: string, kind: "tab.close" | "pane.close", operations: AsyncOperation[]): boolean {
  return operations.some((op) => op.kind === kind && op.target_id === targetId && IN_FLIGHT.has(op.phase));
}

/**
 * The tab the strip marks active: the editor's when it owns the surface, else
 * the Herdr tab the core reports. The core keeps the editor's tabs while a
 * terminal tab shows, so the tabs alone do not say what is drawn.
 */
export function activeStripId(checkout: Checkout, editor: EditorSnapshot | null): string | null {
  const showing = editorFor(editor);
  return showing ? showing.active_tab_id : checkout.active_tab_id;
}

/**
 * One checkout's strip. `activeId` and `showingEditor` say which entry is
 * drawn active: the editor tab when the editor owns the surface, else the
 * Herdr tab.
 */
const Strip = memo(function Strip({
  checkout,
  activeId,
  showingEditor,
  device = false,
  editor,
  savingTabs,
  bufferWarnings,
  operations,
  actions,
}: {
  checkout: Checkout;
  activeId: string | null;
  showingEditor: boolean;
  device?: boolean;
  editor: EditorSnapshot | null;
  savingTabs: Set<string>;
  bufferWarnings: Set<string>;
  operations: AsyncOperation[];
  actions: Actions;
}) {
  const dirty = new Set((editor?.tabs ?? []).filter((tab) => tab.dirty).map((tab) => tab.id));
  const drag = useTabDrag(actions);

  return (
    <div
      className="flex h-[var(--size-tab-strip)] shrink-0 items-stretch overflow-x-auto bg-panel"
      role="tablist"
      data-tab-bar={checkout.id}
      data-remote-tab-bar={device ? "true" : undefined}
    >
      {checkout.strip.map((entry, index) => {
        // Session and Memory viewers are deferred (PRD Non-goals), so their
        // entries are not drawn; the core does not create them in S3.
        if (entry.kind === "session" || entry.kind === "memory") return null;
        const isEditor = entry.kind === "file" || entry.kind === "diff";
        const editorTab = isEditor ? editor?.tabs.find((tab) => tab.id === entry.source_id) : null;
        return (
          <TabButton
            key={entry.id}
            entry={entry}
            identity={editorTab ? editorIdentity(entry, editorTab) : entry.label}
            active={showingEditor === isEditor && entry.source_id === activeId}
            dirty={dirty.has(entry.source_id)}
            saving={savingTabs.has(entry.source_id)}
            tabOnly={bufferWarnings.has(entry.source_id)}
            closing={!isEditor && closingSuffix(entry.source_id, "tab.close", operations)}
            onSelect={() => (isEditor ? actions.focusFileTab(entry.source_id) : actions.focusTab(entry.source_id))}
            onDoubleClick={() => { if (isEditor) actions.keepOpenFile(entry.source_id); }}
            onClose={() => (isEditor ? actions.closeFileTab(entry.source_id) : actions.closeTab(entry.source_id))}
            {...drag(entry, index)}
          />
        );
      })}
      <button
        type="button"
        className="flex w-[var(--size-tab-overflow-control)] shrink-0 items-center justify-center text-secondary hover:bg-elevated hover:text-primary"
        aria-label={`New tab ${checkout.next_tab_label}`}
        title="New tab (⌥T)"
        onClick={() => actions.createTab()}
      >
        +
      </button>
    </div>
  );
});

export const TabButton = memo(function TabButton({
  entry,
  identity,
  active,
  dirty,
  saving,
  tabOnly,
  closing,
  dragging,
  over,
  onSelect,
  onDoubleClick,
  onClose,
  onPointerDown,
  onPointerMove,
  onPointerEnter,
  onPointerUp,
}: {
  entry: StripTab;
  identity: string;
  active: boolean;
  dirty: boolean;
  saving: boolean;
  tabOnly: boolean;
  closing: boolean;
  dragging: boolean;
  over: boolean;
  onSelect: () => void;
  onDoubleClick: () => void;
  onClose: () => void;
  onPointerDown: (x: number) => void;
  onPointerMove: (x: number) => void;
  onPointerEnter: () => void;
  onPointerUp: () => void;
}) {
  return (
    <div
      role="tab"
      aria-selected={active}
      aria-label={identity}
      title={identity}
      tabIndex={0}
      data-tab={entry.source_id}
      data-tab-kind={entry.kind}
      data-preview={entry.preview ? "true" : "false"}
      data-saving={saving ? "true" : "false"}
      data-tab-only={tabOnly ? "true" : "false"}
      data-closing={closing ? "true" : "false"}
      className={`group relative flex max-w-[var(--size-tab-preferred)] min-w-[var(--size-tab-title-min)] shrink-0 cursor-default select-none items-center gap-xs px-sm text-caption ${
        active ? "bg-background text-primary" : "text-secondary hover:bg-elevated"
      } ${dragging ? "opacity-[var(--opacity-dimmed)]" : ""}`}
      onPointerDown={(event) => {
        if (event.button !== 0) return;
        onPointerDown(event.clientX);
      }}
      onPointerMove={(event) => onPointerMove(event.clientX)}
      onPointerEnter={onPointerEnter}
      onPointerUp={onPointerUp}
      onClick={onSelect}
      onDoubleClick={onDoubleClick}
      onKeyDown={(event) => {
        if (event.key === "Enter" || event.key === " ") {
          event.preventDefault();
          onSelect();
        }
      }}
    >
      {over ? <span className="absolute inset-y-0 left-0 w-[var(--size-tab-indicator)] bg-accent" /> : null}
      {entry.kind === "diff" ? <span aria-hidden="true" className="shrink-0 text-warning">±</span> : entry.kind === "file" ? (
        <span aria-hidden="true" className={`shrink-0 ${fileIcon(entry.label).color}`} style={{ fontFamily: "seti" }}>{fileIcon(entry.label).glyph}</span>
      ) : null}
      <span className={`min-w-0 flex-1 truncate ${entry.preview ? "italic" : ""}`}>
        {entry.label}
        {saving ? <span className="text-muted"> saving…</span> : dirty ? <span className="text-warning"> ●</span> : null}
        {tabOnly ? <span className="text-muted"> kept in this tab only</span> : null}
        {closing ? <span className="text-muted"> closing…</span> : null}
      </span>
      <button
        type="button"
        className="invisible rounded-xs px-xxs text-secondary hover:bg-balloon hover:text-primary group-hover:visible"
        aria-label={`Close tab ${entry.label}`}
        onClick={(event) => {
          event.stopPropagation();
          onClose();
        }}
      >
        ×
      </button>
      {active ? <span className="absolute inset-x-0 bottom-0 h-[var(--size-tab-indicator)] bg-accent" /> : null}
    </div>
  );
});
