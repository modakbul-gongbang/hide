import path from "node:path";

/** Longer than any macOS path (`PATH_MAX` is 1024); a longer value is not a path the shell sent. */
const MAX_PATH_LENGTH = 4096;

/**
 * The folder a shell's Reveal in Finder names, or null when the value is not
 * an absolute path: a relative one would resolve against the host's own
 * working directory, which the shell knows nothing of.
 */
export function revealablePath(value: unknown): string | null {
  if (typeof value !== "string" || value.length === 0 || value.length > MAX_PATH_LENGTH) return null;
  if (value.includes("\0") || !path.isAbsolute(value)) return null;
  return path.normalize(value);
}
