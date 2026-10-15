// The mark a file or folder row draws: the Material Icon Theme, the file icon
// theme most VS Code users run (MIT, notice at
// desktop/resources/THIRD_PARTY_NOTICES/material-icon-theme-MIT.txt), resolved
// from the name the way VS Code applies that theme's manifest. The package is a
// pinned dependency read at build time: its manifest is bundled, and each SVG
// is its own asset, fetched only when a row draws it.
import {
  file,
  fileExtensions,
  fileNames,
  folder,
  folderExpanded,
  folderNames,
  folderNamesExpanded,
  iconDefinitions,
  light,
} from "material-icon-theme/dist/material-icons.json";

export type FileIcon = {
  /** The picture the dark theme draws. */
  url: string;
  /** The picture the light theme draws: the theme's own light variant where
   * it has one (a mark that would vanish on a light background), else `url`. */
  lightUrl: string;
};

/** The manifest's lookup tables; the light overlay carries only the names
 * whose mark changes on a light background. */
type Table = {
  fileNames?: Record<string, string>;
  fileExtensions?: Record<string, string>;
  folderNames?: Record<string, string>;
  folderNamesExpanded?: Record<string, string>;
};

const BASE: Table = { fileNames, fileExtensions, folderNames, folderNamesExpanded };
const DARK: Table[] = [BASE];
// A light entry outranks a base entry of the same kind and nothing more: a
// base file name still beats a light extension, as VS Code's rules rank them.
const LIGHT: Table[] = [light, BASE];

const ASSETS = import.meta.glob<string>("../node_modules/material-icon-theme/icons/*.svg", {
  query: "?no-inline",
  import: "default",
  eager: true,
});

/** The asset an icon id names. An id the manifest points at is always shipped
 * in the same package, so a miss is a broken dependency, not a missing mark. */
function assetUrl(id: string): string {
  const path = iconDefinitions[id]?.iconPath;
  const url = path ? ASSETS[`../node_modules/material-icon-theme/icons/${path.slice(path.lastIndexOf("/") + 1)}`] : undefined;
  if (!url) throw new Error(`material-icon-theme has no picture for icon id ${id}`);
  return url;
}

function first(tables: Table[], kind: keyof Table, key: string): string | undefined {
  for (const table of tables) {
    const id = table[kind]?.[key];
    if (id) return id;
  }
  return undefined;
}

/** A whole name first, then every extension the name carries, longest first:
 * `app.test.tsx` is `test.tsx` before `tsx`, and `.bashrc` is `bashrc`. */
function fileId(lowercased: string, tables: Table[]): string {
  const named = first(tables, "fileNames", lowercased);
  if (named) return named;
  for (let dot = lowercased.indexOf("."); dot !== -1; dot = lowercased.indexOf(".", dot + 1)) {
    const id = first(tables, "fileExtensions", lowercased.slice(dot + 1));
    if (id) return id;
  }
  return file;
}

function folderId(lowercased: string, expanded: boolean, tables: Table[]): string {
  return first(tables, expanded ? "folderNamesExpanded" : "folderNames", lowercased) ?? (expanded ? folderExpanded : folder);
}

/** The last name of a path; paths between machines always separate with `/`. */
function nameOf(path: string): string {
  return path.slice(path.lastIndexOf("/") + 1).toLowerCase();
}

export function fileIcon(path: string): FileIcon {
  const name = nameOf(path);
  return { url: assetUrl(fileId(name, DARK)), lightUrl: assetUrl(fileId(name, LIGHT)) };
}

export function folderIcon(path: string, expanded: boolean): FileIcon {
  const name = nameOf(path);
  return { url: assetUrl(folderId(name, expanded, DARK)), lightUrl: assetUrl(folderId(name, expanded, LIGHT)) };
}
