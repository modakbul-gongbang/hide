// Whether macOS lets Hide show notifications (PRD first-run-onboarding D-12,
// B14-B18). Electron has no API for it, and a notification macOS suppresses
// still "shows" without error, so a small helper bundled beside the app's
// executable asks UNUserNotificationCenter and prints the answer
// (`desktop/native/notification-status/main.swift`). It must run from
// Contents/MacOS: macOS keys notification settings to the bundle it finds
// by walking up from the executable, and the helper carries the app's
// identifier for that reason.
//
// One read runs at a time with a hard timeout; a read asked for while one
// runs shares its answer. The shell asks only while a screen showing the
// state is open (the wizard's second step, Settings > General).

import fs from "node:fs";
import path from "node:path";
import type { NotificationPermission } from "../../../web/src/host";
import type { HostLog, LogFields } from "./log";
import { ChildRunner } from "./spawn";

export const NOTIFICATION_HELPER = "hide-notification-status";
export const NOTIFICATION_HELPER_TIMEOUT_MS = 4000;

/** The identifier the packaged app carries (`desktop/scripts/package.mjs` names the same): macOS keys notification settings to it, and Windows groups notifications under it. */
export const APP_ID = "me.grab.hide.desktop";

/** macOS System Settings opened at Hide's notification page (B15). */
export const NOTIFICATION_SETTINGS_URL = `x-apple.systempreferences:com.apple.Notifications-Settings.extension?id=${APP_ID}`;

/** The helper's line as a permission, or why it could not be read. */
export function parsePermission(stdout: string): NotificationPermission | { unreadable: string } {
  let parsed: unknown;
  try {
    parsed = JSON.parse(stdout.trim());
  } catch {
    return { unreadable: "not_json" };
  }
  const authorization = typeof parsed === "object" && parsed !== null ? (parsed as Record<string, unknown>).authorization : undefined;
  switch (authorization) {
    case "authorized":
    case "provisional":
    case "ephemeral":
      return "granted";
    case "denied":
      return "denied";
    case "not-determined":
      return "not_determined";
    default:
      return { unreadable: `authorization_${typeof authorization === "string" ? authorization : "missing"}` };
  }
}

export class NotificationPermissionReader {
  private readonly runner = new ChildRunner();
  private inFlight: Promise<NotificationPermission> | null = null;
  /** The last notification shown on Linux failed, so no notification service answers (`showFailed`). */
  private serviceMissing = false;

  constructor(
    private readonly platform: NodeJS.Platform,
    /** The folder the app's own executable is in: Contents/MacOS in a packaged app. */
    private readonly executableDir: string,
    private readonly log: Pick<HostLog, "event">,
    /** Whether the system can show a notification at all (Electron's `Notification.isSupported`). */
    private readonly notificationsSupported: () => boolean,
  ) {}

  /**
   * A notification failed to show (Electron's `failed`). On Linux,
   * `Notification.isSupported()` says only that libnotify loaded, so a
   * desktop with no notification service answers true and every show fails
   * (`shell/browser/notifications/linux/libnotify_notification.cc`); a
   * failure there reads as `no_service` until a notification shows again
   * (`showSucceeded`), since every later one is still tried. Elsewhere a
   * failure is the system declining one notification (Windows' own settings
   * or Focus, a macOS refusal the helper reads), not a missing service, and
   * only the caller's log records it.
   */
  showFailed(): void {
    if (this.platform === "linux") this.serviceMissing = true;
  }

  /** A notification showed: a notification service answers after all (a daemon that started late). */
  showSucceeded(): void {
    this.serviceMissing = false;
  }

  read(): Promise<NotificationPermission> {
    // Outside macOS there is no permission to read, only whether a notification service answers (D-24).
    if (this.platform !== "darwin") return Promise.resolve(this.notificationsSupported() && !this.serviceMissing ? "unsupported" : "no_service");
    this.inFlight ??= this.run().finally(() => {
      this.inFlight = null;
    });
    return this.inFlight;
  }

  /** Ends a read in flight, on quit. */
  stop(): void {
    this.runner.stop();
  }

  private async run(): Promise<NotificationPermission> {
    const helper = path.join(this.executableDir, NOTIFICATION_HELPER);
    if (!fs.existsSync(helper)) return this.unreadable("helper_missing");
    const result = await this.runner.run(helper, [], NOTIFICATION_HELPER_TIMEOUT_MS);
    if (result.spawnError !== null) return this.unreadable("spawn_failed", { detail: result.spawnError });
    if (result.timedOut) return this.unreadable("timed_out");
    if (result.code !== 0) return this.unreadable("exit", { code: result.code, signal: result.signal });
    const permission = parsePermission(result.stdout);
    if (typeof permission === "object") return this.unreadable(permission.unreadable);
    return permission;
  }

  private unreadable(reason: string, fields: LogFields = {}): NotificationPermission {
    this.log.event("notify.permission_unreadable", { reason, ...fields });
    return "unknown";
  }
}
