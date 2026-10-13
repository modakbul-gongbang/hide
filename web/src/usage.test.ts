import { describe, expect, it } from "vitest";
import { initializeInterfaceI18n } from "./i18n/instance";
import { resetCountdown as resetCountdownIn, usageTone } from "./usage";
const t = initializeInterfaceI18n("en").getFixedT(null, "translation");
const resetCountdown = (resetsAt: number, now: number) => resetCountdownIn(resetsAt, now, t);
it("colors by the used share at the approved 70 and 90 boundaries", () => {
  expect([69.9, 70, 89.9, 90, null].map(usageTone)).toEqual(["success", "warning", "warning", "danger", "muted"]);
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

