// How the image and PDF viewers zoom: the way a browser zooms, around the
// point under the pointer for a pinch or a Ctrl wheel, and in Chrome's steps
// for ⌘= / ⌘- / ⌘0. A zoom is a factor of the viewer's fit, so 1 is the size a
// document opens at and ⌘0 returns to it. It is a view of the moment kept by
// the display that draws it: nothing reaches the core, and a reopened
// document fits again.

import { nextZoomFactor, type PageZoom } from "../zoomSteps";

export const MIN_VIEWER_ZOOM = 0.25;
export const MAX_VIEWER_ZOOM = 8;
/** The space around a viewer's content (`--spacing-lg`). */
export const GUTTER = 16;
/** The space between two PDF pages (`--spacing-lg`). */
export const PAGE_GAP = 16;
/**
 * The most PDF pages that hold a drawn canvas at once, pdf.js's own page
 * buffer (`DEFAULT_CACHE_SIZE`); the pages nearest the middle of the view
 * are drawn first and the others stay blank.
 */
export const MAX_DRAWN_PAGES = 10;
/** pdf.js's bound on one page's canvas (`maxCanvasPixels`); past it the page is drawn smaller and stretched, as pdf.js does. */
export const MAX_PAGE_PIXELS = 2 ** 25;
/** pdf.js's and Chromium's longest canvas side (`maxCanvasDim`). */
const MAX_CANVAS_SIDE = 32767;
/**
 * The largest zoom one wheel event makes. Chromium sends a trackpad pinch as
 * a Ctrl wheel whose deltaY is -100·ln(scale), so exp(-deltaY / 100) is the
 * pinch itself; a mouse notch (about 100 px) is held to about one Chrome step.
 */
const MAX_WHEEL_DELTA = 25;

export type Size = { width: number; height: number };
export type Point = { x: number; y: number };
export type Box = { left: number; top: number; width: number; height: number };
/** Content as a scroller holds it: its whole size and each box in it, in CSS pixels from its top left. */
export type Layout = { width: number; height: number; boxes: Box[] };

export function clampZoom(zoom: number): number {
  return Math.min(MAX_VIEWER_ZOOM, Math.max(MIN_VIEWER_ZOOM, zoom));
}

/** ⌘= and ⌘- move one of Chrome's steps, ⌘0 returns to the fit. */
export function stepZoom(zoom: number, step: PageZoom): number {
  return clampZoom(nextZoomFactor(zoom, step));
}

/** The factor one Ctrl wheel event zooms by; a line or page delta counts as one mouse notch. */
export function wheelZoomFactor(deltaY: number, deltaMode: number): number {
  const pixels = deltaMode === 0 ? deltaY : Math.sign(deltaY) * MAX_WHEEL_DELTA;
  return Math.exp(-Math.max(-MAX_WHEEL_DELTA, Math.min(MAX_WHEEL_DELTA, pixels)) / 100);
}

/** The scale an image opens at: whole inside the viewport and its gutter, never enlarged. */
export function imageFit(image: Size, viewport: Size): number {
  return Math.min(1, room(viewport.width) / image.width, room(viewport.height) / image.height);
}

/** The scale a PDF page opens at: the viewport's width inside its gutter, between half and twice the page. */
export function pdfFit(page: Size, viewportWidth: number): number {
  return Math.min(2, Math.max(0.5, room(viewportWidth) / page.width));
}

function room(length: number): number {
  return Math.max(1, length - 2 * GUTTER);
}

/**
 * Boxes stacked top to bottom with `PAGE_GAP` between them, each centred
 * across the content, inside `GUTTER`. Content smaller than the viewport
 * fills it, so a box that fits is centred (or, with `align: "top"`, starts
 * at the top), and a box that does not starts at the gutter and scrolls to
 * each edge.
 */
export function columnLayout(sizes: readonly Size[], viewport: Size, align: "center" | "top"): Layout {
  let widest = 0;
  let column = 0;
  for (const [index, size] of sizes.entries()) {
    widest = Math.max(widest, size.width);
    column += size.height + (index > 0 ? PAGE_GAP : 0);
  }
  const width = Math.max(viewport.width, widest + 2 * GUTTER);
  const height = Math.max(viewport.height, column + 2 * GUTTER);
  let top = align === "center" ? (height - column) / 2 : GUTTER;
  const boxes = sizes.map((size) => {
    const box = { left: (width - size.width) / 2, top, width: size.width, height: size.height };
    top += size.height + PAGE_GAP;
    return box;
  });
  return { width, height, boxes };
}

/**
 * The scroll position after a relayout that keeps the content under
 * `pointer` (a point in the viewport) under it. The point is held as a
 * fraction of its box, the first box that reaches down to it, so a gap or a
 * gutter that does not scale does not move it; the answer is held to the
 * scroll range, as the browser holds it.
 */
export function anchoredScroll(before: Layout, after: Layout, scroll: Point, pointer: Point, viewport: Size): Point {
  const x = scroll.x + pointer.x;
  const y = scroll.y + pointer.y;
  const index = boxReaching(before.boxes, y);
  const from = before.boxes[index];
  const to = after.boxes[index];
  if (!from || !to) return heldTo(scroll, after, viewport);
  const across = from.width > 0 ? (x - from.left) / from.width : 0.5;
  const down = from.height > 0 ? (y - from.top) / from.height : 0.5;
  return heldTo({ x: to.left + across * to.width - pointer.x, y: to.top + down * to.height - pointer.y }, after, viewport);
}

function heldTo(scroll: Point, layout: Layout, viewport: Size): Point {
  return {
    x: Math.min(Math.max(0, layout.width - viewport.width), Math.max(0, scroll.x)),
    y: Math.min(Math.max(0, layout.height - viewport.height), Math.max(0, scroll.y)),
  };
}

/** The first box whose bottom reaches `y`, else the last; boxes run top to bottom. */
function boxReaching(boxes: readonly Box[], y: number): number {
  let low = 0;
  let high = boxes.length - 1;
  while (low < high) {
    const middle = (low + high) >> 1;
    const box = boxes[middle];
    if (box && box.top + box.height >= y) high = middle;
    else low = middle + 1;
  }
  return Math.max(0, low);
}

/**
 * The boxes that meet the viewport widened by `margin` above and below,
 * nearest the viewport's middle first: the PDF pages worth drawing now.
 */
export function boxesNear(layout: Layout, scrollTop: number, viewportHeight: number, margin: number): number[] {
  const top = scrollTop - margin;
  const bottom = scrollTop + viewportHeight + margin;
  const near: number[] = [];
  for (let index = boxReaching(layout.boxes, top); index < layout.boxes.length; index += 1) {
    const box = layout.boxes[index];
    if (!box || box.top > bottom) break;
    if (box.top + box.height >= top) near.push(index);
  }
  const middle = scrollTop + viewportHeight / 2;
  const distance = (index: number) => {
    const box = layout.boxes[index];
    return box ? Math.abs(box.top + box.height / 2 - middle) : Infinity;
  };
  return near.sort((a, b) => distance(a) - distance(b));
}

/**
 * The scale a PDF page is drawn at: the scale it is shown at times the
 * device pixel ratio, so it stays sharp, lowered to keep its canvas inside
 * `MAX_PAGE_PIXELS` and the longest canvas side.
 */
export function drawScale(page: Size, shownScale: number, pixelRatio: number): number {
  const wanted = shownScale * pixelRatio;
  const byArea = Math.sqrt(MAX_PAGE_PIXELS / (page.width * page.height));
  const bySide = MAX_CANVAS_SIDE / Math.max(page.width, page.height);
  return Math.min(wanted, byArea, bySide);
}

type ZoomCommand = (step: PageZoom) => void;
/** One entry per mounted viewer that can zoom, so at most one per drawn View area. */
const commands = new Map<string, ZoomCommand>();

/** A viewer able to zoom takes the text-size commands while its display holds the keyboard. */
export function registerViewerZoom(displayId: string, command: ZoomCommand): () => void {
  commands.set(displayId, command);
  return () => {
    if (commands.get(displayId) === command) commands.delete(displayId);
  };
}

/** Runs a text-size command on the viewer drawn for `displayId`; false when no viewer there can zoom. */
export function zoomViewer(displayId: string, step: PageZoom): boolean {
  const command = commands.get(displayId);
  if (!command) return false;
  command(step);
  return true;
}
