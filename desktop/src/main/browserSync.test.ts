import { describe, expect, it } from "vitest";
import { browserPartition, loadable, MAX_SYNCED_DISPLAYS, nextZoomFactor, overCap, parseCommand, parseSync, parseTarget, remoteRequest, toBounds } from "./browserSync";

const rect = { x: 0, y: 40, width: 800, height: 600 };
const display = { id: "d1", url: "https://a.test/", load: 3, rect, visible: true };

describe("what the shell may ask of the browser views (issue 155)", () => {
  it("takes a well-formed sync whole", () => {
    const sync = { workspace: "local\u0000/r", displays: [display, { ...display, id: "d2", rect: null, visible: false }], retained: [{ workspace: "local\u0000/r", id: "d1" }, { workspace: "local\u0000/r", id: "d2" }, { workspace: "ssh\u0000/other", id: "d1" }] };
    expect(parseSync(sync)).toEqual(sync);
    expect(parseSync({ workspace: null, displays: [], retained: [] })).toEqual({ workspace: null, displays: [], retained: [] });
  });

  it("drops a sync whole when any part of it is out of shape", () => {
    const bad: unknown[] = [
      null,
      { workspace: "w", displays: "d1" },
      { workspace: "w", displays: [display], retained: [] },
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
    for (const url of ["https://a.test/", "http://localhost:3000/", "file:///Users/example/a.html", "about:blank"]) expect(loadable(url), url).toBe(true);
    for (const url of ["javascript:alert(1)", "data:text/html,x", "chrome://settings", "about:config", "not a url"]) expect(loadable(url), url).toBe(false);
  });

  it("shares web login storage across Workspaces while keeping remote localhost and file previews separate", () => {
    const localA = "local\u0000/a";
    const localB = "local\u0000/b";
    const remoteA = "ssh-a\u0000/a";
    const remoteB = "ssh-a\u0000/b";
    const otherDevice = "ssh-b\u0000/a";
    const web = browserPartition(localA, "https://example.com/");
    expect(browserPartition(localB, "https://example.com/")).toBe(web);
    expect(browserPartition(remoteA, "https://example.com/")).toBe(web);
    expect(browserPartition(localA, "http://localhost:3000/")).toBe(web);
    const remoteLoopback = browserPartition(remoteA, "http://localhost:3000/");
    expect(browserPartition(remoteB, "http://127.0.0.1:3000/")).toBe(remoteLoopback);
    expect(browserPartition(remoteB, "http://[::ffff:127.0.0.1]:3000/")).toBe(remoteLoopback);
    expect(browserPartition(otherDevice, "http://localhost:3000/")).not.toBe(remoteLoopback);
    expect(remoteLoopback).not.toBe(web);
    expect(browserPartition(localA, "file:///a/report.html")).not.toBe(web);
    expect(browserPartition(localB, "file:///b/report.html")).not.toBe(browserPartition(localA, "file:///a/report.html"));
  });

  it("keeps absolute remote loopback requests on the View's SSH route", () => {
    const route = { source_url: "http://127.0.0.2:5173/app", url: "http://127.0.0.1:63001/app" };
    expect(remoteRequest(route, "http://localhost:5173/app.js")).toEqual({ redirectURL: "http://127.0.0.1:63001/app.js" });
    expect(remoteRequest(route, "ws://localhost:5173/live")).toEqual({ redirectURL: "ws://127.0.0.1:63001/live" });
    expect(remoteRequest(route, "http://localhost:9000/private")).toEqual({ cancel: true });
    expect(remoteRequest(route, "http://127.0.0.1:63001/app.js")).toEqual({});
    expect(remoteRequest(route, "http://[::ffff:127.0.0.1]:5173/app.js")).toEqual({ redirectURL: "http://127.0.0.1:63001/app.js" });
    expect(remoteRequest(route, "http://[::ffff:127.0.0.1]:9000/private")).toEqual({ cancel: true });
    expect(remoteRequest({ source_url: "http://127.1:5173/", url: "http://127.1:5173/" }, "http://127.1:5173/")).toEqual({ cancel: true });
    expect(remoteRequest({ source_url: "file:///checkout/page.html", url: "http://127.0.0.1:63002/secret/page.html" }, "https://example.com/leak")).toEqual({ cancel: true });
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

describe("page zoom", () => {
  it("steps through Chrome's zoom levels and stays at either end", () => {
    const walk = (from: number, zoom: "in" | "out", steps: number) => Array.from({ length: steps }).reduce<number>((factor) => nextZoomFactor(factor, zoom), from);
    expect([1, 2, 3].map((steps) => walk(1, "in", steps))).toEqual([1.1, 1.25, 1.5]);
    expect([1, 2, 3].map((steps) => walk(1, "out", steps))).toEqual([0.9, 0.8, 0.75]);
    expect(walk(1, "in", 20)).toBe(5);
    expect(walk(1, "out", 20)).toBe(0.25);
    expect(nextZoomFactor(0.3333333, "in")).toBe(0.5);
    expect(nextZoomFactor(1.2, "in")).toBe(1.25);
    expect(nextZoomFactor(1.2, "out")).toBe(1.1);
    expect(nextZoomFactor(2.5, "reset")).toBe(1);
  });
});
