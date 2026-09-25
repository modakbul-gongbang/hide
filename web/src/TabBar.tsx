import { memo, useRef, useState } from "react";
import type { Actions } from "./actions";
import { AgentMark } from "./AgentMark";
import { fileIcon } from "./fileIcons";
import { ContextMenu, type MenuEntry } from "./Menu";
import type { AgentRow, AsyncOperation, Checkout, EditorSnapshot, EditorTabSnapshot, StripTab } from "./snapshot";
import { useShellStore } from "./store";
import { activeViewTab, agentEntries, tabAgent, tabIdentity, viewEntries } from "./workspace";

// The two tab strips of a Workspace (PRD S6 D-04, B17): the Agent area's
// Herdr tabs and the View area's files and diffs. Both draw what the core put
// in the checkout's strip, in its order; the web joins no lists of its own. A
// preview View tab is titled in italic and a dirty one carries a dot. A drag
// that lands sends one `reorder_tab` with the strip index of the tab it
// landed on, and the bar redraws in the core's order; nothing moves until the
// snapshot says so. Every tab carries its kind's mark and its full identity
// in the tooltip and the accessible name.

const NONE: AsyncOperation[] = [];
const NO_AGENTS: AgentRow[] = [];

/** Phases of a close the core is still confirming with Herdr (`operations.rs`). */
const IN_FLIGHT = new Set(["transmitting", "awaiting_topology", "unknown"]);

/** "closing…" while the core is still confirming a close with Herdr. */
export function closingSuffix(targetId: string, kind: "tab.close" | "pane.close", operations: AsyncOperation[]): boolean {
  return operations.some((op) => op.kind === kind && op.target_id === targetId && IN_FLIGHT.has(op.phase));
}

/**
 * A tab drag. The drag itself lives in a ref: pointer events arrive faster
 * than a continuous-priority render lands, so the release reads the ref and
 * the state only drives the highlight. A drop sends one `reorder_tab` with
 * the strip index of the tab it landed on.
 */
function useTabDrag(actions: Actions, strip: StripTab[]) {
  const drag = useRef<{ id: string; x: number; active: boolean } | null>(null);
  const [dragging, setDragging] = useState<string | null>(null);
  const [over, setOver] = useState<string | null>(null);
  return (entry: StripTab) => ({
    dragging: dragging === entry.id,
    over: over === entry.id && dragging !== entry.id,
    onPointerDown: (x: number) => {
      drag.current = { id: entry.id, x, active: false };
    },
    onPointerMove: (x: number) => {
      const start = drag.current;
      if (!start || start.active) return;
      const threshold = Number.parseFloat(getComputedStyle(document.documentElement).getPropertyValue("--size-tab-drag-activation"));
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
      if (start?.active && start.id !== entry.id) actions.reorderTab(start.id, strip.indexOf(entry));
    },
  });
}

/**
 * The Agent area's strip: the checkout's Herdr tabs. `activeTabId` is the tab
 * the core shows (this machine's visible tab, or a device's own focus);
 * `device` marks a selected SSH device's strip, whose tabs are chosen and
 * closed on that host.
 */
export function AgentTabBar({ checkout, activeTabId, agents, device = false, actions }: { checkout: Checkout; activeTabId: string | null; agents: AgentRow[] | null; device?: boolean; actions: Actions }) {
  const operations = useShellStore((s) => s.rest?.status?.async_operations) ?? NONE;
  const focusedPaneId = useShellStore((s) => s.focusedPaneId);
  const drag = useTabDrag(actions, checkout.strip);
  const entries = agentEntries(checkout);
  return (
    <div className="flex h-[var(--size-tab-strip)] shrink-0 items-stretch overflow-x-auto bg-panel" role="tablist" aria-label="Agent tabs" data-tab-bar={checkout.id} data-agent-tab-bar="true" data-remote-tab-bar={device ? "true" : undefined}>
      {entries.map((entry) => {
        const tab = checkout.tabs.find((row) => row.id === entry.source_id);
        const agent = tabAgent(tab, agents ?? NO_AGENTS, focusedPaneId);
        const identity = tabIdentity(entry, agent, null);
        return (
          <ContextMenu
            key={entry.id}
            label={`${entry.label} tab actions`}
            items={() => agentTabMenu(entry)}
            onSelect={(id) => runAgentTabItem(id, entry, actions)}
            className="flex shrink-0"
            data-tab-menu={entry.source_id}
          >
            <TabButton
              entry={entry}
              identity={identity}
              mark={<AgentMark kind={agent?.agent_kind} />}
              active={entry.source_id === activeTabId}
              dirty={false}
              saving={false}
              tabOnly={false}
              closing={closingSuffix(entry.source_id, "tab.close", operations)}
              closeLabel={`Close tab ${entry.label}`}
              onSelect={() => actions.focusTab(entry.source_id)}
              onDoubleClick={() => {}}
              onClose={() => actions.closeTab(entry.source_id)}
              {...drag(entry)}
            />
          </ContextMenu>
        );
      })}
      <button
        type="button"
        className="flex w-[var(--size-tab-overflow-control)] shrink-0 items-center justify-center text-secondary hover:bg-elevated hover:text-primary focus-visible:bg-elevated"
        aria-label={`New tab ${checkout.next_tab_label}`}
        title="New tab (⌥T)"
        data-new-agent-tab="true"
        onClick={() => actions.createTab()}
      >
        +
      </button>
    </div>
  );
}

type AgentTabItem = "new_tab" | "copy_name" | "close_tab";

function agentTabMenu(entry: StripTab): MenuEntry<AgentTabItem>[] {
  return [
    { id: "new_tab", label: "New tab", unavailable: null },
    { id: "copy_name", label: `Copy tab name “${entry.label}”`, unavailable: null },
    { id: "close_tab", label: "Close tab…", unavailable: null, separated: true },
  ];
}

function runAgentTabItem(id: AgentTabItem, entry: StripTab, actions: Actions) {
  if (id === "new_tab") actions.createTab();
  if (id === "copy_name") void navigator.clipboard?.writeText(entry.label);
  if (id === "close_tab") actions.closeTab(entry.source_id);
}

/** The View area's strip: the checkout's files and diffs, with the View tab the core shows marked. */
export function ViewTabBar({ checkout, actions }: { checkout: Checkout; actions: Actions }) {
  const editor = useShellStore((s) => s.editor);
  const savingTabs = useShellStore((s) => s.savingTabs);
  const bufferWarnings = useShellStore((s) => s.bufferWarnings);
  const drag = useTabDrag(actions, checkout.strip);
  const entries = viewEntries(checkout);
  const active = activeViewTab(editor, checkout);
  if (entries.length === 0) return null;
  return (
    <div className="flex h-[var(--size-tab-strip)] shrink-0 items-stretch overflow-x-auto bg-panel" role="tablist" aria-label="View tabs" data-view-tab-bar={checkout.id}>
      {entries.map((entry) => {
        const editorTab = editorTabOf(editor, entry);
        return (
          <ContextMenu
            key={entry.id}
            label={`${entry.label} view actions`}
            items={() => viewTabMenu(entry, editorTab)}
            onSelect={(id) => runViewTabItem(id, entry, editorTab, actions)}
            className="flex shrink-0"
            data-tab-menu={entry.source_id}
          >
            <TabButton
              entry={entry}
              identity={tabIdentity(entry, null, editorTab)}
              mark={viewMark(entry)}
              active={entry.source_id === active?.id}
              dirty={editorTab?.dirty ?? false}
              saving={savingTabs.has(entry.source_id)}
              tabOnly={bufferWarnings.has(entry.source_id)}
              unavailable={Boolean(editorTab?.unavailable_reason)}
              closing={false}
              closeLabel={`Close view ${entry.label}`}
              onSelect={() => actions.focusFileTab(entry.source_id)}
              onDoubleClick={() => actions.keepOpenFile(entry.source_id)}
              onClose={() => actions.closeFileTab(entry.source_id)}
              {...drag(entry)}
            />
          </ContextMenu>
        );
      })}
    </div>
  );
}

function editorTabOf(editor: EditorSnapshot | null, entry: StripTab): EditorTabSnapshot | null {
  return editor?.tabs.find((tab) => tab.id === entry.source_id) ?? null;
}

function viewMark(entry: StripTab) {
  if (entry.kind === "diff") {
    return (
      <span aria-hidden="true" data-view-mark="diff" className="shrink-0 font-mono text-warning">
        ±
      </span>
    );
  }
  const icon = fileIcon(entry.label);
  return (
    <span aria-hidden="true" data-view-mark="file" className={`shrink-0 ${icon.color}`} style={{ fontFamily: "seti" }}>
      {icon.glyph}
    </span>
  );
}

type ViewTabItem = "keep_open" | "reveal" | "copy_path" | "close_view";

function viewTabMenu(entry: StripTab, tab: EditorTabSnapshot | null): MenuEntry<ViewTabItem>[] {
  return [
    { id: "keep_open", label: "Keep open", unavailable: entry.preview ? null : "This view is already kept open" },
    { id: "reveal", label: "Reveal in Explorer", unavailable: tab?.unavailable_reason ? "The file is unavailable" : null },
    { id: "copy_path", label: "Copy path", unavailable: tab ? null : "The view has no path" },
    // Closing a view closes the document, never the file on disk; a dirty
    // document goes through the same save-on-close as the tab's ×.
    { id: "close_view", label: "Close view", unavailable: null, separated: true },
  ];
}

function runViewTabItem(id: ViewTabItem, entry: StripTab, tab: EditorTabSnapshot | null, actions: Actions) {
  if (id === "keep_open") actions.keepOpenFile(entry.source_id);
  if (id === "reveal" && tab) actions.revealInExplorer(tab.path);
  if (id === "copy_path" && tab) void navigator.clipboard?.writeText(tab.path);
  if (id === "close_view") actions.closeFileTab(entry.source_id);
}

export const TabButton = memo(function TabButton({
  entry,
  identity,
  mark,
  active,
  dirty,
  saving,
  tabOnly,
  unavailable = false,
  closing,
  closeLabel,
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
  mark: React.ReactNode;
  active: boolean;
  dirty: boolean;
  saving: boolean;
  tabOnly: boolean;
  unavailable?: boolean;
  closing: boolean;
  closeLabel: string;
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
      data-unavailable={unavailable ? "true" : "false"}
      className={`group relative flex max-w-[var(--size-tab-preferred)] min-w-[var(--size-tab-title-min)] shrink-0 cursor-default select-none items-center gap-xs px-sm text-caption outline-none focus-visible:ring-1 focus-visible:ring-inset focus-visible:ring-accent ${
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
      {mark}
      <span className={`min-w-0 flex-1 truncate ${entry.preview ? "italic" : ""} ${unavailable ? "text-muted line-through" : ""}`}>
        {entry.label}
        {saving ? <span className="text-muted"> saving…</span> : dirty ? <span className="text-warning"> ●</span> : null}
        {tabOnly ? <span className="text-muted"> kept in this tab only</span> : null}
        {closing ? <span className="text-muted"> closing…</span> : null}
      </span>
      <button
        type="button"
        className={`rounded-xs px-xxs text-secondary hover:bg-balloon hover:text-primary focus-visible:visible group-hover:visible ${active ? "visible" : "invisible"}`}
        aria-label={closeLabel}
        title={closeLabel}
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
