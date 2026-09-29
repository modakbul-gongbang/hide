import { afterEach, describe, expect, it, vi } from "vitest";
import { opensExternally } from "./host";

afterEach(() => vi.unstubAllGlobals());

describe("opensExternally", () => {
  it("takes ⌘ on a Mac and Ctrl elsewhere, never the other one", () => {
    vi.stubGlobal("navigator", { platform: "MacIntel", userAgent: "" });
    expect(opensExternally({ metaKey: true, ctrlKey: false })).toBe(true);
    expect(opensExternally({ metaKey: false, ctrlKey: true })).toBe(false);
    vi.stubGlobal("navigator", { platform: "Win32", userAgent: "" });
    expect(opensExternally({ metaKey: false, ctrlKey: true })).toBe(true);
    expect(opensExternally({ metaKey: true, ctrlKey: false })).toBe(false);
  });
});
