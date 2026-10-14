// @vitest-environment jsdom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import { UsageCost } from "./usage-amount";
import { UsageHistory } from "./usage-history";
import { TooltipProvider } from "./ui/tooltip";
import { ledgerScene } from "../gallery/usageHistorySceneData";
import { clientI18n } from "../i18n/translator";

vi.hoisted(() => { HTMLCanvasElement.prototype.getContext = () => null; });
vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
afterEach(() => { document.body.innerHTML = ""; });

it("distinguishes unknown, pending and free prices, retaining a partial known subtotal", async () => {
  const container = document.createElement("div");
  document.body.append(container);
  const root = createRoot(container);
  const amount = ledgerScene({ node: "local", agents: [], history: null }, "reference").history!.year.range.amount;
  const cases = [
    [{ micro_usd: 0, partial: true }, "Price unavailable"],
    [{ micro_usd: 0, waiting: true }, "Pricing pending"],
    [{ micro_usd: 0 }, "≈ $0.00"],
    [{ micro_usd: 1250000, partial: true }, "≥ $1.25"],
  ] as const;
  try {
    await act(async () => { await clientI18n.changeLanguage("en"); });
    for (const [cost, expected] of cases) {
      await act(async () => root.render(<TooltipProvider><UsageCost amount={{ ...amount, cost: { ...amount.cost, ...cost } }} /></TooltipProvider>));
      expect(container.textContent).toContain(expected);
    }
    await act(async () => root.render(<TooltipProvider><UsageCost amount={{ ...amount, calls: 0 }} /></TooltipProvider>));
    expect(container.textContent).toBe("No data");
  } finally { await act(async () => root.unmount()); }
});

it("pins a day across hover exit, keeps thirty cells and removes the grid on unmount", async () => {
  const container = document.createElement("div");
  document.body.append(container);
  const root = createRoot(container);
  const node = ledgerScene({ node: "local", agents: [], history: null }, "reference");
  try {
    await act(async () => { await clientI18n.changeLanguage("en"); root.render(<TooltipProvider><UsageHistory node={node} surface="popover" outline={null} /></TooltipProvider>); });
    expect(container.querySelectorAll("[data-usage-day]")).toHaveLength(30);
    const day = container.querySelector<HTMLButtonElement>('[data-usage-day="2026-10-11"]')!;
    await act(async () => day.click());
    expect(container.querySelector("[data-usage-date]")!.textContent).toContain("Oct 11");
    expect(container.querySelector("[data-usage-total]")!.textContent).toBe("4M");
    await act(async () => day.dispatchEvent(new MouseEvent("mouseout", { bubbles: true, relatedTarget: document.body })));
    expect(container.querySelector("[data-usage-total]")!.textContent).toBe("4M");
    expect(day.getAttribute("aria-pressed")).toBe("true");
  } finally { await act(async () => root.unmount()); }
  expect(container.querySelectorAll("[data-usage-day]")).toHaveLength(0);
});

it("switches between seven daily bars and thirty contribution days with the matching totals", async () => {
  // The approved comparison retains 7-day bars alongside 30-day grass.
  // Two recorded days straddle the seven-day boundary: $200/100M and $8/4M.
  const container = document.createElement("div");
  document.body.append(container);
  const root = createRoot(container);
  const node = ledgerScene({ node: "local", agents: [], history: null }, "reference");
  const range = node.history!.popover.range;
  range.days = range.days.filter((day) => ["2026-10-01", "2026-10-11"].includes(day.date)).map((day) => ({
    ...day, amount: { ...day.amount, calls: 1, new_tokens: day.date === "2026-10-01" ? 100_000_000 : 4_000_000,
      cost: { ...day.amount.cost, micro_usd: day.date === "2026-10-01" ? 200_000_000 : 8_000_000 } },
  }));
  const periodButton = (label: string) => container.querySelector<HTMLButtonElement>(`button[aria-label="${label}"]`);
  const dayButton = (date: string) => container.querySelector<HTMLButtonElement>(`[data-usage-day="${date}"]`)!;
  const total = () => container.querySelector("[data-usage-period-total]")!.textContent;
  try {
    await act(async () => { await clientI18n.changeLanguage("en"); root.render(<TooltipProvider><UsageHistory node={node} surface="popover" outline={null} /></TooltipProvider>); });
    expect(periodButton("Last 7 days")).not.toBeNull();
    expect(periodButton("Last 30 days")!.getAttribute("aria-checked")).toBe("true");
    expect(total()).toContain("$208.00");
    expect(total()).toContain("104M");
    await act(async () => dayButton("2026-10-01").click());
    await act(async () => periodButton("Last 7 days")!.click());
    expect(periodButton("Last 7 days")!.getAttribute("aria-checked")).toBe("true");
    expect(container.querySelectorAll("[data-usage-day]")).toHaveLength(7);
    expect(dayButton("2026-10-05")).toBeNull();
    expect(dayButton("2026-10-06")).not.toBeNull();
    expect(total()).toContain("$8.00");
    expect(total()).toContain("4M");
    expect(container.querySelector("[data-usage-date]")!.textContent).toBe("Today");
    await act(async () => dayButton("2026-10-12").focus());
    await act(async () => dayButton("2026-10-12").dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowLeft", bubbles: true })));
    expect(document.activeElement).toBe(dayButton("2026-10-11"));
    expect(container.querySelector("[data-usage-date]")!.textContent).toContain("Oct 11");
    expect(container.querySelector("[data-usage-total]")!.textContent).toBe("4M");
    await act(async () => dayButton("2026-10-11").click());
    await act(async () => periodButton("Last 30 days")!.click());
    expect(container.querySelectorAll("[data-usage-day]")).toHaveLength(30);
    expect(container.querySelector("[data-usage-date]")!.textContent).toContain("Oct 11");
    expect(total()).toContain("$208.00");
  } finally { await act(async () => root.unmount()); }
});

async function mount(node: ReturnType<typeof ledgerScene>, surface: "popover" | "year", language = "en") {
  const container = document.createElement("div");
  document.body.append(container);
  const root = createRoot(container);
  await act(async () => { await clientI18n.changeLanguage(language); root.render(<TooltipProvider><UsageHistory node={node} surface={surface} outline={null} /></TooltipProvider>); });
  return { container, unmount: () => act(async () => root.unmount()) };
}
const scene = (history: string) => ledgerScene({ node: "local", agents: [], history: null }, history);

it("reads Hide AI as one line under the summary: tokens and share, no money", async () => {
  const node = scene("reference");
  const view = await mount(node, "popover");
  try {
    const counted = node.history!.popover.range.days.find((day) => day.hide_ai)!;
    await act(async () => view.container.querySelector<HTMLButtonElement>(`[data-usage-day="${counted.date}"]`)!.click());
    const line = view.container.querySelector('[data-usage-hide-ai]')!;
    expect(line.getAttribute("data-usage-hide-ai")).toBe("counted");
    expect(line.textContent).toContain("Hide AI");
    expect(line.textContent).toMatch(/\d%/);
    expect(line.textContent).not.toContain("$");
    expect(line.querySelector("[data-usage-hide-tokens]")!.textContent).not.toContain("≥");
  } finally { await view.unmount(); }
});

it("marks a Hide AI amount that unknown calls make a lower bound", async () => {
  const node = scene("hide-unknown");
  const view = await mount(node, "popover");
  try {
    const day = node.history!.popover.range.days.find((item) => item.hide_ai && item.hide_ai.unknown_calls > 0)!;
    await act(async () => view.container.querySelector<HTMLButtonElement>(`[data-usage-day="${day.date}"]`)!.click());
    const line = view.container.querySelector('[data-usage-hide-ai="counted"]')!;
    expect(line.querySelector("[data-usage-hide-tokens]")!.textContent).toMatch(/^≥ /);
    expect(line.getAttribute("aria-label")).toContain("at least");
  } finally { await view.unmount(); }
});

it("says Hide AI was not counted before its first day and has no calls on a later day when nothing was counted", async () => {
  const node = scene("reference");
  const range = node.history!.popover.range;
  const [early, later] = range.days.filter((day) => day.hide_ai).slice(-2);
  // Counting began after `early`: the core has no Hide AI for it and names the first counted day.
  early!.hide_ai = null;
  range.hide_ai_since = later!.date;
  const view = await mount(node, "popover");
  try {
    await act(async () => view.container.querySelector<HTMLButtonElement>(`[data-usage-day="${early!.date}"]`)!.click());
    expect(view.container.querySelector('[data-usage-hide-ai]')!.getAttribute("data-usage-hide-ai")).toBe("before");
  } finally { await view.unmount(); }
  const none = scene("hide-none");
  const quiet = await mount(none, "popover");
  try {
    const day = none.history!.popover.range.days.at(-1)!;
    await act(async () => quiet.container.querySelector<HTMLButtonElement>(`[data-usage-day="${day.date}"]`)!.click());
    expect(quiet.container.querySelector('[data-usage-hide-ai]')!.getAttribute("data-usage-hide-ai")).toBe("none");
  } finally { await quiet.unmount(); }
});

it("lists Hide AI under an opened date of the full table, expands it into features and the unknown calls, and notes the bound", async () => {
  const node = scene("hide-unknown");
  const view = await mount(node, "year");
  try {
    const shown = new Set([...view.container.querySelectorAll("[data-usage-date-row]")].map((row) => row.getAttribute("data-usage-date-row")));
    const target = node.history!.year.range.days.filter((day) => shown.has(day.date) && day.hide_ai && day.hide_ai.unknown_calls > 0 && !day.today).at(-1)!;
    expect(view.container.querySelector("[data-usage-hide-row]")?.getAttribute("data-usage-hide-row")).not.toBe(target.date);
    await act(async () => view.container.querySelector<HTMLElement>(`[data-usage-date-row="${target.date}"] button`)!.click());
    const hideRow = view.container.querySelector<HTMLElement>(`[data-usage-hide-row="${target.date}"]`)!;
    expect(view.container.querySelector("[data-usage-feature-row]")).toBeNull();
    await act(async () => hideRow.querySelector<HTMLButtonElement>("button")!.click());
    const features = [...view.container.querySelectorAll("[data-usage-feature-row]")].map((row) => row.getAttribute("data-usage-feature-row"));
    expect(features).toEqual(target.hide_ai!.features.map((feature) => feature.feature));
    expect(view.container.querySelector(`[data-usage-unknown-row="${target.date}"]`)).not.toBeNull();
    expect(view.container.querySelector("[data-usage-hide-bound]")).not.toBeNull();
    expect(view.container.querySelector("[data-usage-hide-since]")).not.toBeNull();
  } finally { await view.unmount(); }
});
