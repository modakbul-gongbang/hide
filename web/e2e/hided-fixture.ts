// An isolated `hided` on a private HOME and state directory, for the web e2e
// lanes. It reads the daemon's state file for the loopback origin and token,
// so nothing here touches the operator's running daemon.

import { spawn, spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import type { HerdrFixture } from "./herdr-fixture";

/**
 * `restart` stops the daemon and starts it again on the same state directory
 * and port, as a daemon restart does: the core loses what it held in memory,
 * the host keeps its id, and the browser keeps its origin and so its drafts.
 */
export type Daemon = {
  origin: string;
  token: string;
  home: string;
  hostId: string;
  stop: () => void;
  /** `beforeStart` runs on the daemon's state directory while it is down. */
  restart: (beforeStart?: (stateDir: string) => void) => Promise<Daemon>;
};

export async function startHided(herdr: HerdrFixture, label = "s2", homeOverride?: string): Promise<Daemon> {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), `hide-e2e-${label}-`));
  const home = homeOverride ?? path.join(dir, "home");
  fs.mkdirSync(path.join(home, "projects", "alpha"), { recursive: true });
  fs.mkdirSync(path.join(home, "projects", ".hidden"), { recursive: true });
  fs.writeFileSync(path.join(home, "projects", "notes.txt"), "x");
  return launch(herdr, label, dir, home, "0");
}

async function launch(herdr: HerdrFixture, label: string, dir: string, home: string, port: string): Promise<Daemon> {
  const env = { ...process.env };
  for (const key of ["HERDR_SOCKET_PATH", "HERDR_PANE_ID", "HERDR_TAB_ID", "HERDR_WORKSPACE_ID", "HERDR_ENV"]) delete env[key];
  const statePath = path.join(dir, "hide", "hided.json");
  fs.rmSync(statePath, { force: true });
  const child = spawn(path.resolve("..", "target", "debug", "hided"), [], {
    env: {
      ...env,
      HOME: home,
      HIDE_STATE_DIR: path.join(dir, "hide"),
      HIDE_KEEP_ALIVE: "1",
      HIDE_PORT: port,
      HIDED_UI_DIR: path.resolve("dist"),
      HERDR_SOCKET_PATH: herdr.socket,
      HERDR_BIN_PATH: herdr.bin,
      // `open_external` must not launch a GUI application on the runner.
      HIDE_OPEN_COMMAND: "/usr/bin/true",
    },
    stdio: ["ignore", "pipe", "pipe"],
  });
  const logDir = process.env.HIDE_E2E_SCREENSHOT_DIR;
  if (logDir) {
    const log = fs.createWriteStream(path.join(logDir, `hided-${label}-${path.basename(dir)}.log`), { flags: "a" });
    child.stdout?.pipe(log);
    child.stderr?.pipe(log);
  }
  const exited = new Promise<void>((resolve) => child.once("exit", () => resolve()));
  const stop = () => {
    child.kill();
    // The daemon may still be writing its state file (and, with S3, staged
    // files) as it dies; removing the directory under it fails with ENOTEMPTY.
    for (let attempt = 0; attempt < 40; attempt += 1) {
      try {
        fs.rmSync(dir, { recursive: true, force: true });
        return;
      } catch {
        spawnSync("/bin/sleep", ["0.05"]);
      }
    }
    fs.rmSync(dir, { recursive: true, force: true });
  };
  for (let i = 0; i < 50; i += 1) {
    if (fs.existsSync(statePath)) {
      try {
        // The file may be mid-write on the first read; the next tick reads it whole.
        const state = JSON.parse(fs.readFileSync(statePath, "utf8")) as { port: number; token: string };
        const origin = `http://127.0.0.1:${state.port}`;
        if ((await fetch(`${origin}/health`)).ok) {
          const hostId = fs.readFileSync(path.join(dir, "hide", "host-id"), "utf8").trim();
          const restart = async (beforeStart?: (stateDir: string) => void) => {
            child.kill();
            await exited;
            beforeStart?.(path.join(dir, "hide"));
            return launch(herdr, label, dir, home, String(state.port));
          };
          return { origin, token: state.token, home: fs.realpathSync(home), hostId, stop, restart };
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
