// The Explorer's rows, derived rather than stored. The core owns which folders
// are expanded (ui_state.expanded_paths) and hided answers one listing per
// folder, so a row is a pure function of those two plus the checkout's changed
// files. Nothing here touches the DOM, so the order, the expansion and the Git
// slot are all testable without a browser.

import { fileIcon, type FileIcon } from "./fileIcons";
import type { ChangedFileStatus, ChangesSnapshot, DeviceHost } from "./snapshot";
import type { DirectoryList } from "./store";
import type { MenuEntry } from "./components/entry-menu";
import { revealExternalEntry, type RevealHost } from "./revealExternal";
import type { TFunction } from "i18next";
import { translate } from "./i18n/client";
import type { MessageKey } from "./i18n/catalogs";

/** The one Git slot a row carries: a letter for a file, a dot for a folder. */
export type ExplorerDecoration = {
  badge: string;
  /** The catalog key of the badge's tooltip and accessible name. */
  title: MessageKey;
  status: ChangedFileStatus;
};

export type ExplorerRow = {
  path: string;
  name: string;
  depth: number;
  isDirectory: boolean;
  /** The entry's own inode, which a trash of the row sends so the host moves only this item; null when the listing carried none. */
  inode: string | null;
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

const TITLES = {
  modified: "history.status.modified",
  added: "history.status.added",
  deleted: "history.status.deleted",
  untracked: "history.status.untracked",
  renamed: "history.status.renamed",
  conflict: "history.status.conflict",
} as const satisfies Record<ChangedFileStatus, MessageKey>;

const CONTAINS = {
  modified: "explorer.contains.modified",
  added: "explorer.contains.added",
  deleted: "explorer.contains.deleted",
  untracked: "explorer.contains.untracked",
  renamed: "explorer.contains.renamed",
  conflict: "explorer.contains.conflict",
} as const satisfies Record<ChangedFileStatus, MessageKey>;

/** Index order is risk order, lowest first; a folder shows its riskiest
 * descendant. */
const RISK: ChangedFileStatus[] = ["conflict", "deleted", "renamed", "modified", "added", "untracked"];

/** The colour each status is drawn in. */
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
 * The mark in the Explorer header about the checkout's Git status, with the
 * sentence its tooltip reads: still loading, unavailable, or decorations kept
 * from an earlier read after the latest one failed, which are never shown as
 * current (PRD S5.5 B20, B22). Nothing when the status is current, and nothing
 * for a folder that is not a repository, which the operator has nothing to do
 * about (issue 570). It sits in the header row, so it never moves a row.
 */
export function explorerGitMark(changes: ChangesSnapshot | null, t: TFunction<"translation">): { state: "loading" | "unavailable" | "stale"; text: string } | null {
  if (!changes) return { state: "loading", text: t("explorer.gitLoading") };
  if (changes.not_a_repository) return null;
  if (changes.unavailable_reason) return { state: "unavailable", text: t("explorer.gitUnavailable", { reason: changes.unavailable_reason }) };
  if (changes.stale_reason) return { state: "stale", text: t("explorer.gitStale", { reason: changes.stale_reason }) };
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
    return { badge: "●", title: CONTAINS[status], status };
  }
  return { badge: LETTERS[status], title: TITLES[status], status };
}

/**
 * The rows the tree shows, flattened in the order the tree shows them:
 * the listing's own order, a folder's children directly under it, and
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

/**
 * The rows a filter leaves: each file hided's index matched, under its
 * ancestor folders, every folder open. Folders come before files, each in
 * name order, as a listing orders them; the core's expansion is not read, so
 * clearing the filter shows the tree as it was.
 */
export function filteredRows({
  rootPath,
  files,
  changes,
}: {
  rootPath: string;
  files: { relative_path: string }[];
  changes: ChangesSnapshot | null;
}): ExplorerRow[] {
  type Node = { name: string; path: string; children: Map<string, Node> | null };
  const top: Node = { name: "", path: rootPath, children: new Map() };
  for (const file of files) {
    const parts = file.relative_path.split("/");
    let node = top;
    parts.forEach((part, index) => {
      let next = node.children?.get(part);
      if (!next) {
        next = { name: part, path: `${node.path}/${part}`, children: index === parts.length - 1 ? null : new Map() };
        node.children?.set(part, next);
      }
      node = next;
    });
  }
  const decorations = gitDecorations(changes, rootPath);
  const rows: ExplorerRow[] = [];
  const walk = (node: Node, depth: number) => {
    const children = [...(node.children?.values() ?? [])].sort(
      (a, b) => Number(b.children !== null) - Number(a.children !== null) || a.name.localeCompare(b.name, undefined, { numeric: true }),
    );
    for (const child of children) {
      const isDirectory = child.children !== null;
      rows.push({
        path: child.path,
        name: child.name,
        depth,
        isDirectory,
        inode: null,
        expanded: isDirectory,
        listing: null,
        decoration: decorationFor(decorations, child.path, rootPath, isDirectory),
        icon: fileIcon(child.name),
      });
      if (isDirectory) walk(child, depth + 1);
    }
  };
  walk(top, 0);
  return rows;
}

/** The cell's tooltip. */
export function rowTitle(row: ExplorerRow, rootPath: string, t: TFunction<"translation">): string {
  const presented = relativeTo(row.path, rootPath);
  return row.decoration ? t("explorer.rowTooltip", { path: presented, status: t(row.decoration.title) }) : presented;
}

/** The cell's accessibility label. */
export function rowAccessibilityLabel(row: ExplorerRow, t: TFunction<"translation">): string {
  return row.decoration ? t("explorer.rowLabel", { name: row.name, status: t(row.decoration.title) }) : row.name;
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

export type ExplorerMenuId = "new-file" | "new-folder" | "open-beside" | "open-browser" | "reveal_external" | "rename" | "trash";

/**
 * The tree's context menu (docs/UI_BEHAVIOR.md, Explorer file management): a
 * folder row's two creations, or a file row's opens, then the OS file
 * manager's reveal where the host has one, then Rename and Move to Trash.
 * The empty area below the rows (`row` null) stands for the root and offers
 * only the two creations.
 */
export function explorerMenuItems({ isDirectory, row, html, besideReason, reveal, device, node }: {
  isDirectory: boolean;
  row: Pick<ExplorerRow, "path"> | null;
  /** The file is HTML, which a browser display can show (issue 155). */
  html: boolean;
  /** Why a second display cannot open beside the only View area, or null. */
  besideReason: string | null;
  reveal: RevealHost;
  device: string;
  /** The core's own node id: the OS file manager reveals only its files. */
  node: string;
}): MenuEntry<ExplorerMenuId>[] {
  const items: MenuEntry<ExplorerMenuId>[] = [];
  if (isDirectory) {
    items.push({ id: "new-file", label: translate("explorer.newFile"), unavailable: null });
    items.push({ id: "new-folder", label: translate("explorer.newFolder"), unavailable: null });
  } else if (row) {
    items.push({ id: "open-beside", label: translate("history.openBeside"), unavailable: besideReason });
    if (html) items.push({ id: "open-browser", label: translate("explorer.openBrowser"), unavailable: null });
  }
  if (!row) return items;
  const external = revealExternalEntry(reveal, device, node, translate, null, true);
  items.push(...external);
  items.push({ id: "rename", label: translate("common.rename"), unavailable: null, ...(external.length ? { separated: true } : {}) });
  items.push({ id: "trash", label: translate("explorer.moveToTrash"), unavailable: null, separated: true, destructive: true });
  return items;
}
