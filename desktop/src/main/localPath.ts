// Paths a terminal link names on this Mac (docs/ARCHITECTURE.md, A clicked
// path is one event): whether they exist, and how macOS is handed one.
//
// The shell cannot read the filesystem, and hided answers only for paths
// inside `$HOME` or a registered checkout, because a page reached through a
// forwarded port must not learn what exists elsewhere. This process is the
// desktop app's own and answers only its window's page, so it is the one
// place a link a program printed (`/tmp/out.png`) can be checked and opened.
// Link detection is a guess over arbitrary output, and what a type's default
// application does differs from Mac to Mac (a `.pl` may belong to a terminal
// that runs it), so only a type known to be a plain document is opened, and
// anything else is revealed in Finder, where opening it is the operator's act.

import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";

/** Paths one probe may name: the tokens around one hovered row, with room to spare. */
export const MAX_PROBE_PATHS = 64;
/** Longer than any macOS path (`PATH_MAX` is 1024); a longer value is not a path the shell sent. */
const MAX_PATH_LENGTH = 4096;

/**
 * The file types macOS is asked to open: documents whose usual applications
 * show them and run nothing. HTML and SVG are left out because a browser runs
 * their scripts, CSV and SQL because a spreadsheet or a database client may
 * evaluate them, and interpreted sources (`.py`, `.sh`, `.js`) because some
 * Macs open them in something that runs them.
 */
const DOCUMENTS = new Set([
  ...["txt", "text", "md", "markdown", "rst", "log", "json", "jsonl", "yaml", "yml", "toml", "ini", "diff", "patch", "rtf"],
  ...["c", "h", "cc", "cpp", "hpp", "m", "mm", "swift", "rs", "go", "java", "kt", "cs", "ts", "tsx", "css", "scss", "proto", "graphql"],
  ...["pdf", "png", "jpg", "jpeg", "gif", "webp", "heic", "heif", "tif", "tiff", "bmp", "ico", "avif"],
  ...["mp3", "m4a", "aac", "wav", "aif", "aiff", "flac", "caf", "mp4", "m4v", "mov", "webm", "mkv", "avi"],
]);

/** What a probed path is: its physical path (symlinks and `..` resolved) and whether it is a folder. */
export type Probed = { real: string; kind: "file" | "directory" } | null;

function pathValue(value: unknown): string | null {
  if (typeof value !== "string" || value.length === 0 || value.length > MAX_PATH_LENGTH || value.includes("\0")) return null;
  if (value.startsWith("~/") || path.isAbsolute(value)) return value;
  return null;
}

/**
 * The paths a probe names, or null when the request is not one: an array of
 * at most `MAX_PROBE_PATHS` absolute or `~/` paths. A relative path would
 * resolve against this process's own folder, which the shell knows nothing of.
 */
export function probeRequest(value: unknown): string[] | null {
  if (!Array.isArray(value) || value.length > MAX_PROBE_PATHS) return null;
  const paths = value.map(pathValue);
  return paths.every((each): each is string => each !== null) ? paths : null;
}

function expand(value: string, home: string): string {
  return path.normalize(value.startsWith("~/") ? path.join(home, value.slice(2)) : value);
}

/** Each path's physical spelling and kind, or null for one that is missing, unreadable, or neither a file nor a folder. */
export async function probe(paths: string[], home: string = os.homedir()): Promise<Probed[]> {
  return Promise.all(
    paths.map(async (each) => {
      try {
        const real = await fs.realpath(expand(each, home));
        const stat = await fs.stat(real);
        if (stat.isDirectory()) return { real, kind: "directory" as const };
        if (stat.isFile()) return { real, kind: "file" as const };
        return null;
      } catch {
        return null;
      }
    }),
  );
}

/**
 * How macOS is handed a path: `open` in its default application (a folder as
 * a Finder window), `reveal` in Finder, or `refuse` for a path that is gone or
 * is not its own physical spelling any more.
 */
export type OpenRoute =
  | { action: "open" | "reveal"; kind: "file" | "directory"; reason: "document" | "folder" | "type" | "bundle" | "execute_bit" | "header" }
  | { action: "refuse"; reason: "not_found" | "not_physical" };

/**
 * Whether the file's first bytes say it is a program: Mach-O (thin or fat,
 * either byte order), ELF, a DOS/PE `MZ`, or a `#!` script. A document that
 * merely starts with those bytes is revealed rather than opened, which is the
 * safe way to be wrong.
 */
export function executableHeader(head: Uint8Array): boolean {
  if (head.length >= 2 && ((head[0] === 0x23 && head[1] === 0x21) || (head[0] === 0x4d && head[1] === 0x5a))) return true;
  if (head.length < 4) return false;
  const magic = ((head[0]! << 24) | (head[1]! << 16) | (head[2]! << 8) | head[3]!) >>> 0;
  return [0xfeedface, 0xfeedfacf, 0xcefaedfe, 0xcffaedfe, 0xcafebabe, 0xbebafeca, 0xcafebabf, 0xbfbafeca, 0x7f454c46].includes(magic);
}

/**
 * The route for `target`, judged on the file itself. The shell sends the
 * physical path it probed; a path that resolves elsewhere now was swapped for
 * a link since, and opening it would open whatever the link names.
 */
export async function openRoute(target: string): Promise<OpenRoute> {
  let stat;
  try {
    if ((await fs.realpath(target)) !== target) return { action: "refuse", reason: "not_physical" };
    stat = await fs.lstat(target);
  } catch {
    return { action: "refuse", reason: "not_found" };
  }
  const extension = path.extname(target).slice(1).toLowerCase();
  if (stat.isDirectory()) {
    // A folder with an extension may be a bundle its handler launches or installs.
    return extension ? { action: "reveal", kind: "directory", reason: "bundle" } : { action: "open", kind: "directory", reason: "folder" };
  }
  if (!stat.isFile()) return { action: "refuse", reason: "not_found" };
  if (!DOCUMENTS.has(extension)) return { action: "reveal", kind: "file", reason: "type" };
  if (stat.mode & 0o111) return { action: "reveal", kind: "file", reason: "execute_bit" };
  let head = new Uint8Array(0);
  try {
    const file = await fs.open(target, "r");
    try {
      const buffer = new Uint8Array(4);
      const { bytesRead } = await file.read(buffer, 0, 4, 0);
      head = buffer.subarray(0, bytesRead);
    } finally {
      await file.close();
    }
  } catch {
    // An unreadable file is revealed: its handler would fail to read it too.
    return { action: "reveal", kind: "file", reason: "header" };
  }
  if (executableHeader(head)) return { action: "reveal", kind: "file", reason: "header" };
  return { action: "open", kind: "file", reason: "document" };
}
