import { describe, expect, it } from "vitest";
import { formatElapsed } from "./components/elapsed";

describe("formatElapsed", () => {
  it("moves at the second, the minute, the hour and the day", () => {
    const cases: [number, string][] = [
      [0, "0s"],
      [59_999, "59s"],
      [60_000, "1m"],
      [3_599_999, "59m"],
      [3_600_000, "1h"],
      [86_399_999, "23h"],
      [86_400_000, "1d"],
      [3 * 86_400_000, "3d"],
    ];
    for (const [ms, text] of cases) expect(formatElapsed(ms)).toBe(text);
  });

  it("reads a moment from the future as no time at all", () => {
    expect(formatElapsed(-5_000)).toBe("0s");
  });
});
