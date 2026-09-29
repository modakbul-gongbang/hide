// Which Herdr binary a daemon started from this host attaches pane terminals
// with, decided once from the host's environment.
//
// A packaged app ships its pinned Herdr in `Contents/Resources/herdr` and
// hands it to every `hide` child as HERDR_BIN_PATH. An inherited
// HERDR_BIN_PATH is honored only as an explicit override: Herdr exports its
// own server's path into every pane together with HERDR_PANE_ID, and that
// path dies when the bundle that started the server is replaced, so a
// packaged app opened from a pane would hand `hide connect` a value it
// refuses on every attempt (issue 214). A value that arrives with
// HERDR_PANE_ID is therefore Herdr's, not the operator's, and the bundled
// binary replaces it. An unpackaged host has no bundled binary and adds
// nothing.

import path from "node:path";
import type { ChildResult } from "./spawn";

export type HerdrChoice = {
  /** The HERDR_BIN_PATH every `hide` child gets, or null to leave the inherited environment as it is. */
  path: string | null;
  source: "bundled" | "inherited";
  /** The pane-exported value the bundled binary replaced, kept for the log. */
  replacedPaneValue: string | null;
};

export function chooseHerdr(input: { bundledDir: string | null; herdrBinPath: string | null; herdrPaneId: string | null }): HerdrChoice {
  const { bundledDir, herdrBinPath, herdrPaneId } = input;
  if (bundledDir === null) return { path: null, source: "inherited", replacedPaneValue: null };
  const bundled = path.join(bundledDir, "herdr");
  if (herdrBinPath === null) return { path: bundled, source: "bundled", replacedPaneValue: null };
  if (herdrPaneId !== null) return { path: bundled, source: "bundled", replacedPaneValue: herdrBinPath === bundled ? null : herdrBinPath };
  return { path: null, source: "inherited", replacedPaneValue: null };
}

/**
 * What `herdr status server --json` said about the server on the socket a
 * child's environment names. Only the `running` answer is read: a socket file
 * is no evidence, because a server that died with the machine leaves one.
 */
export type ServerStatus = { running: boolean } | { unreadable: string };

export function parseServerStatus(result: ChildResult): ServerStatus {
  if (result.spawnError) return { unreadable: `herdr could not start: ${result.spawnError}` };
  if (result.timedOut) return { unreadable: "herdr status timed out" };
  if (result.code !== 0) return { unreadable: `herdr status exited ${result.code ?? result.signal ?? "without a code"}` };
  try {
    const parsed: unknown = JSON.parse(result.stdout);
    if (parsed && typeof parsed === "object" && "running" in parsed && typeof parsed.running === "boolean") return { running: parsed.running };
  } catch {
    // Reported below with the same reason as a missing field.
  }
  return { unreadable: "herdr status answered without a running field" };
}

/**
 * The environment a Herdr server this host starts runs with, and every pane
 * shell it starts inherits. An app opened from Finder carries launchd's
 * environment, which has no locale, while a terminal that runs `herdr` sets
 * one; a pane without it cannot edit multibyte text. The character type is
 * set to UTF-8 only when no locale variable names one, so a message language
 * or number format the operator chose is never replaced.
 */
export function serverEnvironment(child: Record<string, string | undefined>): Record<string, string | undefined> {
  if (child.LANG || child.LC_ALL || child.LC_CTYPE) return child;
  return { ...child, LC_CTYPE: "UTF-8" };
}

export type ServerStart =
  | { outcome: "running" }
  | { outcome: "status_failed"; detail: string }
  | { outcome: "started"; pid: number; elapsedMs: number }
  | { outcome: "start_failed"; detail: string; pid?: number; elapsedMs?: number };

/**
 * Starts a server only when Herdr says none is running, then waits a bounded
 * time for it to answer. A status Herdr cannot give starts nothing: a server
 * that may be running is never doubled.
 */
export async function ensureServer(io: {
  status: () => Promise<ServerStatus>;
  start: () => Promise<{ pid: number } | { spawnError: string }>;
  sleep: (ms: number) => Promise<void>;
  now: () => number;
  stopped: () => boolean;
  waitMs: number;
  pollMs: number;
}): Promise<ServerStart> {
  const before = await io.status();
  if ("unreadable" in before) return { outcome: "status_failed", detail: before.unreadable };
  if (before.running) return { outcome: "running" };
  const started = io.now();
  const spawned = await io.start();
  if ("spawnError" in spawned) return { outcome: "start_failed", detail: spawned.spawnError };
  while (io.now() - started < io.waitMs && !io.stopped()) {
    await io.sleep(io.pollMs);
    const now = await io.status();
    if ("running" in now && now.running) return { outcome: "started", pid: spawned.pid, elapsedMs: io.now() - started };
  }
  return { outcome: "start_failed", detail: "no answer", pid: spawned.pid, elapsedMs: io.now() - started };
}
