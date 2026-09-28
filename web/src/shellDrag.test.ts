import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { holdShellDrag, shellDragging } from "./shellDrag";

/** The root element as the mark sees it: its attributes and nothing else. */
function stubRoot(): Map<string, string> {
  const attributes = new Map<string, string>();
  vi.stubGlobal("document", {
    documentElement: {
      setAttribute: (name: string, value: string) => attributes.set(name, value),
      removeAttribute: (name: string) => attributes.delete(name),
      hasAttribute: (name: string) => attributes.has(name),
    },
  });
  return attributes;
}

describe("the shell drag mark", () => {
  let root: Map<string, string>;
  beforeEach(() => {
    root = stubRoot();
  });
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("holds the root while a drag runs and lets go at its release", () => {
    expect(shellDragging()).toBe(false);
    const release = holdShellDrag("col-resize");
    expect(root.get("data-view-drag")).toBe("col-resize");
    expect(shellDragging()).toBe(true);
    release();
    release();
    expect(root.has("data-view-drag")).toBe(false);
    expect(shellDragging()).toBe(false);
  });

  it("keeps an overlapping drag's mark when the other one ends first", () => {
    const divider = holdShellDrag("col-resize");
    const file = holdShellDrag("file");
    expect(root.get("data-view-drag")).toBe("file");
    file();
    expect(root.get("data-view-drag")).toBe("col-resize");
    expect(shellDragging()).toBe(true);
    divider();
    expect(shellDragging()).toBe(false);
  });

  it("shows the latest drag still held when an earlier one ends", () => {
    const first = holdShellDrag("col-resize");
    const second = holdShellDrag("row-resize");
    first();
    expect(root.get("data-view-drag")).toBe("row-resize");
    second();
    expect(shellDragging()).toBe(false);
  });

  it("counts a drag in the Agent column", () => {
    const release = holdShellDrag("move", "data-agent-drag");
    expect(root.has("data-view-drag")).toBe(false);
    expect(shellDragging()).toBe(true);
    release();
    expect(shellDragging()).toBe(false);
  });
});
