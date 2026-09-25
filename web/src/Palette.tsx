import { useEffect, useRef, useState } from "react";
import type { Actions } from "./actions";
import { fileIcon } from "./fileIcons";
import { filterEntries, searchEntries, type SearchEntry } from "./search";
import { explorerContext } from "./snapshot";
import { useShellStore } from "./store";
import { useUiStore } from "./ui";
import { drawnViews } from "./viewFocus";

// The two palettes (PRD B12, B13). They share the shell: a query field, a
// list that ↑↓ walks and Enter commits, and Escape closes. ⌘P lists what
// hided's index ranked for the typed query; ⌘K filters the snapshot's agents,
// projects and checkouts in the web itself, because the data is already here.
// "Open file to the side" is ⌘P's list whose pick opens beside (S7 B4).

export function Palette({ actions }: { actions: Actions }) {
  const overlay = useUiStore((s) => s.overlay);
  if (overlay === "file_palette" || overlay === "file_palette_beside") {
    return <FilePalette key={overlay} beside={overlay === "file_palette_beside"} actions={actions} />;
  }
  if (overlay === "search") return <SearchPalette actions={actions} />;
  return null;
}

function PaletteShell({
  label,
  placeholder,
  query,
  onQuery,
  footer,
  children,
  onKeyDown,
}: {
  label: string;
  placeholder: string;
  query: string;
  onQuery: (query: string) => void;
  footer: string;
  children: React.ReactNode;
  onKeyDown: (event: React.KeyboardEvent) => void;
}) {
  const close = useUiStore((s) => s.closeOverlay);
  return (
    <div className="absolute inset-0 z-40 flex justify-center pt-[var(--size-settings-sheet-window-inset)]" role="presentation" data-palette={label}>
      <div className="absolute inset-0 bg-background opacity-[var(--opacity-secondary)]" onClick={() => close()} />
      <div className="relative w-[var(--size-search-sheet-w)] max-w-full rounded-lg border border-divider bg-panel text-body text-primary shadow-lg">
        <input
          autoFocus
          value={query}
          placeholder={placeholder}
          aria-label={label}
          data-palette-input="true"
          className="w-full rounded-t-lg border-b border-divider bg-panel px-md py-sm text-body text-primary outline-none"
          onChange={(event) => onQuery(event.target.value)}
          onKeyDown={(event) => {
            if (event.nativeEvent.isComposing) return;
            onKeyDown(event);
          }}
        />
        <div className="max-h-[var(--size-search-sheet-h)] overflow-auto" data-palette-list="true">
          {children}
        </div>
        {footer ? (
          <div className="border-t border-divider px-md py-xxs text-caption text-muted" data-palette-footer="true">
            {footer}
          </div>
        ) : null}
      </div>
    </div>
  );
}

function PaletteRow({
  active,
  icon,
  title,
  subtitle,
  unavailable = false,
  onPick,
  onHover,
  testId,
}: {
  active: boolean;
  icon?: React.ReactNode;
  title: string;
  subtitle?: string;
  /** A command that cannot run now: drawn muted with its whole reason under the title, and picking it does nothing. */
  unavailable?: boolean;
  onPick: () => void;
  onHover: () => void;
  testId: string;
}) {
  return (
    <button
      type="button"
      data-palette-row={testId}
      aria-selected={active}
      aria-disabled={unavailable || undefined}
      className={`flex w-full items-baseline gap-sm px-md py-xs text-left ${active ? "bg-elevated" : ""} ${unavailable ? "text-muted" : active ? "text-primary" : "text-secondary"}`}
      onPointerEnter={onHover}
      onClick={onPick}
    >
      {icon}
      {unavailable ? (
        // The whole reason, under the title, as every web menu draws an item it cannot run (B9).
        <span className="flex min-w-0 flex-1 flex-col">
          <span className="truncate">{title}</span>
          {subtitle ? <span className="text-caption text-muted">{subtitle}</span> : null}
        </span>
      ) : (
        <>
          <span className="min-w-0 flex-1 truncate">{title}</span>
          {subtitle ? <span className="max-w-[var(--size-recent-location-max)] shrink-0 truncate text-caption text-muted">{subtitle}</span> : null}
        </>
      )}
    </button>
  );
}

function usePaletteNavigation(count: number, onCommit: (index: number) => void) {
  const [index, setIndex] = useState(0);
  useEffect(() => {
    setIndex((current) => (count === 0 ? 0 : Math.min(current, count - 1)));
  }, [count]);
  const onKeyDown = (event: React.KeyboardEvent) => {
    if (event.key === "ArrowDown") {
      event.preventDefault();
      setIndex((current) => (count === 0 ? 0 : (current + 1) % count));
    } else if (event.key === "ArrowUp") {
      event.preventDefault();
      setIndex((current) => (count === 0 ? 0 : (current - 1 + count) % count));
    } else if (event.key === "Enter") {
      event.preventDefault();
      if (count > 0) onCommit(index);
    }
  };
  return { index, setIndex, onKeyDown };
}

function FilePalette({ beside, actions }: { beside: boolean; actions: Actions }) {
  // The checkout in front on the device in front, as the Explorer shows it.
  const device = useShellStore((s) => explorerContext(s.rest).device);
  const root = useShellStore((s) => explorerContext(s.rest).checkout?.path ?? null);
  // An answer for another device or checkout is not this list.
  const fileIndex = useShellStore((s) =>
    s.fileIndex && s.fileIndex.device_id === device && s.fileIndex.root_path === root ? s.fileIndex : null,
  );
  const [query, setQuery] = useState("");
  const timer = useRef<number | undefined>(undefined);

  useEffect(() => {
    if (!root) return undefined;
    window.clearTimeout(timer.current);
    // A palette keystroke is not one event per character: the index answer is
    // what the list draws, so the query is debounced to one request.
    timer.current = window.setTimeout(() => actions.requestFileIndex(root, query, device), 120);
    return () => window.clearTimeout(timer.current);
  }, [root, query, device, actions]);

  // The first query for a checkout starts the walk and answers `indexing`; ask
  // again while it says so, so the list fills without another keystroke.
  useEffect(() => {
    if (!root || !fileIndex?.indexing) return undefined;
    const poll = window.setTimeout(() => actions.requestFileIndex(root, query, device), 250);
    return () => window.clearTimeout(poll);
  }, [root, query, device, fileIndex, actions]);

  const entries = fileIndex?.files ?? [];
  const open = (path: string) => (beside ? actions.openIndexEntryBeside(path) : actions.openIndexEntry(path));
  const navigation = usePaletteNavigation(entries.length, (index) => {
    const entry = entries[index];
    if (entry) open(entry.path);
  });

  return (
    <PaletteShell
      label={beside ? "Open file to the side" : "Open file"}
      placeholder={beside ? "Search files to open beside the active view" : "Search files by name"}
      query={query}
      onQuery={setQuery}
      onKeyDown={navigation.onKeyDown}
      footer={fileIndex?.truncated ? "The index is truncated at 50,000 files" : ""}
    >
      {fileIndex?.unavailable ? (
        <div className="px-md py-sm text-caption text-muted" role="alert" data-palette-state="unavailable">
          {`Files could not be listed: ${fileIndex.unavailable}`}
        </div>
      ) : fileIndex?.indexing && entries.length === 0 ? (
        <div className="px-md py-sm text-caption text-muted" data-palette-state="indexing">
          Indexing…
        </div>
      ) : entries.length === 0 ? (
        <div className="px-md py-sm text-caption text-muted" data-palette-state="empty">
          {query ? "No matching files" : "Type to search this checkout"}
        </div>
      ) : (
        entries.map((entry, index) => (
          <PaletteRow
            key={entry.path}
            testId={entry.path}
            active={index === navigation.index}
            icon={
              <span className={`shrink-0 ${fileIcon(entry.relative_path).color}`} style={{ fontFamily: "seti" }} aria-hidden="true">
                {fileIcon(entry.relative_path).glyph}
              </span>
            }
            title={entry.relative_path}
            onHover={() => navigation.setIndex(index)}
            onPick={() => open(entry.path)}
          />
        ))
      )}
    </PaletteShell>
  );
}

function SearchPalette({ actions }: { actions: Actions }) {
  const rest = useShellStore((s) => s.rest);
  const [query, setQuery] = useState("");
  const workspaceOnScreen = useUiStore((s) => s.screen?.kind === "workspace");
  const placement = useUiStore((s) => s.toolsPlacement);
  const entries = filterEntries(searchEntries(rest, workspaceOnScreen ? { drawn: drawnViews(), placement } : null), query);
  const navigation = usePaletteNavigation(entries.length, (index) => activate(entries[index]));

  const activate = (entry: SearchEntry | undefined) => {
    // A command that cannot run now stays in the list with its reason.
    if (!entry || entry.unavailable) return;
    useUiStore.getState().closeOverlay();
    if (entry.command) {
      if ("layout" in entry.command) actions.setLayout(entry.command.layout);
      else if ("tool" in entry.command) actions.setTool(entry.command.tool, entry.command.visible);
      else if ("view" in entry.command) actions.runViewCommand(entry.command.view);
      else actions.openFilePaletteBeside();
    } else if (entry.kind === "agent" && entry.paneId) {
      actions.openAgent(entry.paneId);
    } else if (entry.kind === "project" && entry.workspaceId) {
      useUiStore.getState().setScreen({ kind: "overview", projectId: entry.workspaceId });
    } else if (entry.workspaceId && entry.checkoutId) {
      useUiStore.getState().setScreen({ kind: "workspace" });
      actions.focusCheckout(entry.workspaceId, entry.checkoutId);
    }
  };

  return (
    <PaletteShell
      label="Search"
      placeholder="Search agents, workspaces and commands"
      query={query}
      onQuery={setQuery}
      onKeyDown={navigation.onKeyDown}
      footer=""
    >
      {entries.length === 0 ? (
        <div className="px-md py-sm text-caption text-muted" data-palette-state="empty">
          No matching agents or workspaces
        </div>
      ) : (
        entries.map((entry, index) => (
          <PaletteRow
            key={entry.id}
            testId={entry.id}
            active={index === navigation.index}
            title={entry.title}
            subtitle={entry.unavailable ?? entry.kind}
            unavailable={Boolean(entry.unavailable)}
            onHover={() => navigation.setIndex(index)}
            onPick={() => activate(entry)}
          />
        ))
      )}
    </PaletteShell>
  );
}
