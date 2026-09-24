import { useVirtualizer } from "@tanstack/react-virtual";
import { useEffect, useMemo, useRef, useState } from "react";
import type { Actions } from "./actions";
import {
  disclosureMark,
  explorerRows,
  firstChildSelection,
  gitBadgeColor,
  moveSelection,
  parentPath,
  parentSelection,
  rowAccessibilityLabel,
  rowForPath,
  rowTitle,
  selectionAfterRemoval,
  type ExplorerRow,
} from "./explorer";
import { focusedCheckout } from "./snapshot";
import { useShellStore } from "./store";
import { useUiStore, type ExplorerDraft } from "./ui";
import { expandedUnderRoot, watchedFolders } from "./watch";

// The Explorer tree (PRD B1, B3, B9, B10): a lazy tree over the focused
// checkout. The core owns which folders are expanded (`ui_state.expanded_paths`)
// and every filesystem change (`explorer_operation`); hided answers one listing
// per folder, and the rows are derived from those plus the core's changed-file
// set (`explorer.ts`). Nothing here reads the disk or runs git; a row is a pure
// function of state that already arrived.

const EMPTY_PATHS: string[] = [];
const EMPTY_ROWS: ExplorerRow[] = [];

/** One drawn row: an entry, or the inline name field the operator is editing. */
type DisplayRow =
  | { kind: "entry"; row: ExplorerRow }
  | { kind: "draft"; depth: number; parent: string; path: string; initial: string; draftKind: ExplorerDraft["kind"] };

/** The rows with the inline field spliced in: after the folder a new entry
 * lands in, or in place of the entry being renamed. */
export function displayRows(rows: ExplorerRow[], draft: ExplorerDraft | null): DisplayRow[] {
  const base: DisplayRow[] = rows.map((row) => ({ kind: "entry", row }));
  if (!draft) return base;
  if (draft.kind === "rename") {
    const index = rows.findIndex((row) => row.path === draft.path);
    const row = index === -1 ? undefined : rows[index];
    if (!row) return base;
    base.splice(index, 1, { kind: "draft", depth: row.depth, parent: draft.parent, path: draft.path, initial: draft.initial, draftKind: "rename" });
    return base;
  }
  const index = rows.findIndex((row) => row.path === draft.parent);
  const anchor = index === -1 ? undefined : rows[index];
  const depth = anchor ? anchor.depth + 1 : 0;
  const at = index === -1 ? 0 : index + 1;
  base.splice(at, 0, { kind: "draft", depth, parent: draft.parent, path: draft.parent, initial: draft.initial, draftKind: draft.kind });
  return base;
}

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
  const operation = useShellStore((s) => s.rest?.explorer_operation ?? null);
  const folderChanges = useShellStore((s) => s.folderChanges);
  const invalidateListings = useShellStore((s) => s.invalidateListings);
  const selection = useUiStore((s) => s.explorerSelection);
  const setSelection = useUiStore((s) => s.setExplorerSelection);
  const draft = useUiStore((s) => s.explorerDraft);
  const setDraft = useUiStore((s) => s.setExplorerDraft);
  const scrollRef = useRef<HTMLDivElement>(null);
  const [refreshTick, setRefreshTick] = useState(0);
  const [menu, setMenu] = useState<{ path: string; isDirectory: boolean; x: number; y: number } | null>(null);
  // Folders this pane has asked hided for and not yet heard about; a refusal
  // keeps its entry so a refused folder is not re-asked on every render.
  const pending = useRef<Set<string>>(new Set());
  const pendingName = useRef<ExplorerDraft | null>(null);
  /** The watch-frame count already acted on, per folder. */
  const seenChanges = useRef<Record<string, number>>({});

  const expandedKey = expandedPaths.join("\n");
  const rows = useMemo(
    () => (rootPath ? explorerRows({ rootPath, listings, expandedPaths, changes }) : EMPTY_ROWS),
    // `expandedPaths` arrives as a fresh array per section; the joined key is
    // the identity that matters, and the listings/changes objects are shared.
    [rootPath, listings, expandedKey, changes],
  );
  const shown = useMemo(() => displayRows(rows, draft), [rows, draft]);
  const watchedNow = useMemo(
    () => new Set(rootPath ? watchedFolders(rootPath, expandedPaths) : []),
    // `expandedPaths` identity changes per section; the joined key is the one
    // that matters, exactly as the row memo above reads it.
    [rootPath, expandedKey],
  );

  // Only the watched folders are listed and kept live: the root and the most
  // recently expanded ones. A folder past the cap loses its listing and draws
  // the refresh badge instead (B2); pressing it makes that folder most recent.
  useEffect(() => {
    if (!rootPath) return;
    const watched = watchedFolders(rootPath, expandedPaths);
    const watchedNow = new Set(watched);
    // A watch frame that landed while a listing was in flight must be re-read:
    // its folder drops its pending marker and its (possibly older) listing.
    const bumped: string[] = [];
    for (const [path, count] of Object.entries(folderChanges)) {
      if ((seenChanges.current[path] ?? 0) === count) continue;
      seenChanges.current[path] = count;
      if (!watchedNow.has(path)) continue;
      pending.current.delete(path);
      bumped.push(path);
    }
    if (bumped.length > 0) invalidateListings(bumped);
    // Keep only the counts the store still holds, so a capped map does not
    // leave this ref growing for the session.
    for (const path of Object.keys(seenChanges.current)) {
      if (!(path in folderChanges)) delete seenChanges.current[path];
    }
    invalidateListings(
      expandedUnderRoot(rootPath, expandedPaths).filter((path) => path !== rootPath && !watchedNow.has(path)),
    );
    for (const folder of watched) {
      if (listings[folder] || pending.current.has(folder)) continue;
      pending.current.add(folder);
      actions.listChildren(rootPath, folder);
    }
    for (const folder of [...pending.current]) {
      if (listings[folder]) pending.current.delete(folder);
    }
  }, [rootPath, expandedPaths, listings, refreshTick, folderChanges, actions, invalidateListings]);

  // A reveal (or an open from anywhere) sets the core's selected_path. A
  // palette pick also sets the local cursor before the core replies; a panel
  // remount must not replace that newer pick with the older snapshot path.
  const previousCoreSelection = useRef(selectedPath);
  useEffect(() => {
    if (selectedPath && (previousCoreSelection.current !== selectedPath || !useUiStore.getState().explorerSelection)) {
      setSelection(selectedPath);
    }
    previousCoreSelection.current = selectedPath;
  }, [selectedPath, setSelection]);

  // A panel remount keeps the current selection; only a real checkout switch
  // invalidates its cursor and pending inline name.
  const previousRoot = useRef(rootPath);
  useEffect(() => {
    if (previousRoot.current === rootPath) return;
    previousRoot.current = rootPath;
    const selected = useShellStore.getState().rest?.ui_state?.selected_path ?? null;
    setSelection(rootPath && selected?.startsWith(`${rootPath}/`) ? selected : null);
    pending.current.clear();
    setMenu(null);
    setDraft(null);
    pendingName.current = null;
  }, [rootPath, setSelection, setDraft]);

  const lastOperation = useRef<number | null>(null);
  useEffect(() => {
    if (!operation || operation.phase === "working") return;
    if (operation.id === lastOperation.current) return;
    lastOperation.current = operation.id;
    const submitted = pendingName.current;
    if (submitted) {
      const expected = submitted.kind === "rename" ? submitted.path : `${submitted.parent}/${submitted.initial}`;
      const matches = operation.path === expected || (operation.kind === "refused" && operation.path === submitted.parent);
      if (matches) {
        pendingName.current = null;
        // The failed operation leaves its typed name in an editable row, so
        // the operator can correct a collision without starting over (B10).
        if (operation.phase === "failed") setDraft(submitted);
      }
    }
    // A settled change moved a row: the folders it touched are re-read, and
    // the created item is selected so the operator sees it (B9).
    const folders = [parentPath(operation.path), parentPath(operation.destination)];
    invalidateListings(folders);
    for (const folder of folders) pending.current.delete(folder);
    if (operation.phase === "finished") setSelection(operation.destination);
    setRefreshTick((tick) => tick + 1);
  }, [operation, invalidateListings, setSelection]);

  const virtualizer = useVirtualizer({
    count: shown.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: rowHeight,
    overscan: 12,
  });

  useEffect(() => {
    const index = rows.findIndex((row) => row.path === selection);
    if (index >= 0) virtualizer.scrollToIndex(index, { align: "auto" });
    // `shown` shifts when the inline field is spliced in; the scroll follows rows.
  }, [selection, rows, virtualizer]);

  const toggleFolder = (path: string) => {
    const next = new Set(expandedPaths);
    if (next.has(path)) next.delete(path);
    else next.add(path);
    actions.setExpandedPaths([...next]);
  };

  const expand = (path: string) => {
    if (expandedPaths.includes(path)) return;
    actions.setExpandedPaths([...expandedPaths, path]);
  };

  const openRow = (row: ExplorerRow, preview: boolean) => {
    setSelection(row.path);
    if (row.isDirectory) toggleFolder(row.path);
    else actions.openFile(row.path, preview);
  };

  const beginCreate = (parent: string, kind: "file" | "folder") => {
    expand(parent);
    setDraft({ kind, parent, path: parent, initial: "" });
    setMenu(null);
  };

  const beginRename = (row: ExplorerRow) => {
    setDraft({ kind: "rename", parent: parentPath(row.path), path: row.path, initial: row.name });
    setMenu(null);
  };

  const requestTrash = (row: ExplorerRow) => {
    actions.requestTrash(row.path, row.name, row.isDirectory, selectionAfterRemoval(rows, row.path, rootPath ?? row.path));
    setMenu(null);
  };

  const onKeyDown = (event: React.KeyboardEvent<HTMLDivElement>) => {
    const row = rowForPath(rows, selection);
    // ⌘⌫ is the Explorer's trash chord; the registry leaves it to the tree,
    // because in a terminal it is ^U (S2 passthrough).
    if (event.key === "Backspace" && event.metaKey) {
      event.preventDefault();
      if (row) requestTrash(row);
      return;
    }
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
  const failure = operation?.phase === "failed" && operation.message ? operation : null;

  const anchorTop = (path: string): number | null => {
    const index = shown.findIndex((entry) => (entry.kind === "entry" ? entry.row.path : entry.path) === path);
    if (index < 0) return null;
    const item = virtualizer.getVirtualItems().find((virtual) => virtual.index === index);
    return item ? item.start : null;
  };
  const failureTop = failure ? anchorTop(failure.path) ?? anchorTop(parentPath(failure.path)) ?? 0 : null;

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
            // A cached listing is dropped too, so the button re-reads what it
            // is showing rather than only the folders it never listed.
            pending.current.clear();
            invalidateListings(watchedFolders(rootPath, expandedPaths));
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
          onContextMenu={(event) => {
            if (event.target !== event.currentTarget && (event.target as HTMLElement).closest("[data-explorer-row]")) return;
            event.preventDefault();
            setMenu({ path: rootPath, isDirectory: true, x: event.clientX, y: event.clientY });
          }}
        >
          <div className="relative w-full" style={{ height: `${virtualizer.getTotalSize()}px` }}>
            {virtualizer.getVirtualItems().map((virtualRow) => {
              const entry = shown[virtualRow.index];
              if (!entry) return null;
              if (entry.kind === "draft") {
                return (
                  <DraftRowView
                    key="draft"
                    depth={entry.depth}
                    initial={entry.initial}
                    rename={entry.draftKind === "rename"}
                    top={virtualRow.start}
                    onCommit={(name) => {
                      pendingName.current = { kind: entry.draftKind, parent: entry.parent, path: entry.path, initial: name };
                      setDraft(null);
                      if (entry.draftKind === "rename") actions.renameEntry(entry.path, name);
                      else actions.createEntry(entry.parent, name, entry.draftKind === "folder");
                    }}
                    onCancel={() => setDraft(null)}
                  />
                );
              }
              const row = entry.row;
              const needsRefresh = row.isDirectory && row.expanded && !watchedNow.has(row.path);
              return (
                <ExplorerRowView
                  key={row.path}
                  row={row}
                  rootPath={rootPath}
                  selected={row.path === selection}
                  top={virtualRow.start}
                  needsRefresh={needsRefresh}
                  onOpen={openRow}
                  onRefresh={() => {
                    pending.current.delete(row.path);
                    actions.setExpandedPaths([...expandedPaths.filter((path) => path !== row.path), row.path]);
                  }}
                  onContextMenu={(event) => {
                    event.preventDefault();
                    setMenu({ path: row.path, isDirectory: row.isDirectory, x: event.clientX, y: event.clientY });
                  }}
                  onDragStart={() => {
                    dragPath.current = row.path;
                  }}
                  onDrop={(from) => {
                    if (from && from !== row.path && !row.path.startsWith(`${from}/`)) actions.moveEntry(from, row.path);
                  }}
                />
              );
            })}
            {failure && failureTop !== null ? (
              <div
                className="absolute inset-x-0 flex items-center gap-xs pr-md text-caption text-danger"
                data-explorer-failure={failure.path}
                style={{ top: failureTop + rowHeight(), height: "var(--size-pane-child-row)" }}
              >
                <span className="truncate pl-md">{failure.message}</span>
              </div>
            ) : null}
          </div>
        </div>
      )}
      {menu ? <ContextMenu menu={menu} onDismiss={() => setMenu(null)} onNewFile={beginCreate} onRename={beginRename} onTrash={requestTrash} rowFor={(path) => rowForPath(rows, path)} /> : null}
    </div>
  );
}

/** The one path a drag is carrying; a ref because a drag never re-renders. */
const dragPath = { current: null as string | null };

function DraftRowView({
  depth,
  initial,
  rename,
  top,
  onCommit,
  onCancel,
}: {
  depth: number;
  initial: string;
  rename: boolean;
  top: number;
  onCommit: (name: string) => void;
  onCancel: () => void;
}) {
  const [value, setValue] = useState(initial);
  const done = useRef(false);
  const finish = (commit: boolean) => {
    if (done.current) return;
    done.current = true;
    const name = value.trim();
    if (commit && name) onCommit(name);
    else onCancel();
  };
  return (
    <div
      className="absolute inset-x-0 flex items-center pr-md text-body"
      data-explorer-draft={rename ? "rename" : "create"}
      style={{ top: 0, height: "var(--size-pane-child-row)", paddingLeft: `calc(var(--size-lineage-indent) * ${depth})`, transform: `translateY(${top}px)` }}
    >
      <input
        autoFocus
        value={value}
        aria-label={rename ? "New name" : "New entry name"}
        className="min-w-0 flex-1 rounded-xs border border-accent bg-background px-xxs text-body text-primary outline-none"
        onChange={(event) => setValue(event.target.value)}
        onKeyDown={(event) => {
          if (event.key === "Enter") {
            event.preventDefault();
            finish(true);
          } else if (event.key === "Escape") {
            event.preventDefault();
            finish(false);
          }
          event.stopPropagation();
        }}
        onBlur={() => finish(true)}
      />
    </div>
  );
}

function ContextMenu({
  menu,
  onDismiss,
  onNewFile,
  onRename,
  onTrash,
  rowFor,
}: {
  menu: { path: string; isDirectory: boolean; x: number; y: number };
  onDismiss: () => void;
  onNewFile: (parent: string, kind: "file" | "folder") => void;
  onRename: (row: ExplorerRow) => void;
  onTrash: (row: ExplorerRow) => void;
  rowFor: (path: string) => ExplorerRow | null;
}) {
  useEffect(() => {
    const close = () => onDismiss();
    window.addEventListener("pointerdown", close);
    window.addEventListener("blur", close);
    return () => {
      window.removeEventListener("pointerdown", close);
      window.removeEventListener("blur", close);
    };
  }, [onDismiss]);
  const row = rowFor(menu.path);
  return (
    <div
      role="menu"
      data-explorer-menu={menu.path}
      className="fixed z-30 min-w-[var(--size-panel-min)] rounded-sm border border-divider bg-balloon py-xxs text-caption text-primary shadow-none"
      style={{ left: menu.x, top: menu.y }}
      onPointerDown={(event) => event.stopPropagation()}
    >
      {menu.isDirectory ? (
        <>
          <MenuItem label="New File" testId="new-file" onClick={() => onNewFile(menu.path, "file")} />
          <MenuItem label="New Folder" testId="new-folder" onClick={() => onNewFile(menu.path, "folder")} />
        </>
      ) : null}
      {row ? <MenuItem label="Rename" testId="rename" onClick={() => onRename(row)} /> : null}
      {row ? <MenuItem label="Move to Trash" testId="trash" danger onClick={() => onTrash(row)} /> : null}
    </div>
  );
}

function MenuItem({ label, testId, danger, onClick }: { label: string; testId: string; danger?: boolean; onClick: () => void }) {
  return (
    <button
      type="button"
      role="menuitem"
      data-menu-item={testId}
      className={`block w-full px-md py-xs text-left hover:bg-elevated ${danger ? "text-danger" : "text-primary"}`}
      onClick={onClick}
    >
      {label}
    </button>
  );
}

function ExplorerRowView({
  row,
  rootPath,
  selected,
  top,
  needsRefresh,
  onOpen,
  onRefresh,
  onContextMenu,
  onDragStart,
  onDrop,
}: {
  row: ExplorerRow;
  rootPath: string;
  selected: boolean;
  top: number;
  needsRefresh: boolean;
  onOpen: (row: ExplorerRow, preview: boolean) => void;
  onRefresh: () => void;
  onContextMenu: (event: React.MouseEvent) => void;
  onDragStart: () => void;
  onDrop: (from: string | null) => void;
}) {
  const [over, setOver] = useState(false);
  return (
    <div
      role="treeitem"
      aria-selected={selected}
      aria-level={row.depth + 1}
      aria-expanded={row.isDirectory ? row.expanded : undefined}
      aria-label={rowAccessibilityLabel(row)}
      title={rowTitle(row, rootPath)}
      draggable
      data-explorer-row={row.path}
      data-selected={selected ? "true" : "false"}
      data-decoration={row.decoration?.status ?? ""}
      data-drop={over ? "true" : "false"}
      className={`absolute inset-x-0 flex cursor-default items-center gap-xs pr-md text-body ${
        selected ? "bg-elevated text-primary" : "text-secondary hover:bg-elevated"
      } ${over ? "bg-elevated" : ""}`}
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
      onContextMenu={onContextMenu}
      onDragStart={(event) => {
        event.stopPropagation();
        event.dataTransfer.effectAllowed = "move";
        event.dataTransfer.setData("text/plain", row.path);
        onDragStart();
      }}
      onDragOver={(event) => {
        if (!row.isDirectory || !dragPath.current) return;
        event.preventDefault();
        event.dataTransfer.dropEffect = "move";
        setOver(true);
      }}
      onDragLeave={() => setOver(false)}
      onDrop={(event) => {
        event.preventDefault();
        setOver(false);
        const from = dragPath.current ?? event.dataTransfer.getData("text/plain");
        dragPath.current = null;
        onDrop(from || null);
      }}
      onDragEnd={() => {
        dragPath.current = null;
        setOver(false);
      }}
    >
      <span className="w-[var(--size-lineage-chevron)] shrink-0 text-center text-caption text-muted" aria-hidden="true">
        {disclosureMark(row)}
      </span>
      <span className={`shrink-0 ${row.icon.color}`} style={{ fontFamily: "seti" }} aria-hidden="true">
        {row.icon.glyph}
      </span>
      <span className="min-w-0 flex-1 truncate">{row.name}</span>
      {needsRefresh ? (
        <button
          type="button"
          className="shrink-0 rounded-xs px-xxs text-caption text-warning hover:text-primary"
          data-explorer-refresh={row.path}
          aria-label={`Refresh ${row.name}`}
          title="This folder is no longer watched; refresh to read it"
          onClick={(event) => {
            event.stopPropagation();
            onRefresh();
          }}
          onPointerDown={(event) => event.stopPropagation()}
        >
          ↻
        </button>
      ) : null}
      {row.decoration ? (
        <span className={`shrink-0 text-caption ${gitBadgeColor(row.decoration.status)}`} title={row.decoration.title}>
          {row.decoration.badge}
        </span>
      ) : null}
    </div>
  );
}
