// The zoom one viewer display holds (`viewerZoom.ts`) and the inputs that
// move it: a pinch or a Ctrl wheel over the viewer, around the pointer, and
// the text-size commands while its display holds the keyboard, around the
// middle. A burst of wheel events zooms once per animation frame.

import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { flushSync } from "react-dom";
import { anchoredScroll, clampZoom, registerViewerZoom, stepZoom, wheelZoomFactor, type Layout, type Point, type Size } from "./viewerZoom";

/** The content laid out at a zoom for a viewport, or null while the viewer cannot lay it out yet. */
export type LayoutAt = ((zoom: number, viewport: Size) => Layout) | null;

export type ViewerZoom = {
  zoom: number;
  /** The content as drawn now, or null until the viewer and its viewport are measured. */
  layout: Layout | null;
  /** The scroller's client size, measured on every resize. */
  viewport: Size | null;
  scroller: HTMLDivElement | null;
  /** The ref of the element that scrolls the content. */
  scrollerRef: (element: HTMLDivElement | null) => void;
};

export function useViewerZoom(displayId: string, layoutAt: LayoutAt): ViewerZoom {
  const [scroller, scrollerRef] = useState<HTMLDivElement | null>(null);
  const [viewport, setViewport] = useState<Size | null>(null);
  const [zoom, setZoom] = useState(1);
  const layout = useMemo(() => (layoutAt && viewport ? layoutAt(zoom, viewport) : null), [layoutAt, viewport, zoom]);
  // The latest of each for the listeners, which outlive a render.
  const latest = useRef({ zoom, viewport, layoutAt });
  useLayoutEffect(() => {
    latest.current = { zoom, viewport, layoutAt };
  });

  useLayoutEffect(() => {
    if (!scroller) return undefined;
    const measure = () => setViewport((old) => (old?.width === scroller.clientWidth && old.height === scroller.clientHeight ? old : { width: scroller.clientWidth, height: scroller.clientHeight }));
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(scroller);
    return () => observer.disconnect();
  }, [scroller]);

  /** Zooms to `next` keeping the content under `pointer` (the viewport's middle when null) where it is. */
  const zoomTo = useCallback((next: number, pointer: Point | null) => {
    const { zoom: now, viewport: size, layoutAt: lay } = latest.current;
    const target = clampZoom(next);
    if (!scroller || !size || !lay || target === now) return;
    const at = pointer ?? { x: size.width / 2, y: size.height / 2 };
    const scroll = anchoredScroll(lay(now, size), lay(target, size), { x: scroller.scrollLeft, y: scroller.scrollTop }, at, size);
    // The new size and its scroll land in one frame, so the content never
    // shows at the new size from the old scroll position.
    flushSync(() => setZoom(target));
    scroller.scrollLeft = scroll.x;
    scroller.scrollTop = scroll.y;
  }, [scroller]);

  const ready = layout !== null;
  useEffect(() => {
    if (!scroller || !ready) return undefined;
    let factor = 1;
    let client: Point | null = null;
    let frame = 0;
    const onWheel = (event: WheelEvent) => {
      // A pinch arrives as a Ctrl wheel; a plain wheel scrolls.
      if (!event.ctrlKey) return;
      event.preventDefault();
      factor *= wheelZoomFactor(event.deltaY, event.deltaMode);
      client = { x: event.clientX, y: event.clientY };
      if (frame) return;
      frame = requestAnimationFrame(() => {
        frame = 0;
        const rect = scroller.getBoundingClientRect();
        const pointer = client ? { x: client.x - rect.left - scroller.clientLeft, y: client.y - rect.top - scroller.clientTop } : null;
        const by = factor;
        factor = 1;
        zoomTo(latest.current.zoom * by, pointer);
      });
    };
    scroller.addEventListener("wheel", onWheel, { passive: false });
    return () => {
      scroller.removeEventListener("wheel", onWheel);
      cancelAnimationFrame(frame);
    };
  }, [scroller, ready, zoomTo]);

  useEffect(() => {
    if (!ready) return undefined;
    return registerViewerZoom(displayId, (step) => zoomTo(stepZoom(latest.current.zoom, step), null));
  }, [displayId, ready, zoomTo]);

  return { zoom, layout, viewport, scroller, scrollerRef };
}
