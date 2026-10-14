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
