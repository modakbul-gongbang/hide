import { PlusIcon, XIcon } from "lucide-react";
import { memo, useRef, useState } from "react";
import type { Actions } from "./actions";
import { AgentMark } from "./AgentMark";
import { EntryContextMenu, type MenuEntry } from "./components/entry-menu";
import { Button } from "./components/ui/button";
import { Hint } from "./components/ui/tooltip";
import type { AgentRow, AsyncOperation, Checkout, StripTab } from "./snapshot";
import { useShellStore } from "./store";
import { agentEntries, tabAgent, tabIdentity } from "./workspace";

// The Agent area's tab strip (PRD S6 D-04, B17): the checkout's Herdr tabs,
// in the order the core put them in its strip; the web joins no lists of its
// own. A drag that lands sends one `reorder_tab` with the strip index of the
// tab it landed on, and the bar redraws in the core's order; nothing moves
// until the snapshot says so. Every tab carries its agent's mark and its full
// identity in the tooltip and the accessible name. The View areas' tab bars
// are `ViewAreas.tsx`'s.

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
    <div className="flex h-[var(--size-tab-strip)] shrink-0 items-stretch overflow-x-auto bg-card" role="tablist" aria-label="Agent tabs" data-tab-bar={checkout.id} data-agent-tab-bar="true" data-remote-tab-bar={device ? "true" : undefined}>
      {entries.map((entry) => {
        const tab = checkout.tabs.find((row) => row.id === entry.source_id);
        const agent = tabAgent(tab, agents ?? NO_AGENTS, focusedPaneId);
        const identity = tabIdentity(entry, agent);
        return (
          <EntryContextMenu
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
              closing={closingSuffix(entry.source_id, "tab.close", operations)}
              closeLabel={`Close tab ${entry.label}`}
              onSelect={() => actions.focusTab(entry.source_id)}
              onClose={() => actions.closeTab(entry.source_id)}
              {...drag(entry)}
            />
          </EntryContextMenu>
        );
      })}
      <Hint label="New tab" shortcut="⌥T">
        <Button
          variant="ghost"
          className="h-full w-(--size-tab-overflow-control) shrink-0 rounded-none px-none hover:text-foreground focus-visible:bg-accent"
          aria-label={`New tab ${checkout.next_tab_label}`}
          data-new-agent-tab="true"
          onClick={() => actions.createTab()}
        >
          <PlusIcon />
        </Button>
      </Hint>
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

const TabButton = memo(function TabButton({
  entry,
  identity,
  mark,
  active,
  closing,
  closeLabel,
  dragging,
  over,
  onSelect,
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
  closing: boolean;
  closeLabel: string;
  dragging: boolean;
  over: boolean;
  onSelect: () => void;
  onClose: () => void;
  onPointerDown: (x: number) => void;
  onPointerMove: (x: number) => void;
  onPointerEnter: () => void;
  onPointerUp: () => void;
}) {
  return (
    <Hint label={identity} reveals>
    <div
      role="tab"
      aria-selected={active}
      aria-label={identity}
      tabIndex={0}
      data-tab={entry.source_id}
      data-tab-kind={entry.kind}
      data-closing={closing ? "true" : "false"}
      className={`group relative flex max-w-[var(--size-tab-preferred)] min-w-[var(--size-tab-title-min)] shrink-0 cursor-default select-none items-center gap-xs px-sm text-caption outline-none focus-visible:ring-1 focus-visible:ring-inset focus-visible:ring-ring ${
        active ? "bg-background text-foreground" : "text-subtle-foreground hover:bg-accent"
      } ${dragging ? "opacity-[var(--opacity-dimmed)]" : ""}`}
      onPointerDown={(event) => {
        if (event.button !== 0) return;
        onPointerDown(event.clientX);
      }}
      onPointerMove={(event) => onPointerMove(event.clientX)}
      onPointerEnter={onPointerEnter}
      onPointerUp={onPointerUp}
      onClick={onSelect}
      onKeyDown={(event) => {
        if (event.key === "Enter" || event.key === " ") {
          event.preventDefault();
          onSelect();
        }
      }}
    >
      {over ? <span className="absolute inset-y-0 left-0 w-[var(--size-tab-indicator)] bg-primary" /> : null}
      {mark}
      <span className="min-w-0 flex-1 truncate">
        {entry.label}
        {closing ? <span className="text-muted-foreground"> closing…</span> : null}
      </span>
      <Hint label={closeLabel}>
        <Button
          variant="ghost"
          size="icon-sm"
          className={`shrink-0 hover:bg-popover hover:text-foreground focus-visible:visible group-hover:visible ${active ? "visible" : "invisible"}`}
          aria-label={closeLabel}
          onClick={(event) => {
            event.stopPropagation();
            onClose();
          }}
        >
          <XIcon />
        </Button>
      </Hint>
      {active ? <span className="absolute inset-x-0 bottom-0 h-[var(--size-tab-indicator)] bg-primary" /> : null}
    </div>
    </Hint>
  );
});
