import { describe, expect, it } from "vitest";
import { wheelModifiers, wheelRows } from "./wheel";

describe("wheelRows", () => {
  it("accumulates trackpad pixels into whole rows and keeps the remainder", () => {
    const first = wheelRows(10, 0, 16, 0);
    expect(first).toEqual({ rows: 0, remainder: 10 });
    const second = wheelRows(10, 0, 16, first.remainder);
    expect(second).toEqual({ rows: 1, remainder: 4 });
    const up = wheelRows(-40, 0, 16, 0);
    expect(up).toEqual({ rows: -2, remainder: -8 });
  });

  it("moves at least one row per wheel notch and drops the remainder", () => {
    expect(wheelRows(0.4, 1, 16, 7)).toEqual({ rows: 1, remainder: 0 });
    expect(wheelRows(-0.4, 1, 16, 7)).toEqual({ rows: -1, remainder: 0 });
    expect(wheelRows(3, 1, 16, 0)).toEqual({ rows: 3, remainder: 0 });
    expect(wheelRows(0, 1, 16, 0)).toEqual({ rows: 0, remainder: 0 });
  });

  it("moves nothing while the row height is unknown", () => {
    expect(wheelRows(30, 0, 0, 5)).toEqual({ rows: 0, remainder: 5 });
  });
});

describe("wheelModifiers", () => {
  it("packs crossterm's bitset", () => {
    expect(wheelModifiers({ shiftKey: true, ctrlKey: false, altKey: false, metaKey: false })).toBe(1);
    expect(wheelModifiers({ shiftKey: false, ctrlKey: true, altKey: true, metaKey: true })).toBe(14);
  });
});
