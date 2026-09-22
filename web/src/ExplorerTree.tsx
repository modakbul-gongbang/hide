import { useVirtualizer } from "@tanstack/react-virtual";
import { useEffect, useMemo, useRef, useState } from "react";
import type { Actions } from "./actions";
import {
  disclosureMark,
  explorerRows,
  firstChildSelection,
  gitBadgeColor,
  moveSelection,
  parentSelection,
  rowAccessibilityLabel,
  rowForPath,
  rowTitle,
  type ExplorerRow,
} from "./explorer";
import { focusedCheckout } from "./snapshot";
import { useShellStore } from "./store";
import { useUiStore } from "./ui";

// The Explorer tree (PRD B1, B3): a lazy tree over the focused checkout. The
// core owns which folders are expanded (`ui_state.expanded_paths`), hided
// answers one listing per folder, and the rows are derived from those two plus
// the core's changed-file set (`explorer.ts`). Nothing here reads the disk or
// runs git; a row is a pure function of state that already arrived.

const EMPTY_PATHS: string[] = [];
const EMPTY_ROWS: ExplorerRow[] = [];

/** The row height the virtual list lays out with; the Swift outline is 22 and
 * this reads the same order of token rather than writing a second number. */
function rowHeight(): number {
  const value = getComputedStyle(document.documentElement).getPropertyValue("--size-pane-child-row");
  return Number.parseFloat(value) || 24;
}

function baseName(path: string): string {
  const parts = path.split("/").filter(Boolean);
  return parts[parts.length - 1] ?? path;
}

export function ExplorerTree({ actions }: { actions: Actions }) {
  const checkout = useShellStore((s) => focusedCheckout(s.rest));
  const rootPath = checkout?.path ?? null;
  const expandedPaths = useShellStore((s) => s.rest?.ui_state?.expanded_paths ?? EMPTY_PATHS);
  const listings = useShellStore((s) => s.listings);
  const changes = useShellStore((s) => s.changes);
  const selectedPath = useShellStore((s) => s.rest?.ui_state?.selected_path ?? null);
  const pathRefusal = useShellStore((s) => s.pathRefusal);
  const selection = useUiStore((s) => s.explorerSelection);
  const setSelection = useUiStore((s) => s.setExplorerSelection);
  const scrollRef = useRef<HTMLDivElement>(null);
  const [refreshTick, setRefreshTick] = useState(0);
  // Folders this pane has asked hided for and not yet heard about; a refusal
  // keeps its entry so a refused folder is not re-asked on every render.
  const pending = useRef<Set<string>>(new Set());

  const expandedKey = expandedPaths.join("\n");
  const rows = useMemo(
    () =>
      rootPath
        ? explorerRows({ rootPath, listings, expandedPaths, changes })
        : EMPTY_ROWS,
    // `expandedPaths` arrives as a fresh array per section; the joined key is
    // the identity that matters, and the listings/changes objects are shared.
    [rootPath, listings, expandedKey, changes],
  );

  // One listing per expanded folder plus the root, asked once per folder. A
  // folder whose listing was evicted past the store cap is asked again only
  // after a refresh, which is the badge's job (B2).
  useEffect(() => {
    if (!rootPath) return;
    const needed = [rootPath, ...expandedPaths];
    for (const folder of needed) {
      if (listings[folder] || pending.current.has(folder)) continue;
      pending.current.add(folder);
      actions.listChildren(rootPath, folder);
    }
    for (const folder of [...pending.current]) {
      if (listings[folder]) pending.current.delete(folder);
    }
  }, [rootPath, expandedPaths, listings, refreshTick, actions]);

  // A reveal (or an open from anywhere) sets the core's selected_path; the
  // local cursor follows it so the tree highlights what was revealed.
  useEffect(() => {
    if (selectedPath) setSelection(selectedPath);
  }, [selectedPath, setSelection]);

  // Switching checkouts drops the cursor: the previous path is not a row here.
  useEffect(() => {
    setSelection(null);
    pending.current.clear();
  }, [rootPath, setSelection]);

  const virtualizer = useVirtualizer({
    count: rows.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: rowHeight,
    overscan: 12,
  });

  useEffect(() => {
    const index = rows.findIndex((row) => row.path === selection);
    if (index >= 0) virtualizer.scrollToIndex(index, { align: "auto" });
  }, [selection, rows, virtualizer]);

  const toggleFolder = (path: string) => {
    const next = new Set(expandedPaths);
    if (next.has(path)) next.delete(path);
    else next.add(path);
    actions.setExpandedPaths([...next]);
  };

  const openRow = (row: ExplorerRow, preview: boolean) => {
    setSelection(row.path);
    if (row.isDirectory) toggleFolder(row.path);
    else actions.openFile(row.path, preview);
  };

  const onKeyDown = (event: React.KeyboardEvent<HTMLDivElement>) => {
    const row = rowForPath(rows, selection);
    switch (event.key) {
      case "ArrowDown":
        event.preventDefault();
        setSelection(moveSelection(rows, selection, 1));
        return;
      case "ArrowUp":
        event.preventDefault();
        setSelection(moveSelection(rows, selection, -1));
        return;
      case "ArrowRight":
        event.preventDefault();
        if (row?.isDirectory && !row.expanded) toggleFolder(row.path);
        else setSelection(firstChildSelection(rows, selection) ?? selection);
        return;
      case "ArrowLeft":
        event.preventDefault();
        if (row?.isDirectory && row.expanded) toggleFolder(row.path);
        else setSelection(parentSelection(rows, selection));
        return;
      case "Enter":
        event.preventDefault();
        if (row) openRow(row, false);
        return;
      default:
    }
  };

  if (!checkout || !rootPath) {
    return (
      <div className="flex min-h-0 flex-1 items-center justify-center px-md text-center text-caption text-muted" data-explorer-state="no-checkout">
        No checkout
      </div>
    );
  }

  const refused = pathRefusal?.kind === "file_list" && pathRefusal.path === rootPath ? pathRefusal.reason : null;
  const rootListing = listings[rootPath];

  return (
    <div className="flex min-h-0 flex-1 flex-col" data-explorer={rootPath}>
      <div className="flex shrink-0 items-center gap-xs border-b border-divider px-md py-xs text-caption text-secondary">
        <span className="min-w-0 flex-1 truncate" title={rootPath} data-explorer-root="true">
          {baseName(rootPath)}
        </span>
        <button
          type="button"
          className="text-muted hover:text-primary"
          aria-label="Refresh the file tree"
          title="Refresh"
          onClick={() => {
            pending.current.clear();
            setRefreshTick((tick) => tick + 1);
          }}
        >
          ↻
        </button>
      </div>
      {refused ? (
        <div className="border-b border-divider px-md py-sm text-caption text-danger" data-explorer-refusal={refused}>
          {refused}
        </div>
      ) : null}
      {!rootListing && !refused ? (
        <div className="px-md py-sm text-caption text-muted" data-explorer-state="loading">
          Loading…
        </div>
      ) : rows.length === 0 ? (
        <div className="px-md py-sm text-caption text-muted" data-explorer-state="empty">
          Empty folder
        </div>
      ) : (
        <div
          ref={scrollRef}
          role="tree"
          tabIndex={0}
          aria-label="Checkout files"
          className="min-h-0 flex-1 overflow-auto outline-none"
          data-explorer-tree="true"
          onPointerDown={(event) => event.currentTarget.focus()}
          onKeyDown={onKeyDown}
        >
          <div className="relative w-full" style={{ height: `${virtualizer.getTotalSize()}px` }}>
            {virtualizer.getVirtualItems().map((virtualRow) => {
              const row = rows[virtualRow.index];
              if (!row) return null;
              return (
                <ExplorerRowView
                  key={row.path}
                  row={row}
                  rootPath={rootPath}
                  selected={row.path === selection}
                  top={virtualRow.start}
                  onOpen={openRow}
                />
              );
            })}
          </div>
        </div>
      )}
    </div>
  );
}

function ExplorerRowView({
  row,
  rootPath,
  selected,
  top,
  onOpen,
}: {
  row: ExplorerRow;
  rootPath: string;
  selected: boolean;
  top: number;
  onOpen: (row: ExplorerRow, preview: boolean) => void;
}) {
  return (
    <div
      role="treeitem"
      aria-selected={selected}
      aria-level={row.depth + 1}
      aria-expanded={row.isDirectory ? row.expanded : undefined}
      aria-label={rowAccessibilityLabel(row)}
      title={rowTitle(row, rootPath)}
      data-explorer-row={row.path}
      data-selected={selected ? "true" : "false"}
      data-decoration={row.decoration?.status ?? ""}
      className={`absolute inset-x-0 flex cursor-default items-center gap-xs pr-md text-body ${
        selected ? "bg-elevated text-primary" : "text-secondary hover:bg-elevated"
      }`}
      style={{
        top: 0,
        height: "var(--size-pane-child-row)",
        paddingLeft: `calc(var(--size-lineage-indent) * ${row.depth})`,
        transform: `translateY(${top}px)`,
      }}
      onPointerDown={(event) => {
        event.stopPropagation();
        (event.currentTarget.parentElement?.parentElement as HTMLElement | null)?.focus();
      }}
      onClick={() => onOpen(row, true)}
      onDoubleClick={() => onOpen(row, false)}
    >
      <span className="w-[var(--size-lineage-chevron)] shrink-0 text-center text-caption text-muted" aria-hidden="true">
        {disclosureMark(row)}
      </span>
      <span className={`shrink-0 ${row.icon.color}`} style={{ fontFamily: "seti" }} aria-hidden="true">
        {row.icon.glyph}
      </span>
      <span className="min-w-0 flex-1 truncate">{row.name}</span>
      {row.decoration ? (
        <span className={`shrink-0 text-caption ${gitBadgeColor(row.decoration.status)}`} title={row.decoration.title}>
          {row.decoration.badge}
        </span>
      ) : null}
    </div>
  );
}
