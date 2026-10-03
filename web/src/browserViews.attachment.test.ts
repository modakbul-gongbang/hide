// @vitest-environment jsdom
import { act, createElement, Fragment } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterAll, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { createActions } from "./actions";
import { BrowserHost } from "./BrowserDisplay";
import { hostKey, syncBrowserFront, useBrowserStore } from "./browserViews";
import type { BrowserHostEvent, BrowserSync, HostBridge } from "./host";
import { TooltipProvider } from "./components/ui/tooltip";
import type { ViewDisplaySnapshot, ViewLayoutSnapshot } from "./snapshot";
import { useShellStore } from "./store";
import { workspaceKey } from "./viewLayout";
import { DisplayTab } from "./ViewAreas";

const browserCanvas = vi.hoisted(() => {
  const original = HTMLCanvasElement.prototype.getContext;
  HTMLCanvasElement.prototype.getContext = () => null;
  return { restore: () => { HTMLCanvasElement.prototype.getContext = original; } };
});

// Only the platform bridge and frame clock are fakes; sync, stores and subscriptions stay real.
const listeners = new Set<(event: BrowserHostEvent) => void>();
const frames: FrameRequestCallback[] = [];
const sent: BrowserSync[] = [];
const bridge: HostBridge = {
  kind: "electron", platform: "darwin", onCommand: () => () => {}, reportBindings() {},
  revealPath() {}, pickFolder: async () => null, probePaths: async () => [], openPath() {},
  browser: {
    sync: (state: BrowserSync) => { sent.push(state); },
    capture: async () => null, endCycle() {}, command() {},
    onEvent: (listener: (event: BrowserHostEvent) => void) => {
      listeners.add(listener);
      return () => { listeners.delete(listener); };
    },
  },
};
const actions = createActions(() => {});
let root: Root;
let container: HTMLDivElement;
const local = { device_id: "local", path: "/fixture/a" };
const other = { device_id: "local", path: "/fixture/b" };
const remote = { device_id: "ssh", path: "/fixture/a" };
const inventory = [local, other, remote].map((workspace) => ({ ...workspace, view_id: "d1", area_id: "a1" }));
const layout = {
  root: { area: { id: "a1", active: "d1", displays: [{ id: "d1", kind: "browser", url: "https://example.test/", load: 1 }] } },
} as unknown as ViewLayoutSnapshot;

function emit(event: BrowserHostEvent): void {
  act(() => { for (const listener of listeners) listener(event); });
}

function attach(workspace = local, attached = true): void {
  emit({ kind: "attached", workspace: workspaceKey(workspace), id: "d1", attached });
}

beforeAll(() => {
  window.hideHost = bridge;
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => { frames.push(callback); return frames.length; });
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});
beforeEach(() => {
  act(() => {
    useShellStore.setState({ connection: "live" });
    root.render(createElement(BrowserHost, { actions }));
  });
  syncBrowserFront(null, null, []);
  syncBrowserFront(local, layout, inventory);
  sent.length = 0;
});
afterAll(() => {
  act(() => root.unmount());
  container.remove();
  delete window.hideHost;
  browserCanvas.restore();
  vi.unstubAllGlobals();
});

describe("native browser attachment facts", () => {
  it("does not alias display IDs across checkouts or devices", () => {
    attach();
    expect(useBrowserStore.getState().attached).toEqual({ [hostKey(workspaceKey(local), "d1")]: true });
    attach(other);
    attach(remote);
    attach(local, false);
    expect(useBrowserStore.getState().attached).toEqual({
      [hostKey(workspaceKey(other), "d1")]: true,
      [hostKey(workspaceKey(remote), "d1")]: true,
    });
  });

  it("publishes true/false transitions once and rejects unknown page attachments", () => {
    const publish = vi.fn();
    const unsubscribe = useBrowserStore.subscribe(publish);
    try {
      attach();
      attach();
      emit({ kind: "attached", workspace: workspaceKey(local), id: "d1", attached: "true" } as unknown as BrowserHostEvent);
      attach({ device_id: "local", path: "/not-retained" });
      expect(publish).toHaveBeenCalledTimes(1);
      attach(local, false);
      attach(local, false);
      expect(publish).toHaveBeenCalledTimes(2);
      expect(useBrowserStore.getState().attached).toEqual({});
    } finally {
      unsubscribe();
    }
  });

  it("clears an evicted native page even while its display remains in inventory", () => {
    attach();
    emit({ kind: "gone", workspace: workspaceKey(local), id: "d1", load: 1, url: "https://example.test/" });
    expect(useBrowserStore.getState().attached).toEqual({});
  });

  it("keeps the attachment fact when its authoritative View area changes", () => {
    attach();
    const publish = vi.fn();
    const unsubscribe = useBrowserStore.subscribe(publish);
    try {
      syncBrowserFront(local, { root: { area: { id: "a2", active: "d1", displays: [{ id: "d1", kind: "browser", url: "https://example.test/", load: 1 }] } } } as unknown as ViewLayoutSnapshot,
        inventory.map((row) => row.path === local.path && row.device_id === local.device_id ? { ...row, area_id: "a2" } : row));
      expect(useBrowserStore.getState().attached[hostKey(workspaceKey(local), "d1")]).toBe(true);
      expect(publish).not.toHaveBeenCalled();
    } finally {
      unsubscribe();
    }
  });

  it("keeps hidden retained attachments and prunes inventory or display removal", () => {
    attach();
    attach(other);
    syncBrowserFront(other, layout, inventory);
    expect(useBrowserStore.getState().attached[hostKey(workspaceKey(local), "d1")]).toBe(true);
    syncBrowserFront(other, layout, inventory.filter((row) => row.path === other.path));
    expect(useBrowserStore.getState().attached).toEqual({ [hostKey(workspaceKey(other), "d1")]: true });
    syncBrowserFront(other, { root: { area: { id: "a1", active: null, displays: [] } } } as unknown as ViewLayoutSnapshot, inventory);
    attach(other);
    expect(useBrowserStore.getState().attached).toEqual({});
  });

  it("drops facts on app disconnect and ignores late attachment events", () => {
    attach();
    act(() => useShellStore.getState().setConnection("gone"));
    attach();
    expect(useBrowserStore.getState().attached).toEqual({});
    act(() => useShellStore.getState().setConnection("live"));
    expect(useBrowserStore.getState().attached).toEqual({});
    attach();
    expect(useBrowserStore.getState().attached[hostKey(workspaceKey(local), "d1")]).toBe(true);
  });

  it("syncs authoritative areas for both displayed and retained inventory", () => {
    for (const callback of frames.splice(0)) callback(0);
    expect(sent.at(-1)).toEqual({
      workspace: workspaceKey(local),
      displays: [{ id: "d1", area_id: "a1", url: "https://example.test/", load: 1, rect: null, visible: false }],
      retained: inventory.map((row) => ({ workspace: workspaceKey(row), id: row.view_id, area_id: row.area_id })),
    });
    expect(listeners.size).toBe(1);
  });

  it("shows the existing badge only on the attached browser's own Workspace tab", () => {
    const display = { id: "d1", kind: "browser", path: "", label: "한글 브라우저 검증", url: "https://example.test/", state: "open", tab_id: null, preview: false } as ViewDisplaySnapshot;
    const interaction = { selected: true, areaActive: true, fit: "titled" as const, dragging: false, press() {}, select() {} };
    act(() => root.render(createElement(Fragment, null,
      createElement(BrowserHost, { actions }),
      createElement(TooltipProvider, null,
        createElement(DisplayTab, { display, workspace: local, interaction, actions }),
        createElement(DisplayTab, { display, workspace: other, interaction, actions }),
        createElement(DisplayTab, { display: { ...display, kind: "file" }, workspace: local, interaction, actions }),
      ))));
    // Mounting the host first initializes its inventory before the real event arrives.
    syncBrowserFront(local, layout, inventory);
    attach();
    const tabs = [...container.querySelectorAll('[role="tab"]')];
    expect(tabs.map((tab) => tab.querySelector('[data-browser-attached]') !== null)).toEqual([true, false, false]);
    expect(tabs[0]?.getAttribute("aria-label")).toContain("Agent attached");
    expect(tabs[1]?.getAttribute("aria-label")).not.toContain("Agent attached");
    attach(local, false);
    expect(container.querySelector('[data-browser-attached]')).toBeNull();
    expect(tabs[0]?.getAttribute("aria-label")).not.toContain("Agent attached");
  });

  it("unsubscribes and clears facts when the host leaves the app", () => {
    attach();
    act(() => root.render(null));
    expect(listeners.size).toBe(0);
    expect(useBrowserStore.getState().attached).toEqual({});
  });
});
