import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { describe, expect, it } from "vitest";
import { readWindowState, restoreBounds, WINDOW_STATE_SCHEMA, writeWindowState } from "./windowState";

const primary = { x: 0, y: 25, width: 1728, height: 1080 };
const second = { x: 1728, y: 0, width: 2560, height: 1440 };

describe("window bounds (B8)", () => {
  it("restores stored bounds that sit on a display", () => {
    const bounds = { x: 1800, y: 100, width: 1200, height: 800 };
    expect(restoreBounds({ schema: WINDOW_STATE_SCHEMA, bounds }, [primary, second], primary)).toEqual({ bounds, source: "stored" });
  });

  it("opens the default size centered when nothing usable is stored", () => {
    const centered = { x: 144, y: 115, width: 1440, height: 900 };
    expect(restoreBounds(null, [primary], primary)).toEqual({ bounds: centered, source: "default", why: "none" });
    expect(restoreBounds({ schema: 99, bounds: { x: 0, y: 0, width: 900, height: 700 } }, [primary], primary).why).toBe("unusable");
    expect(restoreBounds({ unreadable: "not JSON" }, [primary], primary).why).toBe("unusable");
    // Stored on a display that is no longer attached.
    expect(restoreBounds({ schema: WINDOW_STATE_SCHEMA, bounds: { x: 1800, y: 100, width: 1200, height: 800 } }, [primary], primary)).toEqual({
      bounds: centered,
      source: "default",
      why: "off_screen",
    });
  });

  it("fits the default to a screen smaller than it", () => {
    const small = { x: 0, y: 0, width: 1280, height: 720 };
    expect(restoreBounds(null, [small], small).bounds).toEqual({ x: 0, y: 0, width: 1280, height: 720 });
  });

  it("reads back what it wrote, and says when the file is not JSON", () => {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), "hide-desktop-window-"));
    const file = path.join(dir, "window-state.json");
    expect(readWindowState(file)).toBeNull();
    writeWindowState(file, { x: 1, y: 2, width: 800, height: 600 });
    expect(readWindowState(file)).toEqual({ schema: WINDOW_STATE_SCHEMA, bounds: { x: 1, y: 2, width: 800, height: 600 } });
    fs.writeFileSync(file, "{");
    expect(readWindowState(file)).toEqual({ unreadable: "not JSON" });
    fs.rmSync(dir, { recursive: true, force: true });
  });
});
