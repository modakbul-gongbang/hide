// The desktop app against a private Herdr server and a private hided state
// directory. Nothing here can reach the operator's daemon: the app is
// refused a launch unless HIDE_STATE_DIR, HOME, HERDR_SOCKET_PATH and its
// own userData all sit under this run's temporary directory.

import { _electron as electron, test as base, expect, type ElectronApplication, type Page } from "@playwright/test";
import { spawnSync, type ChildProcess } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import type { HerdrFixture } from "../../web/e2e/herdr-fixture";
import { SHOW_INACTIVE_SWITCH } from "../src/main/launchSwitches";

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
    // The stopped daemon can still be removing its own files; rmSync retries ENOTEMPTY.
    fs.rmSync(root, { recursive: true, force: true, maxRetries: 10, retryDelay: 100 });
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

/**
 * Every launch runs beside the operator's own work (issue 232): the host shows
 * its window behind the operator's without activating the app, so a run never
 * takes the screen, the keyboard or the frontmost app, and the covered window
 * keeps painting, so a capture by window id is current.
 */
const BACKGROUND_SWITCHES = [`--${SHOW_INACTIVE_SWITCH}`, "--disable-backgrounding-occluded-windows"];

/**
 * The tag of a test that needs the key window or native input (a page
 * holding the keyboard, a pinch, a native drag). Such a test brings its
 * window to the front itself, so it takes the keyboard while it runs;
 * `--grep-invert @needs-focus` leaves it out.
 */
export const NEEDS_FOCUS = "@needs-focus";

const FOCUS_GUARD = path.join(__dirname, "focus-guard.cjs");

/** The running test's report folder and the apps it launched; null outside a test from this module's `test`. */
let focusReports: { dir: string; apps: { app: ElectronApplication; child: ChildProcess }[] } | null = null;

/**
 * The desktop e2e `test`: after each test not tagged `NEEDS_FOCUS`, any app
 * it launched that became active or gave a window the keyboard fails it, so
 * "an e2e app never comes to the front" is checked on every run, CI included.
 * It sees only this app; other programs a spec could open (a browser, Finder)
 * are stubbed by the specs that reach them. A spec must take `test` from
 * here; `launch` refuses otherwise.
 */
export const test = base.extend<{ focusGuard: void }>({
  focusGuard: [
    // eslint-disable-next-line no-empty-pattern -- Playwright requires the fixtures argument to be destructured.
    async ({}, use, testInfo) => {
      const dir = fs.mkdtempSync(path.join(os.tmpdir(), "hide-e2e-focus-"));
      focusReports = { dir, apps: [] };
      try {
        await use();
        // A test that timed out skips its own cleanup; an app it left running is closed here, so
        // none outlives its report folder. The guard appends synchronously, so every report is on disk.
        for (const { app, child } of focusReports.apps) {
          if (child.exitCode === null && child.signalCode === null) await app.close().catch(() => undefined);
        }
        const reports = fs.readdirSync(dir).flatMap((file) => fs.readFileSync(path.join(dir, file), "utf8").split("\n").filter(Boolean));
        if (!testInfo.tags.includes(NEEDS_FOCUS)) {
          expect(reports, `the app came to the front in a test not tagged ${NEEDS_FOCUS}`).toEqual([]);
        }
      } finally {
        focusReports = null;
        fs.rmSync(dir, { recursive: true, force: true });
      }
    },
    { auto: true },
  ],
});

/** Starts the app with the focus guard preloaded, reporting into the running test's folder. */
async function start(appDir: string, env: Record<string, string>): Promise<ElectronApplication> {
  assertIsolated(env);
  if (!focusReports) throw new Error("launch the desktop app from a test imported from desktop/e2e/fixture.ts, so its focus guard runs");
  const report = path.join(focusReports.dir, `launch-${focusReports.apps.length + 1}.jsonl`);
  const app = await electron.launch({ args: ["-r", FOCUS_GUARD, appDir, ...BACKGROUND_SWITCHES, `--hide-e2e-focus-report=${report}`], cwd: appDir, env });
  // Taken now: `app.process()` throws once the app is closed.
  focusReports.apps.push({ app, child: app.process() });
  return app;
}

/** `appDir` is the app folder to run, the desktop package unless a test copies it. */
export async function launch(
  env: Record<string, string>,
  { appDir = DESKTOP_DIR }: { appDir?: string } = {},
): Promise<{ app: ElectronApplication; page: Page }> {
  const app = await start(appDir, env);
  const page = await app.firstWindow();
  return { app, page };
}

/**
 * A launch that attaches to a daemon already running and waits for the shell
 * in the window, read through the main process. Such a launch leaves the
 * status page for the daemon's origin within tens of milliseconds, and
 * Playwright can lose that early renderer swap: its page stays on the status
 * page, or no window event arrives at all, while the window shows the shell
 * (checked by reading the window through the main process when Playwright's
 * page did not). The main process sees the window as the operator does.
 */
export async function relaunch(env: Record<string, string>, { appDir = DESKTOP_DIR }: { appDir?: string } = {}): Promise<ElectronApplication> {
  const app = await start(appDir, env);
  await expect
    .poll(
      () =>
        app.evaluate(async ({ BrowserWindow }) => {
          const window = BrowserWindow.getAllWindows()[0];
          if (!window || window.webContents.isLoading()) return false;
          return window.webContents
            .executeJavaScript("document.querySelector('[data-main-screen], [data-workspace-screen]') !== null")
            .catch(() => false) as Promise<boolean>;
        }),
      { timeout: 30_000 },
    )
    .toBe(true);
  return app;
}

/**
 * The page that holds the shell. A browser display is a page of its own to
 * Playwright, and one a relaunch restores can be the first page it reports,
 * so the shell is found by what it draws rather than by order.
 */
export async function shellPage(app: ElectronApplication): Promise<Page> {
  let shell: Page | null = null;
  await expect
    .poll(
      async () => {
        for (const page of app.windows()) {
          const drawn = await page.locator("[data-main-screen], [data-workspace-screen]").count().catch(() => 0);
          if (drawn > 0) {
            shell = page;
            return true;
          }
        }
        return false;
      },
      { timeout: 30_000 },
    )
    .toBe(true);
  return shell!;
}

/**
 * The built app copied under this run's directory, so an unpackaged launch
 * has no worktree `target/` beside it and searches for `hide` the way an
 * installed app does.
 */
export function detachedApp(root: string): string {
  const dir = path.join(root, "app");
  fs.mkdirSync(dir);
  fs.copyFileSync(path.join(DESKTOP_DIR, "package.json"), path.join(dir, "package.json"));
  fs.cpSync(path.join(DESKTOP_DIR, "dist"), path.join(dir, "dist"), { recursive: true });
  return dir;
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

/**
 * A screenshot under the run directory when one is named; nothing is written
 * otherwise. Running CSS transitions are finished first, so a capture taken
 * right after a click shows the state the click produced rather than a fade.
 */
export async function screenshot(page: Page, name: string): Promise<void> {
  const dir = process.env.HIDE_E2E_SCREENSHOT_DIR;
  if (dir) await page.screenshot({ path: path.join(dir, `${name}.png`), animations: "disabled" });
}
