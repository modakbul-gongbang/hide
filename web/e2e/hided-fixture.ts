// An isolated `hided` on a private HOME and state directory, for the web e2e
// lanes. It reads the daemon's state file for the loopback origin and token,
// so nothing here touches the operator's running daemon.

import { spawn } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { linkFixtureTranscripts, type HerdrFixture } from "./herdr-fixture";
import { ownUntilWorkerExit } from "./worker-owned";
import { endWindowsProcesses, fixtureExecutable, fixtureHomeEnv, fixtureOpenCommand, inheritedFixtureEnv, windowsProcessTree, type WindowsProcess } from "./platform-fixture";

/**
 * `restart` stops the daemon and starts it again on the same state directory
 * and port, as a daemon restart does: the core loses what it held in memory,
 * the host keeps its id, and the browser keeps its origin and so its drafts.
 */
export type Daemon = {
  /** Owned fixture process, used for isolated resource measurements. */
  pid: number;
  origin: string;
  token: string;
  home: string;
  stateDir: string;
  hostId: string;
  stop: () => void;
  /** `beforeStart` runs on the daemon's state directory while it is down, and may wait. */
  restart: (beforeStart?: (stateDir: string) => void | Promise<void>) => Promise<Daemon>;
};

/**
 * `extraEnv` is laid over the daemon's environment; an undefined value leaves that variable unset.
 * `bundled` runs the daemon from a `hide.app/Contents/Resources` folder of copies of the debug
 * binaries, which is the one place a daemon installs the kit (`hide_kit::bundled_kit_dir`); the
 * kit then writes into the fixture's private HOME and nowhere else.
 */
export async function startHided(herdr: HerdrFixture, label = "s2", homeOverride?: string, extraEnv: NodeJS.ProcessEnv = {}, bundled = false): Promise<Daemon> {
  // The daemon places pane-bootstrap.sock below this directory. Keep the
  // fixture root short enough for macOS's Unix socket path limit.
  const dir = fs.realpathSync.native(fs.mkdtempSync(path.join(os.tmpdir(), "hde-")));
  const home = homeOverride ?? path.join(dir, "home");
  fs.mkdirSync(path.join(home, "projects", "alpha"), { recursive: true });
  fs.mkdirSync(path.join(home, "projects", ".hidden"), { recursive: true });
  fs.writeFileSync(path.join(home, "projects", "notes.txt"), "x");
  linkFixtureTranscripts(herdr, home);
  return launch(herdr, label, dir, home, "0", extraEnv, bundled ? bundledBinary(dir) : undefined);
}

/** Copies of the debug binaries laid out as an installed app, so the daemon runs the kit. */
function bundledBinary(dir: string): string {
  const resources = path.join(dir, "hide.app", "Contents", "Resources");
  fs.mkdirSync(resources, { recursive: true });
  for (const name of ["hided", "hide", "hide-agent-hooks"]) {
    const target = path.join(resources, fixtureExecutable(name));
    fs.copyFileSync(path.resolve("..", "target", "debug", fixtureExecutable(name)), target);
    fs.chmodSync(target, 0o755);
  }
  return path.join(resources, fixtureExecutable("hided"));
}

async function launch(herdr: HerdrFixture, label: string, dir: string, home: string, port: string, extraEnv: NodeJS.ProcessEnv, bundledAt?: string): Promise<Daemon> {
  const env = inheritedFixtureEnv();
  const statePath = path.join(dir, "hide", "hided.json");
  fs.rmSync(statePath, { force: true });
  const binary = bundledAt ?? path.resolve("..", "target", "debug", fixtureExecutable("hided"));
  const child = spawn(binary, [], {
    env: {
      ...env,
      ...fixtureHomeEnv(home),
      HIDE_STATE_DIR: path.join(dir, "hide"),
      HIDE_KEEP_ALIVE: "1",
      HIDE_PORT: port,
      HIDED_UI_DIR: path.resolve("dist"),
      HERDR_SOCKET_PATH: herdr.socket,
      HERDR_BIN_PATH: herdr.bin,
      // Catalog discovery probes every provider on hided's PATH, so use the
      // same controlled shim and system tools as the private Herdr server.
      // Specs adding a shim prepend it to this path through extraEnv.
      PATH: herdr.fixturePath,
      // `open_external` must not launch a GUI application on the runner.
      HIDE_OPEN_COMMAND: fixtureOpenCommand(herdr.root),
      ...extraEnv,
    },
    stdio: ["ignore", "pipe", "pipe"],
  });
  // A daemon that cannot be spawned (no debug build in this worktree) is an
  // error event; unheard, it kills the worker before any test cleanup runs.
  let spawnFailed = null as Error | null;
  child.once("error", (error) => {
    spawnFailed = error;
  });
  const logDir = process.env.HIDE_E2E_SCREENSHOT_DIR;
  if (logDir) {
    const log = fs.createWriteStream(path.join(logDir, `hided-${label}-${path.basename(dir)}.log`), { flags: "a" });
    // Two pipes share the file: the first stream to end must not close it
    // under the other's last write ("write after end" on a restart), so it
    // closes once the daemon's output has ended on both.
    child.stdout?.pipe(log, { end: false });
    child.stderr?.pipe(log, { end: false });
    child.once("close", () => log.end());
  }
  const exited = new Promise<void>((resolve) => child.once("exit", () => resolve()));
  // Panes Herdr started under the home keep their working folders locked on
  // Windows until its server's processes are gone, so the daemon's directory
  // (the home is inside it) is removed by Herdr's stop, after them.
  const removeDir = () => {
    // The daemon may still be writing its state file (and, with S3, staged
    // files) as it dies; removing the directory under it fails with ENOTEMPTY.
    for (let attempt = 0; attempt < 40; attempt += 1) {
      try {
        fs.rmSync(dir, { recursive: true, force: true });
        return;
      } catch {
        // Synchronous worker-exit cleanup cannot await a timer. Keep the
        // same 50 ms filesystem retry interval without a Unix child tool.
        Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, 50);
      }
    }
    fs.rmSync(dir, { recursive: true, force: true });
  };
  // A killed daemon on Windows keeps the folders it holds open (the checkouts
  // it watches) until it is gone, and Herdr's stop deletes the fixture root
  // next. So its tree is listed while it runs, then ended, and this returns
  // once none of it is left.
  const end = () => {
    if (process.platform !== "win32" || !child.pid || child.exitCode !== null || child.signalCode !== null) {
      child.kill();
      return;
    }
    let tree: WindowsProcess[] = [];
    let failure: unknown;
    try { tree = windowsProcessTree(child.pid); } catch (error) { failure = error; }
    child.kill();
    endWindowsProcesses(tree);
    if (failure !== undefined) throw failure;
  };
  const { stop, disown } = ownUntilWorkerExit(() => {
    herdr.afterStop(removeDir);
    end();
  });
  for (let i = 0; i < 50; i += 1) {
    if (spawnFailed) {
      stop();
      throw new Error(`hided did not start from ${binary}: ${spawnFailed.message}; run cargo build -p hided in this worktree`);
    }
    if (fs.existsSync(statePath)) {
      try {
        // The file may be mid-write on the first read; the next tick reads it whole.
        const state = JSON.parse(fs.readFileSync(statePath, "utf8")) as { port: number; token: string };
        const origin = `http://127.0.0.1:${state.port}`;
        if ((await fetch(`${origin}/health`)).ok) {
          const hostId = fs.readFileSync(path.join(dir, "hide", "host-id"), "utf8").trim();
          const restart = async (beforeStart?: (stateDir: string) => void | Promise<void>) => {
            end();
            await exited;
            await beforeStart?.(path.join(dir, "hide"));
            // The next daemon takes over the directory and its removal.
            disown();
            return launch(herdr, label, dir, home, String(state.port), extraEnv, bundledAt);
          };
          return { pid: child.pid!, origin, token: state.token, home: fs.realpathSync(home), stateDir: path.join(dir, "hide"), hostId, stop, restart };
        }
      } catch {
        /* still starting */
      }
    }
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
  stop();
  throw new Error("hided did not write a state file");
}
