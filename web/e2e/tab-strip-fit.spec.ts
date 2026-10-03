import { expect, test, type Page } from "@playwright/test";
import path from "node:path";
import { startHerdr } from "./herdr-fixture";
import { startHided, type Daemon } from "./hided-fixture";
import { enterWorkspace, screenshot } from "./wire";

test.describe.configure({ timeout: 180_000 });

// The Agent tab strip shrinks in three stages (`tabStripFit`): every tab asks
// for 180 and all shrink alike while an equal share keeps a 104 title; then the
// selected tab keeps 104 and the others split the rest down to the 40 identity,
// compact while they hold 64 and marks below; then the selected tab gives up
// width, compact with its close while it holds 88, else 64 marks with its
// close, and the strip scrolls once even those overflow.
const PREFERRED = 180, TITLE_MIN = 104, ICON = 40, CONTROL = 24;
type Fit = "titled" | "compact" | "marks";

/** What the curve draws for `room` and `count`, written out independently of the shell's code. */
function expected(room: number, count: number): { selected: [number, Fit]; others: [number, Fit] } {
  const share = room / count;
  if (share >= TITLE_MIN) return { selected: [Math.min(PREFERRED, share), "titled"], others: [Math.min(PREFERRED, share), "titled"] };
  if (room >= TITLE_MIN + (count - 1) * ICON) {
    const others = (room - TITLE_MIN) / (count - 1);
    return { selected: [TITLE_MIN, "titled"], others: [Math.max(ICON, others), others >= ICON + CONTROL ? "compact" : "marks"] };
  }
  const selected = room - (count - 1) * ICON;
  return { selected: selected >= ICON + 2 * CONTROL ? [selected, "compact"] : [ICON + CONTROL, "marks"], others: [ICON, "marks"] };
}

type Strip = {
  room: number;
  scrolls: boolean;
  selectedInView: boolean;
  tabs: { selected: boolean; width: number; title: boolean; close: boolean }[];
};

async function strip(page: Page): Promise<Strip> {
  return page.locator("[data-agent-tab-bar] [role=tablist]").evaluate((list) => {
    const shown = (node: Element | null) => node !== null && getComputedStyle(node).display !== "none";
    // The tabs' room is what the bar leaves beside its own controls: the zone
    // holding the strip and New tab, less New tab.
    const zone = list.parentElement!;
    const newTab = zone.querySelector<HTMLElement>("[data-new-agent-tab]")!;
    const bounds = list.getBoundingClientRect();
    const tabs = [...list.querySelectorAll<HTMLElement>("[role=tab]")];
    const selected = tabs.find((tab) => tab.getAttribute("aria-selected") === "true")?.getBoundingClientRect();
    return {
      room: Math.floor(zone.getBoundingClientRect().width - newTab.getBoundingClientRect().width),
      scrolls: list.scrollWidth > list.clientWidth,
      selectedInView: !!selected && selected.left >= bounds.left - 1 && selected.right <= bounds.right + 1,
      tabs: tabs.map((tab) => ({
        selected: tab.getAttribute("aria-selected") === "true",
        width: tab.getBoundingClientRect().width,
        title: shown(tab.querySelector("span.truncate")),
        close: shown(tab.querySelector('button[aria-label^="Close tab"]')),
      })),
    };
  });
}

test("Agent tabs shrink in stages, the selected one keeping its title longest, and marks come only when nothing else fits", async ({ page }) => {
  await page.setViewportSize({ width: 1920, height: 1080 });
  const herdr = await startHerdr({ agents: false });
  let daemon: Daemon | null = null;
  const create = () => herdr.run(["tab", "create", "--workspace", herdr.workspace, "--cwd", path.join(herdr.root, "fixture"), "--no-focus"]);
  try {
    create();
    create();
    daemon = await startHided(herdr, "tab-strip-fit");
    await page.goto(`${daemon.origin}/#token=${daemon.token}`);
    await enterWorkspace(page, "fixture");
    const tabs = page.locator("[data-agent-tab-bar] [role=tab]");
    await expect(tabs).toHaveCount(3);

    // Room to spare: every tab takes the preferred width, whatever its title.
    const rest = await strip(page);
    expect(rest.tabs.map((tab) => Math.round(tab.width))).toEqual([PREFERRED, PREFERRED, PREFERRED]);
    expect(rest.tabs.every((tab) => tab.title)).toBe(true);
    await screenshot(page, "tab-strip-fit-preferred");

    for (let count = 3; count < 16; count += 1) create();
    await expect(tabs).toHaveCount(16);

    const settle = () => page.evaluate(() => new Promise((done) => requestAnimationFrame(() => requestAnimationFrame(done))));
    const check = async (width: number) => {
      await page.setViewportSize({ width, height: 900 });
      await settle();
      const drawn = await strip(page);
      const want = expected(drawn.room, drawn.tabs.length);
      const selected = drawn.tabs.find((tab) => tab.selected)!;
      const unselected = drawn.tabs.filter((tab) => !tab.selected);
      expect(selected.width, `selected at room ${drawn.room}`).toBeCloseTo(want.selected[0], 0);
      expect(selected.title).toBe(want.selected[1] !== "marks");
      expect(selected.close).toBe(true);
      for (const tab of unselected) {
        expect(tab.width, `unselected at room ${drawn.room}`).toBeCloseTo(want.others[0], 0);
        expect(tab.title).toBe(want.others[1] !== "marks");
        // An unselected titled tab keeps its close control's place; a smaller one gives it up.
        expect(tab.close).toBe(want.others[1] === "titled");
      }
      const total = want.selected[0] + unselected.length * want.others[0];
      expect(drawn.scrolls).toBe(total > drawn.room + 0.01);
      expect(drawn.selectedInView).toBe(true);
      const stage = drawn.scrolls ? "scroll"
        : want.others[1] === "titled" ? "titled"
        : want.selected[1] === "titled" ? `others-${want.others[1]}`
        : `selected-${want.selected[1]}`;
      return { room: drawn.room, stage, widths: drawn.tabs.map((tab) => Math.round(tab.width * 10) / 10) };
    };

    // Narrowing and then widening through the same widths draws the same strip both ways.
    const widths: number[] = [];
    for (let width = 2800; width >= 700; width -= 10) widths.push(width);
    const narrowing = new Map<number, Awaited<ReturnType<typeof check>>>();
    const seen = new Set<string>();
    for (const width of widths) {
      const drawn = await check(width);
      narrowing.set(width, drawn);
      if (!seen.has(drawn.stage)) await screenshot(page, `tab-strip-fit-${drawn.stage}`);
      seen.add(drawn.stage);
    }
    expect([...seen].sort()).toEqual(["others-compact", "others-marks", "scroll", "selected-compact", "selected-marks", "titled"]);
    for (const width of [...widths].reverse()) expect(await check(width)).toEqual(narrowing.get(width));

    // Selecting another tab moves the title width to it in the same frame.
    const stage2 = widths.find((width) => narrowing.get(width)!.stage === "others-compact")!;
    await check(stage2);
    const before = await strip(page);
    const target = before.tabs.findIndex((tab) => !tab.selected);
    await tabs.nth(target).click();
    await expect(tabs.nth(target)).toHaveAttribute("aria-selected", "true");
    const after = await strip(page);
    expect(after.tabs[target]!.width).toBeCloseTo(TITLE_MIN, 0);
    expect(after.tabs.filter((tab) => !tab.selected).every((tab) => Math.abs(tab.width - before.tabs[target]!.width) < 1)).toBe(true);

    // Once even marks overflow, the selected tab stays in sight: after it is
    // chosen at the strip's far end, and after the strip narrows further.
    const scrolling = widths.find((width) => narrowing.get(width)!.stage === "scroll")!;
    await check(scrolling);
    await tabs.last().click();
    await expect(tabs.last()).toHaveAttribute("aria-selected", "true");
    await settle();
    expect((await strip(page)).selectedInView).toBe(true);
    await page.setViewportSize({ width: widths.at(-1)!, height: 900 });
    await settle();
    const narrowest = await strip(page);
    expect(narrowest.scrolls).toBe(true);
    expect(narrowest.selectedInView).toBe(true);
  } finally { daemon?.stop(); herdr.stop(); }
});
