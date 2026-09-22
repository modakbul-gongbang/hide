import { describe, expect, it } from "vitest";
import { REGISTRY, chordEquals, displayChord, isChromeReserved, matchBrowser } from "./shortcuts";

describe("shortcut registry", () => {
  it("claims no chord Chrome keeps for itself", () => {
    for (const command of REGISTRY) {
      expect(command.browser && isChromeReserved(command.browser), command.id).toBeFalsy();
    }
  });

  it("marks exactly the seven moved chords", () => {
    const moved = REGISTRY.filter((command) => command.moved).map((command) => command.id).sort();
    expect(moved).toEqual(
      ["close_pane", "close_tab", "new_tab", "new_workspace", "previous_recent_tab", "recent_tab", "reopen_closed_tab"].sort(),
    );
  });

  it("leaves the electron column empty", () => {
    expect(REGISTRY.every((command) => command.electron === null)).toBe(true);
  });

  it("binds every browser chord once", () => {
    const seen: string[] = [];
    for (const command of REGISTRY) {
      if (!command.browser) continue;
      const shown = displayChord(command.browser);
      expect(seen, `${command.id} reuses ${shown}`).not.toContain(shown);
      seen.push(shown);
    }
  });

  it("matches on the physical key and every modifier", () => {
    expect(matchBrowser({ code: "KeyT", metaKey: false, altKey: true, shiftKey: false, ctrlKey: false })?.id).toBe("new_tab");
    expect(matchBrowser({ code: "KeyT", metaKey: false, altKey: true, shiftKey: true, ctrlKey: false })?.id).toBe("reopen_closed_tab");
    expect(matchBrowser({ code: "KeyT", metaKey: true, altKey: false, shiftKey: false, ctrlKey: false })).toBeNull();
    expect(matchBrowser({ code: "Backquote", metaKey: false, altKey: true, shiftKey: false, ctrlKey: false })?.id).toBe("recent_tab");
    expect(matchBrowser({ code: "Slash", metaKey: true, altKey: false, shiftKey: false, ctrlKey: false })?.id).toBe("shortcuts");
    expect(matchBrowser({ code: "Backspace", metaKey: true, altKey: false, shiftKey: false, ctrlKey: false })).toBeNull();
    expect(chordEquals({ code: "KeyA" }, { code: "KeyA", meta: false })).toBe(true);
  });

  it("renders chords in macOS modifier order", () => {
    expect(displayChord({ code: "Enter", meta: true, alt: true })).toBe("⌥⌘↩");
    expect(displayChord({ code: "Backquote", alt: true, shift: true })).toBe("⌥⇧`");
    expect(displayChord({ code: "Digit0", meta: true })).toBe("⌘0");
    expect(displayChord({ code: "Tab", ctrl: true, shift: true })).toBe("⌃⇧⇥");
  });
});
