import { EventEmitter } from "node:events";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { BROWSER_CYCLE_END_CHANNEL, BROWSER_EVENT_CHANNEL } from "../channel";
import { BrowserViews } from "./browser";
import type { HostLog } from "./log";

const ipc = vi.hoisted(() => new Map<string, (event: unknown, value?: unknown) => void>());
vi.mock("electron", () => ({
  ipcMain: { on: (channel: string, listener: (event: unknown, value?: unknown) => void) => ipc.set(channel, listener), handle: vi.fn() },
  Menu: { getApplicationMenu: () => null },
  BrowserWindow: {}, WebContentsView: {}, session: {}, shell: {},
}));

function candidate() {
  const send = vi.fn();
  const shellFocus = vi.fn();
  let windowFocused = true;
  const subject = new BrowserViews({ event: vi.fn() } as unknown as HostLog, () => true, vi.fn(), vi.fn());
  Reflect.set(subject, "window", { isFocused: () => windowFocused, webContents: { isDestroyed: () => false, send, focus: shellFocus } });
  const page = (id: string) => {
    let focused = true;
    const contents = Object.assign(new EventEmitter(), { isFocused: () => focused, setWindowOpenHandler: vi.fn() });
    const value = { key: id, workspace: "fixture", id, visible: true, view: { webContents: contents, setVisible: vi.fn() }, state: {}, report: null, route: null, applied: 1, partition: "fixture", shownAt: 0 };
    const watch = Reflect.get(subject, "watch") as (page: unknown) => void;
    watch.call(subject, value);
    return {
      focus: (next: boolean) => { focused = next; },
      show: (visible: boolean) => Reflect.get(subject, "show").call(subject, value, visible),
      visibility: value.view.setVisible,
      input: (type = "keyDown", key = "Tab", control = true) => {
        const event = { preventDefault: vi.fn() };
        contents.emit("before-input-event", event, { type, key, code: key === "Control" ? "ControlLeft" : key, control, alt: false, meta: false, shift: false, isComposing: false });
        return event.preventDefault;
      },
    };
  };
  const forwarded = () => send.mock.calls.filter(([channel]) => channel === BROWSER_EVENT_CHANNEL).map(([, value]) => value as { kind: string; id: string; cycleId: number; key: string });
  return { page, forwarded, shellFocus, windowFocus: (focused: boolean) => { windowFocused = focused; } };
}

describe("native held cycle delivery", () => {
  beforeEach(() => ipc.clear());
  it("freezing a held native page gives its remaining input to the same focused shell", () => {
    const { page, shellFocus, windowFocus } = candidate();
    const origin = page("origin"), other = page("other");
    other.show(false);
    expect(shellFocus).not.toHaveBeenCalled();
    origin.input();
    origin.show(false);
    expect(shellFocus).toHaveBeenCalledOnce();
    expect(shellFocus.mock.invocationCallOrder[0]).toBeGreaterThan(origin.visibility.mock.invocationCallOrder[0]!);
    origin.show(false);
    expect(shellFocus).toHaveBeenCalledOnce();
    origin.show(true);
    windowFocus(false);
    origin.show(false);
    expect(shellFocus).toHaveBeenCalledOnce();
  });
  it("release and Escape from another page reach the frozen origin once", () => {
    for (const key of ["Control", "Escape"]) {
      const { page, forwarded } = candidate();
      const origin = page("origin"), other = page("other");
      expect(origin.input()).toHaveBeenCalledOnce();
      const started = forwarded()[0]!;
      origin.focus(false);
      other.focus(true);
      expect(other.input(key === "Control" ? "keyUp" : "keyDown", key, key !== "Control")).toHaveBeenCalledOnce();
      expect(forwarded()).toEqual([started, expect.objectContaining({ kind: "cycle-input", id: "origin", cycleId: started.cycleId, key })]);
      expect(other.input("keyUp", "Control", false)).not.toHaveBeenCalled();
      expect(forwarded()).toHaveLength(2);
    }
  });
  it("a delayed old end cannot erase a new held cycle", () => {
    const { page, forwarded } = candidate();
    const origin = page("origin");
    origin.input();
    const oldId = forwarded()[0]!.cycleId;
    origin.input("keyUp", "Control", false);
    origin.input();
    const currentId = forwarded()[2]!.cycleId;
    expect(currentId).not.toBe(oldId);
    const end = ipc.get(BROWSER_CYCLE_END_CHANNEL)!;
    end({}, oldId);
    expect(origin.input("keyUp", "Control", false)).toHaveBeenCalledOnce();
    expect(forwarded()[3]).toMatchObject({ cycleId: currentId, key: "Control" });
    origin.input();
    const newestId = forwarded()[4]!.cycleId;
    end({}, newestId);
    end({}, newestId);
    expect(origin.input("keyUp", "Control", false)).not.toHaveBeenCalled();
    expect(forwarded()).toHaveLength(5);
  });
});
