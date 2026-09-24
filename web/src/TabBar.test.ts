import { describe, expect, it } from "vitest";
import { activeStripId, closingSuffix } from "./TabBar";
import type { AsyncOperation, Checkout, EditorSnapshot } from "./snapshot";

function op(kind: string, target_id: string, phase: string): AsyncOperation {
  return { id: `${kind}:${target_id}`, kind, target_id, scope_id: "c1", phase, stage: "", message: null, retryable: false };
}

const CHECKOUT = { id: "c1", active_tab_id: "h1" } as unknown as Checkout;

function editor(activeTabId: string | null): EditorSnapshot {
  return {
    active_tab_id: activeTabId,
    document: null,
    tabs: [
      {
        id: "f1",
        workspace_id: "w1",
        checkout_id: "c1",
        path: "/checkout/hide/README.md",
        label: "README.md",
        kind: "file",
        diff_committed: null,
        markdown_live: true,
        wrap: false,
        dirty: false,
        preview: true,
      },
    ],
  };
}

describe("closingSuffix", () => {
  it("shows while the close is in flight and not after it settles", () => {
    expect(closingSuffix("t1", "tab.close", [op("tab.close", "t1", "transmitting")])).toBe(true);
    expect(closingSuffix("t1", "tab.close", [op("tab.close", "t1", "awaiting_topology")])).toBe(true);
    expect(closingSuffix("t1", "tab.close", [op("tab.close", "t1", "unknown")])).toBe(true);
    expect(closingSuffix("t1", "tab.close", [op("tab.close", "t1", "completed")])).toBe(false);
    expect(closingSuffix("t1", "tab.close", [op("tab.close", "t1", "failed")])).toBe(false);
    expect(closingSuffix("t1", "tab.close", [op("pane.close", "t1", "transmitting")])).toBe(false);
    expect(closingSuffix("t2", "tab.close", [op("tab.close", "t1", "transmitting")])).toBe(false);
  });
});

describe("activeStripId", () => {
  it("names the editor's active tab while the editor owns the surface", () => {
    expect(activeStripId(CHECKOUT, editor("f1"))).toBe("f1");
  });

  it("names the Herdr tab when no editor tab is showing", () => {
    expect(activeStripId(CHECKOUT, editor(null))).toBe("h1");
    expect(activeStripId(CHECKOUT, null)).toBe("h1");
  });
});
