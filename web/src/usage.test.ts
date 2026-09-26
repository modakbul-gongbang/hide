import { describe, expect, it } from "vitest";
import type { ProviderUsage } from "./generated/hided-ws";
import { resetCountdown, usagePercent, usageSummary, usageTone, usageValueLabel } from "./usage";

function row(patch: Partial<ProviderUsage>): ProviderUsage {
  return {
    provider: "claude",
    label: "Claude Code",
    window_minutes: 10_080,
    state: "available",
    used_percent: 62,
    resets_at_unix_seconds: 1_790_000_000,
    message: null,
    last_checked_at_unix_ms: 1_789_000_000_000,
    last_success_at_unix_ms: 1_789_000_000_000,
    last_error_kind: null,
    buckets: [],
    ...patch,
  };
}

describe("a row's reading (#153: the Swift footer and popover rules)", () => {
  it("shows a percent for a current, stale or fallback read and none while loading or unavailable", () => {
    expect(usagePercent(row({ state: "available", used_percent: 62 }))).toBe(62);
    expect(usagePercent(row({ state: "stale", used_percent: 40 }))).toBe(40);
    expect(usagePercent(row({ state: "fallback", used_percent: 12 }))).toBe(12);
    expect(usagePercent(row({ state: "loading", used_percent: null }))).toBeNull();
    expect(usagePercent(row({ state: "unavailable", used_percent: null }))).toBeNull();
  });

  it("colors by the used share: success below 70, warning from 70, destructive from 90, muted without a reading", () => {
    expect(usageTone(69.9)).toBe("success");
    expect(usageTone(70)).toBe("warning");
    expect(usageTone(89.9)).toBe("warning");
    expect(usageTone(90)).toBe("danger");
    expect(usageTone(null)).toBe("muted");
  });

  it("prints the rounded percent, an ellipsis while the first read runs, and Unavailable otherwise", () => {
    expect(usageValueLabel(row({ used_percent: 61.5 }))).toBe("62%");
    expect(usageValueLabel(row({ state: "loading", used_percent: null }))).toBe("…");
    expect(usageValueLabel(row({ state: "unavailable", used_percent: null, message: "Sign in with claude to see usage" }))).toBe("Unavailable");
  });

  it("names every provider's reading for the chips, in order", () => {
    expect(usageSummary([row({}), row({ provider: "codex", label: "Codex", state: "unavailable", used_percent: null })])).toBe(
      "Weekly usage, Claude Code 62%, Codex unavailable",
    );
    expect(usageSummary([row({ state: "loading", used_percent: null })])).toBe("Weekly usage, Claude Code loading");
  });
});

describe("the reset countdown", () => {
  const now = Date.UTC(2026, 8, 26, 0, 0, 0);
  const at = (seconds: number) => now / 1000 + seconds;

  it("reads days and hours beyond a day, hours and minutes within one, and minutes within an hour", () => {
    expect(resetCountdown(at(5 * 86_400 + 4 * 3_600 + 59 * 60), now)).toBe("in 5d 4h");
    expect(resetCountdown(at(3 * 3_600 + 12 * 60 + 30), now)).toBe("in 3h 12m");
    expect(resetCountdown(at(3 * 3_600), now)).toBe("in 3h 0m");
    expect(resetCountdown(at(7 * 60 + 59), now)).toBe("in 7m");
  });

  it("never reads less than a minute, a reset that just passed included", () => {
    expect(resetCountdown(at(20), now)).toBe("in 1m");
    expect(resetCountdown(at(-30), now)).toBe("in 1m");
  });
});
