import { describe, expect, it } from "vitest";
import { closingSuffix } from "./TabBar";
import type { AsyncOperation } from "./snapshot";

function op(kind: string, target_id: string, phase: string): AsyncOperation {
  return { id: `${kind}:${target_id}`, kind, target_id, scope_id: "c1", phase, stage: "", message: null, retryable: false };
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

describe("tab composition", () => {
  it("renders the core's representative mark, provider and title in that order", async () => {
    const { createElement } = await import("react");
    const { renderToStaticMarkup } = await import("react-dom/server");
    const { AgentTab } = await import("./TabBar");
    const { TooltipProvider } = await import("./components/ui/tooltip");
    const checkout = {
      id: "c1", next_tab_label: "Tab 2",
      strip: [{ id: "herdr:t1", kind: "herdr", source_id: "t1", label: "작업 제목", preview: false }],
      tabs: [{ id: "t1", agent: { agent_kind: "claude", symbol: "!", demand: "approval", activity: "stopped", emphasized: true, waiting_on_descendants: false, status_label: "Needs You" } }],
    } as unknown as import("./snapshot").Checkout;
    const markup = renderToStaticMarkup(createElement(TooltipProvider, { children: createElement(AgentTab, { number: 2, checkout, entry: checkout.strip[0]!, interaction: { selected: true, icon: false, areaActive: true, dragging: false, press: () => {}, select: () => {} }, renaming: false, onCancelRename: () => {}, actions: {} as import("./actions").Actions }) }));
    const mark = markup.indexOf('data-tab-status="Needs You"');
    const logo = markup.indexOf('<img', mark);
    const title = markup.indexOf('작업 제목', logo);
    expect(mark).toBeGreaterThan(0);
    expect(logo).toBeGreaterThan(mark);
    expect(title).toBeGreaterThan(logo);
    expect(markup).toContain('data-keycap="2"');
  });
});
