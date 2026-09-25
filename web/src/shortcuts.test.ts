import { describe, expect, it } from "vitest";
import {
  EDITABLE_PANE_COMMANDS,
  REGISTRY,
  bindingProblem,
  chordEquals,
  displayChord,
  displayCommand,
  effectiveRegistry,
  hostRegistry,
  isChromeReserved,
  matchHost,
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

  it("gives the desktop app the Swift chords", () => {
    // ShellMenuCommand.swift and PaneShortcutSettings.swift, as the operator reads them;
    // Keyboard shortcuts has no Swift chord and keeps the browser's.
    const swift: Record<string, string> = {
      new_tab: "⌘T",
      close_tab: "⌘W",
      reopen_closed_tab: "⇧⌘T",
      recent_tab: "⌃⇥",
      previous_recent_tab: "⌃⇧⇥",
      new_workspace: "⇧⌘N",
      recent_project: "⌥⇥",
      previous_recent_project: "⌥⇧⇥",
      search: "⌘K",
      open_file: "⌘P",
      project_home: "⇧⌘H",
      toggle_left_sidebar: "⌘B",
      toggle_sidebar_view: "⌘E",
      toggle_right_panel: "⇧⌘B",
      find_in_pane: "⌘F",
      save_file: "⌘S",
      keep_open: "⇧⌘K",
      split_right: "⌘D",
      split_down: "⇧⌘D",
      toggle_zoom: "⌥⌘↩",
      close_pane: "⇧⌘W",
      text_larger: "⌘=",
      text_smaller: "⌘-",
      text_reset: "⌘0",
      move_to_trash: "⌘⌫",
      settings: "⌘,",
      shortcuts: "⌘/",
    };
    expect(Object.fromEntries(REGISTRY.map((command) => [command.id, displayCommand(command.id, "electron")]))).toEqual(swift);
  });

  it("binds every electron chord once", () => {
    const shown = REGISTRY.map((command) => displayCommand(command.id, "electron"));
    expect(new Set(shown).size).toBe(shown.length);
  });

  it("answers the native chords in the desktop app and leaves the browser's alone", () => {
    const press = (code: string, mods: { meta?: boolean; alt?: boolean; shift?: boolean; ctrl?: boolean } = {}) => ({
      code,
      metaKey: !!mods.meta,
      altKey: !!mods.alt,
      shiftKey: !!mods.shift,
      ctrlKey: !!mods.ctrl,
    });
    expect(matchHost(press("KeyT", { meta: true }), REGISTRY, "electron")?.id).toBe("new_tab");
    expect(matchHost(press("KeyW", { meta: true }), REGISTRY, "electron")?.id).toBe("close_tab");
    expect(matchHost(press("KeyT", { meta: true, shift: true }), REGISTRY, "electron")?.id).toBe("reopen_closed_tab");
    expect(matchHost(press("Tab", { ctrl: true }), REGISTRY, "electron")?.id).toBe("recent_tab");
    expect(matchHost(press("KeyT", { alt: true }), REGISTRY, "electron")).toBeNull();
    expect(matchHost(press("Backspace", { meta: true }), REGISTRY, "electron")).toBeNull();
    // B12: the browser host still runs its own column.
    expect(matchHost(press("KeyT", { meta: true }), REGISTRY, "browser")).toBeNull();
    expect(matchHost(press("KeyT", { alt: true }), REGISTRY, "browser")?.id).toBe("new_tab");
  });

  it("runs the browser overrides on the browser host only", () => {
    const stored = { split_right: "alt+KeyR" };
    expect(matchHost({ code: "KeyR", metaKey: false, altKey: true, shiftKey: false, ctrlKey: false }, hostRegistry(stored, "browser").registry, "browser")?.id).toBe("split_right");
    expect(hostRegistry(stored, "electron").registry).toBe(REGISTRY);
    expect(matchHost({ code: "KeyD", metaKey: true, altKey: false, shiftKey: false, ctrlKey: false }, hostRegistry(stored, "electron").registry, "electron")?.id).toBe("split_right");
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
    expect(matchHost({ code: "KeyT", metaKey: false, altKey: true, shiftKey: false, ctrlKey: false }, REGISTRY, "browser")?.id).toBe("new_tab");
    expect(matchHost({ code: "KeyT", metaKey: false, altKey: true, shiftKey: true, ctrlKey: false }, REGISTRY, "browser")?.id).toBe("reopen_closed_tab");
    expect(matchHost({ code: "KeyT", metaKey: true, altKey: false, shiftKey: false, ctrlKey: false }, REGISTRY, "browser")).toBeNull();
    expect(matchHost({ code: "Backquote", metaKey: false, altKey: true, shiftKey: false, ctrlKey: false }, REGISTRY, "browser")?.id).toBe("recent_tab");
    expect(matchHost({ code: "Slash", metaKey: true, altKey: false, shiftKey: false, ctrlKey: false }, REGISTRY, "browser")?.id).toBe("shortcuts");
    expect(matchHost({ code: "Backspace", metaKey: true, altKey: false, shiftKey: false, ctrlKey: false }, REGISTRY, "browser")).toBeNull();
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
    expect(matchHost(press("KeyR", { alt: true }), registry, "browser")?.id).toBe("split_right");
    expect(matchHost(press("KeyD", { meta: true }), registry, "browser")).toBeNull();
    expect(matchHost(press("KeyD", { meta: true, shift: true }), registry, "browser")?.id).toBe("split_down");
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
