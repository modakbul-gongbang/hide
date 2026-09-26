// Finding the `hide` CLI and reading its answers. The host meets the daemon
// only through `hide connect` (reuse or start, desktop PRD D-03) and
// `hide status --json` (attach-only), whose JSON contract `hided/src/cli.rs`
// owns.

import path from "node:path";
import type { ChildResult } from "./spawn";

export type CliSource = "env" | "worktree" | "path" | "remembered" | "login" | "well-known";

export type ResolvedCli = {
  found: { path: string; source: CliSource } | null;
  /** Every path looked at, in order and once each, for the log (B2). */
  tried: string[];
};

export type FileProbe = {
  isExecutable(file: string): boolean;
  mtimeMs(file: string): number | null;
};

export type CliSearch = {
  override: string | null;
  /** This worktree's root when run unpackaged; null when packaged. */
  worktreeRoot: string | null;
  /** This process's PATH: the terminal's for `pnpm dev`, launchd's bare one for an app opened from Finder. */
  searchPath: string;
  /** The last CLI that attached, read back from the profile. */
  remembered: string | null;
  /** Asks the login shell for its PATH; called only when every cheaper step missed, and null when unpackaged. */
  loginPath: (() => Promise<string | null>) | null;
  home: string;
};

/** Where the CLI is looked for last: the Swift app's own fallback list (`RuntimeEnvironment.swift`). */
export function wellKnownDirs(home: string): string[] {
  return [path.join(home, ".local", "bin"), "/opt/homebrew/bin", "/usr/local/bin"];
}

/** Finds worth remembering once they attach; an override or a worktree build is not the operator's installed CLI. */
export const REMEMBERED_SOURCES: ReadonlySet<CliSource> = new Set(["path", "login", "well-known"]);

function inDirs(searchPath: string): string[] {
  return searchPath
    .split(":")
    .filter((dir) => dir && path.isAbsolute(dir))
    .map((dir) => path.join(dir, "hide"));
}

/**
 * B2's order: the override, then (unpackaged) this worktree's newest build,
 * then PATH, the CLI that last attached, the login shell's PATH, and the
 * usual install directories. An app opened from Finder gets launchd's PATH,
 * not the operator's, so everything after PATH exists for it; the login shell
 * is the slow step, which the remembered path spares every launch after the
 * first. An override that is set but unusable ends the search: it names what
 * the operator asked for, and quietly using another binary would hide the
 * mistake.
 */
export async function resolveCli(input: CliSearch, probe: FileProbe): Promise<ResolvedCli> {
  const tried: string[] = [];
  const done = (file: string | null, source: CliSource): ResolvedCli => ({ found: file ? { path: file, source } : null, tried });
  if (input.override) {
    tried.push(input.override);
    return done(probe.isExecutable(input.override) ? input.override : null, "env");
  }
  if (input.worktreeRoot) {
    const builds = ["debug", "release"].map((profile) => path.join(input.worktreeRoot as string, "target", profile, "hide"));
    tried.push(...builds);
    const newest = builds
      .filter((file) => probe.isExecutable(file))
      .map((file) => ({ file, mtime: probe.mtimeMs(file) ?? 0 }))
      .sort((a, b) => b.mtime - a.mtime)[0];
    if (newest) return done(newest.file, "worktree");
  }
  const first = (files: string[]): string | null => {
    for (const file of files) {
      if (tried.includes(file)) continue;
      tried.push(file);
      if (probe.isExecutable(file)) return file;
    }
    return null;
  };
  const steps: [CliSource, () => Promise<string[]>][] = [
    ["path", async () => inDirs(input.searchPath)],
    ["remembered", async () => (input.remembered ? [input.remembered] : [])],
    ["login", async () => (input.loginPath ? inDirs((await input.loginPath()) ?? "") : [])],
    ["well-known", async () => wellKnownDirs(input.home).map((dir) => path.join(dir, "hide"))],
  ];
  for (const [source, files] of steps) {
    const file = first(await files());
    if (file) return done(file, source);
  }
  return { found: null, tried };
}

const REMEMBERED_SCHEMA = 1;

export function rememberedCliPath(userData: string): string {
  return path.join(userData, "cli-path.json");
}

/** The remembered CLI path; null when none is stored; `{ unreadable }` when the file is not one this host wrote. */
export function parseRememberedCli(stored: unknown): string | null | { unreadable: string } {
  if (stored === null) return null;
  const value = typeof stored === "object" ? (stored as Record<string, unknown>) : null;
  if (value && typeof value.unreadable === "string") return { unreadable: value.unreadable };
  if (value?.schema !== REMEMBERED_SCHEMA || typeof value.path !== "string" || !path.isAbsolute(value.path)) return { unreadable: "unexpected shape" };
  return value.path;
}

export function rememberedCliValue(file: string): unknown {
  return { schema: REMEMBERED_SCHEMA, path: file };
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

/**
 * The login shell asked for its PATH, printed between two marks past any
 * rc-file output. It runs through `env` to set what shell-env sets: Oh My
 * Zsh's auto-update prompt and its tmux autostart can otherwise hold an
 * interactive shell open until the timeout.
 */
export function loginPathCommand(shell: string): { file: string; args: string[] } {
  return {
    file: "/usr/bin/env",
    args: [
      "DISABLE_AUTO_UPDATE=true",
      "ZSH_TMUX_AUTOSTARTED=true",
      "ZSH_TMUX_AUTOSTART=false",
      shell,
      "-ilc",
      `printf '\\n${PATH_MARK}%s${PATH_MARK}\\n' "$PATH"`,
    ],
  };
}

export function parseLoginPath(stdout: string): string | null {
  const match = new RegExp(`${PATH_MARK}(.*?)${PATH_MARK}`).exec(stdout);
  return match?.[1] ? match[1] : null;
}
