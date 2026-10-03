// The desktop host: finds the daemon through the `hide` CLI, shows the web
// shell it serves in one window, and follows the daemon if it goes away.
//
//   connecting -> attached(url) | failed(reason)
//   attached   -> lost          two missed /health answers in a row
//   lost       -> attached      `hide status --json` names a live daemon
//   failed, lost -> connecting  Retry, a second launch, or a Dock click
//
// Only `connecting` may start a daemon (`hide connect`); `lost` only
// re-attaches. The daemon is never stopped here (desktop PRD D-03).

import fs from "node:fs";
import path from "node:path";
import { pathToFileURL } from "node:url";
import { app, BrowserWindow, dialog, ipcMain, screen, session, shell, type IpcMainEvent, type IpcMainInvokeEvent } from "electron";
import type { CommandId } from "../../../web/src/shortcuts";
import { BINDINGS_CHANNEL, COMMAND_CHANNEL, OPEN_PATH_CHANNEL, PICK_FOLDER_CHANNEL, PROBE_PATHS_CHANNEL, REVEAL_CHANNEL } from "../channel";
import { BrowserViews, type ResolvedPage } from "./browser";
import {
  HAS_LOGIN_SHELL,
  loginPathCommand,
  parseConnect,
  parseLoginPath,
  parseRememberedCli,
  parseStatus,
  REMEMBERED_SOURCES,
  rememberedCliPath,
  rememberedCliValue,
  resolveCli,
  wellKnownDirs,
  type Attached,
  type CliSource,
  type FailureReason,
} from "./cli";
import type { DesktopEnv } from "./env";
import { readJsonFile, writeJsonFile } from "./jsonFile";
import { chooseHerdr, ensureServer, parseServerStatus, serverEnvironment, type HerdrChoice } from "./herdr";
import { loadFailureFields, type HostLog } from "./log";
import { ChildRunner, startDetached, type ChildResult } from "./spawn";
import { MIN_SIZE, readWindowState, restoreBounds, windowStatePath, writeWindowState } from "./windowState";
import { openRoute, probe, probeRequest } from "./localPath";
import { revealTarget } from "./reveal";
import { fromPage, toPage, WirePathError } from "./wirePath";

declare const __HIDE_BACKGROUND__: string;

const CONNECT_TIMEOUT_MS = 25_000;
const STATUS_TIMEOUT_MS = 5_000;
/** VS Code's default for the same question (`application.shellEnvironmentResolutionTimeout`); a warm rc takes seconds. */
const LOGIN_PATH_TIMEOUT_MS = 10_000;
const HEALTH_INTERVAL_MS = 2_000;
const HEALTH_TIMEOUT_MS = 1_500;
const LOST_AFTER_MISSES = 2;
const LOST_POLL_MS = 3_000;
/** A started Herdr answered within 0.1 s in a 2026-09-29 run; restoring many panes can take longer. */
const HERDR_START_WAIT_MS = 5_000;
const HERDR_START_POLL_MS = 200;
const STATUS_PAGE = path.join(__dirname, "static", "status.html");
const STATUS_PAGE_URL = pathToFileURL(STATUS_PAGE).href;
const CLIPBOARD_PERMISSIONS = new Set(["clipboard-read", "clipboard-sanitized-write"]);
const EXTERNAL_PROTOCOLS = new Set(["http:", "https:", "mailto:"]);

export type HostState =
  | { kind: "connecting" }
  | ({ kind: "attached" } & Attached)
  | ({ kind: "lost" } & Attached)
  | { kind: "failed"; reason: FailureReason };

function isExecutable(file: string): boolean {
  try {
    fs.accessSync(file, fs.constants.X_OK);
    return fs.statSync(file).isFile();
  } catch {
    return false;
  }
}

function mtimeMs(file: string): number | null {
  try {
    return fs.statSync(file).mtimeMs;
  } catch {
    return null;
  }
}

export class DesktopHost {
  private state: HostState = { kind: "connecting" };
  private window: BrowserWindow | null = null;
  /**
   * The status page load still in flight. The window takes one navigation at
   * a time, so every render waits for it: a shell load started while the
   * status page is still navigating (a daemon that answers at once, as on a
   * relaunch) lands but is rejected with an empty code and leaves the
   * renderer's navigation tracking pending, and a hash set before the page
   * commits is lost. Electron settles the load when its window is destroyed.
   */
  private statusLoad: Promise<unknown> | null = null;
  /** Whether a render already waits for `statusLoad`; it renders whatever state is current then. */
  private renderWaits = false;
  private readonly runner = new ChildRunner();
  /** Every CLI child runs through this chain, so the runner's cap of one is never crossed. */
  private cliChain: Promise<unknown> = Promise.resolve();
  private discovering: Promise<void> | null = null;
  private attempts = 0;
  private cli: { path: string; source: CliSource } | null = null;
  /** The login shell's PATH once it answered; a shell that failed or timed out is asked again on the next search. */
  private loginPath: string | null = null;
  private watch: NodeJS.Timeout | null = null;
  private quitting = false;
  /** The pages of browser displays; made once the app is ready, since a session needs it. */
  private browsers: BrowserViews | null = null;

  constructor(
    private readonly env: DesktopEnv,
    private readonly log: HostLog,
    /** `SHOW_INACTIVE_SWITCH` was passed: no window ever activates the app. */
    private readonly showInactive: boolean,
  ) {}

  // --- lifecycle ------------------------------------------------------------

  start(): void {
    this.guardSession();
    this.browsers = new BrowserViews(
      this.log,
      (event) => this.fromShell(event),
      (workspace, id, load) => this.resolveBrowserRoute(workspace, id, load),
      (workspace, id, load) => this.releaseBrowserRoute(workspace, id, load),
      // A popup that may not take the keyboard is ordered in without
      // activating the app, behind the operator's windows under the test switch.
      (window, focus) => (focus || this.showInactive ? this.present(window, focus) : window.showInactive()),
    );
    this.listenReveal();
    this.listenPickFolder();
    this.listenLocalPaths();
    this.openWindow();
    void this.discover("launch");
  }

  /** A second launch or a Dock click: bring the window back, and look again when nothing is attached. */
  reopen(trigger: "second-instance" | "activate"): void {
    this.log.event("host.reopen", { trigger, state: this.state.kind, had_window: this.window !== null });
    if (!this.window) this.openWindow();
    else {
      if (this.window.isMinimized()) this.window.restore();
      this.present(this.window, true);
    }
    if (this.state.kind === "failed" || this.state.kind === "lost") void this.discover(trigger);
  }

  quit(): void {
    this.quitting = true;
    this.stopWatch();
    this.runner.stop();
    this.log.event("host.quit", { state: this.state.kind });
  }

  /**
   * The stored macOS pane chords the shell reports, for the menu. Only this
   * window's page on the daemon origin is heard; anything else is logged and
   * dropped.
   */
  setBrowserRegistry(registry: readonly import("../../../web/src/shortcuts").Command[]): void {
    this.browsers?.setRegistry(registry);
  }

  listenBindings(apply: (reported: unknown) => void): void {
    ipcMain.on(BINDINGS_CHANNEL, (event: IpcMainEvent, reported: unknown) => {
      if (!this.fromShell(event)) {
        this.log.event("bindings.refused", { reason: "sender" });
        return;
      }
      apply(reported);
    });
  }

  /**
   * A menu's `reveal_external` (issue 324) on an Explorer, History, View tab
   * or sidebar row: the OS file manager selects the file or folder in its
   * parent folder and opens nothing, so no program on this computer starts
   * from it. Only this window's page on the daemon origin is heard, and only
   * an absolute path that exists; the log records the outcome, never the path.
   */
  private listenReveal(): void {
    ipcMain.on(REVEAL_CHANNEL, (event: IpcMainEvent, reported: unknown) => {
      if (!this.fromShell(event)) {
        this.log.event("reveal.refused", { reason: "sender" });
        return;
      }
      void revealTarget(reported).then((target) => {
        if ("refused" in target) {
          this.log.event("reveal.refused", { reason: target.refused });
          return;
        }
        shell.showItemInFolder(target.path);
        this.log.event("reveal.shown", { kind: target.kind });
      }).catch(() => this.log.event("reveal.failed", {}));
    });
  }

  /**
   * Add a project's Browse folder: the system's own folder picker, a sheet on
   * this window on macOS, which can also make a new folder. Only this window's
   * page on the daemon origin is answered; a refused sender and a cancelled
   * pick both answer null, the chosen folder is answered in the wire spelling
   * (`wirePath.ts`) or refused with the reason when it has none, and hided
   * judges it like any other path. The log records the outcome, never the path.
   */
  private listenPickFolder(): void {
    ipcMain.handle(PICK_FOLDER_CHANNEL, async (event: IpcMainInvokeEvent) => {
      if (!this.fromShell(event) || !this.window) {
        this.log.event("pick_folder.refused", { reason: "sender" });
        return null;
      }
      const picked = await dialog.showOpenDialog(this.window, { properties: ["openDirectory", "createDirectory"] });
      const folder = picked.canceled ? null : (picked.filePaths[0] ?? null);
      if (folder === null) {
        this.log.event("pick_folder.answered", { picked: false });
        return null;
      }
      try {
        const wire = toPage(folder);
        this.log.event("pick_folder.answered", { picked: true });
        return wire;
      } catch (error) {
        if (!(error instanceof WirePathError)) throw error;
        // The page's diagnostic says the picker failed, with the reason and not the path.
        this.log.event("pick_folder.refused", { reason: error.reason });
        throw error;
      }
    });
  }

  /**
   * Terminal links on this computer: a probe answers which of the named paths
   * exist and what they are, and an open hands one to the system, in its
   * default application or as a folder window, or revealed in the file manager
   * when opening would run it (`localPath.ts`). The page names and is
   * answered paths in the wire spelling (`wirePath.ts`). Only this window's
   * page on the daemon origin is heard; a probe is not logged, since it runs
   * on hover, and an open logs its route and never the path.
   */
  private listenLocalPaths(): void {
    ipcMain.handle(PROBE_PATHS_CHANNEL, async (event: IpcMainInvokeEvent, reported: unknown) => {
      if (!this.fromShell(event)) {
        this.log.event("probe_paths.refused", { reason: "sender" });
        return [];
      }
      const paths = probeRequest(reported);
      if (paths === null) {
        this.log.event("probe_paths.refused", { reason: "paths" });
        return [];
      }
      return probe(paths);
    });
    ipcMain.on(OPEN_PATH_CHANNEL, (event: IpcMainEvent, reported: unknown) => {
      if (!this.fromShell(event)) {
        this.log.event("open_path.refused", { reason: "sender" });
        return;
      }
      const target = fromPage(reported);
      if (target === null) {
        this.log.event("open_path.refused", { reason: "path" });
        return;
      }
      void openRoute(target).then(async (route) => {
        if (route.action === "refuse") {
          this.log.event("open_path.refused", { reason: route.reason });
          return;
        }
        if (route.action === "reveal") shell.showItemInFolder(target);
        else {
          // Judged a moment ago; a link swapped in since is refused, not followed.
          if ((await fs.promises.realpath(target).catch(() => null)) !== target) {
            this.log.event("open_path.refused", { reason: "not_physical" });
            return;
          }
          const failure = await shell.openPath(target);
          if (failure) {
            this.log.event("open_path.failed", { kind: route.kind });
            return;
          }
        }
        this.log.event(`open_path.${route.action}`, { kind: route.kind, reason: route.reason });
      }).catch(() => this.log.event("open_path.failed", {}));
    });
  }

  /** An app-menu click, delivered to the shell only while it is loaded. */
  sendCommand(id: CommandId): void {
    // A page's popup window holding the keyboard answers the command itself,
    // whatever the shell's state.
    if (this.browsers?.popupCommand(id)) return;
    if (!this.window || (this.state.kind !== "attached" && this.state.kind !== "lost")) return;
    // Text size in a page is the page's zoom, and the page keeps the keyboard.
    if (this.browsers?.zoomFocused(id)) return;
    // A chord the page of a browser display left unhandled reaches the menu;
    // the command's surface (the palette, a dialog) needs the keyboard.
    if (this.browsers?.hasFocus()) this.window.webContents.focus();
    this.window.webContents.send(COMMAND_CHANNEL, id);
  }

  // --- discovery ------------------------------------------------------------

  discover(trigger: string): Promise<void> {
    this.discovering ??= this.runDiscovery(trigger).finally(() => {
      this.discovering = null;
    });
    return this.discovering;
  }

  private async runDiscovery(trigger: string): Promise<void> {
    const attempt = ++this.attempts;
    this.stopWatch();
    this.setState({ kind: "connecting" });
    const herdr = this.herdr();
    this.log.event("discovery.start", { attempt, trigger, herdr: herdr.source, replaced_pane_herdr: herdr.replacedPaneValue ?? undefined });
    const cli = await this.findCli(attempt);
    if (!cli) return this.fail(attempt, "cli_missing", "no executable hide CLI");
    await this.ensureHerdrServer(attempt);
    if (this.quitting) return;
    const answer = parseConnect(await this.runCli(cli.path, ["connect"], CONNECT_TIMEOUT_MS));
    if (this.quitting) return;
    if (answer.kind === "failed") return this.fail(attempt, answer.reason, answer.detail);
    this.log.event("discovery.attached", { attempt, port: answer.port, pid: answer.pid });
    this.remember(cli);
    this.attach(answer);
  }

  private async findCli(attempt: number): Promise<{ path: string; source: CliSource } | null> {
    const stored = parseRememberedCli(readJsonFile(rememberedCliPath(app.getPath("userData"))));
    if (stored !== null && typeof stored === "object") this.log.event("cli.remembered_unreadable", { attempt, detail: stored.unreadable });
    const resolved = await resolveCli(
      {
        override: this.env.cliPath,
        worktreeRoot: app.isPackaged ? null : path.resolve(app.getAppPath(), ".."),
        bundledDir: this.bundledDir(),
        searchPath: this.env.path,
        remembered: typeof stored === "string" ? stored : null,
        // A packaged app opened from Finder has launchd's PATH, not the operator's.
        loginPath: app.isPackaged && HAS_LOGIN_SHELL ? () => this.readLoginPath(attempt) : null,
        home: this.env.home,
      },
      { isExecutable, mtimeMs },
    );
    this.log.event(resolved.found ? "cli.resolved" : "cli.missing", {
      attempt,
      source: resolved.found?.source,
      path: resolved.found?.path,
      tried: resolved.tried.join(path.delimiter),
    });
    this.cli = resolved.found;
    return this.cli;
  }

  private async readLoginPath(attempt: number): Promise<string | null> {
    if (this.loginPath) return this.loginPath;
    const started = Date.now();
    const command = loginPathCommand(this.env.shell);
    const result = await this.runCli(command.file, command.args, LOGIN_PATH_TIMEOUT_MS);
    this.loginPath = parseLoginPath(result.stdout);
    this.log.event("cli.login_path", {
      attempt,
      ok: this.loginPath !== null,
      code: result.code,
      timed_out: result.timedOut,
      elapsed_ms: Date.now() - started,
    });
    return this.loginPath;
  }

  /** The CLI that just attached, kept for the next launch so it need not ask the login shell again. */
  private remember(cli: { path: string; source: CliSource }): void {
    if (!REMEMBERED_SOURCES.has(cli.source)) return;
    try {
      writeJsonFile(rememberedCliPath(app.getPath("userData")), rememberedCliValue(cli.path));
      this.log.event("cli.remembered", { source: cli.source, path: cli.path });
    } catch (error) {
      this.log.event("cli.remember_failed", { detail: String(error) });
    }
  }

  /** The packaged app's `Contents/Resources`, where `hide`, `hided` and `herdr` ship side by side. */
  private bundledDir(): string | null {
    return app.isPackaged ? process.resourcesPath : null;
  }

  /** The Herdr a daemon started from here attaches pane terminals with; `herdr.ts` owns the rule. */
  private herdr(): HerdrChoice {
    return chooseHerdr({ bundledDir: this.bundledDir(), herdrBinPath: this.env.herdrBinPath, herdrPaneId: this.env.herdrPaneId });
  }

  private childEnvironment(): Record<string, string | undefined> {
    const herdr = this.herdr().path;
    // Finder supplies only launchd's system PATH. The CLI and any daemon it
    // starts need the same standard user install dirs we search for `hide`.
    const inheritedPath = this.env.path || ["/usr/bin", "/bin", "/usr/sbin", "/sbin"].join(path.delimiter);
    const searchPath = [...new Set([...inheritedPath.split(path.delimiter), ...wellKnownDirs(this.env.home)].filter(Boolean))].join(path.delimiter);
    return { ...this.env.inherited, PATH: searchPath, ...(herdr ? { HERDR_BIN_PATH: herdr } : {}) };
  }

  /**
   * Starts the Herdr this app bundles when no server answers on the socket
   * its children would use, as `herdr` does when a terminal runs it: after a
   * reboot nothing else starts one, and hided only reads a missing server as
   * absent. A server that answers is never touched, and neither an unpackaged
   * host nor an explicit HERDR_BIN_PATH override starts anything, so a
   * development or e2e run keeps the server it made. Every failure goes to the
   * log and the connect goes ahead, where the shell shows Herdr unreachable.
   */
  private async ensureHerdrServer(attempt: number): Promise<void> {
    const herdr = this.herdr();
    if (herdr.source !== "bundled" || herdr.path === null) return;
    const bin = herdr.path;
    const env = serverEnvironment(this.childEnvironment());
    const result = await ensureServer({
      status: (timeoutMs) => this.runChild(bin, ["status", "server", "--json"], timeoutMs, env).then(parseServerStatus),
      start: () => startDetached(bin, ["server"], env),
      sleep: (ms) => new Promise((resolve) => setTimeout(resolve, ms)),
      now: Date.now,
      stopped: () => this.quitting,
      statusTimeoutMs: STATUS_TIMEOUT_MS,
      waitMs: HERDR_START_WAIT_MS,
      pollMs: HERDR_START_POLL_MS,
    });
    if (result.outcome === "status_failed") this.log.event("herdr.status_failed", { attempt, detail: result.detail });
    else if (result.outcome === "started") this.log.event("herdr.server_started", { attempt, pid: result.pid, elapsed_ms: result.elapsedMs });
    else if (result.outcome === "start_failed") this.log.event("herdr.server_start_failed", { attempt, pid: result.pid, detail: result.detail, elapsed_ms: result.elapsedMs });
  }

  private runChild(file: string, args: readonly string[], timeoutMs: number, env: Record<string, string | undefined>): Promise<ChildResult> {
    const run = this.cliChain.then(() =>
      this.quitting
        ? { code: null, signal: null, stdout: "", stderr: "", timedOut: false, spawnError: "host is quitting" }
        : this.runner.run(file, args, timeoutMs, env),
    );
    this.cliChain = run.catch(() => undefined);
    return run;
  }

  private runCli(file: string, args: readonly string[], timeoutMs: number): Promise<ChildResult> {
    // A call queued behind the one quit killed never starts; it answers as a
    // child that could not start, which every caller already reads as a stop.
    return this.runChild(file, args, timeoutMs, this.childEnvironment());
  }

  private fail(attempt: number, reason: FailureReason, detail: string): void {
    if (this.quitting) return;
    this.log.event("discovery.failed", { attempt, reason, detail });
    this.setState({ kind: "failed", reason });
  }

  private attach(daemon: Attached): void {
    this.setState({ kind: "attached", ...daemon });
    this.startHealthWatch();
  }

  // --- following the daemon (B4) ---------------------------------------------

  private stopWatch(): void {
    if (this.watch) clearTimeout(this.watch);
    this.watch = null;
  }

  private startHealthWatch(): void {
    let misses = 0;
    const tick = async () => {
      const current = this.state;
      if (current.kind !== "attached" || this.quitting) return;
      let healthy = false;
      try {
        healthy = (await fetch(`${current.origin}/health`, { signal: AbortSignal.timeout(HEALTH_TIMEOUT_MS) })).ok;
      } catch {
        healthy = false;
      }
      if (this.state !== current) return;
      misses = healthy ? 0 : misses + 1;
      if (misses >= LOST_AFTER_MISSES) {
        this.log.event("daemon.lost", { port: current.port, pid: current.pid });
        // The shell keeps its own disconnected state on screen; nothing is reloaded here.
        this.state = { ...current, kind: "lost" };
        this.startLostPoll();
        return;
      }
      this.watch = setTimeout(tick, HEALTH_INTERVAL_MS);
    };
    this.stopWatch();
    this.watch = setTimeout(tick, HEALTH_INTERVAL_MS);
  }

  private startLostPoll(): void {
    const tick = async () => {
      const lost = this.state;
      if (lost.kind !== "lost" || this.quitting || !this.cli) return;
      const status = parseStatus(await this.runCli(this.cli.path, ["status", "--json"], STATUS_TIMEOUT_MS));
      if (this.state !== lost) return;
      if (status?.running) {
        this.log.event("daemon.found", { port: status.port, pid: status.pid, same_url: status.url === lost.url });
        if (status.url === lost.url) {
          // The same daemon answered again; the shell reconnects on its own.
          this.state = { ...lost, kind: "attached" };
          this.startHealthWatch();
        } else {
          this.attach(status);
        }
        return;
      }
      this.watch = setTimeout(tick, LOST_POLL_MS);
    };
    this.stopWatch();
    this.watch = setTimeout(tick, LOST_POLL_MS);
  }

  // --- the window -------------------------------------------------------------

  private setState(next: HostState): void {
    this.state = next;
    this.render();
  }

  private render(): void {
    const window = this.window;
    if (!window) return;
    if (this.statusLoad) {
      // A local file, so this waits a few milliseconds.
      if (!this.renderWaits) {
        this.renderWaits = true;
        const rerender = () => {
          this.renderWaits = false;
          this.render();
        };
        this.statusLoad.then(rerender, rerender);
      }
      return;
    }
    const state = this.state;
    if (state.kind === "attached" || state.kind === "lost") {
      this.load(window.loadURL(state.url), state.kind);
      return;
    }
    const hash = state.kind === "failed" ? `failed=${state.reason}` : "connecting";
    if (window.webContents.getURL().startsWith(STATUS_PAGE_URL)) {
      // Already on the status page: only its hash moves, which it follows itself.
      this.load(window.webContents.executeJavaScript(`location.hash = ${JSON.stringify(hash)}`), state.kind);
      return;
    }
    const loading = window.loadFile(STATUS_PAGE, { hash });
    this.statusLoad = loading;
    const settled = () => {
      if (this.statusLoad === loading) this.statusLoad = null;
    };
    loading.then(settled, settled);
    this.load(loading, state.kind);
  }

  private load(pending: Promise<unknown>, state: HostState["kind"]): void {
    pending.catch((error: unknown) => {
      // A load a newer one replaced is not a failure.
      if ((error as { code?: string }).code === "ERR_ABORTED") return;
      this.log.event("window.load_failed", { state, ...loadFailureFields(error) });
    });
  }

  /**
   * Shows the window, and with `focus` gives it the keyboard. `show()` and
   * `focus()` activate the app, which takes the screen and the keyboard from
   * whatever the operator is using. Under `SHOW_INACTIVE_SWITCH` the window
   * is ordered in without activating the app (`showInactive()` alone would
   * still put it above every other window), then sent behind the operator's
   * windows (`blur()` orders it to the back on macOS).
   */
  private present(window: BrowserWindow, focus: boolean): void {
    if (this.showInactive) {
      window.showInactive();
      window.blur();
      return;
    }
    window.show();
    if (focus) window.focus();
  }

  private openWindow(): void {
    const file = windowStatePath(app.getPath("userData"));
    const displays = screen.getAllDisplays().map((display) => display.workArea);
    const restored = restoreBounds(readWindowState(file), displays, screen.getPrimaryDisplay().workArea);
    this.log.event("window.bounds", { source: restored.source, why: restored.why });
    const window = new BrowserWindow({
      ...restored.bounds,
      minWidth: MIN_SIZE.width,
      minHeight: MIN_SIZE.height,
      show: false,
      title: "hide",
      backgroundColor: __HIDE_BACKGROUND__,
      webPreferences: {
        preload: path.join(__dirname, "preload.js"),
        sandbox: true,
        contextIsolation: true,
        nodeIntegration: false,
        webSecurity: true,
        spellcheck: false,
      },
    });
    this.window = window;
    this.browsers?.attach(window);
    window.once("ready-to-show", () => this.present(window, false));
    window.on("close", () => {
      try {
        writeWindowState(file, window.getNormalBounds());
      } catch (error) {
        this.log.event("window.state_write_failed", { detail: String(error) });
      }
    });
    window.on("closed", () => {
      if (this.window === window) this.window = null;
    });
    this.guardNavigation(window);
    if (this.state.kind === "lost") {
      // A fresh page cannot load a daemon that is gone; show the search instead.
      void this.discover("window");
      return;
    }
    this.render();
  }

  // --- the security boundary (B10, B11) ----------------------------------------

  private daemonOrigin(): string | null {
    return this.state.kind === "attached" || this.state.kind === "lost" ? this.state.origin : null;
  }

  private browserRouteRequest(workspace: string, id: string, load: number): { current: Attached; token: string; body: string } {
    const at = workspace.indexOf("\u0000");
    if (at <= 0 || at === workspace.length - 1 || !id || !Number.isSafeInteger(load)) throw new Error("Invalid Browser Workspace");
    const current = this.state;
    if (current.kind !== "attached") throw new Error("Hide is disconnected");
    const token = new URLSearchParams(new URL(current.url).hash.slice(1)).get("token");
    if (!token) throw new Error("Hide credential is unavailable");
    return { current, token, body: JSON.stringify({ device_id: workspace.slice(0, at), checkout_path: workspace.slice(at + 1), id, load, owner_pid: process.pid }) };
  }

  private async resolveBrowserRoute(workspace: string, id: string, load: number): Promise<ResolvedPage> {
    const { current, token, body } = this.browserRouteRequest(workspace, id, load);
    const answer = await fetch(`${current.origin}/browser-route`, {
      method: "POST", headers: { Authorization: `Bearer ${token}`, "Content-Type": "application/json" },
      body, signal: AbortSignal.timeout(20_000),
    });
    const result: unknown = await answer.json();
    if (!answer.ok) {
      const reason = result && typeof result === "object" && "reason" in result ? String(result.reason) : `HTTP ${answer.status}`;
      throw new Error(reason);
    }
    if (!result || typeof result !== "object" || !("url" in result) || !("source_url" in result) || !("load" in result)
      || typeof result.url !== "string" || typeof result.source_url !== "string" || result.load !== load) throw new Error("Invalid Browser route answer");
    return result as ResolvedPage;
  }

  private releaseBrowserRoute(workspace: string, id: string, load: number): void {
    let request: ReturnType<DesktopHost["browserRouteRequest"]>;
    try { request = this.browserRouteRequest(workspace, id, load); }
    catch { return; }
    const { current, token, body } = request;
    void fetch(`${current.origin}/browser-route`, {
      method: "DELETE", headers: { Authorization: `Bearer ${token}`, "Content-Type": "application/json" },
      body, signal: AbortSignal.timeout(3_000),
    }).then((answer) => {
      if (!answer.ok) this.log.event("browser.route_release_failed", { status: answer.status });
    }).catch((error: unknown) => this.log.event("browser.route_release_failed", { detail: String(error) }));
  }

  /** An IPC message from the shell the daemon serves in this window's own frame; the status page and every browser page are refused. */
  private fromShell(event: IpcMainEvent | IpcMainInvokeEvent): boolean {
    const frame = event.senderFrame;
    return this.window !== null && event.sender === this.window.webContents && frame !== null && frame.parent === null && this.isDaemonUrl(frame.url);
  }

  private isDaemonUrl(url: string): boolean {
    try {
      return new URL(url).origin === this.daemonOrigin();
    } catch {
      return false;
    }
  }

  private openExternal(url: string): void {
    let parsed: URL;
    try {
      parsed = new URL(url);
    } catch {
      this.log.event("link.refused", { reason: "unparsable" });
      return;
    }
    if (!EXTERNAL_PROTOCOLS.has(parsed.protocol)) {
      this.log.event("link.refused", { protocol: parsed.protocol });
      return;
    }
    this.log.event("link.external", { protocol: parsed.protocol, host: parsed.host });
    shell.openExternal(url).catch((error: unknown) => this.log.event("link.open_failed", { detail: String(error) }));
  }

  private guardNavigation(window: BrowserWindow): void {
    const contents = window.webContents;
    contents.on("will-navigate", (event, url) => {
      if (this.isDaemonUrl(url)) return;
      event.preventDefault();
      this.openExternal(url);
    });
    contents.on("will-redirect", (event, url) => {
      if (this.isDaemonUrl(url)) return;
      event.preventDefault();
      this.log.event("navigation.redirect_refused", {});
    });
    contents.setWindowOpenHandler(({ url }) => {
      if (!this.isDaemonUrl(url)) this.openExternal(url);
      else this.log.event("window_open.refused", { reason: "daemon_origin" });
      return { action: "deny" };
    });
    // The status page asks for Retry by moving its own hash; it has no bridge.
    contents.on("did-navigate-in-page", (_event, url) => {
      if (url.startsWith("file:") && new URL(url).hash === "#retry") void this.discover("retry");
    });
  }

  private guardSession(): void {
    session.defaultSession.setPermissionRequestHandler((_contents, permission, callback, details) => {
      callback(this.isDaemonUrl(details.requestingUrl) && CLIPBOARD_PERMISSIONS.has(permission));
    });
    session.defaultSession.setPermissionCheckHandler((_contents, permission, origin) => {
      return origin === this.daemonOrigin() && CLIPBOARD_PERMISSIONS.has(permission);
    });
  }
}
