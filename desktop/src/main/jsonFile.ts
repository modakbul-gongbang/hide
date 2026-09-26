// The small JSON files the host keeps in its profile (window bounds, the last
// `hide` CLI that attached): read once, written whole.

import fs from "node:fs";

/** The parsed JSON; null when no file exists; `{ unreadable }` when it cannot be read or parsed. */
export function readJsonFile(file: string): unknown {
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

/** Written to a temporary file and renamed, so a crash mid-write leaves the last good file. */
export function writeJsonFile(file: string, value: unknown): void {
  const staging = `${file}.${process.pid}.tmp`;
  fs.writeFileSync(staging, JSON.stringify(value));
  fs.renameSync(staging, file);
}
