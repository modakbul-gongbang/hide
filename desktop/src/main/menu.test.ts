import { describe, expect, it } from "vitest";
import { REGISTRY } from "../../../web/src/shortcuts";
import { accelerator, KEYBOARD_ONLY, MENU_LAYOUT, menuBindings, menuTemplate } from "./menu";

describe("the app menu (B9)", () => {
  it("writes registry chords as Electron accelerators", () => {
    expect(accelerator({ code: "KeyT", meta: true })).toBe("Command+T");
    expect(accelerator({ code: "KeyT", meta: true, shift: true })).toBe("Shift+Command+T");
    expect(accelerator({ code: "Enter", meta: true, alt: true })).toBe("Alt+Command+Return");
    expect(accelerator({ code: "Digit0", meta: true })).toBe("Command+0");
    expect(accelerator({ code: "Comma", meta: true })).toBe("Command+,");
    expect(() => accelerator({ code: "IntlRo", meta: true })).toThrow();
  });

  it("places every clickable command exactly once, and the cycles nowhere", () => {
    const placed = Object.values(MENU_LAYOUT).flat().filter((id) => id !== null);
    expect(new Set(placed).size).toBe(placed.length);
    const expected = REGISTRY.map((command) => command.id).filter((id) => !KEYBOARD_ONLY.includes(id));
    expect([...placed].sort()).toEqual([...expected].sort());
  });

  it("shows each item with its electron chord and sends its id when clicked", () => {
    const sent: string[] = [];
    const template = menuTemplate({ appName: "hide", send: (id) => sent.push(id), developer: false });
    const items = template.flatMap((menu) => (Array.isArray(menu.submenu) ? menu.submenu : []));
    const newTab = items.find((item) => item.id === "new_tab");
    expect(newTab?.accelerator).toBe("Command+T");
    const closeTab = items.find((item) => item.id === "close_tab");
    expect(closeTab?.accelerator).toBe("Command+W");
    (newTab?.click as () => void)();
    expect(sent).toEqual(["new_tab"]);
    // No standard item claims a chord the registry owns.
    const roles = items.filter((item) => item.role).map((item) => item.role);
    expect(roles).not.toContain("close");
    expect(roles).not.toContain("reload");
  });

  it("shows the operator's macOS pane chords once the shell reports them", () => {
    const resolved = menuBindings({ toggle_zoom: "command+shift+return", toggle_conversation: "command+option+c" });
    if ("refused" in resolved) throw new Error(resolved.refused);
    expect(resolved.diagnostic).toBeNull();
    const template = menuTemplate({ appName: "hide", send: () => undefined, developer: false, registry: resolved.registry });
    const items = template.flatMap((menu) => (Array.isArray(menu.submenu) ? menu.submenu : []));
    expect(items.find((item) => item.id === "toggle_zoom")?.accelerator).toBe("Shift+Command+Return");
    expect(items.find((item) => item.id === "split_right")?.accelerator).toBe("Command+D");
  });

  it("refuses a report that is not a short string map, and runs the defaults for an unusable set", () => {
    expect(menuBindings(null)).toEqual({ refused: "not a map" });
    expect(menuBindings(["command+d"])).toEqual({ refused: "not a map" });
    expect(menuBindings({ split_right: 4 })).toEqual({ refused: "not short strings" });
    expect(menuBindings(Object.fromEntries(Array.from({ length: 17 }, (_, index) => [`k${index}`, "command+d"])))).toEqual({ refused: "too many entries" });
    const unusable = menuBindings({ split_right: "command+t" });
    expect("refused" in unusable ? null : unusable.registry).toBe(REGISTRY);
  });
});
