import { describe, expect, it } from "vitest";
import { appScheme, browserPartition, isPopup, loadable, popupBounds, MAX_SYNCED_DISPLAYS, nextZoomFactor, overCap, parseCommand, parseSync, parseTarget, remoteRequest, toBounds } from "./browserSync";

const rect = { x: 0, y: 40, width: 800, height: 600 };
const display = { id: "d1", area_id: "a1", url: "https://a.test/", load: 3, rect, visible: true };
const workspace = "local\u0000/r";
const retained = [{ workspace, id: "d1", area_id: "a1" }];
const sync = { workspace, displays: [display], retained };

describe("what the shell may ask of the browser views (issue 155)", () => {
  it("takes a well-formed sync whole", () => {
    const sync = { workspace, displays: [display, { ...display, id: "d2", area_id: "a2", rect: null, visible: false }], retained: [...retained, { workspace, id: "d2", area_id: "a2" }, { workspace: "ssh\u0000/other", id: "d1", area_id: "a3" }] };
    expect(parseSync(sync)).toEqual(sync);
    expect(parseSync({ workspace: null, displays: [], retained: [] })).toEqual({ workspace: null, displays: [], retained: [] });
  });

  it("drops a sync whole when any part of it is out of shape", () => {
    const bad: unknown[] = [
      null,
      { ...sync, displays: "d1" },
      { ...sync, retained: [] },
      { ...sync, workspace: null },
      { ...sync, workspace: "" },
      { ...sync, displays: [display, display] },
      { ...sync, displays: [{ ...display, load: -1 }] },
      { ...sync, displays: [{ ...display, load: 1.5 }] },
      { ...sync, displays: [{ ...display, url: "x".repeat(8193) }] },
      { ...sync, displays: [{ ...display, rect: { ...rect, width: Number.NaN } }] },
      { ...sync, displays: [{ ...display, rect: { ...rect, height: -1 } }] },
      { ...sync, displays: [{ ...display, rect: { ...rect, x: 1e9 } }] },
      { ...sync, displays: [{ ...display, visible: "yes" }] },
      { ...sync, displays: Array.from({ length: MAX_SYNCED_DISPLAYS + 1 }, (_, i) => ({ ...display, id: `d${i}` })) },
    ];
    for (const value of bad) expect(parseSync(value), JSON.stringify(value)?.slice(0, 80)).toBeNull();
  });

  it("requires a bounded area identity on placements and retained pages", () => {
    for (const area_id of [undefined, null, "", 1, "a".repeat(8193), "a\u0000b", "a\u0001b"]) {
      expect(parseSync({ ...sync, displays: [{ ...display, area_id }] })).toBeNull();
      expect(parseSync({ ...sync, retained: [{ ...retained[0], area_id }] })).toBeNull();
    }
    const area_id = "a".repeat(8192);
    expect(parseSync({ ...sync, displays: [{ ...display, area_id }], retained: [{ ...retained[0], area_id }] })).not.toBeNull();
  });

  it("preserves an optional bounded exact attachment epoch and rejects malformed epochs", () => {
    expect(parseSync(sync)).toEqual(sync);
    for (const attachment_epoch of ["epoch-1", "A0-z9", "a".repeat(64), "893b5942-eaa5-49ae-95d1-073c35c07ecb"]) {
      expect(parseSync({ ...sync, attachment_epoch })).toEqual({ ...sync, attachment_epoch });
    }
    for (const attachment_epoch of [null, false, 1, {}, "", "a".repeat(65), "a_b", "a b", "é", "a\n", "a\u0000b", "a\u0001b"]) {
      expect(parseSync({ ...sync, attachment_epoch })).toBeNull();
    }
  });

  it("matches ownership by the complete Workspace, display and authoritative area", () => {
    expect(parseSync({ ...sync, retained: [{ workspace: "local\u0000/other", id: "d1", area_id: "a1" }] })).toBeNull();
    expect(parseSync({ ...sync, retained: [{ workspace: "ssh\u0000/r", id: "d1", area_id: "a1" }] })).toBeNull();
    expect(parseSync({ ...sync, retained: [{ workspace, id: "d1", area_id: "a2" }] })).toBeNull();
    expect(parseSync({ ...sync, retained: [...retained, ...retained] })).toBeNull();
    for (const owner of ["w", "\u0000/r", "local\u0000", "local\u0000/r\u0000other", "local\u0000/r\u0001d1"]) {
      expect(parseSync({ ...sync, workspace: owner, retained: [{ workspace: owner, id: "d1", area_id: "a1" }] })).toBeNull();
    }
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

  it("keeps an opener only for a sized popup, never for a tab or a shift-click", () => {
    expect(isPopup("new-window", "width=420,height=520")).toBe(true);
    expect(isPopup("new-window", "popup")).toBe(true);
    expect(isPopup("new-window", "")).toBe(false);
    for (const disposition of ["foreground-tab", "background-tab", "default", "other"]) expect(isPopup(disposition, "width=420"), disposition).toBe(false);
  });

  it("hands another app only its own links, never a scheme Chromium answers", () => {
    expect(appScheme("slack://channel?team=T1")).toBe("slack:");
    expect(appScheme("zoommtg://zoom.us/join?confno=1")).toBe("zoommtg:");
    expect(appScheme("mailto:a@b.test")).toBe("mailto:");
    for (const url of ["https://a.test/", "file:///a.html", "about:blank", "about:config", "javascript:alert(1)", "data:text/html,x", "blob:https://a.test/1", "chrome://settings", "chrome-error://chromewebdata/", "view-source:https://a.test/", "devtools://devtools/x", "wss://a.test/", "not a url"]) {
      expect(appScheme(url), url).toBeNull();
    }
  });

  it("never offers a link that mounts a share, runs a shell or a script, or opens a remote session", () => {
    for (const url of ["smb://host/share", "afp://host/share", "nfs://host/x", "ftp://host/x", "sftp://host", "ssh://host", "telnet://host", "vnc://host", "news://host/group", "nntp://host/group", "gopher://host", "x-man-page://ls", "applescript://com.apple.scripteditor?action=new", "shortcuts://run-shortcut?name=x", "x-apple.systempreferences:com.apple.preference.security", "disk://x"]) {
      expect(appScheme(url), url).toBeNull();
    }
  });

  it("sizes a popup as asked within the work area, centred over the window", () => {
    const area = { x: 0, y: 25, width: 1440, height: 875 };
    const parent = { x: 100, y: 100, width: 1000, height: 700 };
    expect(popupBounds({ width: 420, height: 520 }, parent, area)).toEqual({ x: 390, y: 190, width: 420, height: 520 });
    expect(popupBounds({ width: 3000, height: 2000 }, parent, area)).toEqual({ x: 0, y: 25, width: 1440, height: 875 });
    expect(popupBounds({ width: 1, height: -5 }, parent, area)).toMatchObject({ width: 320, height: 600 });
    expect(popupBounds({}, { ...parent, x: 1300 }, area)).toEqual({ x: 940, y: 150, width: 500, height: 600 });
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
