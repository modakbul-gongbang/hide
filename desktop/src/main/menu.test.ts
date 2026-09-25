import { describe, expect, it } from "vitest";
import { REGISTRY } from "../../../web/src/shortcuts";
import { accelerator, KEYBOARD_ONLY, MENU_LAYOUT, menuTemplate } from "./menu";

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
});
