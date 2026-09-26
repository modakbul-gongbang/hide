// Finding the `hide` CLI and reading its answers. The host meets the daemon
// only through `hide connect` (reuse or start, desktop PRD D-03) and
// `hide status --json` (attach-only), whose JSON contract `hided/src/cli.rs`
// owns.

import path from "node:path";
import type { ChildResult } from "./spawn";

export type CliSource = "env" | "worktree" | "path";

export type ResolvedCli = {
  found: { path: string; source: CliSource } | null;
  /** Every path looked at, in order, for the log (B2). */
  tried: string[];
};

export type FileProbe = {
  isExecutable(file: string): boolean;
  mtimeMs(file: string): number | null;
};

/**
 * B2's order: the override, then (unpackaged) this worktree's newest build,
 * then PATH. An override that is set but unusable ends the search: it names
 * what the operator asked for, and quietly using another binary would hide
 * the mistake.
 */
export function resolveCli(
  input: { override: string | null; worktreeRoot: string | null; searchPath: string },
  probe: FileProbe,
): ResolvedCli {
  const tried: string[] = [];
  if (input.override) {
    tried.push(input.override);
    return { found: probe.isExecutable(input.override) ? { path: input.override, source: "env" } : null, tried };
  }
  if (input.worktreeRoot) {
    const builds = ["debug", "release"].map((profile) => path.join(input.worktreeRoot as string, "target", profile, "hide"));
    tried.push(...builds);
    const newest = builds
      .filter((file) => probe.isExecutable(file))
      .map((file) => ({ file, mtime: probe.mtimeMs(file) ?? 0 }))
      .sort((a, b) => b.mtime - a.mtime)[0];
    if (newest) return { found: { path: newest.file, source: "worktree" }, tried };
  }
  for (const dir of input.searchPath.split(":")) {
    if (!dir || !path.isAbsolute(dir)) continue;
    const file = path.join(dir, "hide");
    tried.push(file);
    if (probe.isExecutable(file)) return { found: { path: file, source: "path" }, tried };
  }
  return { found: null, tried };
}

export type FailureReason = "cli_missing" | "start_failed" | "no_response";

export type Attached = { url: string; origin: string; port: number; pid: number };

export type ConnectAnswer =
  | ({ kind: "attached" } & Attached)
  | { kind: "failed"; reason: Exclude<FailureReason, "cli_missing">; detail: string };

export type StatusAnswer = ({ running: true } & Attached) | { running: false };

function lastJsonLine(stdout: string): Record<string, unknown> | null {
  const line = stdout
    .split("\n")
    .map((row) => row.trim())
    .filter(Boolean)
    .at(-1);
  if (!line) return null;
  try {
    const value: unknown = JSON.parse(line);
    return value && typeof value === "object" ? (value as Record<string, unknown>) : null;
  } catch {
    return null;
  }
}

/** A daemon URL is loopback HTTP with the token in its hash, or it is not one. */
function attached(value: Record<string, unknown>): Attached | null {
  const { url, port, pid } = value;
  if (typeof url !== "string" || typeof port !== "number" || typeof pid !== "number") return null;
  let parsed: URL;
  try {
    parsed = new URL(url);
  } catch {
    return null;
  }
  if (parsed.protocol !== "http:" || parsed.hostname !== "127.0.0.1" || Number(parsed.port) !== port) return null;
  if (!new URLSearchParams(parsed.hash.slice(1)).get("token")) return null;
  return { url, origin: parsed.origin, port, pid };
}

function unreadable(result: ChildResult): string {
  const stderr = result.stderr.trim().split("\n").at(-1) ?? "";
  return `unreadable answer (exit ${result.code ?? result.signal ?? "none"})${stderr ? `: ${stderr}` : ""}`;
}

export function parseConnect(result: ChildResult): ConnectAnswer {
  if (result.spawnError) return { kind: "failed", reason: "start_failed", detail: result.spawnError };
  if (result.timedOut) return { kind: "failed", reason: "no_response", detail: "hide connect timed out" };
  const value = lastJsonLine(result.stdout);
  if (value?.ok === true) {
    const daemon = attached(value);
    if (daemon) return { kind: "attached", ...daemon };
  }
  if (value?.ok === false && (value.reason === "start_failed" || value.reason === "no_response")) {
    return { kind: "failed", reason: value.reason, detail: typeof value.detail === "string" ? value.detail : "" };
  }
  return { kind: "failed", reason: "start_failed", detail: unreadable(result) };
}

/** Null when the probe itself did not answer; the caller asks again later. */
export function parseStatus(result: ChildResult): StatusAnswer | null {
  if (result.spawnError || result.timedOut) return null;
  const value = lastJsonLine(result.stdout);
  if (value?.running === false) return { running: false };
  if (value?.running === true) {
    const daemon = attached(value);
    return daemon ? { running: true, ...daemon } : null;
  }
  return null;
}

const PATH_MARK = "__HIDE_LOGIN_PATH__";

/** The login shell's command line that prints its PATH between two marks, past any rc-file output. */
export const LOGIN_PATH_ARGS = ["-ilc", `printf '\\n${PATH_MARK}%s${PATH_MARK}\\n' "$PATH"`];

export function parseLoginPath(stdout: string): string | null {
  const match = new RegExp(`${PATH_MARK}(.*?)${PATH_MARK}`).exec(stdout);
  return match?.[1] ? match[1] : null;
}
