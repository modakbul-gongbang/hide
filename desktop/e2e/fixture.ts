// The desktop app against a private Herdr server and a private hided state
// directory. Nothing here can reach the operator's daemon: the app is
// refused a launch unless HIDE_STATE_DIR, HOME, HERDR_SOCKET_PATH and its
// own userData all sit under this run's temporary directory.

import { _electron as electron, type ElectronApplication, type Page } from "@playwright/test";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import type { HerdrFixture } from "../../web/e2e/herdr-fixture";

export const DESKTOP_DIR = path.resolve(__dirname, "..");
export const REPO = path.resolve(DESKTOP_DIR, "..");
export const HIDE_CLI = path.join(REPO, "target", "debug", "hide");

export type Isolated = {
  root: string;
  env: Record<string, string>;
  /** Runs a `hide` verb against this run's state directory. */
  hide: (args: string[]) => { status: number | null; stdout: string };
  daemonPid: () => number | null;
  cleanup: () => void;
};

export function isolate(herdr: HerdrFixture, label: string): Isolated {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), `hide-desktop-${label}-`));
  const home = path.join(root, "home");
  fs.mkdirSync(path.join(home, "projects"), { recursive: true });
  const inherited = Object.fromEntries(
    Object.entries(process.env).filter(
      (entry): entry is [string, string] =>
        entry[1] !== undefined && !entry[0].startsWith("HERDR_") && !entry[0].startsWith("HIDE_") && !entry[0].startsWith("ELECTRON_"),
    ),
  );
  const env: Record<string, string> = {
    ...inherited,
    HOME: home,
    HIDE_STATE_DIR: path.join(root, "state"),
    HIDE_DESKTOP_USER_DATA_DIR: path.join(root, "user-data"),
    HIDE_CLI_PATH: HIDE_CLI,
    HIDED_UI_DIR: path.join(REPO, "web", "dist"),
    HERDR_SOCKET_PATH: herdr.socket,
    HERDR_BIN_PATH: herdr.bin,
    // `open_external` must not launch a GUI application during a test.
    HIDE_OPEN_COMMAND: "/usr/bin/true",
  };
  const hide = (args: string[]) => {
    const run = spawnSync(HIDE_CLI, args, { env, encoding: "utf8", timeout: 20_000 });
    return { status: run.status, stdout: run.stdout };
  };
  const daemonPid = () => {
    const answer = JSON.parse(hide(["status", "--json"]).stdout) as { running: boolean; pid?: number };
    return answer.running ? (answer.pid ?? null) : null;
  };
  const cleanup = () => {
    hide(["stop"]);
    fs.rmSync(root, { recursive: true, force: true });
  };
  return { root, env, hide, daemonPid, cleanup };
}

function assertIsolated(env: Record<string, string>): void {
  // The Herdr fixture keeps its socket directly under /tmp for the path length limit.
  const roots = [fs.realpathSync(os.tmpdir()), fs.realpathSync("/tmp")];
  for (const key of ["HOME", "HIDE_STATE_DIR", "HIDE_DESKTOP_USER_DATA_DIR", "HERDR_SOCKET_PATH"]) {
    const value = env[key];
    const parent = value ? fs.realpathSync(path.dirname(value)) : null;
    if (!parent || !roots.some((root) => parent.startsWith(root))) throw new Error(`${key} is not isolated under ${roots.join(" or ")}`);
  }
}

/** `switches` are Chromium command-line switches for this launch only. */
export async function launch(env: Record<string, string>, switches: string[] = []): Promise<{ app: ElectronApplication; page: Page }> {
  assertIsolated(env);
  const app = await electron.launch({ args: [DESKTOP_DIR, ...switches], cwd: DESKTOP_DIR, env });
  const page = await app.firstWindow();
  return { app, page };
}

/** The host's structured log lines for this run's profile. */
export function hostLog(env: Record<string, string>): { event: string; [key: string]: unknown }[] {
  const file = path.join(env.HIDE_DESKTOP_USER_DATA_DIR!, "logs", "desktop.log");
  if (!fs.existsSync(file)) return [];
  return fs
    .readFileSync(file, "utf8")
    .split("\n")
    .filter(Boolean)
    .map((line) => JSON.parse(line) as { event: string });
}

/** A screenshot under the run directory when one is named; nothing is written otherwise. */
export async function screenshot(page: Page, name: string): Promise<void> {
  const dir = process.env.HIDE_E2E_SCREENSHOT_DIR;
  if (dir) await page.screenshot({ path: path.join(dir, `${name}.png`) });
}
