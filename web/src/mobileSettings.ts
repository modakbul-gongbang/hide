// The pure rules behind Settings > Mobile (PRD mobile-companion B1-B7,
// B10, B14, B15, B29): what each checklist step says and which one carries
// the action, how the code countdown and a phone's row read, and the push
// choices. hided owns every value; this only words what its `mobile` frame
// reported, so each rule is tested without a browser.

import type { TFunction } from "i18next";
import { formatRelativeTime } from "./i18n/format";
import type { MessageKey } from "./i18n/catalogs";
import type { InterfaceLanguage } from "./i18n/locale";

export type StepState = "ok" | "failed" | "waiting";

/** The daemon's `mobile` frame (hided/src/mobile/mod.rs `frame_value`). */
export type MobileState = {
  enabled: boolean;
  exposure: "off" | "checking" | "blocked" | "foreign" | "failed" | "exposed";
  checklist: { installed: StepState; logged_in: StepState; https: StepState; host_name: string | null } | null;
  download_url: string;
  admin_url: string;
  foreign_target: string | null;
  failure: { step: string; message: string } | null;
  url: string | null;
  qr: string | null;
  code_expires_at_ms: number | null;
  phones: PhoneRow[];
  max_phones: number;
  push_mode: PushMode;
  now_ms: number;
};

export type PhoneRow = {
  id: string;
  name: string;
  last_seen_ms: number;
  connected: boolean;
  notifications: "on" | "off" | "unasked";
  revoke_at_ms: number;
};

export type PushMode = "off" | "app_closed" | "always";

export const PUSH_CHOICES: readonly { id: PushMode; label: MessageKey; detail: MessageKey | null }[] = [
  { id: "off", label: "mobileSetup.push.off", detail: null },
  { id: "app_closed", label: "mobileSetup.push.appClosed", detail: "mobileSetup.push.appClosedDescription" },
  { id: "always", label: "mobileSetup.push.always", detail: null },
];

export type ChecklistRow = {
  id: "installed" | "logged_in" | "https" | "phone";
  state: StepState;
  title: string;
  /** What to do, on the first failing step only. */
  action: string | null;
  link: { label: string; href: string } | null;
};

/**
 * The four steps in order (B2): passed steps read as done, the first failing
 * one carries the exact action, and everything after it waits. The phone
 * step is guidance hide cannot check; it reads as done once the QR shows (B4).
 */
export function checklistRows(state: MobileState, t: TFunction<"translation">): ChecklistRow[] {
  const steps = state.checklist ?? { installed: "waiting", logged_in: "waiting", https: "waiting", host_name: null };
  const rows: ChecklistRow[] = [
    steps.installed === "failed"
      ? {
          id: "installed",
          state: "failed",
          title: t("mobileSetup.installMissing"),
          action: t("mobileSetup.installAction"),
          link: { label: t("mobileSetup.download"), href: state.download_url },
        }
      : { id: "installed", state: steps.installed, title: t("mobileSetup.installed"), action: null, link: null },
    steps.logged_in === "failed"
      ? {
          id: "logged_in",
          state: "failed",
          title: t("mobileSetup.loginMissing"),
          action: t("mobileSetup.loginAction"),
          link: null,
        }
      : {
          id: "logged_in",
          state: steps.logged_in,
          title: steps.logged_in === "ok" && steps.host_name ? t("mobileSetup.loggedInHost", { host: steps.host_name }) : t("mobileSetup.loggedIn"),
          action: null,
          link: null,
        },
    steps.https === "failed"
      ? {
          id: "https",
          state: "failed",
          title: t("mobileSetup.httpsMissing"),
          action: t("mobileSetup.httpsAction"),
          link: { label: t("mobileSetup.adminConsole"), href: state.admin_url },
        }
      : { id: "https", state: steps.https, title: t("mobileSetup.httpsReady"), action: null, link: null },
    {
      id: "phone",
      state: state.qr ? "ok" : "waiting",
      title: t("mobileSetup.phoneStep"),
      action: null,
      link: null,
    },
  ];
  return rows;
}

/** The one line under the checklist when there is no QR, or null when the QR shows. */
export function exposureLine(state: MobileState, t: TFunction<"translation">): { tone: "pending" | "warn" | "error"; text: string } | null {
  switch (state.exposure) {
    case "exposed":
      return state.qr ? null : { tone: "pending", text: t("mobileSetup.generatingCode") };
    case "checking":
      return { tone: "pending", text: t("mobileSetup.checking") };
    case "foreign":
      return { tone: "warn", text: t("mobileSetup.foreign", { target: state.foreign_target ?? t("mobileSetup.unknownTarget") }) };
    case "failed":
      return { tone: "error", text: t("mobileSetup.failed", { step: t(failureStep(state.failure?.step)), message: state.failure?.message ?? "" }).trim() };
    default:
      return { tone: "pending", text: t("mobileSetup.qrWaiting") };
  }
}

function failureStep(step: string | undefined): MessageKey {
  switch (step) {
    case "add":
      return "mobileSetup.failure.add";
    case "remove":
      return "mobileSetup.failure.remove";
    case "funnel":
      return "mobileSetup.failure.funnel";
    default:
      return "mobileSetup.failure.inspect";
  }
}

/** `m:ss` until the code expires, from the daemon's clock; null once expired. */
export function codeCountdown(expiresAtMs: number | null, daemonNowMs: number, elapsedSinceFrameMs: number): string | null {
  if (expiresAtMs === null) return null;
  const left = Math.ceil((expiresAtMs - daemonNowMs - elapsedSinceFrameMs) / 1000);
  if (left <= 0) return null;
  return `${Math.floor(left / 60)}:${String(left % 60).padStart(2, "0")}`;
}

const DAY_MS = 24 * 60 * 60 * 1000;

/** "Just now" under a minute, then minutes, hours and days ago in the interface language. */
export function lastSeen(lastSeenMs: number, nowMs: number, connected: boolean, t: TFunction<"translation">, language: InterfaceLanguage): string {
  if (connected) return t("mobileSetup.justNow");
  const ago = Math.max(0, nowMs - lastSeenMs);
  if (ago < 60_000) return t("mobileSetup.justNow");
  if (ago < 3_600_000) return formatRelativeTime(language, -Math.floor(ago / 60_000), "minute");
  if (ago < DAY_MS) return formatRelativeTime(language, -Math.floor(ago / 3_600_000), "hour");
  return formatRelativeTime(language, -Math.floor(ago / DAY_MS), "day");
}

/**
 * A phone row's second line (B15): last seen, the notification state when
 * the phone answered the permission question, and the days until the
 * seven-day revoke once it has been away a day.
 */
export function phoneLine(phone: PhoneRow, nowMs: number, t: TFunction<"translation">, language: InterfaceLanguage): string {
  const parts = [lastSeen(phone.last_seen_ms, nowMs, phone.connected, t, language)];
  if (phone.notifications === "on") parts.push(t("mobileSetup.notificationsOn"));
  if (phone.notifications === "off") parts.push(t("mobileSetup.notificationsOff"));
  const away = nowMs - phone.last_seen_ms;
  if (!phone.connected && away >= DAY_MS) {
    const days = Math.max(1, Math.ceil((phone.revoke_at_ms - nowMs) / DAY_MS));
    parts.push(t("mobileSetup.autoRevoke", { count: days }));
  }
  return parts.join(" · ");
}

export function phonesTitle(state: MobileState, t: TFunction<"translation">): string {
  return t("mobileSetup.phones", { count: state.phones.length, limit: state.max_phones });
}
