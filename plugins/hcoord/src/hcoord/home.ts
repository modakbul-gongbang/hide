import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { HcoordError } from "./model";
import { bootoutLabel, bootstrapLabel, labelStatus, type LaunchdEnvironment } from "../support/launchd";
import { DEFAULT_LABEL, plistPath } from "./platform";
import { accountHome, dataDir } from "./store";

/**
 * The one-time move of `~/.hcoord` into this build's default home (PRD
 * hide-home-layout D-09). Both ends come from the same HOME and nothing
 * else: a caller cannot name another folder to move, so the command on PATH
 * moves nothing but hcoord's own old home. hide's kit runs it before it installs a new
 * hcoord, while the old daemon still runs the old code, so nothing reads the
 * new home before the move is whole.
 *
 * The daemon that ran from the old home is booted out first, so it stops
 * writing there; the folder is renamed in one step, so the ledger, the
 * letters and a manual stop arrive together or not at all; then only the
 * junk is dropped: the socket, the lock that names a dead daemon, the old
 * daemon's start records (which would count its stop against the new one)
 * and the temporary files an interrupted write left. Any failure before the rename
 * loads the old daemon again from its untouched plist, so the old home keeps
 * working as before.
 */
export interface AdoptOptions {
  home?: string;
  launchd?: LaunchdEnvironment;
  /** The old daemon's LaunchAgent label; null when no launchd job ran from the old home. Tests pass both. */
  legacyLabel?: string | null;
}

export interface Adopted {
  moved: boolean;
  from: string;
  to: string;
  /** The label booted out before the move, which the kit's next ensure loads again. */
  stoppedLabel: string | null;
  /** What was dropped as junk after the move, by name. */
  dropped: string[];
}

/** Lock, socket and start-record files a stopped daemon leaves, and the temporary files of an interrupted write. */
function isJunk(name: string): boolean {
  return name === "api.sock" || name === "api.sock.lock" || name === "api.sock.lock.recovery" || name === "health.json" || (name.startsWith(".") && name.endsWith(".tmp"));
}

function readPid(file: string): number | null {
  try {
    const pid = Number.parseInt(fs.readFileSync(file, "utf8").trim(), 10);
    return Number.isInteger(pid) && pid > 1 ? pid : null;
  } catch { return null; }
}

function alive(pid: number): boolean {
  try { process.kill(pid, 0); return true; } catch (error) { return (error as NodeJS.ErrnoException).code === "EPERM"; }
}

/** A real folder this account owns; a link or another account's folder is never moved or moved into. */
function ownFolder(dir: string): boolean {
  const stat = fs.lstatSync(dir);
  return stat.isDirectory() && !stat.isSymbolicLink() && stat.uid === process.getuid!();
}

export function adoptLegacyHome(options: AdoptOptions = {}): Adopted {
  const relocated = process.env["HCOORD_HOME"];
  if (relocated !== undefined && relocated !== "") throw new HcoordError("relocated", "HCOORD_HOME relocates hcoord, so no home is moved");
  const home = options.home ?? os.homedir();
  if (!path.isAbsolute(home)) throw new HcoordError("invalid_argument", `HOME ${JSON.stringify(home)} is not an absolute path, so no home is moved`);
  const from = path.join(home, ".hcoord");
  const to = dataDir(home);
  const nothing: Adopted = { moved: false, from, to, stoppedLabel: null, dropped: [] };
  if (!fs.existsSync(from) && !isLink(from)) return nothing;
  if (!ownFolder(from)) throw new HcoordError("unsafe_home", `${from} is not a folder of this account, so hcoord left it`);
  if (fs.existsSync(to) || isLink(to)) throw new HcoordError("home_conflict", `${to} already exists, so ${from} was left as it is; move one of them away and Reinstall hcoord`);
  const parent = path.dirname(to);
  fs.mkdirSync(parent, { recursive: true, mode: 0o700 });
  const parentStat = fs.lstatSync(parent);
  if (!ownFolder(parent) || (parentStat.mode & 0o022) !== 0) throw new HcoordError("unsafe_home", `${parent} is not a private folder of this account, so hcoord was not moved into it`);

  // Only the account's own ~/.hcoord ran under the account's plain label; an
  // old home under another HOME had a label of its own, which its owner stops.
  const label = options.legacyLabel !== undefined ? options.legacyLabel : (path.resolve(home) === path.resolve(accountHome()) ? DEFAULT_LABEL : null);
  const environment = options.launchd ?? {};
  const agent = label === null ? null : { label, plistPath: plistPath(home, label) };
  let stopped = false;
  if (agent !== null) {
    const status = labelStatus(agent, environment);
    if (status.loaded === null) throw new HcoordError("launchd_unavailable", `launchd could not say whether the coordinator runs: ${status.detail}; nothing was moved`);
    if (status.loaded) {
      const out = bootoutLabel(agent, environment);
      if (!out.ok) throw new HcoordError("adopt_failed", `the coordinator could not be stopped: ${out.detail}; nothing was moved`);
      stopped = true;
    }
  }
  const fail = (reason: string): never => {
    if (stopped && agent !== null) {
      const restored = bootstrapLabel(agent, environment);
      throw new HcoordError("adopt_failed", `${reason}; ${restored.ok ? "the coordinator runs from the old home again" : `the coordinator could not be started again: ${restored.detail}`}`);
    }
    throw new HcoordError("adopt_failed", reason);
  };
  const lockPid = readPid(path.join(from, "api.sock.lock"));
  if (lockPid !== null && lockPid !== process.pid && alive(lockPid)) fail(`a coordinator (pid ${lockPid}) not run by launchd still runs from ${from}; stop it and retry`);
  try { fs.renameSync(from, to); }
  catch (error) { fail(`${from} could not be moved to ${to}: ${(error as Error).message}`); }
  const dropped: string[] = [];
  for (const name of fs.readdirSync(to)) {
    if (!isJunk(name)) continue;
    fs.rmSync(path.join(to, name), { recursive: true, force: true });
    dropped.push(name);
  }
  return { moved: true, from, to, stoppedLabel: stopped ? label : null, dropped: dropped.sort() };
}

function isLink(file: string): boolean {
  try { return fs.lstatSync(file).isSymbolicLink(); } catch { return false; }
}
