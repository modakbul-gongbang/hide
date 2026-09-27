// The packaged app owns the hcoord runtime it ships. It writes the stable
// user-facing shim atomically, then asks that exact runtime to converge its
// LaunchAgent. The daemon owns itself after launch; the desktop child is
// short-lived and remains inside DesktopHost's one-child queue.

import fs from "node:fs";
import path from "node:path";
import type { ChildResult } from "./spawn";

export type HcoordBundle = { executable: string; cli: string; herdr: string };

const shellQuote = (value: string): string => `'${value.replaceAll("'", `'\\''`)}'`;

export function bundledHcoord(resources: string, executable: string): HcoordBundle {
  return {
    executable,
    cli: path.join(resources, "hcoord", "dist", "hcoord", "cli.js"),
    herdr: path.join(resources, "herdr"),
  };
}

export function hcoordEnvironment(
  inherited: Record<string, string | undefined>,
  home: string,
  bundle: HcoordBundle,
): Record<string, string | undefined> {
  return {
    ...inherited,
    HOME: home,
    ELECTRON_RUN_AS_NODE: "1",
    HERDR_BIN_PATH: bundle.herdr,
  };
}

/** The remote protocol's fixed command path, updated without a missing-file window. */
export function installHcoordShim(home: string, bundle: HcoordBundle): string {
  const directory = path.join(home, ".hcoord", "bin");
  const target = path.join(directory, "hcoord");
  const temporary = path.join(directory, `.hcoord.${process.pid}.tmp`);
  fs.mkdirSync(directory, { recursive: true, mode: 0o700 });
  const script = `#!/bin/sh\nELECTRON_RUN_AS_NODE=1 HERDR_BIN_PATH=${shellQuote(bundle.herdr)} exec ${shellQuote(bundle.executable)} ${shellQuote(bundle.cli)} "$@"\n`;
  fs.writeFileSync(temporary, script, { mode: 0o700 });
  fs.renameSync(temporary, target);
  return target;
}

export function parseHcoordEnsure(result: ChildResult):
  | { ok: true; changed: boolean; manualStop: boolean; version: string | null }
  | { ok: false; reason: string } {
  if (result.timedOut) return { ok: false, reason: "hcoord ensure timed out" };
  if (result.spawnError) return { ok: false, reason: `hcoord runtime could not start: ${result.spawnError}` };
  let parsed: { ok?: unknown; value?: Record<string, unknown>; error?: { code?: unknown; message?: unknown } };
  try { parsed = JSON.parse(result.stdout.trim().split("\n").at(-1) ?? ""); }
  catch { return { ok: false, reason: `hcoord ensure returned no JSON (exit ${result.code ?? "signal"})` }; }
  if (parsed.ok !== true) {
    const code = typeof parsed.error?.code === "string" ? parsed.error.code : "failed";
    const message = typeof parsed.error?.message === "string" ? parsed.error.message : "no reason";
    return { ok: false, reason: `${code}: ${message}` };
  }
  const value = parsed.value ?? {};
  return {
    ok: true,
    changed: value.changed === true,
    manualStop: value.manualStop === true,
    version: typeof value.hcoordVersion === "string" ? value.hcoordVersion : null,
  };
}
