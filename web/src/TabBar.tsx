import { memo, useRef, useState } from "react";
import type { Actions } from "./actions";
import { focusedCheckout, type AsyncOperation, type Checkout, type StripTab } from "./snapshot";
import { useShellStore } from "./store";

// The tab bar draws the focused checkout's strip (PRD S2 B3). Only Herdr
// tabs are drawn: the web shell has no editor surface yet, so a file, diff
// or session entry has nothing to show. A drag that lands sends one
// `reorder_tab` with the strip index of the tab it landed on, and the bar
// redraws in the core's order; nothing moves until the snapshot says so.

export function TabBar({ actions }: { actions: Actions }) {
  const checkout = useShellStore((s) => focusedCheckout(s.rest));
  const operations = useShellStore((s) => s.rest?.status?.async_operations);
  if (!checkout) return <div className="h-[var(--size-tab-strip)] shrink-0 bg-panel" data-tab-bar="empty" />;
  return <Strip checkout={checkout} operations={operations ?? NONE} actions={actions} />;
}

const NONE: AsyncOperation[] = [];

/** Phases of a close the core is still confirming with Herdr (`operations.rs`). */
const IN_FLIGHT = new Set(["transmitting", "awaiting_topology", "unknown"]);

/** "closing…" while the core is still confirming a close with Herdr. */
export function closingSuffix(targetId: string, kind: "tab.close" | "pane.close", operations: AsyncOperation[]): boolean {
  return operations.some((op) => op.kind === kind && op.target_id === targetId && IN_FLIGHT.has(op.phase));
}

const Strip = memo(function Strip({
  checkout,
  operations,
  actions,
}: {
  checkout: Checkout;
  operations: AsyncOperation[];
  actions: Actions;
}) {
  const herdrTabs = checkout.strip
    .map((entry, index) => ({ entry, index }))
    .filter(({ entry }) => entry.kind === "herdr");
  const [dragging, setDragging] = useState<string | null>(null);
  const [over, setOver] = useState<string | null>(null);
  const activation = useRef<{ id: string; x: number } | null>(null);

  return (
    <div
      className="flex h-[var(--size-tab-strip)] shrink-0 items-stretch overflow-x-auto bg-panel"
      role="tablist"
      data-tab-bar={checkout.id}
    >
      {herdrTabs.map(({ entry, index }) => (
        <TabButton
          key={entry.id}
          entry={entry}
          active={entry.source_id === checkout.active_tab_id}
          closing={closingSuffix(entry.source_id, "tab.close", operations)}
          dragging={dragging === entry.id}
          over={over === entry.id && dragging !== entry.id}
          onSelect={() => actions.focusTab(entry.source_id)}
          onClose={() => actions.closeTab(entry.source_id)}
          onPointerDown={(x) => {
            activation.current = { id: entry.id, x };
          }}
          onPointerMove={(x) => {
            const start = activation.current;
            if (!start || dragging) return;
            const threshold = Number.parseFloat(
              getComputedStyle(document.documentElement).getPropertyValue("--size-tab-drag-activation"),
            );
            if (Math.abs(x - start.x) >= threshold) setDragging(start.id);
          }}
          onPointerEnter={() => {
            if (dragging) setOver(entry.id);
          }}
          onPointerUp={() => {
            const from = dragging;
            activation.current = null;
            setDragging(null);
            setOver(null);
            if (from && from !== entry.id) actions.reorderTab(from, index);
          }}
        />
      ))}
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

const TabButton = memo(function TabButton({
  entry,
  active,
  closing,
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
  active: boolean;
  closing: boolean;
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
    <div
      role="tab"
      aria-selected={active}
      data-tab={entry.source_id}
      data-closing={closing ? "true" : "false"}
      className={`group relative flex max-w-[var(--size-tab-preferred)] min-w-[var(--size-tab-title-min)] shrink-0 cursor-default items-center gap-xs px-sm text-caption ${
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
    >
      {over ? <span className="absolute inset-y-0 left-0 w-[var(--size-tab-indicator)] bg-accent" /> : null}
      <span className="min-w-0 flex-1 truncate">
        {entry.label}
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
