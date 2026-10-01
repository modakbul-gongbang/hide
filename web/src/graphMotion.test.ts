import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { sameTargets, Tween, type Targets } from "./graphMotion";

// A picture of the graph is a map of named numbers; the tween owns when each
// is painted. Frames here are driven by hand so a test sees every one.

let frames: ((now: number) => void)[] = [];

beforeEach(() => {
  frames = [];
  vi.stubGlobal("requestAnimationFrame", (callback: (now: number) => void) => frames.push(callback));
  vi.stubGlobal("cancelAnimationFrame", () => {
    frames = [];
  });
  vi.spyOn(performance, "now").mockReturnValue(0);
});

afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

function picture(entries: Record<string, number>): Targets {
  return new Map(Object.entries(entries));
}

function tween(reduced = false) {
  const painted: Targets[] = [];
  const motion = new Tween({ durationMs: 100, reduced: () => reduced, paint: (values) => painted.push(new Map(values)) });
  return { motion, painted };
}

describe("the graph's tween", () => {
  it("draws the first picture at once and asks for no frame", () => {
    const { motion, painted } = tween();
    const result = motion.retarget(picture({ "bx:a": 10, "by:a": 20 }));
    expect(result.changed).toBe(true);
    expect(painted).toHaveLength(1);
    expect(painted[0]!.get("bx:a")).toBe(10);
    expect(frames).toHaveLength(0);
    expect(motion.frames).toBe(0);
  });

  it("does nothing for a picture equal to the one on screen, however it was computed", () => {
    const { motion, painted } = tween();
    motion.retarget(picture({ "bx:a": 10 }));
    const result = motion.retarget(picture({ "bx:a": 10.004 }));
    expect(result).toEqual({ changed: false, added: [] });
    expect(painted).toHaveLength(1);
    expect(frames).toHaveLength(0);
    expect(motion.revision).toBe(1);
  });

  it("glides an existing number to its new place over the duration, then rests", () => {
    const { motion, painted } = tween();
    motion.retarget(picture({ "bx:a": 0 }));
    motion.retarget(picture({ "bx:a": 100 }));
    expect(motion.running).toBe(true);
    frames.shift()!(50);
    const middle = painted.at(-1)!.get("bx:a")!;
    expect(middle).toBeGreaterThan(0);
    expect(middle).toBeLessThan(100);
    frames.shift()!(100);
    expect(painted.at(-1)!.get("bx:a")).toBe(100);
    expect(motion.running).toBe(false);
    expect(frames).toHaveLength(0);
  });

  it("puts a number new to the picture at its place at once and names it, while the old ones glide", () => {
    const { motion, painted } = tween();
    motion.retarget(picture({ "bx:a": 0 }));
    const result = motion.retarget(picture({ "bx:a": 100, "bx:b": 40 }));
    expect(result.added).toEqual(["bx:b"]);
    expect(painted.at(-1)!.get("bx:b")).toBe(40);
    expect(painted.at(-1)!.get("bx:a")).toBe(0);
  });

  it("retargets mid-flight from where the numbers are now, not from where they began", () => {
    const { motion, painted } = tween();
    motion.retarget(picture({ "bx:a": 0 }));
    motion.retarget(picture({ "bx:a": 100 }));
    frames.shift()!(50);
    const now = painted.at(-1)!.get("bx:a")!;
    motion.retarget(picture({ "bx:a": 0 }));
    expect(painted.at(-1)!.get("bx:a")).toBe(now);
  });

  it("jumps when the system asks for less motion", () => {
    const { motion, painted } = tween(true);
    motion.retarget(picture({ "bx:a": 0 }));
    motion.retarget(picture({ "bx:a": 100 }));
    expect(painted.at(-1)!.get("bx:a")).toBe(100);
    expect(frames).toHaveLength(0);
  });

  it("compares pictures by name, so a missing or extra number is a change", () => {
    expect(sameTargets(picture({ a: 1 }), picture({ a: 1 }))).toBe(true);
    expect(sameTargets(picture({ a: 1 }), picture({ b: 1 }))).toBe(false);
    expect(sameTargets(picture({ a: 1 }), picture({ a: 1, b: 2 }))).toBe(false);
  });
});
