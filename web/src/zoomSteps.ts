// Chrome's page zoom steps, shared by the two surfaces that zoom the way a
// browser does on ⌘= / ⌘- / ⌘0: a browser display's page
// (`desktop/src/main/browser.ts`) and the image and PDF viewers
// (`src/viewers/viewerZoom.ts`).

/** Chrome's page zoom steps (`kPresetZoomFactors`), as zoom factors. */
const PAGE_ZOOM_FACTORS = [0.25, 1 / 3, 0.5, 2 / 3, 0.75, 0.8, 0.9, 1, 1.1, 1.25, 1.5, 1.75, 2, 2.5, 3, 4, 5];
/** How near two factors count as the same step, so a stored 0.333… is 1/3. */
const ZOOM_EPSILON = 0.001;

export type PageZoom = "in" | "out" | "reset";

/** The zoom factor one step from `current`, as Chrome steps it; past either end it stays. */
export function nextZoomFactor(current: number, zoom: PageZoom): number {
  if (zoom === "reset") return 1;
  if (zoom === "in") return PAGE_ZOOM_FACTORS.find((step) => step > current + ZOOM_EPSILON) ?? current;
  return [...PAGE_ZOOM_FACTORS].reverse().find((step) => step < current - ZOOM_EPSILON) ?? current;
}
