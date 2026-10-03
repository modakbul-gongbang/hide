import { afterEach, describe, expect, it, vi } from "vitest";
import { keySystem, holdsCommandKey } from "./host";

afterEach(() => vi.unstubAllGlobals());

describe("holdsCommandKey", () => {
  it("takes ⌘ on a Mac and Ctrl elsewhere, never the other one", () => {
    vi.stubGlobal("navigator", { platform: "MacIntel", userAgent: "" });
    expect(holdsCommandKey({ metaKey: true, ctrlKey: false })).toBe(true);
    expect(holdsCommandKey({ metaKey: false, ctrlKey: true })).toBe(false);
    vi.stubGlobal("navigator", { platform: "Win32", userAgent: "" });
    expect(holdsCommandKey({ metaKey: false, ctrlKey: true })).toBe(true);
    expect(holdsCommandKey({ metaKey: true, ctrlKey: false })).toBe(false);
  });
});

describe("keySystem", () => {
  it("follows the browser's platform, and the desktop app's OS when the host bridge is there", () => {
    vi.stubGlobal("navigator", { platform: "Linux x86_64", userAgent: "" });
    expect(keySystem()).toBe("pc");
    vi.stubGlobal("navigator", { platform: "", userAgent: "", userAgentData: { platform: "macOS" } });
    expect(keySystem()).toBe("mac");
    vi.stubGlobal("window", { hideHost: { kind: "electron", platform: "win32" } });
    expect(keySystem()).toBe("pc");
  });
});
