import { describe, expect, it } from "vitest";
import { memoryCards, memoryTime, sourceTime, visibleCards } from "./memoryList";
import type { MemoryRow, ProjectMemory } from "./snapshot";

const row = (number: number): MemoryRow => ({
  number, title: `card ${number}`, kind: "rule", human_source_count: 0, provided_session_count: 0,
  pull_count: 0, updated_at_unix_ms: 0, verifying: false, promotion: null,
});
const project = (overrides: Partial<ProjectMemory>): ProjectMemory => ({
  workspace_id: "p", unavailable: null, loading: false, enabled: true, capacity_reached: false,
  rows: [1, 2, 3, 4, 5, 6, 7].map(row), questions: [], analysis: "idle", forgotten: null,
  pending_action: null, expanded: null, search: null, ...overrides,
});

describe("the Memory tab's card list", () => {
  it("draws a question's card once, in 물어볼 것, and a search's answer in its own order", () => {
    const asked = project({ questions: [{ kind: "failed", number: 2, title: "card 2" }] });
    expect(memoryCards(asked).map((card) => card.number)).toEqual([1, 3, 4, 5, 6, 7]);
    const searched = project({ search: { query: "x", numbers: [6, 2, 9] } });
    expect(memoryCards(searched).map((card) => card.number)).toEqual([6, 2]);
  });

  it("folds the cards past the first five, but never the card shown in full", () => {
    const cards = project({}).rows;
    expect(visibleCards(cards, null, false)).toEqual({ shown: cards.slice(0, 5), folded: 2 });
    const opened = visibleCards(cards, 7, false);
    expect(opened.shown.map((card) => card.number)).toEqual([1, 2, 3, 4, 5, 7]);
    expect(opened.folded).toBe(1);
    expect(visibleCards(cards, null, true).folded).toBe(0);
    // A card on its way into the repository is never folded either.
    const moving = cards.map((card) => (card.number === 6 ? { ...card, promotion: { state: "running" as const, agent: null, pr_number: null, target_path: null, github_failure: null } } : card));
    expect(visibleCards(moving, null, false).shown.map((card) => card.number)).toEqual([1, 2, 3, 4, 5, 6]);
  });

  it("names a row's time as minutes, hours, yesterday, then the date", () => {
    const now = new Date(2026, 9, 12, 15, 0).getTime();
    expect(memoryTime(now - 12 * 60_000, "ko", now)).toBe("12분 전");
    expect(memoryTime(now - 3 * 3_600_000, "ko", now)).toBe("3시간 전");
    expect(memoryTime(new Date(2026, 9, 11, 9, 0).getTime(), "ko", now)).toBe("어제");
    expect(memoryTime(new Date(2026, 9, 7, 9, 0).getTime(), "ko", now)).toBe("10-07");
    expect(memoryTime(new Date(2025, 0, 2, 9, 0).getTime(), "ko", now)).toBe("2025-01-02");
  });

  it("names a source's time with its clock, as the design's 어제 14:02 and 10-09 21:40", () => {
    const now = new Date(2026, 9, 12, 15, 0).getTime();
    expect(sourceTime(new Date(2026, 9, 12, 9, 5).getTime(), "ko", now)).toBe("09:05");
    expect(sourceTime(new Date(2026, 9, 11, 14, 2).getTime(), "ko", now)).toBe("어제 14:02");
    expect(sourceTime(new Date(2026, 9, 9, 21, 40).getTime(), "ko", now)).toBe("10-09 21:40");
  });
});
