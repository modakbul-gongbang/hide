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
  parseMacosChord,
  serializeChord,
  serializeMacosChord,
  storedKey,
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
      ["close_pane", "close_tab", "new_tab", "new_workspace", "previous_recent_panel", "recent_panel", "reopen_closed_tab", "settings"].sort(),
    );
  });

  it("gives the desktop app the Swift chords", () => {
    // ShellMenuCommand.swift and PaneShortcutSettings.swift, as the operator reads them;
    // Keyboard shortcuts has no Swift chord and keeps the browser's.
    const swift: Record<string, string> = {
      new_tab: "⌘T",
      close_tab: "⌘W",
      reopen_closed_tab: "⇧⌘T",
      recent_panel: "⌃⇥",
      previous_recent_panel: "⌃⇧⇥",
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
    expect(matchHost(press("Tab", { ctrl: true }), REGISTRY, "electron")?.id).toBe("recent_panel");
    expect(matchHost(press("KeyT", { alt: true }), REGISTRY, "electron")).toBeNull();
    expect(matchHost(press("Backspace", { meta: true }), REGISTRY, "electron")).toBeNull();
    // B12: the browser host still runs its own column.
    expect(matchHost(press("KeyT", { meta: true }), REGISTRY, "browser")).toBeNull();
    expect(matchHost(press("KeyT", { alt: true }), REGISTRY, "browser")?.id).toBe("new_tab");
  });

  it("runs each host's own stored set and never the other's", () => {
    const uiState = { browser_shortcut_bindings: { split_right: "alt+KeyR" }, shortcut_bindings: { toggle_zoom: "command+shift+return" } };
    const browser = hostRegistry(uiState, "browser").registry;
    const desktop = hostRegistry(uiState, "electron").registry;
    expect(matchHost({ code: "KeyR", metaKey: false, altKey: true, shiftKey: false, ctrlKey: false }, browser, "browser")?.id).toBe("split_right");
    expect(matchHost({ code: "KeyR", metaKey: false, altKey: true, shiftKey: false, ctrlKey: false }, desktop, "electron")).toBeNull();
    expect(matchHost({ code: "KeyD", metaKey: true, altKey: false, shiftKey: false, ctrlKey: false }, desktop, "electron")?.id).toBe("split_right");
    expect(matchHost({ code: "Enter", metaKey: true, altKey: false, shiftKey: true, ctrlKey: false }, desktop, "electron")?.id).toBe("toggle_zoom");
    expect(matchHost({ code: "Enter", metaKey: true, altKey: false, shiftKey: true, ctrlKey: false }, browser, "browser")).toBeNull();
    expect(hostRegistry({}, "electron").registry).toBe(REGISTRY);
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
    expect(matchHost({ code: "Backquote", metaKey: false, altKey: true, shiftKey: false, ctrlKey: false }, REGISTRY, "browser")?.id).toBe("recent_panel");
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

describe("the desktop app's macOS pane chords (user decision 2026-09-26)", () => {
  // The operator's Swift state.json on the day of the decision.
  const operatorSet = {
    close_pane: "command+shift+w",
    split_down: "command+shift+d",
    split_right: "command+d",
    toggle_zoom: "command+shift+return",
    increase_text_size: "command+=",
    decrease_text_size: "command+-",
    reset_text_size: "command+0",
  };

  it("reads the Swift app's chord text the way the Swift app does", () => {
    expect(parseMacosChord("command+shift+return")).toEqual({ code: "Enter", meta: true, shift: true });
    expect(parseMacosChord("Cmd + Opt + Enter")).toEqual({ code: "Enter", meta: true, alt: true });
    expect(parseMacosChord("command+=")).toEqual({ code: "Equal", meta: true });
    expect(parseMacosChord("command+command+d")).toBeNull();
    expect(parseMacosChord("d")).toBeNull();
    expect(parseMacosChord("command+tab")).toBeNull();
    expect(serializeMacosChord({ code: "Enter", meta: true, shift: true, alt: true, ctrl: true })).toBe("command+control+option+shift+return");
    expect(serializeMacosChord({ code: "ArrowUp", meta: true })).toBeNull();
    expect(storedKey("text_larger", "electron")).toBe("increase_text_size");
    expect(storedKey("text_larger", "browser")).toBe("text_larger");
  });

  it("runs the operator's set: ⇧⌘↩ zooms and ⌥⌘↩ no longer does", () => {
    const { registry, diagnostic } = effectiveRegistry(operatorSet, "electron");
    expect(diagnostic).toBeNull();
    expect(displayCommand("toggle_zoom", "electron", registry)).toBe("⇧⌘↩");
    expect(matchHost(press("Enter", { meta: true, shift: true }), registry, "electron")?.id).toBe("toggle_zoom");
    expect(matchHost(press("Enter", { meta: true, alt: true }), registry, "electron")).toBeNull();
    expect(displayCommand("text_larger", "electron", registry)).toBe("⌘=");
    // The browser column is untouched by the macOS set.
    expect(registry.find((command) => command.id === "toggle_zoom")?.browser).toEqual({ code: "Enter", meta: true, alt: true });
  });

  it("keeps the Swift-only Toggle Conversation in the set without running it", () => {
    const { registry, diagnostic } = effectiveRegistry({ toggle_conversation: "command+option+c", split_right: "command+option+r" }, "electron");
    expect(diagnostic).toBeNull();
    expect(matchHost(press("KeyR", { meta: true, alt: true }), registry, "electron")?.id).toBe("split_right");
  });

  it("lets two commands trade chords in one set", () => {
    const { diagnostic } = effectiveRegistry({ split_right: "command+shift+d", split_down: "command+d" }, "electron");
    expect(diagnostic).toBeNull();
  });

  it("refuses what the Swift app refuses, and falls back to the defaults as a whole", () => {
    expect(bindingProblem("split_right", { code: "KeyR", alt: true }, REGISTRY, "electron")).toMatch(/Include ⌘/);
    expect(bindingProblem("split_right", { code: "KeyQ", meta: true }, REGISTRY, "electron")).toMatch(/kept by macOS/);
    expect(bindingProblem("split_right", { code: "ArrowUp", meta: true }, REGISTRY, "electron")).toMatch(/Use one letter/);
    expect(bindingProblem("split_right", { code: "KeyT", meta: true }, REGISTRY, "electron")).toMatch(/already New tab/);
    // A chord Chrome keeps is the desktop app's to use.
    expect(bindingProblem("close_pane", { code: "KeyW", meta: true, shift: true }, REGISTRY, "electron")).toBeNull();
    const unusable: Record<string, string>[] = [{ split_right: "command+t" }, { split_right: "option+r" }, { zoom: "command+z" }, { split_right: "command+x", split_down: "command+x" }];
    for (const stored of unusable) {
      const resolved = effectiveRegistry(stored, "electron");
      expect(resolved.registry, JSON.stringify(stored)).toBe(REGISTRY);
      expect(resolved.diagnostic).toMatch(/defaults are in use/);
    }
  });
});
