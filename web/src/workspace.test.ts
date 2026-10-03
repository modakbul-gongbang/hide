import { describe, expect, it } from "vitest";
import type { Checkout, StripTab } from "./snapshot";
import { agentEntries, bodyStep, columnFrame, dividerLanding, DEFAULT_SLOTS, NO_CALLS_SEEN, readCalls, slotsForStep, type ColumnSizes, type ColumnSlots } from "./workspace";

const strip: StripTab[] = [
  { id: "herdr:1", kind: "herdr", source_id: "t1", label: "1", preview: false },
  { id: "file:a", kind: "file", source_id: "f-a", label: "a.md", preview: true },
  { id: "herdr:2", kind: "herdr", source_id: "t2", label: "2", preview: false },
  { id: "diff:b", kind: "diff", source_id: "d-b", label: "b.rs (working diff)", preview: false },
];

const checkout = { id: "c1", strip, tabs: [], active_tab_id: "t1" } as unknown as Checkout;

describe("the Workspace strips", () => {
  it("takes the Agent tabs from the core strip in the core's order", () => {
    expect(agentEntries(checkout).map((entry) => entry.source_id)).toEqual(["t1", "t2"]);
  });
});

// The token values (`--size-agent-views-min`, `--size-file-views-min`,
// `--size-panel-min`, `--size-file-views-ideal`, `--size-panel-ideal`,
// `--spacing-sm`), so the steps read as the PRD states them.
const sizes: ColumnSizes = { agentMin: 480, viewsMin: 360, toolsMin: 260, viewsIdeal: 640, toolsIdeal: 355, divider: 8 };

const frame = (input: { views?: boolean; tools?: boolean; viewsWidth?: number | null; toolsWidth?: number | null; body: number; slots?: ColumnSlots }) =>
  columnFrame({ views: input.views ?? false, tools: input.tools ?? false, viewsWidth: input.viewsWidth ?? null, toolsWidth: input.toolsWidth ?? null, body: input.body, sizes, slots: input.slots ?? DEFAULT_SLOTS });

describe("the Workspace columns (PRD three-column-panel)", () => {
  it("steps where the column minimums and their dividers fit: 1100 and 840 plus the dividers (D-07, B25)", () => {
    expect([1116, 1115, 848, 847].map((body) => bodyStep(body, sizes))).toEqual(["wide", "mid", "mid", "narrow"]);
    // Just over each step Agent Views still has its minimum.
    expect(columnFrame({ views: true, tools: true, viewsWidth: null, toolsWidth: null, body: 1116, sizes, slots: DEFAULT_SLOTS }).agents).toBe(480);
    expect(columnFrame({ views: true, tools: false, viewsWidth: null, toolsWidth: null, body: 848, sizes, slots: DEFAULT_SLOTS }).agents).toBe(480);
  });

  it("shows Agent Views alone with nothing on, and gives it the whole body (B1, B2)", () => {
    expect(frame({ body: 1600 })).toEqual({ step: "wide", agents: 1600, views: null, tools: null });
  });

  it("docks every column that is on at its default width, Agent Views taking the rest (B1, D-12)", () => {
    const wide = frame({ views: true, tools: true, body: 1600 });
    expect(wide).toEqual({ step: "wide", agents: 1600 - 16 - 640 - 355, views: 640, tools: 355 });
    expect(frame({ tools: true, body: 1600 })).toEqual({ step: "wide", agents: 1600 - 8 - 355, views: null, tools: 355 });
  });

  it("keeps a stored width, but never under a column's minimum or over what leaves Agent Views its own (B21)", () => {
    expect(frame({ views: true, viewsWidth: 100, body: 1600 }).views).toBe(360);
    const greedy = frame({ views: true, tools: true, viewsWidth: 5000, toolsWidth: 300, body: 1200 });
    expect(greedy.views).toBe(1200 - 16 - 300 - 480);
    expect(greedy.agents).toBe(480);
    const toolsGreedy = frame({ views: true, tools: true, toolsWidth: 5000, body: 1200 });
    expect(toolsGreedy.views).toBe(360);
    expect(toolsGreedy.agents).toBe(480);
  });

  it("between the steps shows one column beside Agent Views, Tools giving way first unless it was called (B25, B26)", () => {
    expect(frame({ views: true, tools: true, body: 1000 })).toMatchObject({ step: "mid", views: 1000 - 8 - 480, tools: null });
    expect(frame({ views: true, tools: true, body: 1000, slots: { side: "tools", single: "tools" } })).toMatchObject({ views: null, tools: 355 });
    // With only one of them on, that one shows.
    expect(frame({ tools: true, body: 1000 })).toMatchObject({ views: null, tools: 355 });
  });

  it("forgets a wide Tools call when entering mid, retaining only calls made there (B25, B26)", () => {
    const called: ColumnSlots = { side: "tools", single: "tools" };
    expect(frame({ views: true, tools: true, body: 1600, slots: called }).tools).toBe(355);
    const entered = slotsForStep(called, "wide", "mid");
    for (const body of [1100, 1115]) {
      expect(frame({ views: true, tools: true, body, slots: entered })).toMatchObject({ views: body - 8 - 480, tools: null });
    }
    const midCall = slotsForStep(called, "mid", "mid");
    expect(frame({ views: true, tools: true, body: 1100, slots: midCall })).toMatchObject({ views: null, tools: 355 });
    const widened = slotsForStep(midCall, "mid", "wide");
    expect(frame({ views: true, tools: true, body: 1100, slots: slotsForStep(widened, "wide", "mid") }).tools).toBeNull();
  });

  it("starts a newly narrow body on Agent Views, while a first-measurement reveal still shows its called column (B27)", () => {
    const called: ColumnSlots = { side: "views", single: "views" };
    expect(frame({ views: true, body: 847, slots: slotsForStep(called, "mid", "narrow") }).agents).toBe(847);
    expect(frame({ views: true, body: 847, slots: slotsForStep(called, null, "narrow") }).views).toBe(847);
    expect(frame({ views: true, body: 846, slots: slotsForStep(called, "narrow", "narrow") }).views).toBe(846);
  });

  it("below the narrow step shows one column, Agent Views unless another that is on was called (B25, B27)", () => {
    expect(frame({ views: true, tools: true, body: 700 })).toEqual({ step: "narrow", agents: 700, views: null, tools: null });
    expect(frame({ views: true, body: 700, slots: { side: "views", single: "views" } })).toEqual({ step: "narrow", agents: null, views: 700, tools: null });
    // A column that is off cannot fill the slot.
    expect(frame({ body: 700, slots: { side: "views", single: "views" } }).agents).toBe(700);
  });

  it("places nothing beside Agent Views before the body is measured, so no terminal fits to a guess", () => {
    expect(frame({ views: true, tools: true, body: 0 })).toMatchObject({ views: null, tools: null });
  });

  it("lands the File Views divider as its width, within the minimums (D-12, B20)", () => {
    const shown = frame({ views: true, body: 1600 });
    // The divider dragged 100px left widens File Views by 100px.
    const x = 1600 - 640 - 8 - 100;
    expect(dividerLanding({ divider: "views", x, body: 1600, frame: shown, sizes })).toEqual({ views_width: 740 });
    expect(dividerLanding({ divider: "views", x: 0, body: 1600, frame: shown, sizes })).toEqual({ views_width: 1600 - 8 - 480 });
    expect(dividerLanding({ divider: "views", x: 1590, body: 1600, frame: shown, sizes })).toEqual({ views_width: 360 });
  });

  it("trades width between File Views and Tools at the divider between them, so Agent Views keeps its width", () => {
    const shown = frame({ views: true, tools: true, body: 1600 });
    const x = 1600 - 355 - 8 - 45;
    const landed = dividerLanding({ divider: "tools", x, body: 1600, frame: shown, sizes });
    expect(landed).toEqual({ views_width: 640 - 45, tools_width: 400 });
    expect((landed.views_width ?? 0) + (landed.tools_width ?? 0)).toBe(640 + 355);
    // File Views keeps its minimum.
    expect(dividerLanding({ divider: "tools", x: 0, body: 1600, frame: shown, sizes })).toEqual({ views_width: 360, tools_width: 640 + 355 - 360 });
  });

  it("lands a lone Tools divider as Tools' width against Agent Views", () => {
    const shown = frame({ tools: true, body: 1600 });
    expect(dividerLanding({ divider: "tools", x: 1600 - 8 - 500, body: 1600, frame: shown, sizes })).toEqual({ tools_width: 500 });
    expect(dividerLanding({ divider: "tools", x: 1590, body: 1600, frame: shown, sizes })).toEqual({ tools_width: 260 });
  });
});

describe("readCalls", () => {
  const view = (path: string, views_called: number, views_calls: number) => ({ device_id: "local", path, views_called, views_calls });

  it("takes the first count as history, then shows File Views for a call to the Workspace in front", () => {
    const first = readCalls(NO_CALLS_SEEN, view("/a", 3, 5));
    expect(first).toMatchObject({ reset: true, call: false });
    const same = readCalls(first.seen, view("/a", 3, 5));
    expect(same).toMatchObject({ reset: false, call: false });
    const called = readCalls(same.seen, view("/a", 6, 6));
    expect(called).toMatchObject({ reset: false, call: true });
    expect(readCalls(called.seen, view("/a", 6, 6)).call).toBe(false);
  });

  it("leaves the front Workspace alone for a call into another, and shows the one a call brings in front", () => {
    const start = readCalls(NO_CALLS_SEEN, view("/a", 0, 2)).seen;
    const elsewhere = readCalls(start, view("/a", 0, 3));
    expect(elsewhere.call).toBe(false);
    // That older call does not move its Workspace when it is chosen later.
    expect(readCalls(elsewhere.seen, view("/b", 3, 3))).toMatchObject({ reset: true, call: false });
    // A reveal from the Overview: the called Workspace comes in front with the call.
    const away = readCalls(elsewhere.seen, null).seen;
    expect(readCalls(away, view("/c", 4, 4))).toMatchObject({ reset: true, call: true });
  });

  it("counts again from a core that started again", () => {
    const old = readCalls(NO_CALLS_SEEN, view("/a", 9, 9)).seen;
    const restarted = readCalls(old, view("/a", 0, 0));
    expect(restarted.call).toBe(false);
    expect(readCalls(restarted.seen, view("/a", 1, 1)).call).toBe(true);
    // A core first read at zero that starts again at zero still counts its calls.
    const fresh = readCalls(readCalls(NO_CALLS_SEEN, view("/a", 0, 0)).seen, view("/a", 3, 3)).seen;
    const again = readCalls(fresh, view("/a", 0, 0)).seen;
    expect(readCalls(again, view("/a", 1, 1)).call).toBe(true);
  });
});
