import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { Flow, sameTargets, Tween, type Targets } from "./graphMotion";

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

describe("the flowing dashes", () => {
  function path() {
    const offsets: string[] = [];
    return { offsets, setAttribute: (_name: string, value: string) => offsets.push(value) };
  }

  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("steps the pattern a quarter period at a time and starts over after a whole one", () => {
    const flow = new Flow({ period: 18, stepMs: 250, steps: 4, reduced: () => false });
    const a = path();
    flow.set([a]);
    expect(a.offsets).toEqual(["0"]);
    vi.advanceTimersByTime(1000);
    expect(a.offsets).toEqual(["0", "-4.5", "-9", "-13.5", "0"]);
    flow.dispose();
  });

  it("runs no timer while no line works, and no frame is ever asked for", () => {
    const flow = new Flow({ period: 18, stepMs: 250, steps: 4, reduced: () => false });
    flow.set([]);
    expect(vi.getTimerCount()).toBe(0);
    flow.set([path()]);
    expect(vi.getTimerCount()).toBe(1);
    flow.set([]);
    expect(vi.getTimerCount()).toBe(0);
    expect(frames).toHaveLength(0);
  });

  it("stands still when the system asks for less motion, also when it asks after the dashes began", () => {
    let reduced = false;
    const flow = new Flow({ period: 18, stepMs: 250, steps: 4, reduced: () => reduced });
    const a = path();
    flow.set([a]);
    reduced = true;
    vi.advanceTimersByTime(500);
    expect(a.offsets).toEqual(["0"]);
    expect(vi.getTimerCount()).toBe(0);
    flow.set([a]);
    expect(vi.getTimerCount()).toBe(0);
    // The setting is turned off again: the dashes go on from where the line is.
    reduced = false;
    flow.resume();
    expect(vi.getTimerCount()).toBe(1);
    vi.advanceTimersByTime(250);
    expect(a.offsets.at(-1)).toBe("-4.5");
  });

  it("says whether the timer runs: on with a working line, off with none, off under less motion, on again when the setting returns", () => {
    let reduced = false;
    const told: boolean[] = [];
    const flow = new Flow({ period: 18, stepMs: 250, steps: 4, reduced: () => reduced, running: (running) => told.push(running) });
    const a = path();
    flow.set([]);
    expect(told.at(-1)).toBe(false);
    flow.set([a]);
    expect(told.at(-1)).toBe(true);
    // A new canvas holding the same line is told again while the timer goes on.
    flow.set([a]);
    expect(told.at(-1)).toBe(true);
    expect(vi.getTimerCount()).toBe(1);
    reduced = true;
    vi.advanceTimersByTime(250);
    expect(told.at(-1)).toBe(false);
    expect(vi.getTimerCount()).toBe(0);
    reduced = false;
    flow.resume();
    expect(told.at(-1)).toBe(true);
    expect(vi.getTimerCount()).toBe(1);
    flow.dispose();
    expect(told.at(-1)).toBe(false);
    expect(vi.getTimerCount()).toBe(0);
  });
});
