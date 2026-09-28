import { useLayoutEffect, useState, type RefObject } from "react";

/** One drawn line between two measured nodes. */
export type MeasuredPath = { id: string; d: string };

/**
 * Lines drawn between nodes a board laid out with CSS, measured after layout
 * against `root` and again whenever the root or a node changes size (the
 * Dependencies mode's arrows, the Overview's delegation lines). `selector`
 * finds the nodes to watch; `compute` turns the root's rect and a node lookup
 * into the paths. Measuring is local layout: it publishes nothing.
 */
export function useMeasuredPaths(
  root: RefObject<HTMLElement | null>,
  selector: string,
  compute: (origin: DOMRect, at: (attribute: string, id: string) => DOMRect | null) => MeasuredPath[],
  deps: readonly unknown[],
): MeasuredPath[] {
  const [paths, setPaths] = useState<MeasuredPath[]>([]);
  useLayoutEffect(() => {
    const box = root.current;
    if (!box) return;
    const measure = () => {
      const origin = box.getBoundingClientRect();
      const at = (attribute: string, id: string) => box.querySelector(`[${attribute}="${CSS.escape(id)}"]`)?.getBoundingClientRect() ?? null;
      const next = compute(origin, at);
      setPaths((current) => (JSON.stringify(current) === JSON.stringify(next) ? current : next));
    };
    measure();
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(measure);
    observer.observe(box);
    for (const node of box.querySelectorAll(selector)) observer.observe(node);
    return () => observer.disconnect();
    // `compute` is recreated each render; the caller's deps say when it changes.
  }, deps);
  return paths;
}
