import { describe, expect, it } from "vitest";
import { revealablePath } from "./reveal";

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
