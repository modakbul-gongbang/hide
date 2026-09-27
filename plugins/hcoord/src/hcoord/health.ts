import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { HcoordError } from "./model";
import { dataDir, writeFileAtomic } from "./store";

/**
 * Daemon availability evidence (PRD D-16, B14). The daemon appends one start
 * per process that owns the lock and marks it ready and cleanly ended; any
 * hcoord process derives instability from those records. A start that never
 * ended cleanly before the next start is an unexpected exit.
 */
export const UNSTABLE_DEATHS = 3;
export const DEATH_WINDOW_MS = 10 * 60_000;
export const READY_DEADLINE_MS = 60_000;
export const RECOVERY_MS = 10 * 60_000;
const MAX_STARTS = 20;

export interface StartRecord { pid: number; at: string; readyAt: string | null; cleanAt: string | null }
export interface Health { starts: StartRecord[] }
export interface Alert { since: string; reason: string; notifiedAt: string | null }

const healthPath = (home: string): string => path.join(dataDir(home), "health.json");
const alertPath = (home: string): string => path.join(dataDir(home), "alert.json");
export const logPaths = (home = os.homedir()): string[] => [path.join(dataDir(home), "daemon.log"), path.join(dataDir(home), "daemon.err.log")];

function writeAtomic(file: string, value: unknown): void { writeFileAtomic(file, `${JSON.stringify(value)}\n`); }

/** A corrupt record must not read as "healthy"; it is reported like any unreadable hcoord config. */
function readRecord<T>(file: string, valid: (value: unknown) => value is T): T | null {
  let text: string;
  try { text = fs.readFileSync(file, "utf8"); }
  catch (error) { if ((error as NodeJS.ErrnoException).code === "ENOENT") return null; throw error; }
  let parsed: unknown;
  try { parsed = JSON.parse(text); } catch { parsed = undefined; }
  if (!valid(parsed)) throw new HcoordError("corrupt_config", `${file} is unreadable; inspect or remove it`);
  return parsed;
}

export function readHealth(home = os.homedir()): Health {
  return readRecord(healthPath(home), (value): value is Health => value !== null && typeof value === "object" && Array.isArray((value as Health).starts)) ?? { starts: [] };
}

function update(home: string, change: (health: Health) => void): void {
  fs.mkdirSync(dataDir(home), { recursive: true, mode: 0o700 });
  const health = readHealth(home);
  change(health);
  health.starts = health.starts.slice(-MAX_STARTS);
  writeAtomic(healthPath(home), health);
}

export function recordStart(pid: number, at: string, home = os.homedir()): void { update(home, (health) => { health.starts.push({ pid, at, readyAt: null, cleanAt: null }); }); }
export function recordReady(pid: number, at: string, home = os.homedir()): void { update(home, (health) => { const own = [...health.starts].reverse().find((entry) => entry.pid === pid); if (own) own.readyAt = at; }); }
export function recordClean(pid: number, at: string, home = os.homedir()): void { update(home, (health) => { const own = [...health.starts].reverse().find((entry) => entry.pid === pid); if (own) own.cleanAt = at; }); }

/** Unexpected exits whose replacement started inside the window ending at `now`. */
export function recentDeaths(health: Health, now: number): number {
  let deaths = 0;
  health.starts.forEach((entry, index) => {
    const next = health.starts[index + 1];
    if (next && entry.cleanAt === null && now - Date.parse(next.at) <= DEATH_WINDOW_MS) deaths += 1;
  });
  return deaths;
}

/**
 * The instability a caller can observe now, or null. `answering` says whether
 * the daemon socket answered this caller; a start that stayed silent past the
 * deadline is unstable even when no request was attempted meanwhile.
 */
export function instability(health: Health, now: number, answering: boolean): string | null {
  const deaths = recentDeaths(health, now);
  if (deaths >= UNSTABLE_DEATHS) return `${deaths} unexpected daemon exits within 10 minutes`;
  const last = health.starts.at(-1);
  if (last && last.cleanAt === null && !answering && now - Date.parse(last.at) >= READY_DEADLINE_MS) return last.readyAt === null ? "the daemon did not answer on its socket within 1 minute of starting" : "the daemon stopped answering on its socket without a clean exit";
  return null;
}

/** Recovery is ten minutes of continuous readiness with no restart. */
export function recovered(health: Health, now: number, answering: boolean): boolean {
  const last = health.starts.at(-1);
  return answering && last !== undefined && last.readyAt !== null && last.cleanAt === null && now - Date.parse(last.at) >= RECOVERY_MS;
}

export function readAlert(home = os.homedir()): Alert | null {
  return readRecord(alertPath(home), (value): value is Alert => value !== null && typeof value === "object" && typeof (value as Alert).since === "string" && typeof (value as Alert).reason === "string");
}

/**
 * Opens, keeps, or clears the alert. Exactly one process wins the exclusive
 * create of a new alert and is the one that sends the Herdr notification.
 */
export function reconcileAlert(home: string, now: number, answering: boolean, notify: (text: string) => boolean): Alert | null {
  const health = readHealth(home);
  const current = readAlert(home);
  if (current !== null) {
    if (recovered(health, now, answering)) { fs.rmSync(alertPath(home), { force: true }); return null; }
    return current;
  }
  const reason = instability(health, now, answering);
  if (reason === null) return null;
  const alert: Alert = { since: new Date(now).toISOString(), reason, notifiedAt: null };
  try { fs.writeFileSync(alertPath(home), `${JSON.stringify(alert)}\n`, { flag: "wx", mode: 0o600 }); }
  catch (error) { if ((error as NodeJS.ErrnoException).code === "EEXIST") return readAlert(home); throw error; }
  if (notify(`hcoord daemon unstable: ${reason}; see ${logPaths(home)[1]}`)) {
    alert.notifiedAt = new Date().toISOString();
    writeAtomic(alertPath(home), alert);
  }
  return alert;
}

export function warningLine(alert: Alert, home = os.homedir()): string {
  return `hcoord warning: coordinator daemon unstable since ${alert.since}: ${alert.reason}; logs: ${logPaths(home).join(", ")}`;
}
