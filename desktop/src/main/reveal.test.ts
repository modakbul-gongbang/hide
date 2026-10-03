import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterAll, describe, expect, it } from "vitest";
import { revealablePath, revealTarget } from "./reveal";

describe("revealablePath", () => {
  it("takes an absolute folder as it is", () => {
    expect(revealablePath("/Users/example/projects/hide")).toBe("/Users/example/projects/hide");
    expect(revealablePath("/Users/example/projects/hide/../hide.worktrees/feature")).toBe("/Users/example/projects/hide.worktrees/feature");
  });

  it("refuses anything that is not an absolute path", () => {
    expect(revealablePath("projects/hide")).toBeNull();
    expect(revealablePath("")).toBeNull();
    expect(revealablePath(42)).toBeNull();
    expect(revealablePath({ path: "/tmp" })).toBeNull();
    expect(revealablePath("/tmp/a\0b")).toBeNull();
    expect(revealablePath(`/${"a".repeat(5000)}`)).toBeNull();
  });
});

describe("revealTarget", () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "hide-reveal-"));
  afterAll(() => fs.rmSync(root, { recursive: true, force: true }));

  it("hands over a file or a folder that exists, with its kind", async () => {
    const file = path.join(root, "notes.md");
    fs.writeFileSync(file, "notes");
    expect(await revealTarget(file)).toEqual({ path: file, kind: "file" });
    expect(await revealTarget(root)).toEqual({ path: root, kind: "directory" });
  });

  it("refuses a path that is not absolute or no longer exists", async () => {
    expect(await revealTarget("notes.md")).toEqual({ refused: "path" });
    expect(await revealTarget(path.join(root, "gone.md"))).toEqual({ refused: "missing" });
  });
});
