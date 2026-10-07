import ELK from "elkjs/lib/elk.bundled.js";
import { describe, expect, it, vi } from "vitest";
import { LAYOUT_NODE_LIMIT, edgePath, placeGraph, placeWithin, type NodeBox } from "./dependencyLayout";

const SPACING = { betweenLayers: 96, betweenNodes: 24 };
const card = (id: string): NodeBox => ({ id, width: 288, height: 120 });

/** Points along a path drawn by `edgePath`: its move, then each cubic step sampled. */
function samples(d: string): { x: number; y: number }[] {
  const numbers = d.match(/-?\d+(\.\d+)?/g)!.map(Number);
  let at = { x: numbers[0]!, y: numbers[1]! };
  const points = [at];
  for (let index = 2; index + 5 < numbers.length + 0.5; index += 6) {
    const [c1x, c1y, c2x, c2y, x, y] = numbers.slice(index, index + 6) as [number, number, number, number, number, number];
    for (let step = 1; step <= 40; step += 1) {
      const t = step / 40;
      const u = 1 - t;
      points.push({ x: u ** 3 * at.x + 3 * u ** 2 * t * c1x + 3 * u * t ** 2 * c2x + t ** 3 * x, y: u ** 3 * at.y + 3 * u ** 2 * t * c1y + 3 * u * t ** 2 * c2y + t ** 3 * y });
    }
    at = { x, y };
  }
  return points;
}

describe("placeGraph", () => {
  it("routes a long edge around the cards of the columns it passes, not across them", async () => {
    // L-1 waits on nothing; L-2 on L-1; L-3 on L-2; L-4 on L-3; L-5 on L-1 and L-4, so L-1 to L-5 spans three columns.
    const nodes = ["L-1", "L-2", "L-3", "L-4", "L-5", "L-6"].map(card);
    const edges = [
      { from: "L-1", to: "L-2" },
      { from: "L-2", to: "L-3" },
      { from: "L-3", to: "L-4" },
      { from: "L-4", to: "L-5" },
      { from: "L-1", to: "L-5" },
      { from: "L-2", to: "L-6" },
    ];
    const placement = await placeGraph(new ELK(), nodes, edges, SPACING);
    const long = placement.edges.find((edge) => edge.id === "L-1>L-5")!;
    expect(long.d).toMatch(/^M \S+ \S+ C /);
    for (const node of nodes) {
      if (node.id === "L-1" || node.id === "L-5") continue;
      const place = placement.nodes.get(node.id)!;
      const inside = samples(long.d).filter((point) => point.x > place.x && point.x < place.x + node.width && point.y > place.y && point.y < place.y + node.height);
      expect(inside, `the L-1 to L-5 edge crosses ${node.id}`).toEqual([]);
    }
    expect(new Set([...placement.nodes.values()].map((node) => node.layer)).size).toBe(5);
  });

  it("refuses more cards than the layout limit, so the columns stay", async () => {
    const nodes = Array.from({ length: LAYOUT_NODE_LIMIT + 1 }, (_, index) => card(`T-${index}`));
    await expect(placeGraph(new ELK(), nodes, [], SPACING)).rejects.toThrow(/over the layout limit/);
  });
});

describe("placeWithin", () => {
  it("gives up on a layout that does not answer in time and ends its worker", async () => {
    vi.useFakeTimers();
    try {
      const elk = { layout: () => new Promise<never>(() => {}), terminateWorker: vi.fn() };
      const placed = placeWithin(elk, [card("A")], [], SPACING, 50);
      const refused = expect(placed).rejects.toThrow(/longer than 50 ms/);
      await vi.advanceTimersByTimeAsync(51);
      await refused;
      expect(elk.terminateWorker).toHaveBeenCalledOnce();
    } finally {
      vi.useRealTimers();
    }
  });
});

describe("edgePath", () => {
  it("passes through every routed point and leaves and enters each one level", () => {
    expect(edgePath([{ x: 0, y: 0 }, { x: 100, y: 0 }, { x: 200, y: 50 }])).toBe("M 0 0 C 50 0, 50 0, 100 0 C 150 0, 150 50, 200 50");
  });
});
