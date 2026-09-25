// The Explorer's rows, derived rather than stored. The core owns which folders
// are expanded (ui_state.expanded_paths) and hided answers one listing per
// folder, so a row is a pure function of those two plus the checkout's changed
// files. Nothing here touches the DOM, so the order, the expansion and the Git
// slot are all testable without a browser.

import { fileIcon, type FileIcon } from "./fileIcons";
import type { ChangedFileStatus, ChangesSnapshot, DeviceHost } from "./snapshot";
import type { DirectoryList } from "./store";

/** The one Git slot a row carries: a letter for a file, a dot for a folder. */
export type ExplorerDecoration = {
  badge: string;
  title: string;
  status: ChangedFileStatus;
};

export type ExplorerRow = {
  path: string;
  name: string;
  depth: number;
  isDirectory: boolean;
  /** The entry's own inode, which a trash of the row sends so the host moves only this item; null when the listing carried none. */
  inode: number | null;
  /** True while the operator has this folder expanded. */
  expanded: boolean;
  /** The children hided answered for this folder; null while none has arrived. */
  listing: DirectoryList | null;
  decoration: ExplorerDecoration | null;
  icon: FileIcon;
};

/** The single letter the row shows, which is how Git itself names these. */
const LETTERS: Record<ChangedFileStatus, string> = {
  modified: "M",
  added: "A",
  deleted: "D",
  untracked: "U",
  renamed: "R",
  conflict: "!",
};

const TITLES: Record<ChangedFileStatus, string> = {
  modified: "Modified",
  added: "Added",
  deleted: "Deleted",
  untracked: "Untracked",
  renamed: "Renamed",
  conflict: "Conflict",
};

/** Index order is risk order, lowest first; a folder shows its riskiest
 * descendant, which is the rule the Swift outline's decorations apply. */
const RISK: ChangedFileStatus[] = ["conflict", "deleted", "renamed", "modified", "added", "untracked"];

/** The colour the Swift outline gives each status (WorkspaceOutlineView). */
export function gitBadgeColor(status: ChangedFileStatus): string {
  switch (status) {
    case "added":
    case "untracked":
      return "text-success";
    case "deleted":
    case "conflict":
      return "text-destructive";
    case "modified":
      return "text-warning";
    case "renamed":
      return "text-primary";
  }
}

/** The path relative to the tree's root; the root itself is ".", and a path
 * outside the root is returned whole rather than guessed at. */
export function relativeTo(path: string, root: string): string {
  if (path === root) return ".";
  const prefix = root.endsWith("/") ? root : root + "/";
  return path.startsWith(prefix) ? path.slice(prefix.length) : path;
}

export type GitDecorations = {
  files: Map<string, ChangedFileStatus>;
  directories: Map<string, ChangedFileStatus>;
};

function higherRisk(current: ChangedFileStatus | undefined, candidate: ChangedFileStatus): ChangedFileStatus {
  if (!current) return candidate;
  return RISK.indexOf(candidate) < RISK.indexOf(current) ? candidate : current;
}

/**
 * The line above the Explorer tree about the checkout's Git status: still
 * loading, unavailable, or decorations kept from an earlier read after the
 * latest one failed, which are never shown as current (PRD S5.5 B20, B22).
 * Nothing when the status is current.
 */
export function explorerGitLine(changes: ChangesSnapshot | null): { state: "loading" | "unavailable" | "stale"; text: string } | null {
  if (!changes) return { state: "loading", text: "Loading Git status" };
  if (changes.unavailable_reason) return { state: "unavailable", text: `Git status unavailable: ${changes.unavailable_reason}` };
  if (changes.stale_reason) return { state: "stale", text: `Git status may be out of date: ${changes.stale_reason}` };
  return null;
}

/**
 * Whether a device's files wait on a Settings decision rather than on the
 * device: without the helper consent, or after its identity changed, only
 * Settings > Devices can change the answer, so retrying the listing cannot.
 */
export function helperNeedsSettings(host: DeviceHost | null | undefined): boolean {
  return host?.state === "not_allowed" || host?.state === "identity_changed";
}

/**
 * The root-scoped Git slot of every changed path, derived once per changes
 * section rather than per row: a folder's badge is the riskiest status under
 * it, so every ancestor of every changed file is marked.
 */
export function gitDecorations(changes: ChangesSnapshot | null, rootPath: string | null): GitDecorations {
  const files = new Map<string, ChangedFileStatus>();
  const directories = new Map<string, ChangedFileStatus>();
  if (!changes || !rootPath || !changes.root_path ||
      (changes.root_path !== rootPath && !changes.root_path.startsWith(`${rootPath}/`))) return { files, directories };
  for (const entry of changes.entries) {
    if (!entry.path.startsWith(`${changes.root_path}/`)) continue;
    const relative = relativeTo(entry.path, rootPath);
    files.set(relative, higherRisk(files.get(relative), entry.status));
    const parts = relative.split("/");
    for (let depth = 1; depth < parts.length; depth += 1) {
      const parent = parts.slice(0, depth).join("/");
      directories.set(parent, higherRisk(directories.get(parent), entry.status));
    }
    directories.set(".", higherRisk(directories.get("."), entry.status));
  }
  return { files, directories };
}

export function decorationFor(
  decorations: GitDecorations,
  path: string,
  rootPath: string,
  isDirectory: boolean,
): ExplorerDecoration | null {
  const relative = relativeTo(path, rootPath);
  const status = isDirectory ? decorations.directories.get(relative) : decorations.files.get(relative);
  if (!status) return null;
  if (isDirectory) {
    return {
      badge: "●",
      title: `Contains changed files; highest priority is ${TITLES[status].toLowerCase()}`,
      status,
    };
  }
  return { badge: LETTERS[status], title: TITLES[status], status };
}

/**
 * The rows the tree shows, flattened in the order the Swift outline shows
 * them: the listing's own order, a folder's children directly under it, and
 * nothing under a folder whose listing has not arrived yet.
 */
export function explorerRows({
  rootPath,
  listings,
  expandedPaths,
  changes,
}: {
  rootPath: string;
  listings: Record<string, DirectoryList>;
  expandedPaths: string[];
  changes: ChangesSnapshot | null;
}): ExplorerRow[] {
  const decorations = gitDecorations(changes, rootPath);
  const expanded = new Set(expandedPaths);
  const rows: ExplorerRow[] = [];
  const walk = (folder: string, depth: number) => {
    const listing = listings[folder];
    if (!listing) return;
    for (const entry of listing.entries) {
      const open = entry.is_directory && expanded.has(entry.path);
      rows.push({
        path: entry.path,
        name: entry.name,
        depth,
        isDirectory: entry.is_directory,
        inode: entry.inode ?? null,
        expanded: open,
        listing: entry.is_directory ? (listings[entry.path] ?? null) : null,
        decoration: decorationFor(decorations, entry.path, rootPath, entry.is_directory),
        icon: fileIcon(entry.name),
      });
      if (open) walk(entry.path, depth + 1);
    }
  };
  walk(rootPath, 0);
  return rows;
}

/** The cell's tooltip, as the Swift outline sets it. */
export function rowTitle(row: ExplorerRow, rootPath: string): string {
  const presented = relativeTo(row.path, rootPath);
  return row.decoration ? `${presented} · ${row.decoration.title}` : presented;
}

/** The cell's accessibility label, as the Swift outline sets it. */
export function rowAccessibilityLabel(row: ExplorerRow): string {
  return row.decoration ? `${row.name}, ${row.decoration.title}` : row.name;
}

/** The disclosure mark a folder row draws; the sidebar already uses these. */
export function disclosureMark(row: ExplorerRow): string {
  if (!row.isDirectory) return "";
  return row.expanded ? "▾" : "▸";
}

/** The folder a path sits in, as written; a top-level path returns itself. */
export function parentPath(path: string): string {
  const slash = path.lastIndexOf("/");
  return slash <= 0 ? path : path.slice(0, slash);
}

/** The row a path names, or null when the tree does not show it. */
export function rowForPath(rows: ExplorerRow[], path: string | null): ExplorerRow | null {
  if (!path) return null;
  return rows.find((row) => row.path === path) ?? null;
}

/**
 * The row `delta` places from `path`, clamped to the tree's ends; a path the
 * tree no longer shows starts at the first row, so a collapsed or vanished
 * selection never leaves the cursor off the list.
 */
export function moveSelection(rows: ExplorerRow[], path: string | null, delta: number): string | null {
  if (rows.length === 0) return null;
  const index = rows.findIndex((row) => row.path === path);
  const from = index === -1 ? (delta > 0 ? -1 : 0) : index;
  const next = Math.min(Math.max(from + delta, 0), rows.length - 1);
  return rows[next]?.path ?? null;
}

/**
 * The row one level up from `path`: its parent folder's row, or the nearest
 * preceding row with a smaller depth when the parent is not shown (a folder
 * whose row was filtered out). Null at the top.
 */
export function parentSelection(rows: ExplorerRow[], path: string | null): string | null {
  const index = rows.findIndex((row) => row.path === path);
  const current = index === -1 ? undefined : rows[index];
  if (!current) return null;
  for (let i = index - 1; i >= 0; i -= 1) {
    const candidate = rows[i];
    if (candidate && candidate.depth < current.depth) return candidate.path;
  }
  return null;
}

/** The first child row of an expanded folder, or null when it has none shown. */
export function firstChildSelection(rows: ExplorerRow[], path: string | null): string | null {
  const index = rows.findIndex((row) => row.path === path);
  const parent = index === -1 ? undefined : rows[index];
  if (!parent) return null;
  const child = rows[index + 1];
  return child && child.depth === parent.depth + 1 ? child.path : null;
}

/**
 * The row the tree selects once `path` is gone: its next sibling, else its
 * previous sibling, else its parent, else the root. This is the order the core
 * documents for `path_trash`'s `select_after`, and the tree decides it because
 * only the tree knows its own row order.
 */
export function selectionAfterRemoval(rows: ExplorerRow[], path: string, rootPath: string): string {
  const index = rows.findIndex((row) => row.path === path);
  const target = index === -1 ? undefined : rows[index];
  if (!target) return rootPath;
  const depth = target.depth;
  for (let i = index + 1; i < rows.length; i += 1) {
    const candidate = rows[i];
    if (!candidate || candidate.depth < depth) break;
    if (candidate.depth === depth) return candidate.path;
  }
  for (let i = index - 1; i >= 0; i -= 1) {
    const candidate = rows[i];
    if (!candidate || candidate.depth < depth) break;
    if (candidate.depth === depth) return candidate.path;
  }
  return parentSelection(rows, path) ?? rootPath;
}
