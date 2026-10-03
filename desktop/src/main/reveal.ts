import fs from "node:fs";
import path from "node:path";

/** Longer than any macOS path (`PATH_MAX` is 1024); a longer value is not a path the shell sent. */
const MAX_PATH_LENGTH = 4096;

/**
 * The path a shell's reveal or open names, or null when the value is not
 * an absolute path: a relative one would resolve against the host's own
 * working directory, which the shell knows nothing of.
 */
export function revealablePath(value: unknown): string | null {
  if (typeof value !== "string" || value.length === 0 || value.length > MAX_PATH_LENGTH) return null;
  if (value.includes("\0") || !path.isAbsolute(value)) return null;
  return path.normalize(value);
}

/**
 * What the OS file manager is handed for a menu's `reveal_external`
 * (issue 324): an absolute path that exists now, with its kind for the log,
 * or why nothing is handed over. A file removed while its menu was open is
 * refused here rather than left to a file manager that would show nothing.
 */
export async function revealTarget(value: unknown): Promise<{ path: string; kind: "file" | "directory" } | { refused: "path" | "missing" }> {
  const target = revealablePath(value);
  if (target === null) return { refused: "path" };
  const stat = await fs.promises.stat(target).catch(() => null);
  if (!stat) return { refused: "missing" };
  return { path: target, kind: stat.isDirectory() ? "directory" : "file" };
}
