// Presentation rules for the typed limit cells. Declarations decide support;
// only the core node's reported values decide account availability.
import type { TFunction } from "i18next";
import { AGENT_ADAPTERS } from "./agentAdapters";
import type { LimitCell, WindowKind, WindowReading } from "./generated/hided-ws";
import { percentLabel, resetCountdown } from "./usage";

export type UsageAdapter = (typeof AGENT_ADAPTERS)[number];
export const WINDOW_ORDER: readonly WindowKind[] = ["five_hour", "weekly", "monthly"];

export function hasLimits(adapter: UsageAdapter): boolean {
  return WINDOW_ORDER.some((kind) => adapter.usage.limits[kind].support === "supported");
}

export function accountWindows(adapter: UsageAdapter, cell: LimitCell | undefined): WindowKind[] {
  return WINDOW_ORDER.filter((kind) => adapter.usage.limits[kind].support === "supported" && limitWindow(cell, kind)?.outcome.state !== "not_offered");
}

export function limitWindow(cell: LimitCell | undefined, kind: WindowKind): WindowReading | undefined {
  return cell?.state === "reading" ? cell.value.windows.find((row) => row.kind === kind && row.scope.kind === "all") : undefined;
}

export function windowLabel(kind: WindowKind, t: TFunction<"translation">): string {
  return t(`usage.window.${kind}`);
}

export function limitValue(cell: LimitCell | undefined, kind: WindowKind, now: number, t: TFunction<"translation">): string {
  if (!cell || cell.state === "loading") return "…";
  const row = limitWindow(cell, kind);
  if (!row) return t("common.unavailable");
  switch (row.outcome.state) {
    case "open": return row.outcome.resets_at_s * 1000 <= now ? t("usage.expired") : `${percentLabel(row.outcome.used_percent)} · ${resetCountdown(row.outcome.resets_at_s, now, t)}`;
    case "not_started": return t("usage.notStarted");
    case "expired": return t("usage.expired");
    case "not_offered": return "";
    case "unreadable": return t("common.unavailable");
  }
}

/** Unsupported wording follows the declared absence, never a guessed login. */
export function absenceReason(adapter: UsageAdapter, t: TFunction<"translation">): string {
  const weekly = adapter.usage.limits.weekly;
  if (weekly.support !== "unsupported" || typeof weekly.detail !== "object") return "";
  switch (weekly.detail.kind.kind) {
    case "not_built": return t("usage.reason.notBuilt", { agent: adapter.label });
    case "no_such_data": return t("usage.reason.account");
    case "excluded": return t(adapter.id === "opencode" ? "usage.reason.opencode" : "usage.reason.cursor");
  }
  return weekly.detail.reason;
}
