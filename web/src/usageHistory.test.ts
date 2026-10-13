import { describe, expect, it } from "vitest";
import type { UsageAmount, UsageRange } from "./generated/hided-ws";
import { usageDays } from "./usageHistory";

const empty: UsageAmount = {
  calls: 0, new_tokens: 0,
  tokens: { input: 0, output: 0, cache_read: 0, cache_write: 0, cache_write_1h: 0, reasoning: null },
  cost: { micro_usd: 0, partial: false, waiting: false, stale: false, incomplete: false, unpriced_models: [], other_unpriced_models: 0 },
};
const range = (reading: boolean): UsageRange => ({
  calendar_start: "2026-03-08", calendar_end: "2026-03-10", days: [], amount: empty,
  since: null, active_days: 0, observed_days: 0, peak: null, current_streak: 0, longest_streak: 0,
  reading, limited: false, files_done: 0, files_total: 1,
});

describe("sparse usage calendar", () => {
  it("fills civil dates across a daylight-saving boundary without inventing records", () => {
    const days = usageDays(range(false));
    expect(days.map(({ date, weekday, week, today }) => ({ date, weekday, week, today }))).toEqual([
      { date: "2026-03-08", weekday: 0, week: 0, today: false },
      { date: "2026-03-09", weekday: 1, week: 0, today: false },
      { date: "2026-03-10", weekday: 2, week: 0, today: true },
    ]);
    expect(days.map((day) => [day.pending, day.amount.calls, day.level])).toEqual([[false, 0, 0], [false, 0, 0], [false, 0, 0]]);
  });

  it("keeps a zero-token recorded call distinct from pending empty cells", () => {
    const sparse = range(true);
    const recorded = {
      date: "2026-03-09", weekday: 1, week: 0, today: false, recent_week: true,
      level: 0, pending: false, agents: [], top_agents: [], other_agents: 0,
      amount: { ...empty, calls: 1, tokens: { ...empty.tokens, cache_read: 900 }, cost: { ...empty.cost, micro_usd: 125_000 } },
    };
    sparse.days = [recorded];
    const days = usageDays(sparse);
    expect(days.map((day) => day.pending)).toEqual([true, false, true]);
    expect(days[1]).toBe(recorded);
    expect(days[1]?.amount.cost.micro_usd).toBe(125_000);
    expect(sparse.days).toEqual([recorded]);
  });

  it("places the final empty week and recent-week outline using the core bounds", () => {
    const days = usageDays({ ...range(false), calendar_end: "2026-03-16" });
    expect(days).toHaveLength(9);
    expect(days[7]).toMatchObject({ date: "2026-03-15", week: 1, weekday: 0 });
    expect(days.filter((day) => day.recent_week).map((day) => day.date)).toEqual([
      "2026-03-10", "2026-03-11", "2026-03-12", "2026-03-13", "2026-03-14", "2026-03-15", "2026-03-16",
    ]);
  });
});
