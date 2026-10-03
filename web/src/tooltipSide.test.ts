import { describe, expect, it } from "vitest";
import { hintSide } from "./tooltipSide";

describe("a hint beside a browser page", () => {
  // The window of the desktop e2e: a page tab at the top edge, its page below.
  const viewport = { width: 1024, height: 640 };
  const page = { x: 624, y: 66, width: 398, height: 540 };
  const tab = { x: 624, y: 0, width: 180, height: 32 };
  const size = { width: 360, height: 60 };
  const base = { side: "top" as const, align: "center" as const, offset: 4, trigger: tab, size, viewport };

  it("keeps its own side while no page is shown", () => {
    expect(hintSide({ ...base, pages: [] })).toBe("top");
  });

  it("skips a side with no room and one that meets the page, and opens on the first that does neither", () => {
    // Top has no room above the tab, bottom meets the page, right leaves the window.
    expect(hintSide({ ...base, pages: [page] })).toBe("left");
  });

  it("stays on its own side when it fits there clear of every page", () => {
    expect(hintSide({ ...base, trigger: { x: 100, y: 200, width: 180, height: 32 }, pages: [page] })).toBe("top");
  });

  it("is held inside the window along its trigger before it is checked against a page", () => {
    // Centred on a tab at the right edge, a bottom hint would leave the
    // window; held inside it, it lies over a page to its left.
    const right = { x: 900, y: 0, width: 120, height: 32 };
    expect(hintSide({ ...base, trigger: right, pages: [{ x: 670, y: 70, width: 50, height: 100 }] })).toBe("left");
  });

  it("opens on its own side when every side that fits meets a page", () => {
    expect(hintSide({ ...base, trigger: { ...tab, y: 300 }, pages: [{ x: 0, y: 0, width: 1024, height: 640 }] })).toBe("top");
  });

  it("tries the opposite side before the two across", () => {
    const middle = { x: 400, y: 300, width: 100, height: 30 };
    expect(hintSide({ ...base, trigger: middle, pages: [{ x: 0, y: 0, width: 1024, height: 290 }] })).toBe("bottom");
    expect(hintSide({ ...base, side: "left", trigger: middle, size: { width: 120, height: 30 }, pages: [{ x: 0, y: 0, width: 400, height: 640 }] })).toBe("right");
  });
});
