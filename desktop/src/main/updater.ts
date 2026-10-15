// This app's own updates (PRD auto-update): when it looks, which release it
// takes, the download, and the install on quit or on Restart. It owns nothing
// else and shows nothing itself; the shell draws what `status()` says (the
// dot on Help, Help's last line, Settings > About) and the host relays it.
//
//   checks       at launch and every six hours, and when asked (D-17)
//   chooses      the highest release of with-hide/hide whose update file and
//                file for this system are both up, stable only unless preview
//                releases are on, never lower than this app (D-22, D-23)
//   downloads    in the background from that release, checked against the
//                sha512 its update file names; macOS also requires the same
//                Developer ID signature (D-24)
//   installs     on quit (macOS, Windows), or on Restart, which closes and
//                reopens only this window: the core and its agents keep
//                running, and the reopened app replaces the core as it does
//                today (D-18, D-19)
//
// A development or CI build (any version that is not `X.Y.Z` or
// `X.Y.Z-rc.N`), an unpackaged app, and a Linux app not installed from the deb
// or rpm never look (D-27). Every failure is a state the About row shows and
// an event in the host's log; nothing else on screen changes (D-21).
//
// The feed is pinned to with-hide/hide here, not read from the package
// (electron-builder does not write one for a prepackaged app); the CI update
// test points it at a local folder with HIDE_UPDATE_FEED_URL (D-29).

import fs from "node:fs";
import path from "node:path";
import type { AppUpdater } from "electron-updater";
import type { UpdatePhase, UpdateStatus } from "../../../web/src/host";
import { readJsonFile, writeJsonFile } from "./jsonFile";
import type { HostLog } from "./log";

export const RELEASES_REPOSITORY = "with-hide/hide";
const RELEASES_API = `https://api.github.com/repos/${RELEASES_REPOSITORY}/releases?per_page=30`;
const RELEASE_DOWNLOADS = `https://github.com/${RELEASES_REPOSITORY}/releases/download`;
/** The first look waits for the window and the core to settle. */
export const FIRST_CHECK_DELAY_MS = 15_000;
export const CHECK_INTERVAL_MS = 6 * 60 * 60 * 1000;
const RELEASES_TIMEOUT_MS = 30_000;
/** A release list is a few hundred KB; more is not GitHub's answer. */
const RELEASES_BYTES_CAP = 8 * 1024 * 1024;

/** The versions a release is published under (`releaseKind` in scripts/check-release-assets.mjs). */
const RELEASE_VERSION = /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-rc\.([1-9]\d*))?$/;

type Version = { major: number; minor: number; patch: number; rc: number | null };

export function parseVersion(text: string): Version | null {
  const match = RELEASE_VERSION.exec(text);
  if (!match) return null;
  return { major: Number(match[1]), minor: Number(match[2]), patch: Number(match[3]), rc: match[4] === undefined ? null : Number(match[4]) };
}

/** Semver order for the release shapes: a release is newer than every rc of the same version. */
export function compareVersions(a: Version, b: Version): number {
  const rank = (rc: number | null) => (rc === null ? Number.MAX_SAFE_INTEGER : rc);
  return a.major - b.major || a.minor - b.minor || a.patch - b.patch || rank(a.rc) - rank(b.rc);
}

/** The system's files, as `systemFiles` in scripts/check-release-assets.mjs names them. */
export type UpdateSystem = "macos" | "windows" | "deb" | "rpm";

export function updateFiles(version: string, system: UpdateSystem): { info: string; file: string } {
  const base = `hide-v${version}`;
  switch (system) {
    case "macos":
      return { info: "latest-mac.yml", file: `${base}-macos-arm64.zip` };
    case "windows":
      return { info: "latest.yml", file: `${base}-windows-x64-setup.exe` };
    case "deb":
      return { info: "latest-linux.yml", file: `${base}-linux-x64.deb` };
    case "rpm":
      return { info: "latest-linux.yml", file: `${base}-linux-x64.rpm` };
  }
}

export type ChosenRelease = { tag: string; version: string };

/**
 * The release to update to, from GitHub's list (newest first, any order
 * accepted): published, tagged `vX.Y.Z` or, with preview releases on,
 * `vX.Y.Z-rc.N` marked as a pre-release; with this system's update file and
 * update both uploaded; newer than `current`. A tag of another shape (an
 * evidence tag) and a release still uploading are skipped. Null when none is.
 */
export function chooseRelease(releases: unknown, input: { current: string; prerelease: boolean; system: UpdateSystem }): ChosenRelease | null {
  const current = parseVersion(input.current);
  if (!current || !Array.isArray(releases)) return null;
  let best: { tag: string; version: string; parsed: Version } | null = null;
  for (const release of releases) {
    if (!release || typeof release !== "object") continue;
    const { tag_name: tag, draft, prerelease, assets } = release as Record<string, unknown>;
    if (typeof tag !== "string" || !tag.startsWith("v") || draft !== false || !Array.isArray(assets)) continue;
    const version = tag.slice(1);
    const parsed = parseVersion(version);
    if (!parsed || (parsed.rc !== null) !== (prerelease === true)) continue;
    if (parsed.rc !== null && !input.prerelease) continue;
    if (compareVersions(parsed, current) <= 0 || (best && compareVersions(parsed, best.parsed) <= 0)) continue;
    const { info, file } = updateFiles(version, input.system);
    const uploaded = (name: string) => assets.some((asset) => asset && typeof asset === "object" && (asset as Record<string, unknown>).name === name && (asset as Record<string, unknown>).state === "uploaded");
    if (!uploaded(info) || !uploaded(file)) continue;
    best = { tag, version, parsed };
  }
  return best ? { tag: best.tag, version: best.version } : null;
}

/**
 * Whether this app updates itself, and as which system. `packageType` is the
 * `resources/package-type` a Linux package carries; `writable` whether a
 * macOS app's bundle can be replaced where it is.
 */
export function updateSystem(input: { packaged: boolean; version: string; platform: NodeJS.Platform; packageType: string | null; macosLocation: "ok" | "read_only" }): UpdateSystem | { off: "development" | "location" } {
  if (!input.packaged || !parseVersion(input.version)) return { off: "development" };
  switch (input.platform) {
    case "darwin":
      return input.macosLocation === "ok" ? "macos" : { off: "location" };
    case "win32":
      return "windows";
    case "linux":
      return input.packageType === "deb" || input.packageType === "rpm" ? input.packageType : { off: "development" };
    default:
      return { off: "development" };
  }
}

/**
 * Whether a macOS app at `bundle` can replace itself: not on a disk image or
 * a read-only volume, not run from Gatekeeper's translocated copy, and in a
 * folder this account may write.
 */
export function macosLocation(bundle: string, canWrite: (folder: string) => boolean): "ok" | "read_only" {
  if (bundle.startsWith("/Volumes/") || bundle.includes("/AppTranslocation/")) return "read_only";
  return canWrite(path.dirname(bundle)) ? "ok" : "read_only";
}

/** The rc choice, kept in the desktop profile (D-22). */
export function updatesFile(userData: string): string {
  return path.join(userData, "updates.json");
}

export function readPrerelease(file: string): boolean {
  const stored = readJsonFile(file);
  return Boolean(stored && typeof stored === "object" && (stored as Record<string, unknown>).prerelease === true);
}

/** The slice of electron-updater this module drives, so tests can stand in for it. */
export type UpdaterLike = Pick<AppUpdater, "setFeedURL" | "checkForUpdates" | "downloadUpdate" | "quitAndInstall" | "on"> & {
  autoDownload: boolean;
  autoInstallOnAppQuit: boolean;
  allowDowngrade: boolean;
};

export type UpdatesIo = {
  /** GitHub's release list, parsed; throws on a failed or oversized answer. */
  releases: () => Promise<unknown>;
  now: () => number;
  /** Schedules `run` after `ms`; the answer cancels it. */
  later: (run: () => void, ms: number) => () => void;
  savePrerelease: (on: boolean) => void;
};

/**
 * The updater's state machine. `system` decides whether it ever looks;
 * `updater` is created only for a system that does. One check or download
 * runs at a time. A ready update stays ready while a later check looks; a
 * newer release that check finds is downloaded in its place, and the line
 * names it once it is ready (B19).
 */
export class AppUpdates {
  private phase: UpdatePhase;
  private prerelease: boolean;
  private running: Promise<void> | null = null;
  private cancelTimer: (() => void) | null = null;
  private installing = false;
  private readonly feedOverride: string | null;
  private readonly listeners = new Set<(status: UpdateStatus) => void>();

  constructor(
    private readonly system: UpdateSystem | { off: "development" | "location" },
    private readonly current: string,
    private readonly updater: UpdaterLike | null,
    private readonly io: UpdatesIo,
    private readonly log: HostLog,
    options: { prerelease: boolean; feedOverride: string | null },
  ) {
    this.prerelease = options.prerelease;
    this.feedOverride = options.feedOverride;
    this.phase = typeof system === "string" ? { kind: "idle", checkedAt: null } : { kind: "off", reason: system.off };
    if (updater) {
      updater.autoDownload = false;
      updater.allowDowngrade = false;
      updater.autoInstallOnAppQuit = system !== "deb" && system !== "rpm";
      updater.on("download-progress", (progress: { percent: number }) => {
        if (this.phase.kind === "downloading") this.set({ ...this.phase, percent: Math.max(0, Math.min(100, Math.round(progress.percent))) });
      });
      // An install that failed after Restart (a cancelled password prompt on
      // Linux) keeps the update ready; electron-updater reports it here.
      updater.on("error", (error: Error) => {
        if (!this.installing) return;
        this.installing = false;
        this.log.event("update.install_failed", { detail: errorDetail(error) });
      });
    }
  }

  status(): UpdateStatus {
    return { phase: this.phase, prerelease: this.prerelease, installOnQuit: this.system !== "deb" && this.system !== "rpm" };
  }

  onChange(listener: (status: UpdateStatus) => void): () => void {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  /** Looks soon after launch and then every six hours, until `stop`. */
  start(): void {
    if (typeof this.system !== "string") {
      this.log.event("update.off", { reason: this.system.off, version: this.current });
      return;
    }
    this.schedule(FIRST_CHECK_DELAY_MS);
  }

  stop(): void {
    this.cancelTimer?.();
    this.cancelTimer = null;
  }

  /** A check now; one already running is the answer. */
  check(trigger: "launch" | "interval" | "operator"): Promise<void> {
    if (!this.updater || typeof this.system !== "string") return Promise.resolve();
    this.running ??= this.run(trigger, this.system, this.updater).finally(() => {
      this.running = null;
    });
    return this.running;
  }

  /** Installs the ready update and reopens the app; nothing happens unless one is ready. */
  restart(): void {
    if (!this.updater || this.phase.kind !== "ready" || this.installing) return;
    this.installing = true;
    this.log.event("update.install", { version: this.phase.version, trigger: "restart" });
    // Silent, and opening the new version once installed.
    this.updater.quitAndInstall(true, true);
  }

  setPrerelease(on: boolean): void {
    if (on === this.prerelease) return;
    this.prerelease = on;
    try {
      this.io.savePrerelease(on);
    } catch (error) {
      this.log.event("update.prerelease_save_failed", { detail: errorDetail(error) });
    }
    this.log.event("update.prerelease", { on });
    this.emit();
  }

  private schedule(ms: number): void {
    this.cancelTimer?.();
    this.cancelTimer = this.io.later(() => {
      void this.check(ms === FIRST_CHECK_DELAY_MS ? "launch" : "interval").finally(() => this.schedule(CHECK_INTERVAL_MS));
    }, ms);
  }

  private async run(trigger: string, system: UpdateSystem, updater: UpdaterLike): Promise<void> {
    const ready = this.phase.kind === "ready" ? this.phase : null;
    if (!ready) this.set({ kind: "checking" });
    this.log.event("update.check", { trigger, version: this.current, prerelease: this.prerelease });
    let version: string;
    try {
      const chosen = this.feedOverride ? null : chooseRelease(await this.io.releases(), { current: this.current, prerelease: this.prerelease, system });
      if (!this.feedOverride && (!chosen || chosen.version === ready?.version)) {
        this.log.event("update.none", { latest: chosen?.version ?? null });
        if (!ready) this.set({ kind: "idle", checkedAt: this.io.now() });
        return;
      }
      updater.setFeedURL({ provider: "generic", url: this.feedOverride ?? `${RELEASE_DOWNLOADS}/${chosen!.tag}` });
      const result = await updater.checkForUpdates();
      if (!result?.isUpdateAvailable || result.updateInfo.version === ready?.version) {
        this.log.event("update.none", { latest: result?.updateInfo.version ?? null });
        if (!ready) this.set({ kind: "idle", checkedAt: this.io.now() });
        return;
      }
      version = result.updateInfo.version;
    } catch (error) {
      this.log.event("update.check_failed", { detail: errorDetail(error) });
      if (!ready) this.set({ kind: "failed", stage: "check" });
      return;
    }
    this.log.event("update.found", { version });
    this.set({ kind: "downloading", version, percent: 0 });
    try {
      await updater.downloadUpdate();
    } catch (error) {
      // A file whose sha512 (or macOS signature) does not match is discarded
      // by electron-updater and lands here; nothing was installed.
      this.log.event("update.download_failed", { version, detail: errorDetail(error) });
      this.set({ kind: "failed", stage: "download" });
      return;
    }
    this.log.event("update.ready", { version });
    this.set({ kind: "ready", version });
  }

  private set(phase: UpdatePhase): void {
    this.phase = phase;
    this.emit();
  }

  private emit(): void {
    const status = this.status();
    for (const listener of this.listeners) listener(status);
  }
}

/** A failure for the log: electron-updater's code and message, never a response body. */
function errorDetail(error: unknown): string {
  if (error instanceof Error) {
    const code = (error as Error & { code?: unknown }).code;
    return `${typeof code === "string" ? `${code}: ` : ""}${error.message}`.slice(0, 500);
  }
  return String(error).slice(0, 500);
}

/** GitHub's release list over Electron's network stack (the system's proxy), bounded in time and size. */
export async function fetchReleases(fetchImpl: (url: string, init: RequestInit) => Promise<Response>): Promise<unknown> {
  const response = await fetchImpl(RELEASES_API, {
    headers: { Accept: "application/vnd.github+json", "X-GitHub-Api-Version": "2022-11-28" },
    signal: AbortSignal.timeout(RELEASES_TIMEOUT_MS),
  });
  if (!response.ok) {
    await response.body?.cancel();
    throw new Error(`GitHub answered the release list with HTTP ${response.status}`);
  }
  const text = await response.text();
  if (text.length > RELEASES_BYTES_CAP) throw new Error("GitHub's release list exceeds 8 MiB");
  return JSON.parse(text);
}

/** The Linux package type electron-builder writes beside the app (`resources/package-type`), or null. */
export function readPackageType(resources: string): string | null {
  try {
    return fs.readFileSync(path.join(resources, "package-type"), "utf8").trim();
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code === "ENOENT") return null;
    throw error;
  }
}

export function savePrerelease(file: string, on: boolean): void {
  writeJsonFile(file, { prerelease: on });
}
