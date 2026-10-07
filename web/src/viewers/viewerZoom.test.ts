import { describe, expect, it } from "vitest";
import {
  anchoredScroll,
  boxesNear,
  columnLayout,
  drawScale,
  imageFit,
  MAX_PAGE_PIXELS,
  pdfFit,
  registerViewerZoom,
  stepZoom,
  wheelZoomFactor,
  zoomViewer,
  type Layout,
  type Point,
} from "./viewerZoom";

const viewport = { width: 1000, height: 800 };

/** Where `pointer` falls in box `index`, as fractions of its size: the point a zoom must keep under the pointer. */
function pointUnder(layout: Layout, scroll: Point, pointer: Point, index: number): Point {
  const box = layout.boxes[index]!;
  return { x: (scroll.x + pointer.x - box.left) / box.width, y: (scroll.y + pointer.y - box.top) / box.height };
}

function expectSamePoint(actual: Point, expected: Point): void {
  expect(actual.x).toBeCloseTo(expected.x, 9);
  expect(actual.y).toBeCloseTo(expected.y, 9);
}

describe("viewer zoom range and steps", () => {
  it("steps in Chrome's levels from the fit, holds 25% to 800%, and ⌘0 returns to the fit", () => {
    expect(stepZoom(1, "in")).toBe(1.1);
    expect(stepZoom(1, "out")).toBe(0.9);
    expect(stepZoom(0.25, "out")).toBe(0.25);
    expect(stepZoom(6, "out")).toBe(5);
    expect(stepZoom(8, "in")).toBe(8);
    expect(stepZoom(3.7, "reset")).toBe(1);
  });

  it("reads a pinch as its own scale and holds a mouse notch to about one step", () => {
    expect(wheelZoomFactor(-100 * Math.log(1.1), 0)).toBeCloseTo(1.1);
    expect(wheelZoomFactor(100 * Math.log(1.1), 0)).toBeCloseTo(1 / 1.1);
    expect(wheelZoomFactor(-120, 0)).toBeCloseTo(Math.exp(0.25));
    expect(wheelZoomFactor(3, 1)).toBeCloseTo(Math.exp(-0.25));
  });
});

describe("viewer fit", () => {
  it("fits a large image whole inside the gutter and never enlarges a small one", () => {
    expect(imageFit({ width: 2000, height: 1000 }, { width: 1032, height: 532 })).toBe(0.5);
    expect(imageFit({ width: 1000, height: 4000 }, { width: 1032, height: 1032 })).toBe(0.25);
    expect(imageFit({ width: 10, height: 10 }, viewport)).toBe(1);
  });

  it("fits a PDF page to the width inside the gutter, between half and twice its size", () => {
    expect(pdfFit({ width: 1000, height: 1400 }, 632)).toBe(0.6);
    expect(pdfFit({ width: 200, height: 200 }, 632)).toBe(2);
    expect(pdfFit({ width: 2000, height: 2000 }, 632)).toBe(0.5);
  });
});

describe("viewer layout", () => {
  it("centres content that fits and starts content that does not at the gutter", () => {
    expect(columnLayout([{ width: 400, height: 200 }], viewport, "center")).toEqual({ width: 1000, height: 800, boxes: [{ left: 300, top: 300, width: 400, height: 200 }] });
    expect(columnLayout([{ width: 1600, height: 1200 }], viewport, "center")).toEqual({ width: 1632, height: 1232, boxes: [{ left: 16, top: 16, width: 1600, height: 1200 }] });
  });

  it("stacks PDF pages from the top with a gap, each centred across the widest", () => {
    const layout = columnLayout([{ width: 600, height: 800 }, { width: 400, height: 300 }], { width: 1000, height: 600 }, "top");
    expect(layout).toEqual({
      width: 1000,
      height: 1148,
      boxes: [{ left: 200, top: 16, width: 600, height: 800 }, { left: 300, top: 832, width: 400, height: 300 }],
    });
  });
});

describe("zooming around the pointer", () => {
  it("keeps the image point under the pointer while the image grows past the viewport", () => {
    const before = columnLayout([{ width: 400, height: 400 }], viewport, "center");
    const after = columnLayout([{ width: 1600, height: 1600 }], viewport, "center");
    const pointer = { x: 400, y: 350 };
    const fraction = pointUnder(before, { x: 0, y: 0 }, pointer, 0);
    const scroll = anchoredScroll(before, after, { x: 0, y: 0 }, pointer, viewport);
    expect(scroll).toEqual({ x: 16, y: 266 });
    expectSamePoint(pointUnder(after, scroll, pointer, 0), fraction);
  });

  it("keeps a point on the second PDF page under the pointer, though the gaps do not scale", () => {
    const pages = [{ width: 600, height: 800 }, { width: 600, height: 800 }];
    const at = (zoom: number) => columnLayout(pages.map((page) => ({ width: page.width * zoom, height: page.height * zoom })), viewport, "top");
    const scroll = { x: 0, y: 600 };
    const pointer = { x: 500, y: 500 };
    const fraction = pointUnder(at(1), scroll, pointer, 1);
    const next = anchoredScroll(at(1), at(2), scroll, pointer, viewport);
    expect(next.x).toBeCloseTo(116);
    expect(next.y).toBeCloseTo(1668);
    expectSamePoint(pointUnder(at(2), next, pointer, 1), fraction);
  });

  it("stops at the scroll range's edges, as the browser would", () => {
    const before = columnLayout([{ width: 1600, height: 1600 }], viewport, "center");
    const after = columnLayout([{ width: 400, height: 400 }], viewport, "center");
    expect(anchoredScroll(before, after, { x: 600, y: 800 }, { x: 10, y: 10 }, viewport)).toEqual({ x: 0, y: 0 });
    // The pointer on the far gutter of an image scrolled to its end.
    const larger = columnLayout([{ width: 3200, height: 3200 }], viewport, "center");
    expect(anchoredScroll(before, larger, { x: 632, y: 832 }, { x: 990, y: 790 }, viewport)).toEqual({ x: 2232, y: 2432 });
  });
});

describe("which PDF pages are drawn", () => {
  const pages = columnLayout(Array.from({ length: 50 }, () => ({ width: 600, height: 784 })), viewport, "top");

  it("draws the pages in view and one view either side, nearest the middle first", () => {
    // Page n (from 0) spans 16 + 800n to 800 + 800n; the view is 4000 to 4800.
    expect(boxesNear(pages, 4000, 800, 0)).toEqual([5, 4]);
    expect(boxesNear(pages, 4000, 800, 800)).toEqual([5, 4, 6, 3]);
    expect(boxesNear(pages, 0, 800, 800)).toEqual([0, 1]);
    expect(boxesNear({ width: 0, height: 0, boxes: [] }, 0, 800, 800)).toEqual([]);
  });

  it("draws at the device pixel ratio and keeps the canvas inside pdf.js's bound", () => {
    const letter = { width: 612, height: 792 };
    expect(drawScale(letter, 1.5, 2)).toBe(3);
    const capped = drawScale(letter, 10, 2);
    expect(capped).toBeLessThan(20);
    expect(letter.width * letter.height * capped * capped).toBeCloseTo(MAX_PAGE_PIXELS);
    expect(drawScale({ width: 40000, height: 10 }, 1, 1)).toBeCloseTo(32767 / 40000);
  });
});

describe("the text-size commands reach a viewer", () => {
  it("only while a viewer for the display can zoom, and a stale release keeps the newer viewer", () => {
    const steps: string[] = [];
    expect(zoomViewer("d1", "in")).toBe(false);
    const first = registerViewerZoom("d1", (step) => steps.push(`first:${step}`));
    expect(zoomViewer("d1", "in")).toBe(true);
    const second = registerViewerZoom("d1", (step) => steps.push(`second:${step}`));
    first();
    expect(zoomViewer("d1", "reset")).toBe(true);
    second();
    expect(zoomViewer("d1", "out")).toBe(false);
    expect(steps).toEqual(["first:in", "second:reset"]);
  });
});
