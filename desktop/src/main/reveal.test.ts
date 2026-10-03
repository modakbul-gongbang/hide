import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterAll, describe, expect, it } from "vitest";
import { revealTarget } from "./reveal";

describe("revealTarget", () => {
  const root = fs.realpathSync.native(fs.mkdtempSync(path.join(os.tmpdir(), "hide-reveal-")));
  afterAll(() => fs.rmSync(root, { recursive: true, force: true }));
  /** The page names a path with `/` between names on every system. */
  const page = (native: string): string => native.split(path.sep).join("/");

  it("hands over a file or a folder that exists, in this system's spelling, with its kind", async () => {
    const file = path.join(root, "notes.md");
    fs.writeFileSync(file, "notes");
    expect(await revealTarget(page(file))).toEqual({ path: file, kind: "file" });
    expect(await revealTarget(page(root))).toEqual({ path: root, kind: "directory" });
  });

  it("refuses a path that is not absolute or no longer exists", async () => {
    expect(await revealTarget("notes.md")).toEqual({ refused: "path" });
    expect(await revealTarget("\\notes.md")).toEqual({ refused: "path" });
    expect(await revealTarget(page(path.join(root, "gone.md")))).toEqual({ refused: "missing" });
  });
});
