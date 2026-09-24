import { describe, expect, it } from "vitest";
import {
  EDITABLE_PANE_COMMANDS,
  REGISTRY,
  bindingProblem,
  chordEquals,
  displayChord,
  effectiveRegistry,
  isChromeReserved,
  matchBrowser,
  parseChord,
  serializeChord,
} from "./shortcuts";

describe("shortcut registry", () => {
  it("claims no chord Chrome keeps for itself", () => {
    for (const command of REGISTRY) {
      expect(command.browser && isChromeReserved(command.browser), command.id).toBeFalsy();
    }
  });

  it("marks exactly the eight moved chords", () => {
    const moved = REGISTRY.filter((command) => command.moved).map((command) => command.id).sort();
    expect(moved).toEqual(
      ["close_pane", "close_tab", "new_tab", "new_workspace", "previous_recent_tab", "recent_tab", "reopen_closed_tab", "settings"].sort(),
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

const press = (code: string, modifiers: { meta?: boolean; alt?: boolean; shift?: boolean; ctrl?: boolean } = {}) => ({
  code,
  metaKey: !!modifiers.meta,
  altKey: !!modifiers.alt,
  shiftKey: !!modifiers.shift,
  ctrlKey: !!modifiers.ctrl,
});

describe("browser pane chord overrides (S5 B9, B10)", () => {
  it("round-trips a chord through its stored form", () => {
    const chord = { code: "KeyR", alt: true, shift: true };
    expect(serializeChord(chord)).toBe("alt+shift+KeyR");
    expect(parseChord("alt+shift+KeyR")).toEqual(chord);
    expect(parseChord("alt+alt+KeyR")).toBeNull();
    expect(parseChord("hyper+KeyR")).toBeNull();
    expect(parseChord("alt+Process")).toBeNull();
  });

  it("runs and lists a stored override in place of the default", () => {
    const { registry, diagnostic } = effectiveRegistry({ split_right: "alt+KeyR" });
    expect(diagnostic).toBeNull();
    expect(matchBrowser(press("KeyR", { alt: true }), registry)?.id).toBe("split_right");
    expect(matchBrowser(press("KeyD", { meta: true }), registry)).toBeNull();
    expect(matchBrowser(press("KeyD", { meta: true, shift: true }), registry)?.id).toBe("split_down");
  });

  it("refuses a chord with no command modifier, a Chrome chord, and another command's chord", () => {
    expect(bindingProblem("split_right", { code: "KeyR" }, REGISTRY)).toMatch(/Include/);
    expect(bindingProblem("split_right", { code: "KeyR", shift: true }, REGISTRY)).toMatch(/Include/);
    expect(bindingProblem("split_right", { code: "KeyW", meta: true }, REGISTRY)).toMatch(/Chrome/);
    expect(bindingProblem("split_right", { code: "KeyT", alt: true }, REGISTRY)).toMatch(/already New tab/);
    expect(bindingProblem("split_right", { code: "KeyD", meta: true }, REGISTRY)).toBeNull();
  });

  it("falls back to the defaults as a whole when a stored map cannot run", () => {
    const unusable: Record<string, string>[] = [{ split_right: "KeyR" }, { split_right: "meta+KeyW" }, { split_right: "alt+KeyT" }, { toggle_conversation: "alt+KeyC" }];
    for (const stored of unusable) {
      const resolved = effectiveRegistry(stored);
      expect(resolved.registry, JSON.stringify(stored)).toBe(REGISTRY);
      expect(resolved.diagnostic).toMatch(/defaults are in use/);
    }
    // Two overrides that collide with each other are refused together.
    expect(effectiveRegistry({ split_right: "alt+KeyR", split_down: "alt+KeyR" }).registry).toBe(REGISTRY);
  });

  it("edits only the seven pane commands the browser host runs", () => {
    expect([...EDITABLE_PANE_COMMANDS].sort()).toEqual(
      ["close_pane", "split_down", "split_right", "text_larger", "text_reset", "text_smaller", "toggle_zoom"].sort(),
    );
    for (const id of EDITABLE_PANE_COMMANDS) expect(REGISTRY.some((command) => command.id === id)).toBe(true);
  });
});
