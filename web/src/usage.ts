// What the weekly usage chips and popover print for a provider row or one of
// its buckets (docs/UI_BEHAVIOR.md "Weekly usage"). The core reads the numbers
// and names the state (herdr-core/src/usage.rs); these rules only decide how a
// state reads, the way the native `SidebarUtilityBar` and `HideUsagePopover` do.

import type { TFunction } from "i18next";
import type { ProviderUsage, ProviderUsageBucket } from "./generated/hided-ws";

export type UsageReading = Pick<ProviderUsage | ProviderUsageBucket, "state" | "used_percent">;

export type UsageTone = "success" | "warning" | "danger" | "muted";

/** The states whose percent is a reading: stale and fallback carry an older one, still worth showing. */
const READINGS: ReadonlySet<string> = new Set(["available", "stale", "fallback"]);

/** The percent a row shows, or null while it has none: loading, unavailable, or no number. */
export function usagePercent(row: UsageReading): number | null {
  return READINGS.has(row.state) && row.used_percent !== null ? row.used_percent : null;
}

/** From 70% the window is getting tight and from 90% it is nearly spent; no reading is muted. */
export function usageTone(percent: number | null): UsageTone {
  if (percent === null) return "muted";
  if (percent >= 90) return "danger";
  if (percent >= 70) return "warning";
  return "success";
}

export function percentLabel(percent: number): string {
  return `${Math.round(percent)}%`;
}

/** What stands where the percent goes: the percent, `…` while the first read runs, or `Unavailable`. */
export function usageValueLabel(row: UsageReading, t: TFunction<"translation">): string {
  const percent = usagePercent(row);
  if (percent !== null) return percentLabel(percent);
  return row.state === "loading" ? "…" : t("common.unavailable");
}

/** `in 5d 4h`, `in 3h 12m` or `in 7m`: the time left before the window resets, never less than a minute. */
export function resetCountdown(resetsAtUnixSeconds: number, nowMs: number, t: TFunction<"translation">): string {
  const seconds = Math.max(0, Math.floor(resetsAtUnixSeconds - nowMs / 1000));
  const days = Math.floor(seconds / 86_400);
  const hours = Math.floor((seconds % 86_400) / 3_600);
  const minutes = Math.floor((seconds % 3_600) / 60);
  if (days > 0) return t("shell.usage.resetDaysHours", { days, hours });
  if (hours > 0) return t("shell.usage.resetHoursMinutes", { hours, minutes });
  return t("shell.usage.resetMinutes", { minutes: Math.max(1, minutes) });
}

/** The chips' accessible name: every provider with its reading, in the order the chips draw them. */
export function usageSummary(rows: readonly ProviderUsage[], t: TFunction<"translation">): string {
  const readings = rows.map((row) => {
    const percent = usagePercent(row);
    if (percent !== null) return t("shell.usage.providerValue", { provider: row.label, value: percentLabel(percent) });
    return t(row.state === "loading" ? "shell.usage.providerLoading" : "shell.usage.providerUnavailable", { provider: row.label });
  });
  return [t("shell.weeklyUsage"), ...readings].join(", ");
}
