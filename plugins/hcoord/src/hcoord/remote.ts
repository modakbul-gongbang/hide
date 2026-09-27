import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { spawn, spawnSync } from "node:child_process";
import { runHerdrCommand, type HerdrEnvironment } from "../implement/herdr";
import { HcoordError, REMOTE_PROTOCOL } from "./model";
import { dataDir, writeFileAtomic } from "./store";

/**
 * A participant's machine is "local" (or this host's name) or the label of a
 * Herdr saved SSH machine. Every remote effect starts at the HQ: Herdr calls
 * gain `--machine <label>`, and outbox collection uses the same saved
 * machine's SSH target. Remote hosts never open a connection back (PRD D-08).
 */
export function isLocalMachine(machine: string): boolean { return machine === "local" || machine === os.hostname(); }

// Measured 2026-09-24: `herdr --machine mini agent list` took 2.8 s over
// Tailscale, so the local 0.5-2 s probe deadlines would read every remote
// call as a failure. The cap stays below defaultRun's 35 s process deadline.
const REMOTE_HERDR_TIMEOUT_MS = 20_000;
// After a connection-level failure the daemon stops calling that machine for
// this long, so an asleep laptop or offline mini cannot stall every tick for
// a full SSH timeout. Participants read as unavailable meanwhile (PRD B11).
const UNREACHABLE_BACKOFF_MS = 30_000;
// One bound for every SSH run of the remote hcoord: its deadline and the
// largest reply read (a take returns at most 4 MB of letters).
const REMOTE_CALL_TIMEOUT_MS = 30_000;
const REMOTE_OUTPUT_BYTES = 16 * 1024 * 1024;
const unreachableUntil = new Map<string, number>();

export function machineBackoff(machine: string): number | null {
  const until = unreachableUntil.get(machine);
  return until !== undefined && until > Date.now() ? until : null;
}

/** A Herdr failure that never reached the remote server's API. */
function connectionFailure(result: { status: number | null; stdout: string; stderr: string; errorCode?: string }): boolean {
  if (result.status === 0) return false;
  if (result.errorCode !== undefined && ["auth_failed", "machine_disabled", "machine_unknown", "permission_denied", "remote_not_installed", "version_mismatch"].includes(result.errorCode)) return false;
  if (/^\s*\{/.test(result.stderr) || /^\s*\{/.test(result.stdout)) return false;
  if (/unknown machine/i.test(result.stderr)) return false;
  return true;
}

/** The Herdr environment that addresses one participant's execution. */
export function herdrRoute(machine: string, hostScope: string): HerdrEnvironment {
  const env = { ...process.env };
  if (!isLocalMachine(machine)) delete env["HERDR_SOCKET_PATH"];
  else if (hostScope !== "default") env["HERDR_SOCKET_PATH"] = hostScope;
  if (isLocalMachine(machine)) return { env, run: (args, _cwd, timeoutMs) => runHerdrCommand(args, timeoutMs, env) };
  return {
    env,
    run: (args, _cwd, timeoutMs) => {
      if (machineBackoff(machine) !== null) return { status: null, stdout: "", stderr: `machine ${machine} is unreachable; retrying after backoff` };
      let result: { status: number | null; stdout: string; stderr: string; errorCode?: string };
      try {
        const saved = savedMachine(machine);
        const raw = remoteCall(machine, ["herdr", ...(saved.session ? ["--session", saved.session] : []), "--", ...args]);
        result = {
          status: typeof raw["status"] === "number" || raw["status"] === null ? raw["status"] as number | null : null,
          stdout: typeof raw["stdout"] === "string" ? raw["stdout"] : "",
          stderr: typeof raw["stderr"] === "string" ? raw["stderr"] : "remote hcoord returned no stderr",
          ...(typeof raw["errorCode"] === "string" ? { errorCode: raw["errorCode"] } : {}),
        };
      } catch (error) {
        result = {
          status: null,
          stdout: "",
          stderr: error instanceof Error ? error.message : "remote Herdr call failed",
          ...(error instanceof HcoordError ? { errorCode: error.code } : {}),
        };
      }
      if (connectionFailure(result)) unreachableUntil.set(machine, Date.now() + UNREACHABLE_BACKOFF_MS);
      else unreachableUntil.delete(machine);
      return result;
    },
  };
}

export interface SavedMachine { id: string; label: string; target: string; session: string | null; enabled: boolean }

/** The saved machine a label names; Herdr's machine list is the only remote configuration (PRD D-08, D-17). */
export function savedMachine(label: string): SavedMachine {
  const help = runHerdrCommand(["--help"], 2000);
  if (help.status !== 0 || !/--machine <label-or-id>/.test(`${help.stdout}${help.stderr}`)) throw new HcoordError("unsupported_runtime", "the installed Herdr has no --machine API forwarding; install Herdr 0.9.1 or later on the HQ");
  const listed = runHerdrCommand(["machine", "list", "--json"], 5000);
  if (listed.status !== 0) throw new HcoordError("runtime_unavailable", "Herdr could not list saved machines");
  let machines: unknown;
  try { machines = JSON.parse(listed.stdout); } catch { throw new HcoordError("runtime_unavailable", "Herdr returned an invalid saved machine list"); }
  const found = Array.isArray(machines) ? (machines as SavedMachine[]).find((entry) => entry && (entry.label === label || entry.id === label)) : undefined;
  if (!found) throw new HcoordError("machine_unknown", `machine ${label} is not saved in Herdr; add it with herdr machine add --label ${label} <ssh-target>`);
  if (!found.enabled) throw new HcoordError("machine_disabled", `Herdr machine ${label} is disabled; enable it with herdr machine enable ${label}`);
  if (typeof found.target !== "string" || found.target === "") throw new HcoordError("runtime_unavailable", `Herdr machine ${label} has no SSH target`);
  return found;
}

/**
 * Confirms the Herdr features a remote participant needs: API forwarding,
 * agent prompt and worktree create on the HQ CLI, and a reachable, API
 * compatible remote server. Refusal names the missing piece (PRD B16).
 */
export function requireRemoteHerdr(machine: string): SavedMachine {
  const saved = savedMachine(machine);
  for (const [argv, pattern, feature] of [[["agent", "prompt", "--help"], /herdr agent prompt <TARGET> <TEXT>/, "agent prompt"], [["worktree", "create", "--help"], /Usage: herdr worktree create/, "worktree create"]] as const) {
    const help = runHerdrCommand([...argv], 2000);
    if (help.status !== 0 || !pattern.test(`${help.stdout}${help.stderr}`)) throw new HcoordError("unsupported_runtime", `the installed Herdr lacks ${feature}; remote participants need Herdr 0.9.1 or later`);
  }
  const probe = herdrRoute(machine, "default").run!(["agent", "list"], undefined, REMOTE_HERDR_TIMEOUT_MS);
  if (probe.status !== 0) {
    const text = `${probe.stderr}${probe.stdout}`.trim().slice(0, 300);
    if (probe.errorCode !== undefined && ["auth_failed", "machine_disabled", "machine_unknown", "permission_denied", "remote_not_installed", "version_mismatch"].includes(probe.errorCode)) {
      throw new HcoordError(probe.errorCode, text || `remote Herdr check failed with ${probe.errorCode}`);
    }
    if (/permission denied|authentication|host key verification/i.test(text)) throw new HcoordError("auth_failed", `SSH authentication to ${machine} failed; fix the key for ${saved.target} (hcoord stores no credentials): ${text}`);
    if (/incompatible|protocol|version/i.test(text)) throw new HcoordError("version_mismatch", `the Herdr server on ${machine} is not API compatible with this HQ: ${text}`);
    throw new HcoordError("machine_unreachable", `Herdr on ${machine} did not answer through its saved SSH machine: ${text || "no diagnostic"}`);
  }
  return saved;
}

const quote = (value: string): string => `'${value.split("'").join(`'\\''`)}'`;

/**
 * The shell command a remote runs for one hcoord remote subcommand. The
 * installer writes <data dir>/bin/hcoord with an absolute node path, so the
 * command needs no login PATH (a non-login SSH shell on the measured mini had
 * only /usr/bin:/bin). HCOORD_REMOTE_HOME relocates the remote data dir for
 * an isolated install.
 */
function remoteShell(argv: string[]): string {
  const override = process.env["HCOORD_REMOTE_HOME"];
  const dir = override !== undefined && override !== "" ? quote(override) : `"$HOME/.hcoord"`;
  const divider = argv.indexOf("--");
  const before = divider < 0 ? argv : argv.slice(0, divider);
  const tail = divider < 0 ? [] : argv.slice(divider + 1);
  return `HCOORD_HOME=${dir} exec ${dir}/bin/hcoord remote ${before.map(quote).join(" ")} --json${divider < 0 ? "" : ` -- ${tail.map(quote).join(" ")}`}`;
}

const sshArgs = (target: string, argv: string[]): string[] => ["-o", "BatchMode=yes", "-o", "ConnectTimeout=10", "-o", "ServerAliveInterval=5", "-o", "ServerAliveCountMax=2", target, "--", remoteShell(argv)];

export type Raw = { status: number | null; stdout: string; stderr: string };

/** Maps an SSH run of the remote hcoord to its JSON value or an explicit refusal (PRD D-17). */
export function remoteOutcome(machine: string, raw: Raw): Record<string, unknown> {
  const text = raw.stderr.trim().slice(0, 300);
  if (raw.status === 255) {
    if (/permission denied|authentication|host key verification/i.test(text)) throw new HcoordError("auth_failed", `SSH authentication to ${machine} failed; fix its key (hcoord stores no credentials): ${text}`);
    throw new HcoordError("machine_unreachable", `SSH to ${machine} failed: ${text || "no diagnostic"}`);
  }
  if (raw.status === null) throw new HcoordError("machine_unreachable", `SSH to ${machine} did not finish in time`);
  if (raw.status === 127 || (/no such file|not found/i.test(text) && raw.stdout.trim() === "")) throw new HcoordError("remote_not_installed", `hcoord is not installed on ${machine}; run the repository installer there (scripts/install-local-skills.mjs)`);
  if (raw.status === 126) throw new HcoordError("permission_denied", `the remote hcoord on ${machine} is not executable: ${text}`);
  let parsed: { ok?: boolean; value?: Record<string, unknown>; error?: { code?: string; message?: string } };
  try { parsed = JSON.parse(raw.stdout.trim().split("\n").at(-1) ?? ""); }
  catch { throw new HcoordError("protocol", `the remote hcoord on ${machine} returned no JSON result: ${text}`); }
  if (parsed.ok !== true) throw new HcoordError(parsed.error?.code ?? "remote_failed", `${machine}: ${parsed.error?.message ?? "remote hcoord refused"}`);
  const value = parsed.value ?? {};
  if (value["protocol"] !== REMOTE_PROTOCOL) throw new HcoordError("version_mismatch", `hcoord on ${machine} speaks remote protocol ${String(value["protocol"])}; this HQ speaks ${REMOTE_PROTOCOL}; install the same hcoord version on both`);
  return value;
}

export function remoteCall(machine: string, argv: string[]): Record<string, unknown> {
  const saved = savedMachine(machine);
  const result = spawnSync("ssh", sshArgs(saved.target, argv), { encoding: "utf8", timeout: REMOTE_CALL_TIMEOUT_MS, killSignal: "SIGKILL", maxBuffer: REMOTE_OUTPUT_BYTES });
  return remoteOutcome(machine, { status: result.error ? null : result.status, stdout: result.stdout ?? "", stderr: result.stderr ?? String(result.error ?? "") });
}

/** The same call without blocking the daemon's operation queue; killed at its deadline. */
export function remoteCallAsync(target: string, argv: string[], signal?: AbortSignal): Promise<Raw> {
  return new Promise((resolve) => {
    const child = spawn("ssh", sshArgs(target, argv), { stdio: ["ignore", "pipe", "pipe"], signal, killSignal: "SIGKILL" });
    let stdout = "", stderr = "", done = false;
    const finish = (status: number | null): void => { if (done) return; done = true; clearTimeout(timer); resolve({ status, stdout, stderr }); };
    const timer = setTimeout(() => { child.kill("SIGKILL"); finish(null); }, REMOTE_CALL_TIMEOUT_MS);
    child.stdout.on("data", (chunk: Buffer) => { stdout += chunk.toString("utf8"); if (stdout.length > REMOTE_OUTPUT_BYTES) { child.kill("SIGKILL"); finish(null); } });
    child.stderr.on("data", (chunk: Buffer) => { stderr += chunk.toString("utf8").slice(0, 4096); });
    child.on("error", (error) => { stderr += String(error); finish(null); });
    child.on("close", (code) => finish(code));
  });
}

/** Where this machine's HQ is: "local" or a machine name. The file exists only on a non-HQ machine. */
const hqPath = (home: string): string => path.join(dataDir(home), "hq.json");
export function readHq(home = os.homedir()): string {
  try {
    const parsed = JSON.parse(fs.readFileSync(hqPath(home), "utf8")) as { hq?: unknown };
    return typeof parsed.hq === "string" && parsed.hq !== "" ? parsed.hq : "local";
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code === "ENOENT") return "local";
    throw new HcoordError("corrupt_config", `${hqPath(home)} is unreadable; fix or remove it`);
  }
}
export function writeHq(hq: string, home = os.homedir()): void {
  fs.mkdirSync(dataDir(home), { recursive: true, mode: 0o700 });
  if (hq === "local") { fs.rmSync(hqPath(home), { force: true }); return; }
  writeFileAtomic(hqPath(home), `${JSON.stringify({ hq, setAt: new Date().toISOString() })}\n`);
}
