import { EventEmitter } from "node:events";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { initializeInterfaceI18n } from "../../../web/src/i18n/instance";
import { REGISTRY, type Command } from "../../../web/src/shortcuts";
import { BROWSER_CYCLE_END_CHANNEL, BROWSER_EVENT_CHANNEL } from "../channel";
import { BrowserViews, type ResolvedPage } from "./browser";
import { browserPartition } from "./browserSync";
import type { HostLog } from "./log";

const english = initializeInterfaceI18n("en").getFixedT(null, "translation");
const ipc = vi.hoisted(() => new Map<string, (event: unknown, value?: unknown) => void>());
vi.mock("electron", () => ({
  ipcMain: { on: (channel: string, listener: (event: unknown, value?: unknown) => void) => ipc.set(channel, listener), handle: vi.fn() },
  Menu: { getApplicationMenu: () => null },
  BrowserWindow: {}, WebContentsView: {}, session: {}, shell: {},
}));

describe("native browser load generations", () => {
  it.each(["canonical", "superseded", "partition"] as const)("handles a delayed %s route after its loading report", async (scenario) => {
    vi.useFakeTimers();
    try {
      let answer!: (route: ResolvedPage) => void;
      const resolve = vi.fn(() => new Promise<ResolvedPage>((done) => { answer = done; }));
      const subject = new BrowserViews({ event: vi.fn() } as unknown as HostLog, () => true, resolve, vi.fn(), vi.fn(), () => english);
      const workspace = "local\u0000/checkout";
      const requested = "file:///var/checkout/manual.html";
      const canonical = "file:///private/var/checkout/manual.html";
      const loadURL = vi.fn().mockResolvedValue(undefined);
      const page = {
        key: "manual", workspace, id: "manual", applied: 1,
        partition: browserPartition(workspace, requested), route: null,
        state: { url: requested, loading: true, failure: null }, report: null,
        view: { webContents: { loadURL } },
      };
      (Reflect.get(subject, "pages") as Map<string, unknown>).set(page.key, page);
      Reflect.get(subject, "load").call(subject, page, requested);
      await vi.advanceTimersByTimeAsync(100);
      if (scenario === "superseded") page.applied = 2;
      const source = scenario === "partition" ? "https://example.test/" : canonical;
      answer({ url: source, source_url: source, load: 1 });
      await vi.advanceTimersByTimeAsync(0);
      if (scenario === "canonical") {
        expect(loadURL).toHaveBeenCalledExactlyOnceWith(canonical);
        expect(page.route).toEqual({ url: canonical, source_url: canonical, load: 1 });
      } else {
        expect(loadURL).not.toHaveBeenCalled();
        if (scenario === "partition") expect(page.state).toMatchObject({ loading: false, failure: expect.any(String) });
      }
    } finally {
      vi.clearAllTimers();
      vi.useRealTimers();
    }
  });
});

function candidate() {
  const send = vi.fn();
  const shellFocus = vi.fn();
  let windowFocused = true;
  const subject = new BrowserViews({ event: vi.fn() } as unknown as HostLog, () => true, vi.fn(), vi.fn(), vi.fn(), () => english);
  const shellContents = Object.assign(new EventEmitter(), { isDestroyed: () => false, send, focus: shellFocus });
  const window = Object.assign(new EventEmitter(), { isFocused: () => windowFocused, webContents: shellContents });
  subject.attach(window as never);
  const input = (contents: EventEmitter, type = "keyDown", key = "Tab", control = true, alt = false) => {
    const event = { preventDefault: vi.fn() };
    contents.emit("before-input-event", event, { type, key, code: key === "Control" ? "ControlLeft" : key, control, alt, meta: false, shift: false, isComposing: false });
    return event.preventDefault;
  };
  const page = (id: string) => {
    let focused = true;
    const pageFocus = vi.fn();
    const contents = Object.assign(new EventEmitter(), { isFocused: () => focused, setWindowOpenHandler: vi.fn(), focus: pageFocus });
    const value = { key: id, workspace: "fixture", id, visible: true, view: { webContents: contents, setVisible: vi.fn() }, state: {}, report: null, route: null, applied: 1, partition: "fixture", shownAt: 0 };
    const watch = Reflect.get(subject, "watch") as (page: unknown) => void;
    watch.call(subject, value);
    (Reflect.get(subject, "pages") as Map<string, unknown>).set(id, value);
    return {
      pageFocus,
      focus: (next: boolean) => { focused = next; },
      crash: () => contents.emit("render-process-gone", {}, { reason: "crashed" }),
      show: (visible: boolean) => Reflect.get(subject, "show").call(subject, value, visible),
      visibility: value.view.setVisible,
      input: (type?: string, key?: string, control?: boolean, alt?: boolean) => input(contents, type, key, control, alt),
    };
  };
  const forwarded = () => send.mock.calls.filter(([channel]) => channel === BROWSER_EVENT_CHANNEL).map(([, value]) => value as { kind: string; id: string; cycleId: number; key: string });
  return { page, forwarded, shellFocus, shellInput: (type?: string, key?: string, control?: boolean, alt?: boolean) => input(shellContents, type, key, control, alt), registry: (rows: readonly Command[]) => subject.setRegistry(rows), blur: () => window.emit("blur"), windowReturn: () => window.emit("focus"), shellRegainsFocus: () => shellContents.emit("focus"), windowFocus: (focused: boolean) => { windowFocused = focused; } };
}

describe("native held cycle delivery", () => {
  beforeEach(() => ipc.clear());
  it("freezing a held native page gives its remaining input to the same focused shell", () => {
    const { page, shellFocus, windowFocus } = candidate();
    const origin = page("origin"), other = page("other");
    other.show(false);
    expect(shellFocus).not.toHaveBeenCalled();
    origin.input();
    expect(shellFocus).toHaveBeenCalledOnce();
    origin.show(false);
    expect(shellFocus).toHaveBeenCalledTimes(2);
    expect(shellFocus.mock.invocationCallOrder[1]).toBeGreaterThan(origin.visibility.mock.invocationCallOrder[0]!);
    origin.show(false);
    expect(shellFocus).toHaveBeenCalledTimes(2);
    origin.show(true);
    windowFocus(false);
    origin.show(false);
    expect(shellFocus).toHaveBeenCalledTimes(2);
  });
  it("hands off a still-visible origin once and orders a quick shell release after the undelivered start", () => {
    const { page, shellFocus, shellInput, forwarded } = candidate();
    const origin = page("uncovered");
    expect(shellInput()).not.toHaveBeenCalled();
    expect(origin.input()).toHaveBeenCalledOnce();
    expect(shellFocus).toHaveBeenCalledOnce();
    expect(origin.visibility).not.toHaveBeenCalled();
    // No renderer has received the queued start yet. Native shell interception
    // still consumes the release and appends it on the same bridge, once.
    expect(shellInput("keyUp", "Control", false)).toHaveBeenCalledOnce();
    const queued = forwarded();
    expect(queued).toEqual([
      expect.objectContaining({ kind: "cycle-input", id: "uncovered", key: "Tab" }),
      expect.objectContaining({ kind: "cycle-input", id: "uncovered", key: "Control", cycleId: queued[0]!.cycleId }),
    ]);
    expect(shellInput("keyUp", "Control", false)).not.toHaveBeenCalled();
    expect(forwarded()).toHaveLength(2);
    expect(shellFocus).toHaveBeenCalledOnce();
  });
  it("routes rebound held repeats and Escape once without handing off again", () => {
    const { page, registry, shellInput, shellFocus, forwarded } = candidate();
    registry(REGISTRY.map((row) => row.id === "recent_area_tab" ? { ...row, electron: { code: "Tab", ctrl: true, alt: true } } : row));
    expect(page("origin").input("keyDown", "Tab", true, true)).toHaveBeenCalledOnce();
    expect(shellInput("keyDown", "Tab", true, true)).toHaveBeenCalledOnce();
    expect(shellInput("keyDown", "Escape", true, true)).toHaveBeenCalledOnce();
    expect(forwarded().map((row) => [row.id, row.key, row.cycleId])).toEqual([
      ["origin", "Tab", 1], ["origin", "Tab", 1], ["origin", "Escape", 1],
    ]);
    expect(shellFocus).toHaveBeenCalledOnce();
    expect(shellInput("keyUp", "Control", false, true)).not.toHaveBeenCalled();
  });
  it("never focuses a background window and cancels its held route on window blur", () => {
    const { page, shellFocus, shellInput, forwarded, windowFocus, blur } = candidate();
    windowFocus(false);
    page("origin").input();
    expect(shellFocus).not.toHaveBeenCalled();
    blur();
    expect(forwarded()).toEqual([
      expect.objectContaining({ kind: "cycle-input", cycleId: 1 }),
      expect.objectContaining({ kind: "cycle-cancel", id: "origin", cycleId: 1, windowLost: true }),
    ]);
    expect(shellInput("keyUp", "Control", false)).not.toHaveBeenCalled();
  });
  it("says whether the window or the page ended a hold, and never refocuses the page itself", () => {
    const { page, blur, windowReturn, forwarded } = candidate();
    const origin = page("origin");
    origin.input();
    blur();
    windowReturn();
    expect(forwarded().at(-1)).toMatchObject({ kind: "cycle-cancel", windowLost: true });
    expect(origin.pageFocus).not.toHaveBeenCalled();
    origin.input();
    origin.crash();
    expect(forwarded().at(-1)).toMatchObject({ kind: "cycle-cancel", windowLost: false });
  });
  it("tells the shell, in order after a cancel, when its own contents hold the keyboard again", () => {
    const { page, blur, forwarded, shellRegainsFocus } = candidate();
    page("origin").input();
    blur();
    shellRegainsFocus();
    expect(forwarded().map((event) => event.kind)).toEqual(["cycle-input", "cycle-cancel", "window-key"]);
  });
  it("a hold the shell ends while the window is not key was ended by losing it", () => {
    for (const focused of [true, false]) {
      const { page, forwarded, windowFocus } = candidate();
      const origin = page("origin");
      origin.input();
      windowFocus(focused);
      ipc.get(BROWSER_CYCLE_END_CHANNEL)!({}, forwarded()[0]!.cycleId);
      expect(forwarded().filter((event) => event.kind === "cycle-cancel")).toEqual(focused ? [] : [expect.objectContaining({ id: "origin", windowLost: true })]);
    }
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
