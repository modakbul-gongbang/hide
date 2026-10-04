// Native spellings for the isolated e2e homes and tools. The home/state
// contract is hide-platform::host; Herdr accepts a filesystem socket path
// on every OS and maps that path to a named pipe on Windows.
import fs from "node:fs";
import path from "node:path";
import crypto from "node:crypto";
import { spawn, type ChildProcess, type SpawnOptions } from "node:child_process";
import { cleanupAfterFailure, throwFixtureFailures } from "./worker-owned";
import { run as runOwnedCommand } from "../../scripts/ci-owned-command.cjs";
export { toPage as fixturePagePath } from "../../desktop/src/main/wirePath";

export const fixtureExecutable = (name: string): string => `${name}${process.platform === "win32" ? ".exe" : ""}`;

/** The receiver executes the unchanged POSIX printf input through the same
 * installed Git Bash used by Windows verification, with literal argv. */
export function fixturePosixShell(): string {
  const shell = process.platform === "win32"
    ? path.join(inheritedFixtureEnv().PROGRAMFILES ?? "", "Git", "bin", "bash.exe") : "/bin/sh";
  if (!path.isAbsolute(shell) || !fs.existsSync(shell)) throw new Error("fixture POSIX shell is unavailable");
  return shell;
}

/** The configured Windows pane shell is cmd; native argv quotes differ from
 * a Unix shell's exec. The called receiver owns the same pane stdin. */
export function fixtureShellCommand(executable: string, args: string[]): string {
  if (process.platform !== "win32") return "exec " + [executable, ...args].map(value => `'${value.replaceAll("'", "'\\''")}'`).join(" ");
  if ([executable, ...args].some(value => /[\r\n"%!]/.test(value))) throw new Error("fixture cmd argument cannot be represented without expansion");
  return [executable, ...args].map(value => `"${value}"`).join(" ");
}

type NativeReceipt = { phase: string; pid: number; birth: string; code: number; survivors: number; error: string };
type Owner = { root: string; file: string; spawnError?: Error; released: boolean; failures?: unknown[] };
const owners = new WeakMap<ChildProcess, Owner>();
const activeOwners = new Set<Owner>();
const MAX_OWNERS = 256, MAX_RECEIPTS = 10_000, MAX_RECEIPT_BYTES = 16 * 1024;
const pause = () => Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, 5);
// Both ordinary package entrypoints run from web/ or desktop/, as the
// existing binary fixtures do; this shared module can be ESM or CommonJS.
const repository = path.resolve("..");
const receiptDirectory = path.join(repository, "agents/runs/ci-fixture-owners");
const nativeFixtureOwner = () => path.join(repository, "target/debug/examples", fixtureExecutable("fixture-owner"));

/** Native inventory observes the same launch identity before and after the
 * walk. A subsequent observation cannot silently adopt a reused shell PID. */
export async function fixtureProcessTree(pid: number, timeout: number, env: NodeJS.ProcessEnv, birth?: string): Promise<{ birth: string; descendants: number }> {
  if (!Number.isSafeInteger(pid) || pid <= 1 || timeout <= 0) throw new Error("fixture process inventory needs a live PID and remaining deadline");
  const args = ["--probe-tree", String(pid), ...(birth === undefined ? [] : [birth])];
  const output = (await runOwnedCommand(repository, nativeFixtureOwner(), args, { timeout, env, maxBuffer: 1024, subject: "fixture-shell-inventory" })).stdout;
  const fields = output.trimEnd().split("\t");
  if (fields.length !== 5 || fields[0] !== "v1" || fields[1] !== String(pid)
    || !/^\d{1,20}$/.test(fields[2]!) || !/^\d{1,3}$/.test(fields[3]!) || !/^\d{1,20}$/.test(fields[4]!)
    || Number(fields[3]) > 256 || (birth !== undefined && fields[2] !== birth)) throw new Error("invalid native fixture process inventory");
  return { birth: fields[2]!, descendants: Number(fields[3]) };
}

function readReceipt(owner: Owner): NativeReceipt | undefined {
  let stat: fs.Stats;
  try { stat = fs.lstatSync(owner.file); }
  catch (error) { if ((error as NodeJS.ErrnoException).code === "ENOENT") return; throw error; }
  if (!stat.isFile() || stat.size > MAX_RECEIPT_BYTES) throw new Error("native fixture receipt file/byte cap violated");
  const [version, phase, pid, birth, code, survivors, detail, ...extra] = fs.readFileSync(owner.file, "utf8").replace(/\n$/, "").split("\t");
  if (version !== "v1" || !phase || !/^\d+$/.test(pid ?? "") || !/^\d+$/.test(birth ?? "")
    || !/^-?\d+$/.test(code ?? "") || !/^-?\d+$/.test(survivors ?? "") || detail === undefined
    || !/^(?:[a-f0-9]{2})*$/.test(detail) || extra.length) throw new Error("invalid native fixture receipt");
  return { phase, pid: Number(pid), birth: birth!, code: Number(code), survivors: Number(survivors), error: Buffer.from(detail, "hex").toString("utf8") };
}

/** The native owner captures the target identity at launch, before its parent
 * can exit. Only the worker holds its stdin pipe; SIGKILL therefore still
 * ends the original process group/job and preserves an unconfirmed home. */
export function spawnFixtureProcess(executable: string, args: string[], root: string, options: SpawnOptions = {}): ChildProcess {
  if (activeOwners.size >= MAX_OWNERS) throw new Error("worker exceeded 256 native fixture owners");
  fs.mkdirSync(receiptDirectory, { recursive: true });
  if (fs.readdirSync(receiptDirectory).length >= MAX_RECEIPTS) throw new Error("native fixture receipt file cap exceeded");
  const file = path.join(receiptDirectory, `${crypto.randomBytes(16).toString("hex")}.receipt`);
  const helper = nativeFixtureOwner();
  if (!fs.existsSync(helper)) throw new Error("native fixture owner is missing; build this worktree's fixture-owner example");
  const stdio = options.stdio === undefined || options.stdio === "pipe" ? ["pipe", "pipe", "pipe"]
    : options.stdio === "ignore" ? ["pipe", "ignore", "ignore"]
    : options.stdio === "inherit" ? ["pipe", "inherit", "inherit"] : ["pipe", ...options.stdio.slice(1)];
  const child = spawn(helper, [file, root, executable, ...args], { ...options, stdio: stdio as SpawnOptions["stdio"], detached: process.platform !== "win32" });
  const owner: Owner = { root, file, released: false };
  owners.set(child, owner);
  activeOwners.add(owner);
  child.once("error", (error) => { owner.spawnError = error; });
  return child;
}

/** Readiness and metrics address the launched target, never the supervisor. */
export function fixtureProcessId(child: ChildProcess): number {
  const owner = owners.get(child);
  if (!owner) throw new Error("fixture process has no native launch owner");
  if (owner.spawnError) throw owner.spawnError;
  const value = readReceipt(owner);
  if (!value || value.pid <= 0 || !Number.isSafeInteger(value.pid)) throw new Error("fixture target launch identity is not available");
  if (value.error) throw new Error(value.error);
  return value.pid;
}

export function fixtureProcessFailure(child: ChildProcess): boolean {
  const owner = owners.get(child);
  if (!owner) throw new Error("fixture process has no native launch owner");
  if (owner.spawnError) throw owner.spawnError;
  const value = readReceipt(owner);
  if (!value) return false;
  if (value?.error) throw new Error(value.error);
  if (value && value.phase !== "running") throw new Error(`fixture target ended during setup: ${value.phase}, code=${value.code}`);
  if (!Number.isSafeInteger(value.pid) || value.pid <= 0 || BigInt(value.birth) <= 0n) throw new Error("invalid native fixture launch identity");
  return true;
}

/** Every callback failure still reaches native termination. The same launch
 * receipt authorizes stop; a late PID lookup never authorizes a replacement. */
export function stopFixtureProcess(child: ChildProcess, graceful?: () => void): void {
  const owner = owners.get(child);
  if (!owner) throw new Error("fixture process has no native launch owner");
  if (owner.released) {
    if (owner.failures?.length) throwFixtureFailures(owner.failures);
    return;
  }
  const failures: unknown[] = [];
  try { graceful?.(); } catch (error) { failures.push(error); }
  const deadline = Date.now() + 2000;
  let final: NativeReceipt | undefined;
  try {
    let value = readReceipt(owner);
    while (!value && Date.now() < deadline && !owner.spawnError) { pause(); value = readReceipt(owner); }
    if (owner.spawnError) throw owner.spawnError;
    if (!value) throw new Error("fixture native launch/exit unconfirmed; preserve home");
    if (value.phase === "running" || value.phase === "stop-refused") {
      const request = owner.file.replace(/\.receipt$/, ".stop"), temporary = `${request}.tmp`;
      fs.writeFileSync(temporary, `${value.pid} ${value.birth}`, { flag: "wx", mode: 0o600 });
      fs.renameSync(temporary, request);
      while (Date.now() < deadline) {
        value = readReceipt(owner);
        if (value && value.phase !== "running" && value.phase !== "stop-refused") break;
        pause();
      }
    }
    final = value;
    if (!final || final.survivors !== 0) throw new Error(final?.error || "fixture exit unconfirmed; preserve home");
    owner.released = true;
    activeOwners.delete(owner);
    if (final.error) throw new Error(final.error);
    if (!failures.length) fs.unlinkSync(owner.file);
  } catch (error) {
    failures.push(error);
    // Request I/O can fail too. EOF always addresses the original native
    // owner, and still attempts termination before we report the failure.
    child.stdin?.destroy();
    while (Date.now() < deadline) {
      try {
        final = readReceipt(owner);
        if (final?.survivors === 0) {
          owner.released = true;
          activeOwners.delete(owner);
          if (final.error && !failures.some(error => error instanceof Error && error.message === final!.error)) failures.push(new Error(final.error));
          break;
        }
      }
      catch (secondary) { failures.push(secondary); break; }
      pause();
    }
    if (!owner.released) failures.push(new Error("fixture owned exit remains unconfirmed; preserve home"));
  }
  if (failures.length) { owner.failures = failures; throwFixtureFailures(failures); }
}

/** A failed compiler or setup must not remove a still-owned target's home. */
export function assertFixtureRootReleased(root: string): void {
  if ([...activeOwners].some(owner => owner.root === root)) throw new Error("fixture child exit unconfirmed; preserve home");
}

/** A reported stop error does not skip independent releases after confirmed
 * exit. Unknown exit gates every filesystem release and preserves the home. */
export function releaseFixtureRoot(root: string, stop: () => void, releases: (() => void)[]): void {
  const failures: unknown[] = releases.length > 256 ? [new Error("fixture release cap exceeded")] : [];
  try { stop(); } catch (error) { failures.push(error); }
  if (releases.length > 256) throwFixtureFailures(failures);
  try { assertFixtureRootReleased(root); }
  catch (error) { failures.push(error); throwFixtureFailures(failures); }
  for (const release of releases) {
    try { release(); } catch (error) { failures.push(error); }
  }
  if (failures.length) throwFixtureFailures(failures);
}

/** A fixture compiler is an owned, bounded child, separate from test deadlines. */
export function compileFixtureC(source: string, executable: string): void {
  const compiler = process.platform === "win32" ? "clang.exe" : "cc";
  const child = spawnFixtureProcess(compiler, ["-O1", "-o", executable, source], path.dirname(source), { stdio: "inherit" });
  const owner = owners.get(child)!;
  const deadline = Date.now() + 20_000;
  try {
    let result: NativeReceipt | undefined;
    while (Date.now() < deadline) {
      result = readReceipt(owner);
      if (result && result.phase !== "running") break;
      pause();
    }
    if (!result || result.phase === "running") throw new Error(`fixture C compiler ${compiler} timed out (20 second process bound)`);
    if (result.error) throw new Error(result.error);
    if (result.code !== 0) throw new Error(`fixture C compiler ${compiler} exited ${result.code}; the runner needs its native C toolchain`);
  } catch (error) {
    cleanupAfterFailure(error, () => stopFixtureProcess(child));
  }
  stopFixtureProcess(child);
}

/** Windows environment keys are case-insensitive, including Path/PATH. */
export function inheritedFixtureEnv(): Record<string, string> {
  return Object.fromEntries(Object.entries(process.env).flatMap(([key, value]) => {
    const canonical = process.platform === "win32" ? key.toUpperCase() : key;
    if (value === undefined || ["HERDR_", "HIDE_", "ELECTRON_", "HCOORD_", "SASU_"].some((prefix) => key.toUpperCase().startsWith(prefix))) return [];
    return [[canonical, value]];
  }));
}

/** Move native home lookups and CLI credential discovery together. */
export function fixtureHomeEnv(home: string): Record<string, string> {
  const windows: Record<string, string> = process.platform === "win32" ? {
    USERPROFILE: home,
    APPDATA: path.join(home, "AppData", "Roaming"),
    LOCALAPPDATA: path.join(home, "AppData", "Local"),
  } : {};
  for (const dir of Object.values(windows)) fs.mkdirSync(dir, { recursive: true });
  return {
    HOME: home,
    ...windows,
    HCOORD_HOME: path.join(home, ".hcoord"),
    XDG_CONFIG_HOME: path.join(home, ".config"),
    XDG_STATE_HOME: path.join(home, ".local", "state"),
    CLAUDE_CONFIG_DIR: path.join(home, ".claude"),
    CODEX_HOME: path.join(home, ".codex"),
  };
}

/** System tools only: do not append the account's agent-provider PATH. */
export function fixtureToolPath(bin: string): string {
  if (process.platform !== "win32") return [bin, "/usr/bin", "/bin", "/usr/sbin", "/sbin"].join(path.delimiter);
  const env = inheritedFixtureEnv();
  const system = env.SYSTEMROOT;
  const programs = env.PROGRAMFILES;
  if (!system || !path.isAbsolute(system) || !programs || !path.isAbsolute(programs)) throw new Error("Windows fixture needs absolute SystemRoot and ProgramFiles for its controlled system-tool PATH");
  return [bin, path.join(system, "System32"), system, path.join(system, "System32", "WindowsPowerShell", "v1.0"), path.join(programs, "Git", "cmd"), path.join(programs, "Git", "usr", "bin")].join(path.delimiter);
}

/** The Unix no-op has no native Windows counterpart; use the compiled shim. */
export function fixtureOpenCommand(root?: string, privateRoot?: string): string {
  if (process.platform !== "win32") return "/usr/bin/true";
  if (!root && !privateRoot) throw new Error("Windows fixture opener needs a private root");
  const command = path.join((root ?? privateRoot)!, "bin", fixtureExecutable("hide-open"));
  // Unit/private-home fixtures need no Herdr server. Give those homes the
  // same native no-op without falling back to an account's GUI opener.
  if (!root) {
    fs.mkdirSync(path.dirname(command), { recursive: true });
    const source = path.join(privateRoot!, "hide-open.c");
    fs.writeFileSync(source, "int main(void) { return 0; }\n");
    compileFixtureC(source, command);
  }
  if (!fs.existsSync(command)) throw new Error(`Windows fixture opener is missing: ${command}`);
  return command;
}
