import { describe, expect, it } from "vitest";
import { resizeStep } from "./resize";

describe("resizeStep", () => {
  it("names the direction and the fraction of the split span", () => {
    expect(resizeStep(100, 1000, true)).toEqual({ direction: "right", amount: 0.1 });
    expect(resizeStep(-250, 1000, true)).toEqual({ direction: "left", amount: 0.25 });
    expect(resizeStep(50, 500, false)).toEqual({ direction: "down", amount: 0.1 });
    expect(resizeStep(-5, 500, false)).toEqual({ direction: "up", amount: 0.01 });
  });

  it("sends nothing outside the core's accepted range", () => {
    expect(resizeStep(0, 1000, true)).toBeNull();
    expect(resizeStep(0.5, 1000, true)).toBeNull();
    expect(resizeStep(600, 1000, true)).toBeNull();
    expect(resizeStep(-600, 1000, false)).toBeNull();
    expect(resizeStep(500, 1000, true)).toEqual({ direction: "right", amount: 0.5 });
  });
});
