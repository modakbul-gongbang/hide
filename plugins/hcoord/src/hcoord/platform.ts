import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import crypto from "node:crypto";
import { HcoordError } from "./model";
import { runHerdrCommand } from "../implement/herdr";
import { convergeLaunchAgent, kickstartLabel, removeLaunchAgent, type LaunchdEnvironment } from "../support/launchd";
import { accountHome, dataDir, defaultDataDir, stopMarkerPath } from "./store";

export const DEFAULT_LABEL = "com.hcoord.daemon";
/**
 * The account's one coordinator keeps the plain label: the daemon whose home
 * is the account's default `~/.hide/hcoord`, however that home was named (an
 * explicit HCOORD_HOME equal to it, as the remote shell passes, is the same
 * coordinator). Any other home gets a label of its own. launchd domains are
 * per account, not per HOME, so the account's home is read from the user
 * database: a test that only moved HOME can never take the account's label.
 */
export function daemonLabel(home = os.homedir()): string {
  const dir = path.resolve(dataDir(home));
  if (dir === path.resolve(defaultDataDir(accountHome()))) return DEFAULT_LABEL;
  const suffix = crypto.createHash("sha256").update(dir).digest("hex").slice(0, 12);
  return `${DEFAULT_LABEL}.${suffix}`;
}
export const plistPath = (home: string, label: string): string => path.join(home, "Library", "LaunchAgents", `${label}.plist`);
const escapeXml = (text: string): string => text.replaceAll("&", "&amp;").replaceAll("<", "&lt;").replaceAll(">", "&gt;").replaceAll('"', "&quot;");
// Each value states what was actually observed (PRD B18): "verified_isolated"
// means a real run in an isolated HOME or test session, not a production login.
// Remote rows rest on the 2026-09-24 laptop HQ <-> mini run (Herdr 0.9.1,
// test-only remote session) and the isolated launchd KeepAlive check.
export const platformSupport = () => ({
  macos: { localSocket: "verified_isolated", loginStart: "unverified", crashRestart: "verified_isolated", manualStop: "verified_isolated", herdrNotification: "unverified", systemNotification: "unsupported", remoteHerdr: "verified_isolated", remoteOutbox: "verified_isolated", remoteWorktreeSpawn: "verified_isolated" },
  windows: { sharedCore: "implemented_unverified", namedPipe: "unsupported", loginStart: "unsupported", manualStop: "unsupported", desktopNotification: "unsupported", remoteHerdrHost: "unsupported" },
});

/** The title carries only an opaque request ID; the question stays in the private ledger. */
export function notifyHuman(requestId: string): { ok: boolean; code: string } {
  const result = runHerdrCommand(["notification", "show", `hcoord request ${requestId}: open the CLI inbox`, "--sound", "request"], 2000);
  return { ok: result.status === 0, code: result.status === 0 ? "herdr_accepted" : result.status === null ? "runtime_unavailable" : "notification_refused" };
}

/** A Herdr notification with caller-chosen text; true only when Herdr accepted it. */
/** What a person prepares once so a machine can host remote agents (PRD B18, D-13). */
export const REMOTE_SETUP = [
  "on the HQ: herdr machine add --label <name> <ssh-target> (hcoord reads only this saved machine; it stores no credentials)",
  "on the remote: add it as a device in hide (Settings > Devices); hide installs hcoord there at ~/.hide/hcoord/bin/hcoord for the HQ's SSH calls",
  "on the remote: clone the source repository that agent spawn --repo names",
] as const;

export function notifyText(text: string): boolean {
  return runHerdrCommand(["notification", "show", text, "--sound", "request"], 2000).status === 0;
}

/**
 * KeepAlive restarts only an unsuccessful exit: a crash or kill comes back,
 * while `hcoord daemon stop` (and a start that finds the manual-stop marker)
 * exits 0 and stays stopped (PRD D-11, B13).
 */
export function daemonPlist(home: string, args: string[], label = DEFAULT_LABEL): string {
  const environment: Array<[string, string]> = [["HOME", home], ["PATH", process.env["PATH"] ?? ""]];
  if (process.env["HCOORD_HOME"]) environment.push(["HCOORD_HOME", process.env["HCOORD_HOME"]]);
  if (process.env["HCOORD_REMOTE_HOME"]) environment.push(["HCOORD_REMOTE_HOME", process.env["HCOORD_REMOTE_HOME"]]);
  if (process.env["HERDR_BIN_PATH"]) environment.push(["HERDR_BIN_PATH", process.env["HERDR_BIN_PATH"]]);
  if (process.env["HERDR_SOCKET_PATH"]) environment.push(["HERDR_SOCKET_PATH", process.env["HERDR_SOCKET_PATH"]]);
  if (process.env["ELECTRON_RUN_AS_NODE"]) environment.push(["ELECTRON_RUN_AS_NODE", process.env["ELECTRON_RUN_AS_NODE"]]);
  return `<?xml version="1.0" encoding="UTF-8"?>\n<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">\n<plist version="1.0"><dict><key>Label</key><string>${escapeXml(label)}</string><key>ProgramArguments</key><array>${args.map((a) => `<string>${escapeXml(a)}</string>`).join("")}</array><key>RunAtLoad</key><true/><key>KeepAlive</key><dict><key>SuccessfulExit</key><false/></dict><key>EnvironmentVariables</key><dict>${environment.map(([key, value]) => `<key>${key}</key><string>${escapeXml(value)}</string>`).join("")}</dict><key>StandardOutPath</key><string>${escapeXml(path.join(dataDir(home), "daemon.log"))}</string><key>StandardErrorPath</key><string>${escapeXml(path.join(dataDir(home), "daemon.err.log"))}</string></dict></plist>\n`;
}

/**
 * Converges the LaunchAgent on this build's plist, then asks launchd to run
 * it. A stopped daemon keeps its label loaded (KeepAlive leaves a clean exit
 * down), and launchd refuses to bootstrap a loaded label with "5:
 * Input/output error"; reading the loaded state first is what lets a start
 * after `hcoord daemon stop` reach the kickstart (2026-09-26).
 */
export function startDaemon(home = os.homedir(), environment: LaunchdEnvironment = {}): { label: string; path: string; launchctl: string[] } {
  if (process.platform !== "darwin") throw new HcoordError("unsupported_platform", "automatic daemon start is implemented only for macOS; see hcoord daemon run on a supported host");
  const label = daemonLabel(home);
  const file = plistPath(home, label);
  fs.mkdirSync(path.dirname(file), { recursive: true, mode: 0o700 });
  fs.mkdirSync(dataDir(home), { recursive: true, mode: 0o700 });
  fs.rmSync(stopMarkerPath(home), { force: true });
  const executable = path.resolve(__dirname, "cli.js");
  const converged = convergeLaunchAgent({ label, plistPath: file }, daemonPlist(home, [process.execPath, executable, "daemon", "run"], label), environment);
  if (converged.problem !== null) throw new HcoordError("start_failed", `launchd could not load the coordinator: ${converged.problem}; inspect its stderr log`);
  const kick = kickstartLabel(label, environment);
  if (!kick.ok) throw new HcoordError("start_failed", `launchd could not start the coordinator: ${kick.detail}; inspect its stderr log`);
  return { label, path: file, launchctl: [...converged.launchctl, `kickstart ${label}`] };
}

/** Explicit removal belongs to the home that installed the service, not to an app window. */
export function uninstallDaemon(home = os.homedir(), environment: LaunchdEnvironment = {}) {
  if (process.platform !== "darwin") throw new HcoordError("unsupported_platform", "daemon uninstall removes a macOS LaunchAgent; stop a foreground daemon with daemon stop");
  const label = daemonLabel(home);
  const file = plistPath(home, label);
  // Keep a later kit ensure from undoing the explicit removal. Conversation
  // data is retained; an already absent home is not recreated just to remove it.
  if (fs.existsSync(dataDir(home))) fs.writeFileSync(stopMarkerPath(home), `${new Date().toISOString()}\n`, { mode: 0o600 });
  const removed = removeLaunchAgent({ label, plistPath: file }, environment);
  if (removed.problem !== null) throw new HcoordError("uninstall_failed", `coordinator removal is incomplete: ${removed.problem}; keep this HOME and retry daemon uninstall after resolving the launchctl or filesystem failure`, { label, path: file });
  return { label, path: file, plist: removed.plist, launchctl: removed.launchctl };
}
