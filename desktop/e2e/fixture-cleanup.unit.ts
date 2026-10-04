// Exercise the real fixture teardown and filesystem with Electron and service
// calls replaced at their external boundaries. No native process is started.
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { ChildProcess, execFileSync, spawnSync } from "node:child_process";
import fs from "node:fs";
import net from "node:net";
import os from "node:os";
import path from "node:path";
import { isolate, launch, type Isolated } from "./fixture";

type Guard = (fixtures: Record<string, never>, use: () => Promise<void>, info: { tags: string[] }) => Promise<void>;
const boundary = vi.hoisted(() => ({ launch: vi.fn(), guard: null as Guard | null }));
vi.mock("@playwright/test", () => ({
  _electron: { launch: boundary.launch },
  test: { extend: (fixtures: { focusGuard: [Guard, unknown] }) => { boundary.guard = fixtures.focusGuard[0]; return {}; } },
  expect,
}));
vi.mock("node:child_process", async (original) => ({
  ...await original<typeof import("node:child_process")>(),
  execFileSync: vi.fn((command: string, args?: readonly string[], options?: { timeout?: number }) => {
    if (process.platform !== "win32" || command !== "clang.exe" || args?.length !== 4
      || args[0] !== "-O1" || args[1] !== "-o" || options?.timeout !== 20_000
      || Object.keys(options).some((key) => key !== "timeout")) {
      throw new Error(`unexpected fixture compiler: ${command}`);
    }
    const [, , output, source] = args;
    const privateRoot = path.dirname(source);
    if (!root || !path.isAbsolute(privateRoot) || path.dirname(privateRoot) !== fs.realpathSync.native(root)
      || !path.basename(privateRoot).startsWith("hide-desktop-")
      || source !== path.join(privateRoot, "hide-open.c")
      || output !== path.join(privateRoot, "bin", "hide-open.exe")
      || fs.realpathSync.native(source) !== source
      || fs.realpathSync.native(path.dirname(output)) !== path.join(privateRoot, "bin")
      || fs.readFileSync(source, "utf8") !== "int main(void) { return 0; }\n") {
      throw new Error(`unexpected fixture compiler files: ${command}`);
    }
    // The real helper checks existence; native executable validity belongs to e2e.
    fs.writeFileSync(output, Buffer.alloc(0), { flag: "wx" });
    return Buffer.alloc(0);
  }),
  spawnSync: vi.fn((command: string) => {
    if (command !== "launchctl") throw new Error(`unexpected fixture process: ${command}`);
    return { status: 113, stdout: "", stderr: "No such service" };
  }),
}));

let root: string;
let runs: Isolated[];
let children: ChildProcess[];
beforeEach(() => {
  root = fs.mkdtempSync(path.join(os.tmpdir(), "hide-fixture-regression-"));
  vi.spyOn(os, "tmpdir").mockReturnValue(root);
  boundary.launch.mockReset();
  vi.mocked(execFileSync).mockClear();
  vi.mocked(spawnSync).mockClear();
  runs = [];
  children = [];
});
afterEach(() => {
  // These are inert handles, not real PIDs. Restore the reported exit state
  // so the same recovery entrypoint can clean every test's retained home.
  for (const child of children) reportExit(child);
  for (const run of runs) run.cleanup();
  vi.restoreAllMocks();
  fs.rmSync(root, { recursive: true, force: true });
});

function privateHome(label: string): Isolated {
  const run = isolate({ bin: "/unused/herdr", socket: path.join(root, "unused.sock") }, label);
  runs.push(run);
  return run;
}

test.skipIf(process.platform === "win32")("a private coordinator socket can bind with a long system temporary directory", async () => {
  // beforeEach supplies a long temporary directory, like macOS's per-user
  // TMPDIR. The kit must be able to probe the coordinator's real socket.
  const run = privateHome("server-search");
  fs.mkdirSync(run.env.HCOORD_HOME!, { recursive: true, mode: 0o700 });
  const server = net.createServer();
  try {
    await new Promise<void>((resolve, reject) => {
      server.once("error", reject);
      server.listen(path.join(run.env.HCOORD_HOME!, "api.sock"), resolve);
    });
    expect(server.listening).toBe(true);
  } finally {
    if (server.listening) await new Promise<void>((resolve, reject) => server.close((error) => error ? reject(error) : resolve()));
  }
});

function reportExit(child: ChildProcess, signal: NodeJS.Signals | null = null): void {
  Object.assign(child, { exitCode: signal ? null : 0, signalCode: signal });
}

function candidate(close: (child: ChildProcess) => Promise<void>): ChildProcess {
  const child = new ChildProcess();
  children.push(child);
  boundary.launch.mockResolvedValueOnce({ process: () => child, firstWindow: async () => ({}), close: () => close(child) });
  return child;
}

async function teardown(use: () => Promise<void>): Promise<AggregateError | undefined> {
  try { await boundary.guard!({}, use, { tags: [] }); }
  catch (error) {
    expect(error).toBeInstanceOf(AggregateError);
    return error as AggregateError;
  }
}

test("rejected close preserves only the live candidate's home and permits recovery after exit", async () => {
  const closeFailure = new Error("injected Electron close failure");
  const live = candidate(async () => { throw closeFailure; });
  candidate(async (child) => { reportExit(child); });
  const run = privateHome("live");
  const other = privateHome("exited");
  const error = await teardown(async () => {
    await launch(run.env);
    await launch(other.env);
  });
  expect(live.exitCode).toBeNull();
  expect(error?.errors).toContain(closeFailure);
  expect(fs.existsSync(run.env.HOME!)).toBe(true);
  expect(fs.existsSync(other.root)).toBe(false);
  const recovery = error?.errors.find((item: Error) => item.message.includes(run.root)) as Error;
  expect(recovery.message).toMatch(/confirm.*exit.*cleanup/i);
  // Repeated cleanup and a later test cannot forget the still-live handle.
  expect(() => run.cleanup()).toThrow(run.root);
  expect(fs.existsSync(run.root)).toBe(true);
  reportExit(live);
  run.cleanup();
  run.cleanup();
  expect(fs.existsSync(run.root)).toBe(false);
});

test("a rejected close after confirmed candidate exit still cleans home and reports the close error", async () => {
  const closeFailure = new Error("injected close response failure after exit");
  candidate(async (child) => { reportExit(child); throw closeFailure; });
  const run = privateHome("exited-close-error");
  const error = await teardown(async () => { await launch(run.env); });
  expect(error?.errors).toEqual([closeFailure]);
  expect(fs.existsSync(run.root)).toBe(false);
});

test("a successful close must confirm exit before home deletion", async () => {
  const child = candidate(async () => {});
  const run = privateHome("unconfirmed-exit");
  const error = await teardown(async () => { await launch(run.env); });
  expect(error?.errors.some((item: Error) => item.message.includes(run.root))).toBe(true);
  expect(fs.existsSync(run.env.HOME!)).toBe(true);
  reportExit(child, "SIGTERM");
  run.cleanup();
  expect(fs.existsSync(run.root)).toBe(false);
});

test("manual cleanup rejects a live candidate before stopping any private service", async () => {
  candidate(async (child) => { reportExit(child); });
  const run = privateHome("manual-cleanup");
  const error = await teardown(async () => {
    await launch(run.env);
    expect(() => run.cleanup()).toThrow(run.root);
    expect(fs.existsSync(run.env.HOME!)).toBe(true);
    expect(spawnSync).not.toHaveBeenCalled();
  });
  expect(error).toBeUndefined();
  expect(fs.existsSync(run.root)).toBe(false);
});

test("pending launches retain HOME and the seventeenth candidate is refused before launch", async () => {
  const complete: (() => void)[] = [];
  boundary.launch.mockImplementation(() => new Promise((resolve) => {
    const child = new ChildProcess();
    children.push(child);
    complete.push(() => resolve({ process: () => child, firstWindow: async () => ({}), close: async () => { reportExit(child); } }));
  }));
  const run = privateHome("pending-candidates");
  const error = await teardown(async () => {
    const pending = Array.from({ length: 16 }, () => launch(run.env));
    try {
      expect(() => run.cleanup()).toThrow(run.root);
      expect(fs.existsSync(run.env.HOME!)).toBe(true);
      expect(spawnSync).not.toHaveBeenCalled();
      await expect(launch(run.env)).rejects.toThrow("16 live or launching candidates");
      expect(boundary.launch).toHaveBeenCalledTimes(16);
    } finally {
      for (const resolve of complete) resolve();
      await Promise.all(pending);
    }
  });
  expect(error).toBeUndefined();
  expect(fs.existsSync(run.root)).toBe(false);
});
