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
import { app, BrowserWindow, ipcMain, screen, session, shell, type IpcMainEvent, type IpcMainInvokeEvent } from "electron";
import type { CommandId } from "../../../web/src/shortcuts";
import { BINDINGS_CHANNEL, COMMAND_CHANNEL } from "../channel";
import { BrowserViews } from "./browser";
import {
  LOGIN_PATH_ARGS,
  parseConnect,
  parseLoginPath,
  parseStatus,
  resolveCli,
  type Attached,
  type FailureReason,
} from "./cli";
import type { DesktopEnv } from "./env";
import { loadFailureFields, type HostLog } from "./log";
import { ChildRunner, type ChildResult } from "./spawn";
import { MIN_SIZE, readWindowState, restoreBounds, windowStatePath, writeWindowState } from "./windowState";

declare const __HIDE_BACKGROUND__: string;

const CONNECT_TIMEOUT_MS = 15_000;
const STATUS_TIMEOUT_MS = 5_000;
const LOGIN_PATH_TIMEOUT_MS = 5_000;
const HEALTH_INTERVAL_MS = 2_000;
const HEALTH_TIMEOUT_MS = 1_500;
const LOST_AFTER_MISSES = 2;
const LOST_POLL_MS = 3_000;
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
  private readonly runner = new ChildRunner();
  /** Every CLI child runs through this chain, so the runner's cap of one is never crossed. */
  private cliChain: Promise<unknown> = Promise.resolve();
  private discovering: Promise<void> | null = null;
  private attempts = 0;
  private cli: string | null = null;
  private loginPath: string | null | undefined = undefined;
  private watch: NodeJS.Timeout | null = null;
  private quitting = false;
  /** The pages of browser displays; made once the app is ready, since a session needs it. */
  private browsers: BrowserViews | null = null;

  constructor(
    private readonly env: DesktopEnv,
    private readonly log: HostLog,
  ) {}

  // --- lifecycle ------------------------------------------------------------

  start(): void {
    this.guardSession();
    this.browsers = new BrowserViews(this.log, (event) => this.fromShell(event));
    this.openWindow();
    void this.discover("launch");
  }

  /** A second launch or a Dock click: bring the window back, and look again when nothing is attached. */
  reopen(trigger: "second-instance" | "activate"): void {
    this.log.event("host.reopen", { trigger, state: this.state.kind, had_window: this.window !== null });
    if (!this.window) this.openWindow();
    else {
      if (this.window.isMinimized()) this.window.restore();
      this.window.show();
      this.window.focus();
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
  listenBindings(apply: (reported: unknown) => void): void {
    ipcMain.on(BINDINGS_CHANNEL, (event: IpcMainEvent, reported: unknown) => {
      if (!this.fromShell(event)) {
        this.log.event("bindings.refused", { reason: "sender" });
        return;
      }
      apply(reported);
    });
  }

  /** An app-menu click, delivered to the shell only while it is loaded. */
  sendCommand(id: CommandId): void {
    if (!this.window || (this.state.kind !== "attached" && this.state.kind !== "lost")) return;
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
    this.log.event("discovery.start", { attempt, trigger });
    const cli = await this.findCli(attempt);
    if (!cli) return this.fail(attempt, "cli_missing", "no executable hide CLI");
    const answer = parseConnect(await this.runCli(cli, ["connect"], CONNECT_TIMEOUT_MS));
    if (this.quitting) return;
    if (answer.kind === "failed") return this.fail(attempt, answer.reason, answer.detail);
    this.log.event("discovery.attached", { attempt, port: answer.port, pid: answer.pid });
    this.attach(answer);
  }

  private async findCli(attempt: number): Promise<string | null> {
    let searchPath = this.env.path;
    if (app.isPackaged && !this.env.cliPath) {
      // Finder starts an app with launchd's PATH, not the operator's.
      if (this.loginPath === undefined) {
        const result = await this.runCli(this.env.shell, LOGIN_PATH_ARGS, LOGIN_PATH_TIMEOUT_MS);
        this.loginPath = parseLoginPath(result.stdout);
        if (!this.loginPath) this.log.event("cli.login_path_unavailable", { attempt, code: result.code, timed_out: result.timedOut });
      }
      if (this.loginPath) searchPath = `${this.loginPath}:${searchPath}`;
    }
    const resolved = resolveCli(
      { override: this.env.cliPath, worktreeRoot: app.isPackaged ? null : path.resolve(app.getAppPath(), ".."), searchPath },
      { isExecutable, mtimeMs },
    );
    this.log.event(resolved.found ? "cli.resolved" : "cli.missing", {
      attempt,
      source: resolved.found?.source,
      path: resolved.found?.path,
      tried: resolved.tried.join(":"),
    });
    this.cli = resolved.found?.path ?? null;
    return this.cli;
  }

  private runCli(file: string, args: readonly string[], timeoutMs: number): Promise<ChildResult> {
    // A call queued behind the one quit killed never starts; it answers as a
    // child that could not start, which every caller already reads as a stop.
    const run = this.cliChain.then(() =>
      this.quitting
        ? { code: null, signal: null, stdout: "", stderr: "", timedOut: false, spawnError: "host is quitting" }
        : this.runner.run(file, args, timeoutMs),
    );
    this.cliChain = run.catch(() => undefined);
    return run;
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
      const status = parseStatus(await this.runCli(this.cli, ["status", "--json"], STATUS_TIMEOUT_MS));
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
    this.load(window.loadFile(STATUS_PAGE, { hash }), state.kind);
  }

  private load(pending: Promise<unknown>, state: HostState["kind"]): void {
    pending.catch((error: unknown) => {
      // A load a newer one replaced is not a failure.
      if ((error as { code?: string }).code === "ERR_ABORTED") return;
      this.log.event("window.load_failed", { state, ...loadFailureFields(error) });
    });
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
    window.once("ready-to-show", () => window.show());
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
