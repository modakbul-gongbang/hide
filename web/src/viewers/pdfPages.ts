// Draws a PDF's pages into the boxes the viewer lays out, the way pdf.js's
// own viewer does: only the pages in view and one view either side, at most
// `MAX_DRAWN_PAGES` of them, each at the scale it is shown times the device
// pixel ratio, one page at a time. A zoom or a resize first stretches the
// canvases already drawn and draws again once the input has settled; a
// scroll draws the pages it brings in on the next frame. A page that leaves
// gives its canvas back at once.

import type { PDFPageProxy, RenderTask } from "pdfjs-dist";
import { boxesNear, drawScale, MAX_DRAWN_PAGES, type Layout, type Size } from "./viewerZoom";

/** How long a zoom or resize must rest before the pages are drawn again at the new scale. */
export const SETTLE_MS = 200;

type Drawing = { index: number; scale: number; task: RenderTask };

export class PdfPages {
  private layout: Layout | null = null;
  /** The scale each drawn page's canvas holds. */
  private readonly drawn = new Map<number, number>();
  /** The pages to draw, nearest the middle first, with the scale each needs. */
  private queue: [number, number][] = [];
  private drawing: Drawing | null = null;
  private settle = 0;
  private frame = 0;
  private closed = false;
  private overNoted = false;

  constructor(
    private readonly pages: readonly PDFPageProxy[],
    private readonly sizes: readonly Size[],
    private readonly scroller: HTMLElement,
    private readonly failed: () => void,
    private readonly note: (message: string) => void,
  ) {
    scroller.addEventListener("scroll", this.onScroll, { passive: true });
  }

  /** The boxes as laid out now: the first layout is drawn at once, a later one once it has settled. */
  show(layout: Layout): void {
    const first = this.layout === null;
    this.layout = layout;
    if (first) {
      this.plan();
      return;
    }
    window.clearTimeout(this.settle);
    this.settle = window.setTimeout(() => {
      this.settle = 0;
      this.plan();
    }, SETTLE_MS);
  }

  close(): void {
    this.closed = true;
    window.clearTimeout(this.settle);
    cancelAnimationFrame(this.frame);
    this.scroller.removeEventListener("scroll", this.onScroll);
    this.drawing?.task.cancel();
    this.drawing = null;
    this.queue = [];
    for (const index of [...this.drawn.keys()]) this.release(index);
  }

  private readonly onScroll = () => {
    // A zoom still settling draws everything once it has.
    if (this.settle || this.frame) return;
    this.frame = requestAnimationFrame(() => {
      this.frame = 0;
      this.plan();
    });
  };

  private plan(): void {
    const layout = this.layout;
    if (this.closed || !layout) return;
    const { scrollTop, clientHeight } = this.scroller;
    const inView = boxesNear(layout, scrollTop, clientHeight, 0).length;
    if (inView > MAX_DRAWN_PAGES && !this.overNoted) {
      this.overNoted = true;
      this.note(`pdf_view.page_cap: ${inView} pages in view, ${MAX_DRAWN_PAGES} drawn`);
    }
    const ratio = window.devicePixelRatio || 1;
    const wanted = new Map<number, number>();
    for (const index of boxesNear(layout, scrollTop, clientHeight, clientHeight).slice(0, MAX_DRAWN_PAGES)) {
      const size = this.sizes[index];
      const box = layout.boxes[index];
      if (size && box) wanted.set(index, drawScale(size, box.width / size.width, ratio));
    }
    for (const index of [...this.drawn.keys()]) if (!wanted.has(index)) this.release(index);
    if (this.drawing && wanted.get(this.drawing.index) !== this.drawing.scale) {
      this.drawing.task.cancel();
      this.drawing = null;
    }
    this.queue = [...wanted].filter(([index, scale]) => this.drawn.get(index) !== scale && this.drawing?.index !== index);
    this.pump();
  }

  private pump(): void {
    if (this.closed || this.drawing) return;
    const next = this.queue.shift();
    const page = next && this.pages[next[0]];
    if (!next || !page) return;
    const [index, scale] = next;
    const viewport = page.getViewport({ scale });
    const canvas = document.createElement("canvas");
    canvas.width = Math.max(1, Math.floor(viewport.width));
    canvas.height = Math.max(1, Math.floor(viewport.height));
    canvas.className = "block size-full";
    const task = page.render({ canvas, viewport });
    const drawing = { index, scale, task };
    this.drawing = drawing;
    task.promise.then(
      () => {
        if (this.drawing !== drawing) return;
        this.drawing = null;
        this.place(index, canvas);
        this.drawn.set(index, scale);
        this.pump();
      },
      (error: unknown) => {
        canvas.width = 0;
        canvas.height = 0;
        if (this.drawing === drawing) this.drawing = null;
        if (cancelled(error)) {
          this.pump();
          return;
        }
        if (!this.closed) this.failed();
      },
    );
  }

  /** Puts a finished canvas in its page's box, so the page never shows blank while it is drawn again. */
  private place(index: number, canvas: HTMLCanvasElement): void {
    const box = this.box(index);
    if (!box) {
      canvas.width = 0;
      canvas.height = 0;
      return;
    }
    for (const old of box.querySelectorAll("canvas")) free(old);
    box.replaceChildren(canvas);
  }

  private release(index: number): void {
    this.drawn.delete(index);
    for (const canvas of this.box(index)?.querySelectorAll("canvas") ?? []) {
      free(canvas);
      canvas.remove();
    }
  }

  private box(index: number): HTMLElement | null {
    return this.scroller.querySelector<HTMLElement>(`[data-pdf-page="${index + 1}"]`);
  }
}

/** Gives a canvas's pixels back now rather than when it is collected. */
function free(canvas: HTMLCanvasElement): void {
  canvas.width = 0;
  canvas.height = 0;
}

function cancelled(error: unknown): boolean {
  return error instanceof Error && error.name === "RenderingCancelledException";
}
