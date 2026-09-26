import { describe, expect, it } from "vitest";
import { loadable, MAX_SYNCED_DISPLAYS, overCap, parseCommand, parseSync, parseTarget, toBounds } from "./browserSync";

const rect = { x: 0, y: 40, width: 800, height: 600 };
const display = { id: "d1", url: "https://a.test/", load: 3, rect, visible: true };

describe("what the shell may ask of the browser views (issue 155)", () => {
  it("takes a well-formed sync whole", () => {
    const sync = { workspace: "local\u0000/r", displays: [display, { ...display, id: "d2", rect: null, visible: false }] };
    expect(parseSync(sync)).toEqual(sync);
    expect(parseSync({ workspace: null, displays: [] })).toEqual({ workspace: null, displays: [] });
  });

  it("drops a sync whole when any part of it is out of shape", () => {
    const bad: unknown[] = [
      null,
      { workspace: "w", displays: "d1" },
      { workspace: null, displays: [display] },
      { workspace: "", displays: [] },
      { workspace: "w", displays: [display, display] },
      { workspace: "w", displays: [{ ...display, load: -1 }] },
      { workspace: "w", displays: [{ ...display, load: 1.5 }] },
      { workspace: "w", displays: [{ ...display, url: "x".repeat(8193) }] },
      { workspace: "w", displays: [{ ...display, rect: { ...rect, width: Number.NaN } }] },
      { workspace: "w", displays: [{ ...display, rect: { ...rect, height: -1 } }] },
      { workspace: "w", displays: [{ ...display, rect: { ...rect, x: 1e9 } }] },
      { workspace: "w", displays: [{ ...display, visible: "yes" }] },
      { workspace: "w", displays: Array.from({ length: MAX_SYNCED_DISPLAYS + 1 }, (_, i) => ({ ...display, id: `d${i}` })) },
    ];
    for (const value of bad) expect(parseSync(value), JSON.stringify(value)?.slice(0, 80)).toBeNull();
  });

  it("names a page and a toolbar command only in shape", () => {
    expect(parseTarget({ workspace: "w", id: "d1" })).toEqual({ workspace: "w", id: "d1" });
    expect(parseTarget({ workspace: "w" })).toBeNull();
    expect(parseCommand("reload")).toBe("reload");
    expect(parseCommand("eval")).toBeNull();
  });

  it("loads only the web, a local file, or a blank page", () => {
    for (const url of ["https://a.test/", "http://localhost:3000/", "file:///Users/me/a.html", "about:blank"]) expect(loadable(url), url).toBe(true);
    for (const url of ["javascript:alert(1)", "data:text/html,x", "chrome://settings", "about:config", "not a url"]) expect(loadable(url), url).toBe(false);
  });
});

describe("keeping pages alive", () => {
  it("closes the hidden pages shown longest ago, never one on screen", () => {
    const views = [
      { key: "a", visible: true, shownAt: 1 },
      { key: "b", visible: false, shownAt: 5 },
      { key: "c", visible: false, shownAt: 2 },
      { key: "d", visible: false, shownAt: 9 },
    ];
    expect(overCap(views, 4)).toEqual([]);
    expect(overCap(views, 2)).toEqual(["c", "b"]);
    expect(overCap(views, 0)).toEqual(["c", "b", "d"]);
  });

  it("places a page on whole window points at the shell's zoom", () => {
    expect(toBounds({ x: 10.4, y: 20.6, width: 100.2, height: 50 }, 1)).toEqual({ x: 10, y: 21, width: 101, height: 50 });
    expect(toBounds({ x: 10, y: 20, width: 100, height: 50 }, 1.25)).toEqual({ x: 13, y: 25, width: 125, height: 63 });
    expect(toBounds(rect, Number.NaN)).toEqual(rect);
  });
});
