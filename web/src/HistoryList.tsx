import { useVirtualizer } from "@tanstack/react-virtual";
import { useEffect, useMemo, useRef, useState } from "react";
import type { Actions } from "./actions";
import { fileIcon } from "./fileIcons";
import { EntryContextMenu, type MenuEntry } from "./components/entry-menu";
import { changesFor, frontCheckout, type ChangedFileSnapshot, type ChangedFileStatus } from "./snapshot";
import { useShellStore } from "./store";
import { drawnViews } from "./viewFocus";
import { besideUnavailable } from "./viewLayout";
import { workspaceViewOf } from "./workspace";

const STATUS: Record<ChangedFileStatus, { mark: string; label: string; color: string }> = {
  modified: { mark: "M", label: "Modified", color: "text-warning" },
  added: { mark: "A", label: "Added", color: "text-success" },
  deleted: { mark: "D", label: "Deleted", color: "text-destructive" },
  untracked: { mark: "U", label: "Untracked", color: "text-success" },
  renamed: { mark: "R", label: "Renamed", color: "text-warning" },
  conflict: { mark: "!", label: "Conflict", color: "text-destructive" },
};

function identity(entry: ChangedFileSnapshot, committed: boolean): string {
  const path = entry.previous_relative_path
    ? `${entry.previous_relative_path} → ${entry.relative_path}`
    : entry.relative_path;
  return `${committed ? "Committed on branch" : "Uncommitted"}: ${path}, ${STATUS[entry.status].label}`;
}

function ChangeRow({ entry, committed, selected, actions }: {
  entry: ChangedFileSnapshot;
  committed: boolean;
  selected: boolean;
  actions: Actions;
}) {
  const parts = entry.relative_path.split("/");
  const name = parts.pop() ?? entry.relative_path;
  const parent = parts.join("/");
  const icon = fileIcon(name);
  const status = STATUS[entry.status];
  const title = identity(entry, committed);
  // The row's diff beside the active View area (S7 B4, contract 4.2), as a
  // click would open it in the active area; a deleted file has a diff too.
  const menu = (): MenuEntry<"open_beside">[] => [
    { id: "open_beside", label: "Open to the side", unavailable: besideUnavailable(workspaceViewOf(useShellStore.getState().rest)?.layout, drawnViews()) },
  ];
  return (
    <EntryContextMenu
      label={`${name} actions`}
      items={menu}
      onSelect={() => actions.openChangeBeside(entry.path, committed)}
      className="block"
      data-history-menu={entry.relative_path}
    >
      <button
        type="button"
        className={`flex h-[var(--size-pane-child-row)] w-full min-w-0 items-center gap-xs px-sm text-left text-caption hover:bg-accent ${selected ? "bg-secondary text-foreground" : "text-subtle-foreground"}`}
        aria-label={title}
        aria-current={selected ? "true" : undefined}
        title={title}
        data-history-path={entry.relative_path}
        data-history-group={committed ? "committed" : "working"}
        onClick={() => actions.selectChange(entry.path, committed, true)}
        onDoubleClick={() => actions.selectChange(entry.path, committed, false)}
      >
        <span aria-hidden="true" className={`shrink-0 ${icon.color}`} style={{ fontFamily: "seti" }}>{icon.glyph}</span>
        <span className="flex min-w-0 flex-1 items-baseline gap-xs overflow-hidden">
          <span className="shrink-0 truncate">{name}</span>
          {parent ? <span className="min-w-0 truncate text-muted-foreground">{parent}</span> : null}
        </span>
        {entry.added_lines !== null ? <span className="shrink-0 text-success" aria-label={`${entry.added_lines} lines added`}>+{entry.added_lines}</span> : null}
        {entry.removed_lines !== null ? <span className="shrink-0 text-destructive" aria-label={`${entry.removed_lines} lines removed`}>-{entry.removed_lines}</span> : null}
        <span className={`shrink-0 ${status.color}`} aria-hidden="true">{status.mark}</span>
      </button>
    </EntryContextMenu>
  );
}

type Item = { kind: "group"; committed: boolean; title: string; count: number } | { kind: "row"; committed: boolean; entry: ChangedFileSnapshot };

export function HistoryList({ actions }: { actions: Actions }) {
  const checkout = useShellStore((s) => frontCheckout(s.rest));
  const rootPath = useShellStore((s) => s.rest?.navigator?.changes_root_path ?? null);
  const changes = useShellStore((s) => changesFor(s.changes, rootPath));
  const [expanded, setExpanded] = useState({ working: true, committed: true });
  const scrollRef = useRef<HTMLDivElement>(null);
  useEffect(() => setExpanded({ working: true, committed: true }), [rootPath]);
  const items = useMemo(() => {
    const result: Item[] = [];
    if (!changes || changes.unavailable_reason) return result;
    if (changes.entries.length > 0) {
      result.push({ kind: "group", committed: false, title: "UNCOMMITTED", count: changes.entries.length });
      if (expanded.working) result.push(...changes.entries.map((entry): Item => ({ kind: "row", committed: false, entry })));
    }
    if (changes.base_branch && changes.committed.length > 0) {
      result.push({ kind: "group", committed: true, title: `COMMITTED ON BRANCH · ${changes.base_branch}`, count: changes.committed.length });
      if (expanded.committed) result.push(...changes.committed.map((entry): Item => ({ kind: "row", committed: true, entry })));
    }
    return result;
  }, [changes, expanded]);
  const virtualizer = useVirtualizer({
    count: items.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => Number.parseFloat(getComputedStyle(document.documentElement).getPropertyValue("--size-pane-child-row")) || 24,
    overscan: 12,
  });
  if (!checkout) return <p className="px-md py-sm text-caption text-muted-foreground" data-history-state="no-workspace">Open a workspace to see History.</p>;
  if (!rootPath) return <p className="px-md py-sm text-caption text-muted-foreground" data-history-state="no-folder">History has no folder for this checkout.</p>;
  if (!changes) return <p className="px-md py-sm text-caption text-muted-foreground" data-history-state="loading">Reading changes…</p>;
  // The core's own reason, which names what to do where there is something
  // to do (allow the helper, install Git); History reads again on its own.
  if (changes.unavailable_reason) return <p className="break-words px-md py-sm text-caption text-warning" data-history-state="unavailable">History is unavailable: {changes.unavailable_reason}</p>;
  const stale = changes.stale_reason ? (
    <p className="break-words px-md py-xs text-caption text-warning" data-history-state="stale">
      Showing the last confirmed changes; the latest read failed: {changes.stale_reason}
    </p>
  ) : null;
  if (items.length === 0) {
    return (
      <>
        {stale}
        <p className="px-md py-sm text-caption text-muted-foreground" data-history-state="clean">No changes in this checkout.</p>
      </>
    );
  }
  return (
    <>
    {stale}
    <div ref={scrollRef} className="min-h-0 flex-1 overflow-auto" data-history-root={rootPath} aria-label="History changes">
      <div className="relative w-full" style={{ height: `${virtualizer.getTotalSize()}px` }}>
        {virtualizer.getVirtualItems().map((virtualRow) => {
          const item = items[virtualRow.index];
          if (!item) return null;
          // Placed by `top`, not a transform: a transformed row would hold its
          // row menu's fixed position, drawing the menu away from the pointer
          // and clipping it inside this list (S7 B4, B9).
          return <div key={item.kind === "group" ? `group:${item.committed}` : `${item.committed}:${item.entry.path}`} className="absolute inset-x-0" style={{ top: virtualRow.start }}>
            {item.kind === "group" ? <button
              type="button"
              className="flex h-[var(--size-pane-child-row)] w-full items-center gap-xs px-sm text-left text-caption text-subtle-foreground hover:bg-accent"
              aria-expanded={item.committed ? expanded.committed : expanded.working}
              aria-label={`${item.title}, ${item.count} files`}
              data-history-group-section={item.committed ? "committed" : "working"}
              onClick={() => setExpanded((current) => item.committed ? { ...current, committed: !current.committed } : { ...current, working: !current.working })}
            >
              <span aria-hidden="true">{(item.committed ? expanded.committed : expanded.working) ? "▾" : "▸"}</span>
              <span className="min-w-0 flex-1 truncate">{item.title}</span>
              <span aria-hidden="true">{item.count}</span>
            </button> : <ChangeRow entry={item.entry} committed={item.committed} selected={changes.selected_path === item.entry.path && changes.selected_committed === item.committed} actions={actions} />}
          </div>;
        })}
      </div>
    </div>
    </>
  );
}
