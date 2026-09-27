import { describe, expect, it } from "vitest";
import { draggedSidebarWidth, sidebarWidthToSend } from "./sidebarWidth";

const bounds = { min: 220, max: 440 };

describe("draggedSidebarWidth", () => {
  it("follows the pointer between the bounds", () => {
    expect(draggedSidebarWidth(292, 40, bounds)).toBe(332);
    expect(draggedSidebarWidth(292, -50.4, bounds)).toBe(242);
  });

  it("stops at the bounds when the pointer passes them (D-18)", () => {
    expect(draggedSidebarWidth(292, -200, bounds)).toBe(220);
    expect(draggedSidebarWidth(292, 500, bounds)).toBe(440);
    expect(draggedSidebarWidth(230, -10.6, bounds)).toBe(220);
  });
});

describe("sidebarWidthToSend", () => {
  it("sends only a width that moved", () => {
    expect(sidebarWidthToSend(292, 292)).toBeNull();
    expect(sidebarWidthToSend(360, 292)).toBe(292);
  });
});
