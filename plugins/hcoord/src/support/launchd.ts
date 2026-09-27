import crypto from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { spawnSync } from "node:child_process";

/**
 * Converging one user LaunchAgent on a rendered plist, shared by the Sasu
 * supervisor and the hcoord daemon. launchd refuses `bootstrap` of a label it
 * already has loaded with "5: Input/output error", so every caller reads the
 * label's loaded state first instead of matching that error text; the hcoord
 * daemon's start after a stop failed on exactly that (2026-09-26).
 *
 * `bootout` also returns before launchd has let go of the label: the job is
 * still exiting, and a bootstrap sent right after it fails with the same
 * error. The live daemon start that reloaded a changed plist stopped the
 * daemon that way until a person bootstrapped it by hand seconds later
 * (2026-09-26), so every bootout here waits for the label to read unloaded.
 */
export type LaunchctlRun = (args: string[]) => { status: number | null; stdout: string; stderr: string };

function defaultLaunchctl(args: string[]): { status: number | null; stdout: string; stderr: string } {
  const executed = spawnSync("launchctl", args, { encoding: "utf8", shell: false, timeout: 15_000 });
  if (executed.error !== undefined) return { status: null, stdout: "", stderr: String(executed.error) };
  return { status: executed.status, stdout: executed.stdout ?? "", stderr: executed.stderr ?? "" };
}

export interface LaunchdEnvironment {
  env?: NodeJS.ProcessEnv;
  launchctl?: LaunchctlRun;
  uid?: number;
  /** Injectable filesystem boundaries keep failure-path tests deterministic. */
  writeFile?: (file: string, contents: string) => void;
  rename?: (from: string, to: string) => void;
}

export const launchdDomain = (environment: LaunchdEnvironment): string => `gui/${environment.uid ?? os.userInfo().uid}`;
const domain = launchdDomain;

/** One LaunchAgent: its label and the plist file launchd loads it from. */
export interface LaunchAgentTarget {
  label: string;
  plistPath: string;
}

export interface LaunchAgentStatus {
  plistPath: string;
  installed: boolean;
  /** launchd's answer for the label: loaded, not loaded, or a launchctl failure. */
  loaded: boolean | null;
  detail: string | null;
}

export function labelStatus(agent: LaunchAgentTarget, environment: LaunchdEnvironment = {}): LaunchAgentStatus {
  const { plistPath } = agent;
  const launchctl = environment.launchctl ?? defaultLaunchctl;
  const printed = launchctl(["print", `${domain(environment)}/${agent.label}`]);
  if (printed.status === 0) return { plistPath, installed: fs.existsSync(plistPath), loaded: true, detail: null };
  if (printed.status === null) return { plistPath, installed: fs.existsSync(plistPath), loaded: null, detail: `launchctl unavailable: ${printed.stderr.trim()}` };
  return { plistPath, installed: fs.existsSync(plistPath), loaded: false, detail: (printed.stderr || printed.stdout).trim() || `launchctl print exited ${printed.status}` };
}

/** Runs launchctl and records the argv, so a result can say what launchd was asked. */
function recordingCall(environment: LaunchdEnvironment, asked: string[]): (args: string[]) => { ok: boolean; detail: string } {
  const launchctl = environment.launchctl ?? defaultLaunchctl;
  return (args) => {
    asked.push(args.join(" "));
    const executed = launchctl(args);
    return { ok: executed.status === 0, detail: (executed.stderr || executed.stdout).trim() || `launchctl ${args[0]} exited ${executed.status ?? "without status"}` };
  };
}

/**
 * launchd's default ExitTimeOut is 20 s before it kills a job that ignores
 * SIGTERM, so a label still loaded after 30 s is not settling on its own.
 */
const BOOTOUT_SETTLE_MS = 30_000;
const BOOTOUT_POLL_MS = 100;

function pause(ms: number): void { Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, ms); }

/** Boot the label out and return once launchd reports it unloaded, or say why not. */
function bootoutSettled(agent: LaunchAgentTarget, environment: LaunchdEnvironment, call: (args: string[]) => { ok: boolean; detail: string }): { ok: boolean; detail: string } {
  const out = call(["bootout", `${domain(environment)}/${agent.label}`]);
  if (!out.ok) return out;
  const deadline = Date.now() + BOOTOUT_SETTLE_MS;
  for (;;) {
    const status = labelStatus(agent, environment);
    if (status.loaded === false) return { ok: true, detail: "" };
    if (status.loaded === null) return { ok: false, detail: `could not confirm the label unloaded after bootout: ${status.detail}` };
    if (Date.now() >= deadline) return { ok: false, detail: `launchd still reports ${agent.label} loaded ${BOOTOUT_SETTLE_MS / 1000} s after bootout` };
    pause(BOOTOUT_POLL_MS);
  }
}

export interface InstallResult {
  plistPath: string;
  /** written when the plist bytes changed, unchanged otherwise. */
  plist: "written" | "unchanged";
  /** What launchctl was asked to do, in order; a converged install asks nothing. */
  launchctl: string[];
  loaded: boolean;
  problem: string | null;
}

/**
 * Converge the LaunchAgent on the given spec (engineering 11): unchanged
 * bytes and a loaded label ask launchd nothing; changed bytes reload the
 * label (bootout, bootstrap) so the running definition matches the file;
 * an unloaded label is bootstrapped.
 */
export function convergeLaunchAgent(agent: LaunchAgentTarget, rendered: string, environment: LaunchdEnvironment = {}): InstallResult {
  const { plistPath } = agent;
  const asked: string[] = [];
  const call = recordingCall(environment, asked);
  const current = fs.existsSync(plistPath) ? fs.readFileSync(plistPath, "utf8") : null;
  const changed = current !== rendered;
  const before = labelStatus(agent, environment);
  let staged: string | null = null;
  if (changed) {
    fs.mkdirSync(path.dirname(plistPath), { recursive: true });
    staged = `${plistPath}.${process.pid}.${crypto.randomUUID()}.tmp`;
    try {
      (environment.writeFile ?? ((file, contents) => fs.writeFileSync(file, contents)))(staged, rendered);
    } catch (error) {
      try { fs.rmSync(staged, { force: true }); } catch {}
      return { plistPath, plist: "unchanged", launchctl: asked, loaded: before.loaded === true, problem: `could not stage replacement before changing the loaded service: ${error instanceof Error ? error.message : String(error)}` };
    }
  }
  if (before.loaded === true && changed) {
    const out = bootoutSettled(agent, environment, call);
    if (!out.ok) {
      if (staged !== null) fs.rmSync(staged, { force: true });
      return { plistPath, plist: "unchanged", launchctl: asked, loaded: labelStatus(agent, environment).loaded === true, problem: `bootout failed, the old plist remains in place: ${out.detail}` };
    }
  }
  if (changed) {
    try {
      (environment.rename ?? fs.renameSync)(staged!, plistPath);
    } catch (error) {
      try { fs.rmSync(staged!, { force: true }); } catch {}
      if (before.loaded === true) {
        const restored = call(["bootstrap", domain(environment), plistPath]);
        return { plistPath, plist: "unchanged", launchctl: asked, loaded: restored.ok, problem: `could not activate the staged definition; ${restored.ok ? "the prior service was restored" : `the service is stopped and restoration failed: ${restored.detail}`}: ${error instanceof Error ? error.message : String(error)}` };
      }
      return { plistPath, plist: "unchanged", launchctl: asked, loaded: false, problem: `could not activate the staged definition: ${error instanceof Error ? error.message : String(error)}` };
    }
  }
  if (before.loaded !== true || changed) {
    const boot = call(["bootstrap", domain(environment), plistPath]);
    if (!boot.ok) return { plistPath, plist: changed ? "written" : "unchanged", launchctl: asked, loaded: false, problem: `bootstrap failed: ${boot.detail}` };
  }
  return { plistPath, plist: changed ? "written" : "unchanged", launchctl: asked, loaded: true, problem: null };
}

export interface UninstallResult {
  plistPath: string;
  plist: "removed" | "absent";
  launchctl: string[];
  problem: string | null;
}

/** Remove only what install created: the label and its plist. */
export function removeLaunchAgent(agent: LaunchAgentTarget, environment: LaunchdEnvironment = {}): UninstallResult {
  const { plistPath } = agent;
  const asked: string[] = [];
  const call = recordingCall(environment, asked);
  const before = labelStatus(agent, environment);
  let problem: string | null = null;
  if (before.loaded === true) {
    const out = bootoutSettled(agent, environment, call);
    if (!out.ok) problem = `bootout failed: ${out.detail}`;
  }
  const existed = fs.existsSync(plistPath);
  if (existed && problem === null) fs.rmSync(plistPath);
  return { plistPath, plist: existed && problem === null ? "removed" : "absent", launchctl: asked, problem };
}

/** Ask launchd to start a loaded label now; a missing label is reported, never bootstrapped here. */
export function kickstartLabel(label: string, environment: LaunchdEnvironment = {}): { ok: boolean; detail: string | null } {
  const launchctl = environment.launchctl ?? defaultLaunchctl;
  const out = launchctl(["kickstart", `${domain(environment)}/${label}`]);
  return { ok: out.status === 0, detail: out.status === 0 ? null : (out.stderr || out.stdout).trim() || `launchctl kickstart exited ${out.status ?? "without status"}` };
}
