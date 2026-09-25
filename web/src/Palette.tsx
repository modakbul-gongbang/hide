import type { ReactNode } from "react";
import { useEffect, useRef, useState } from "react";
import type { Actions } from "./actions";
import { Command, CommandDialog, CommandInput, CommandItem, CommandList } from "./components/ui/command";
import { fileIcon } from "./fileIcons";
import { filterEntries, searchEntries, type SearchEntry } from "./search";
import { explorerContext } from "./snapshot";
import { useShellStore } from "./store";
import { useUiStore } from "./ui";
import { drawnViews } from "./viewFocus";

// The two palettes (PRD B12, B13) on the System command palette: a query
// field, a list cmdk's own arrow keys and Enter walk, and Escape closes
// through the shell's one layer owner. ⌘P lists what hided's index ranked
// for the typed query; ⌘K filters the snapshot's agents, projects and
// checkouts in the web itself, because the data is already here. "Open file
// to the side" is ⌘P's list whose pick opens beside (S7 B4). The screens
// already rank and filter their own entries, so `shouldFilter` stays off and
// cmdk is used only for the list's selection and keyboard behavior.

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
}: {
  label: string;
  placeholder: string;
  query: string;
  onQuery: (query: string) => void;
  footer: string;
  children: ReactNode;
}) {
  const close = useUiStore((s) => s.closeOverlay);
  return (
    <CommandDialog open title={label} description={placeholder} onOpenChange={(open) => { if (!open) close(); }}>
      <Command shouldFilter={false} loop label={label} data-palette={label}>
        <CommandInput value={query} placeholder={placeholder} data-palette-input="true" onValueChange={onQuery} />
        <CommandList data-palette-list="true">{children}</CommandList>
        {footer ? (
          <div className="border-t border-border px-md py-xxs text-caption text-muted-foreground" data-palette-footer="true">
            {footer}
          </div>
        ) : null}
      </Command>
    </CommandDialog>
  );
}

function PaletteRow({
  icon,
  title,
  subtitle,
  unavailable = false,
}: {
  icon?: ReactNode;
  title: string;
  subtitle?: string;
  /** A command that cannot run now: drawn muted with its whole reason under the title, and picking it does nothing (B9). */
  unavailable?: boolean;
}) {
  if (unavailable) {
    return (
      <>
        {icon}
        <span className="flex min-w-0 flex-1 flex-col">
          <span className="truncate">{title}</span>
          {subtitle ? <span className="text-caption text-muted-foreground">{subtitle}</span> : null}
        </span>
      </>
    );
  }
  return (
    <>
      {icon}
      <span className="min-w-0 flex-1 truncate">{title}</span>
      {subtitle ? <span className="max-w-[var(--size-recent-location-max)] shrink-0 truncate text-caption text-muted-foreground">{subtitle}</span> : null}
    </>
  );
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

  return (
    <PaletteShell
      label={beside ? "Open file to the side" : "Open file"}
      placeholder={beside ? "Search files to open beside the active view" : "Search files by name"}
      query={query}
      onQuery={setQuery}
      footer={fileIndex?.truncated ? "The index is truncated at 50,000 files" : ""}
    >
      {fileIndex?.unavailable ? (
        <div className="px-md py-sm text-caption text-muted-foreground" role="alert" data-palette-state="unavailable">
          {`Files could not be listed: ${fileIndex.unavailable}`}
        </div>
      ) : fileIndex?.indexing && entries.length === 0 ? (
        <div className="px-md py-sm text-caption text-muted-foreground" data-palette-state="indexing">
          Indexing…
        </div>
      ) : entries.length === 0 ? (
        <div className="px-md py-sm text-caption text-muted-foreground" data-palette-state="empty">
          {query ? "No matching files" : "Type to search this checkout"}
        </div>
      ) : (
        entries.map((entry) => (
          <CommandItem key={entry.path} asChild value={entry.path} onSelect={() => open(entry.path)}>
            <button type="button" data-palette-row={entry.path} className="w-full text-left">
              <PaletteRow
                icon={
                  <span className={`shrink-0 ${fileIcon(entry.relative_path).color}`} style={{ fontFamily: "seti" }} aria-hidden="true">
                    {fileIcon(entry.relative_path).glyph}
                  </span>
                }
                title={entry.relative_path}
              />
            </button>
          </CommandItem>
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
    <PaletteShell label="Search" placeholder="Search agents, workspaces and commands" query={query} onQuery={setQuery} footer="">
      {entries.length === 0 ? (
        <div className="px-md py-sm text-caption text-muted-foreground" data-palette-state="empty">
          No matching agents or workspaces
        </div>
      ) : (
        entries.map((entry) => (
          <CommandItem key={entry.id} asChild value={entry.id} disabled={Boolean(entry.unavailable)} onSelect={() => activate(entry)}>
            <button type="button" data-palette-row={entry.id} className="w-full text-left">
              <PaletteRow title={entry.title} subtitle={entry.unavailable ?? entry.kind} unavailable={Boolean(entry.unavailable)} />
            </button>
          </CommandItem>
        ))
      )}
    </PaletteShell>
  );
}
