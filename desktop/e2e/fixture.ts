// The desktop app against a private Herdr server and a private hided state
// directory. Nothing here can reach the operator's daemon: the app is
// refused a launch unless HIDE_STATE_DIR, HOME, HCOORD_HOME, HERDR_SOCKET_PATH and its
// own userData all sit under this run's temporary directory.

import { _electron as electron, test as base, expect, type ElectronApplication, type Page } from "@playwright/test";
import { spawnSync, type ChildProcess } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { linkFixtureTranscripts, type HerdrFixture } from "../../web/e2e/herdr-fixture";
import { cleanupAfterFailure, ownUntilWorkerExit, throwFixtureFailures } from "../../web/e2e/worker-owned";
import { fixtureExecutable, fixtureHomeEnv, fixtureOpenCommand, fixtureToolPath, inheritedFixtureEnv } from "../../web/e2e/platform-fixture";
import { bootoutTestLabel, hcoordLabel } from "./launchd";
import { SHOW_INACTIVE_SWITCH } from "../src/main/launchSwitches";

export const DESKTOP_DIR = path.resolve(__dirname, "..");
export const REPO = path.resolve(DESKTOP_DIR, "..");
export const HIDE_CLI = path.join(REPO, "target", "debug", fixtureExecutable("hide"));
const isolations = new Map<string, { cleanup: () => void; candidates: Set<ChildProcess>; launching: number }>();
const MAX_ISOLATIONS = 16;
const MAX_CANDIDATES_PER_HOME = 16;

export type Isolated = {
  root: string;
  env: Record<string, string>;
  /** Runs a `hide` verb against this run's state directory. */
  hide: (args: string[]) => { status: number | null; stdout: string };
  daemonPid: () => number | null;
  cleanup: () => void;
};

/**
 * A Herdr fixture with its `root` also lends the daemon its `claude`, which
 * is the label provider the fixture's transcripts are answered by, and those
 * transcripts; without one the daemon finds whatever `claude` PATH has, on a
 * HOME where it is not logged in.
 */
export function isolate(herdr: Pick<HerdrFixture, "socket" | "bin"> & Partial<Pick<HerdrFixture, "root">>, label: string): Isolated {
  if (isolations.size >= MAX_ISOLATIONS) throw new Error(`desktop fixture has ${MAX_ISOLATIONS} unclosed homes; clean an owned fixture before creating another`);
  const root = fs.realpathSync.native(fs.mkdtempSync(path.join(os.tmpdir(), `hide-desktop-${label}-`)));
  const home = path.join(root, "home");
  let env: Record<string, string>;
  try {
    fs.mkdirSync(path.join(home, "projects"), { recursive: true });
    if (herdr.root) linkFixtureTranscripts({ root: herdr.root }, home);
    env = {
      ...inheritedFixtureEnv(),
      ...fixtureHomeEnv(home),
      HIDE_STATE_DIR: path.join(root, "state"),
      HIDE_DESKTOP_USER_DATA_DIR: path.join(root, "user-data"),
      HIDE_CLI_PATH: HIDE_CLI,
      HIDED_UI_DIR: path.join(REPO, "web", "dist"),
      HERDR_SOCKET_PATH: herdr.socket,
      HERDR_BIN_PATH: herdr.bin,
      PATH: fixtureToolPath(path.join(herdr.root ?? root, "bin")),
      // `open_external` must not launch a GUI application during a test.
      HIDE_OPEN_COMMAND: fixtureOpenCommand(undefined, root),
    };
  } catch (error) {
    cleanupAfterFailure(error, () => fs.rmSync(root, { recursive: true, force: true }));
  }
  const cleanupEnv = { ...env };
  const hide = (args: string[]) => {
    const run = spawnSync(HIDE_CLI, args, { env, encoding: "utf8", timeout: 20_000 });
    return { status: run.status, stdout: run.stdout, stderr: run.error?.message ?? run.stderr };
  };
  const daemonPid = () => {
    const answer = JSON.parse(hide(["status", "--json"]).stdout) as { running: boolean; pid?: number };
    return answer.running ? (answer.pid ?? null) : null;
  };
  const owner = { cleanup: () => {}, candidates: new Set<ChildProcess>(), launching: 0 };
  let cleaned = false;
  const cleanup = () => {
    if (cleaned) return;
    const live = [...owner.candidates].filter((child) => child.exitCode === null && child.signalCode === null);
    if (live.length || owner.launching) {
      const pids = live.map((child) => child.pid ?? "unavailable").join(", ");
      throw new Error(`fixture cleanup incomplete; preserve ${root}: candidate exit unconfirmed (PIDs: ${pids || "none"}, pending launches: ${owner.launching}); close only the recorded owned candidates, confirm their exit, then call cleanup() again`);
    }
    const errors: unknown[] = [];
    for (const state of [cleanupEnv.HIDE_STATE_DIR!, path.join(home, ".hide", "state"), path.join(home, ".local", "state", "hide")]) {
      if (!fs.existsSync(state)) continue;
      const stopped = spawnSync(HIDE_CLI, ["stop"], { env: { ...cleanupEnv, HIDE_STATE_DIR: state }, encoding: "utf8", timeout: 20_000 });
      if (stopped.error || stopped.status !== 0) errors.push(new Error(`private hided stop failed for ${state}: ${stopped.error?.message ?? (stopped.stderr || stopped.stdout || stopped.status)}`));
    }
    // The kit may adopt the legacy home into the default home. Both belong
    // to this fixture, regardless of environment changes a spec makes later.
    if (process.platform === "darwin") {
      for (const data of [path.join(home, ".hcoord"), path.join(home, ".hide", "hcoord")]) {
        try { bootoutTestLabel(hcoordLabel(data)); } catch (error) { errors.push(error); }
      }
    }
    if (errors.length) throwFixtureFailures(errors);
    fs.rmSync(root, { recursive: true, force: true });
    cleaned = true;
    isolations.delete(home);
    owned.disown();
  };
  const owned = ownUntilWorkerExit(() => {
    try { cleanup(); } catch (error) { process.exitCode = 1; throw error; }
  });
  owner.cleanup = cleanup;
  isolations.set(home, owner);
  return { root, env, hide, daemonPid, cleanup };
}

export function assertIsolated(env: Record<string, string>): void {
  // The Herdr fixture keeps its socket directly under /tmp for the path length limit.
  const roots = [fs.realpathSync.native(os.tmpdir()), ...(process.platform === "win32" ? [] : [fs.realpathSync.native("/tmp")])];
  const nativeHomeKeys = process.platform === "win32" ? ["USERPROFILE", "APPDATA", "LOCALAPPDATA"] : [];
  for (const key of ["HOME", "HCOORD_HOME", "HIDE_STATE_DIR", "HIDE_DESKTOP_USER_DATA_DIR", "HERDR_SOCKET_PATH", ...nativeHomeKeys]) {
    const value = env[key];
    const parent = value ? fs.realpathSync.native(path.dirname(value)) : null;
    if (!parent || !roots.some((root) => {
      const relative = path.relative(root, parent);
      return relative === "" || (!relative.startsWith(`..${path.sep}`) && relative !== ".." && !path.isAbsolute(relative));
    })) throw new Error(`${key} is not isolated under ${roots.join(" or ")}`);
  }
  if (env.HCOORD_HOME !== path.join(env.HOME!, ".hcoord")) throw new Error("HCOORD_HOME must name this fixture's private coordinator");
  if (process.platform === "win32" && (env.USERPROFILE !== env.HOME || env.APPDATA !== path.join(env.HOME!, "AppData", "Roaming") || env.LOCALAPPDATA !== path.join(env.HOME!, "AppData", "Local"))) throw new Error("Windows native home and AppData must belong to this fixture's HOME");
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
      const errors: unknown[] = [];
      try {
        await use();
      } catch (error) {
        errors.push(error);
      } finally {
        // Test failures and timeouts skip local finally blocks. The fixture
        // still closes each app; cleanup retains any home whose candidate
        // has not exited, even if close returned or threw.
        for (const { app, child } of focusReports.apps) {
          if (child.exitCode === null && child.signalCode === null) {
            try { await app.close(); } catch (error) { errors.push(error); }
          }
        }
        for (const { cleanup } of [...isolations.values()]) {
          try { cleanup(); } catch (error) { errors.push(error); }
        }
        const reports = fs.readdirSync(dir).flatMap((file) => fs.readFileSync(path.join(dir, file), "utf8").split("\n").filter(Boolean));
        focusReports = null;
        fs.rmSync(dir, { recursive: true, force: true });
        if (!testInfo.tags.includes(NEEDS_FOCUS)) {
          try { expect(reports, `the app came to the front in a test not tagged ${NEEDS_FOCUS}`).toEqual([]); } catch (error) { errors.push(error); }
        }
      }
      if (errors.length) throwFixtureFailures(errors);
    },
    { auto: true },
  ],
});

/** Starts the app with the focus guard preloaded, reporting into the running test's folder. */
async function start(appDir: string, env: Record<string, string>, executablePath?: string): Promise<ElectronApplication> {
  assertIsolated(env);
  if (!focusReports) throw new Error("launch the desktop app from a test imported from desktop/e2e/fixture.ts, so its focus guard runs");
  const owner = isolations.get(env.HOME!);
  if (!owner) throw new Error("launch the desktop app with the HOME of an unclosed isolate() fixture");
  for (const child of owner.candidates) {
    if (child.exitCode !== null || child.signalCode !== null) owner.candidates.delete(child);
  }
  if (owner.candidates.size + owner.launching >= MAX_CANDIDATES_PER_HOME) throw new Error(`desktop fixture has ${MAX_CANDIDATES_PER_HOME} live or launching candidates; close an owned candidate before launching another`);
  const report = path.join(focusReports.dir, `launch-${focusReports.apps.length + 1}.jsonl`);
  owner.launching++;
  try {
    const app = await electron.launch({ executablePath, args: ["-r", FOCUS_GUARD, ...(!executablePath ? [appDir] : []), ...BACKGROUND_SWITCHES, `--hide-e2e-focus-report=${report}`], cwd: appDir, env });
    // Taken now: `app.process()` throws once the app is closed. The home
    // retains this handle beyond test teardown if closing the app fails.
    const child = app.process();
    owner.candidates.add(child);
    focusReports.apps.push({ app, child });
    return app;
  } finally {
    owner.launching--;
  }
}

/** `appDir` is the app folder to run, the desktop package unless a test copies it. */
export async function launch(
  env: Record<string, string>,
  { appDir = DESKTOP_DIR, executablePath }: { appDir?: string; executablePath?: string } = {},
): Promise<{ app: ElectronApplication; page: Page }> {
  const app = await start(appDir, env, executablePath);
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
 * Sizes the first window to what a layout needs, within the primary work
 * area: a CI runner's screen is 1024 points wide and its usable height
 * differs by runner (681 on one, 700 or more on another), and macOS clamps a
 * window to the work area without saying so. The width is the layout's and
 * must be granted whole; the height is the work area's when that is shorter.
 * Returns the size macOS granted, so a spec asserts its layout against it.
 */
export async function fitWindow(app: ElectronApplication, wanted: { width: number; height: number }): Promise<{ width: number; height: number }> {
  const { granted, area } = await app.evaluate(({ BrowserWindow, screen }, size) => {
    const work = screen.getPrimaryDisplay().workArea;
    const window = BrowserWindow.getAllWindows()[0]!;
    window.setBounds({ x: work.x, y: work.y, width: size.width, height: Math.min(size.height, work.height) });
    const bounds = window.getBounds();
    return { granted: { width: bounds.width, height: bounds.height }, area: { width: work.width, height: work.height } };
  }, wanted);
  expect(granted.width, `the screen's work area is ${area.width} wide and this layout needs ${wanted.width}`).toBe(wanted.width);
  expect(granted.height, "macOS granted a different height than the work area allows").toBe(Math.min(wanted.height, area.height));
  return granted;
}

/**
 * A launch that returns once the host's own navigation from the status page
 * to the shell has landed, for a spec that reloads or navigates the window
 * itself. A spec navigation sent while the host's is in flight replaces it:
 * the host drops a superseded load as not a failure, so the window stays on
 * the status page with no socket, or the spec's reload is the one aborted.
 */
export async function launchShell(env: Record<string, string>): Promise<{ app: ElectronApplication; page: Page }> {
  const app = await relaunch(env);
  return { app, page: await shellPage(app) };
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
