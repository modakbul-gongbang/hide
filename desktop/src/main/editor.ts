// Open in VS Code (PRD sidebar-titlebar-refresh D-13, D-14, B15, B16): a
// checkout's or plain folder's menu hands its folder to VS Code on this
// computer. The host finds VS Code once per launch, and the shell offers the
// item only when it was found. An open that fails (the app or the folder went
// away) shows nothing on screen; the log records why, never the path.
//
// Finding and opening each run at most one short child through `spawn.ts`,
// with a hard timeout, except Windows, whose `Code.exe` is handed the folder
// detached: it is the operator's editor window, not a child of this app.

import fs from "node:fs";
import path from "node:path";
import type { HostLog } from "./log";
import { ChildRunner, startDetached, type ChildResult } from "./spawn";
import { fromPage } from "./wirePath";

/** VS Code's bundle identifier; `open -b` finds the app wherever macOS registered it. */
export const VSCODE_BUNDLE_ID = "com.microsoft.VSCode";
/** Spotlight answers in well under a second on a warm index. */
export const EDITOR_PROBE_TIMEOUT_MS = 3_000;
/** `open -b` and the Linux `code` launcher hand the folder over and exit. */
export const EDITOR_OPEN_TIMEOUT_MS = 10_000;

/** How the folder is handed to VS Code: `file` with `args` and the folder last, waited for or detached. */
export type EditorLaunch = { file: string; args: string[]; detached: boolean; via: "bundle" | "spotlight" | "path" };

export type EditorProbeDeps = {
  platform: NodeJS.Platform;
  home: string;
  /** The folders the host's children search, in order. */
  searchPath: readonly string[];
  exists(file: string): boolean;
  isExecutable(file: string): boolean;
  run(file: string, args: readonly string[], timeoutMs: number): Promise<ChildResult>;
};

const MAC_APP = "Visual Studio Code.app";

/**
 * Where VS Code is on this computer, or null with why. macOS: the app in
 * either Applications folder, else wherever Spotlight registered its bundle
 * id. Linux: the `code` launcher on the search path. Windows: `code.cmd` on
 * the search path with `Code.exe` one folder up, as its installer lays out.
 */
export async function findEditor(deps: EditorProbeDeps): Promise<EditorLaunch | { missing: string }> {
  // The probe is spelled for the system it describes, not the one running it.
  const { join, resolve } = deps.platform === "win32" ? path.win32 : path.posix;
  if (deps.platform === "darwin") {
    const open = { file: "/usr/bin/open", args: ["-b", VSCODE_BUNDLE_ID], detached: false };
    if ([join("/Applications", MAC_APP), join(deps.home, "Applications", MAC_APP)].some((app) => deps.exists(app))) return { ...open, via: "bundle" };
    const found = await deps.run("/usr/bin/mdfind", [`kMDItemCFBundleIdentifier == "${VSCODE_BUNDLE_ID}"`], EDITOR_PROBE_TIMEOUT_MS);
    if (found.spawnError !== null) return { missing: "spotlight_spawn" };
    if (found.timedOut) return { missing: "spotlight_timed_out" };
    if (found.code !== 0) return { missing: "spotlight_exit" };
    return found.stdout.split("\n").some((line) => line.trim().endsWith(".app")) ? { ...open, via: "spotlight" } : { missing: "not_installed" };
  }
  if (deps.platform === "win32") {
    for (const dir of deps.searchPath) {
      if (!deps.exists(join(dir, "code.cmd"))) continue;
      const exe = resolve(dir, "..", "Code.exe");
      if (deps.exists(exe)) return { file: exe, args: [], detached: true, via: "path" };
    }
    return { missing: "not_installed" };
  }
  for (const dir of deps.searchPath) {
    const launcher = join(dir, "code");
    if (deps.isExecutable(launcher)) return { file: launcher, args: [], detached: false, via: "path" };
  }
  return { missing: "not_installed" };
}

/**
 * The folder the page named, in this system's spelling, when it is an
 * absolute path to a folder that exists now, or why nothing is handed over.
 */
export async function editorTarget(value: unknown): Promise<{ path: string } | { refused: "path" | "missing" | "not_directory" }> {
  const target = fromPage(value);
  if (target === null) return { refused: "path" };
  const stat = await fs.promises.stat(target).catch(() => null);
  if (!stat) return { refused: "missing" };
  if (!stat.isDirectory()) return { refused: "not_directory" };
  return { path: target };
}

/** The host's VS Code: found once per launch, and one open at a time. */
export class Editor {
  private readonly runner = new ChildRunner();
  private found: Promise<EditorLaunch | null> | null = null;

  constructor(
    private readonly log: Pick<HostLog, "event">,
    /** Read when VS Code is first looked for, so the login shell's PATH counts once it answered. */
    private readonly probe: () => Omit<EditorProbeDeps, "run">,
    /** The environment a child gets: the host's children's own. */
    private readonly env: () => Record<string, string | undefined>,
  ) {}

  /** Whether VS Code is installed, asked of the system on the first call and kept for the launch. */
  available(): Promise<boolean> {
    return this.launch().then((launch) => launch !== null);
  }

  /** Hands a folder the page named to VS Code; every outcome goes to the log. */
  async open(reported: unknown): Promise<void> {
    const target = await editorTarget(reported);
    if ("refused" in target) return this.log.event("editor.refused", { reason: target.refused });
    const launch = await this.launch();
    if (launch === null) return this.log.event("editor.refused", { reason: "unavailable" });
    const args = [...launch.args, target.path];
    if (launch.detached) {
      const started = await startDetached(launch.file, args, this.env());
      if ("spawnError" in started) return this.log.event("editor.failed", { reason: "spawn" });
      return this.log.event("editor.opened", { via: launch.via });
    }
    if (this.runner.busy) return this.log.event("editor.refused", { reason: "busy" });
    const result = await this.runner.run(launch.file, args, EDITOR_OPEN_TIMEOUT_MS, this.env());
    if (result.spawnError !== null) return this.log.event("editor.failed", { reason: "spawn" });
    if (result.timedOut) return this.log.event("editor.failed", { reason: "timed_out" });
    if (result.code !== 0) return this.log.event("editor.failed", { reason: "exit", code: result.code });
    this.log.event("editor.opened", { via: launch.via });
  }

  /** Ends a find or an open in flight, on quit. */
  stop(): void {
    this.runner.stop();
  }

  private launch(): Promise<EditorLaunch | null> {
    this.found ??= findEditor({ ...this.probe(), run: (file, args, timeoutMs) => this.runner.run(file, args, timeoutMs, this.env()) }).then(
      (found) => {
        if ("missing" in found) {
          this.log.event("editor.probed", { found: false, reason: found.missing });
          return null;
        }
        this.log.event("editor.probed", { found: true, via: found.via });
        return found;
      },
      () => {
        // Asked again next time: a runner that was busy says nothing about VS Code.
        this.found = null;
        this.log.event("editor.probed", { found: false, reason: "busy" });
        return null;
      },
    );
    return this.found;
  }
}
