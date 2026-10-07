import { describe, expect, it } from "vitest";
import { nextZoomFactor } from "./zoomSteps";

describe("page zoom", () => {
  it("steps through Chrome's zoom levels and stays at either end", () => {
    const walk = (from: number, zoom: "in" | "out", steps: number) => Array.from({ length: steps }).reduce<number>((factor) => nextZoomFactor(factor, zoom), from);
    expect([1, 2, 3].map((steps) => walk(1, "in", steps))).toEqual([1.1, 1.25, 1.5]);
    expect([1, 2, 3].map((steps) => walk(1, "out", steps))).toEqual([0.9, 0.8, 0.75]);
    expect(walk(1, "in", 20)).toBe(5);
    expect(walk(1, "out", 20)).toBe(0.25);
    expect(nextZoomFactor(0.3333333, "in")).toBe(0.5);
    expect(nextZoomFactor(1.2, "in")).toBe(1.25);
    expect(nextZoomFactor(1.2, "out")).toBe(1.1);
    expect(nextZoomFactor(2.5, "reset")).toBe(1);
  });
});
