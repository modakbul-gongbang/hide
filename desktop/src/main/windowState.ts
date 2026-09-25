// The main window's bounds across launches (B8): a small JSON file in the
// host's userData, read once at window creation and written when the window
// closes. A file that is missing, unreadable, from another schema, or places
// the window off every display opens the default size centered instead.

import fs from "node:fs";
import path from "node:path";

export const WINDOW_STATE_SCHEMA = 1;
export const DEFAULT_SIZE = { width: 1440, height: 900 };
export const MIN_SIZE = { width: 720, height: 480 };
/** How much of a restored window must sit on a display's work area. */
const MIN_VISIBLE = 96;

export type Rect = { x: number; y: number; width: number; height: number };

function isRect(value: unknown): value is Rect {
  if (!value || typeof value !== "object") return false;
  const rect = value as Record<string, unknown>;
  return ["x", "y", "width", "height"].every((key) => typeof rect[key] === "number" && Number.isFinite(rect[key]));
}

function overlap(a: Rect, b: Rect): { width: number; height: number } {
  return {
    width: Math.min(a.x + a.width, b.x + b.width) - Math.max(a.x, b.x),
    height: Math.min(a.y + a.height, b.y + b.height) - Math.max(a.y, b.y),
  };
}

export function defaultBounds(primary: Rect): Rect {
  const width = Math.min(DEFAULT_SIZE.width, primary.width);
  const height = Math.min(DEFAULT_SIZE.height, primary.height);
  return {
    x: Math.round(primary.x + (primary.width - width) / 2),
    y: Math.round(primary.y + (primary.height - height) / 2),
    width,
    height,
  };
}

export type Restored = { bounds: Rect; source: "stored" | "default"; why?: "none" | "unusable" | "off_screen" };

/** The bounds to open with, from what was stored and the displays there are now. */
export function restoreBounds(stored: unknown, workAreas: readonly Rect[], primary: Rect): Restored {
  if (stored === null || stored === undefined) return { bounds: defaultBounds(primary), source: "default", why: "none" };
  const saved = typeof stored === "object" ? (stored as Record<string, unknown>) : null;
  const bounds = saved?.schema === WINDOW_STATE_SCHEMA ? saved.bounds : null;
  if (!isRect(bounds) || bounds.width < MIN_SIZE.width || bounds.height < MIN_SIZE.height) {
    return { bounds: defaultBounds(primary), source: "default", why: "unusable" };
  }
  const onScreen = workAreas.some((area) => {
    const shared = overlap(bounds, area);
    return shared.width >= MIN_VISIBLE && shared.height >= MIN_VISIBLE;
  });
  return onScreen ? { bounds, source: "stored" } : { bounds: defaultBounds(primary), source: "default", why: "off_screen" };
}

export function windowStatePath(userData: string): string {
  return path.join(userData, "window-state.json");
}

/** The stored JSON; null when no file exists; a marker the restore refuses when the file cannot be read. */
export function readWindowState(file: string): unknown {
  let text: string;
  try {
    text = fs.readFileSync(file, "utf8");
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code === "ENOENT") return null;
    return { unreadable: String(error) };
  }
  try {
    return JSON.parse(text);
  } catch {
    return { unreadable: "not JSON" };
  }
}

/** Written to a temporary file and renamed, so a crash mid-write leaves the last good state. */
export function writeWindowState(file: string, bounds: Rect): void {
  const staging = `${file}.${process.pid}.tmp`;
  fs.writeFileSync(staging, JSON.stringify({ schema: WINDOW_STATE_SCHEMA, bounds }));
  fs.renameSync(staging, file);
}
