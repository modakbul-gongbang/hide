import fs from "node:fs";
import { fromPage } from "./wirePath";

/**
 * What the OS file manager is handed for a menu's `reveal_external`
 * (issue 324): the page's absolute path in this system's own spelling, when
 * it exists now, with its kind for the log, or why nothing is handed over. A
 * file removed while its menu was open is refused here rather than left to a
 * file manager that would show nothing.
 */
export async function revealTarget(value: unknown): Promise<{ path: string; kind: "file" | "directory" } | { refused: "path" | "missing" }> {
  const target = fromPage(value);
  if (target === null) return { refused: "path" };
  const stat = await fs.promises.stat(target).catch(() => null);
  if (!stat) return { refused: "missing" };
  return { path: target, kind: stat.isDirectory() ? "directory" : "file" };
}
