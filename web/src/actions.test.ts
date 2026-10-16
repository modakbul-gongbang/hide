// @vitest-environment jsdom
import { afterAll, afterEach, describe, expect, it, vi } from "vitest";
import { createActions } from "./actions";
import { useUiStore } from "./ui";
import { workspaceKey } from "./viewLayout";
import type { DispatchFn } from "./ws";

// No terminal renders here. Keep the real actions import while answering
// xterm's canvas capability probe at the browser boundary.
const browserCanvas = vi.hoisted(() => {
  const original = HTMLCanvasElement.prototype.getContext;
  HTMLCanvasElement.prototype.getContext = () => null;
  return { restore: () => { HTMLCanvasElement.prototype.getContext = original; } };
});
afterAll(() => browserCanvas.restore());
afterEach(() => useUiStore.getState().setViewFocusRequest(null));

describe("which pages the keyboard follows", () => {
  const workspace = { device_id: "local", path: "/repo" };
  const opened = () => {
    const sent: Parameters<DispatchFn>[0][] = [];
    const actions = createActions((event) => { sent.push(event); return true; });
    const requestIds = () => sent.filter((event) => event.kind === "browser_open").map((event) => (event.payload as { request_id?: string }).request_id);
    return { actions, requestIds };
  };

  it("names a page the operator opens and waits for the display the core answers with", () => {
    const { actions, requestIds } = opened();
    actions.openBrowser("https://a.test/", workspace);
    const [id] = requestIds();
    expect(id).toMatch(/^follow:/);
    expect(useUiStore.getState().viewFocusRequest).toEqual({ workspace: workspaceKey(workspace), browserOpen: id });
  });

  it("leaves the keyboard alone for a New tab, whose address field takes it, and for a page's own new tab", () => {
    const { actions, requestIds } = opened();
    actions.openBrowser("", workspace, "a1");
    actions.openBrowser("https://b.test/", workspace, undefined, "page-1");
    expect(requestIds()).toEqual([undefined, undefined]);
    expect(useUiStore.getState().viewFocusRequest).toBeNull();
  });
});
