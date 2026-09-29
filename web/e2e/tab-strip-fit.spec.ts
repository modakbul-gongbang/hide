import { expect, test, type Page } from "@playwright/test";
import path from "node:path";
import { startHerdr } from "./herdr-fixture";
import { startHided, type Daemon } from "./hided-fixture";
import { enterWorkspace, screenshot } from "./wire";

test.describe.configure({ timeout: 180_000 });

// The Pen library's `Component / Adaptive Work Tab`: every tab asks for 180 and
// all shrink alike while an equal share keeps a 104 title; below that every tab
// is a 40 identity, the selected one 64 with its close control, and the strip
// scrolls once even those overflow.
const PREFERRED = 180, TITLE_MIN = 104, ICON = 40, SELECTED_ICON = 64;

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
      room: zone.clientWidth - newTab.offsetWidth,
      scrolls: list.scrollWidth > list.clientWidth + 1,
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

test("Agent tabs shrink alike to their title minimum, then turn to marks and scroll, like a browser", async ({ page }) => {
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

    const seen = new Set<string>();
    for (let width = 2800; width >= 700; width -= 100) {
      await page.setViewportSize({ width, height: 900 });
      await page.evaluate(() => new Promise((done) => requestAnimationFrame(() => requestAnimationFrame(done))));
      const drawn = await strip(page);
      const share = drawn.room / drawn.tabs.length;
      const icon = share < TITLE_MIN;
      const selected = drawn.tabs.find((tab) => tab.selected)!;
      const unselected = drawn.tabs.filter((tab) => !tab.selected);
      if (!icon) {
        for (const tab of drawn.tabs) {
          expect(tab.width).toBeCloseTo(Math.min(PREFERRED, share), 0);
          expect(tab.title).toBe(true);
        }
        expect(drawn.scrolls).toBe(false);
      } else {
        expect(selected.width).toBeCloseTo(SELECTED_ICON, 0);
        expect(selected.title).toBe(false);
        for (const tab of unselected) {
          expect(tab.width).toBeCloseTo(ICON, 0);
          expect(tab.title).toBe(false);
          expect(tab.close).toBe(false);
        }
        expect(drawn.scrolls).toBe(ICON * unselected.length + SELECTED_ICON > drawn.room + 1);
      }
      expect(selected.close).toBe(true);
      expect(drawn.selectedInView).toBe(true);
      const regime = drawn.scrolls ? "scroll" : icon ? "icon" : "titled";
      if (!seen.has(regime)) await screenshot(page, `tab-strip-fit-${regime}`);
      seen.add(regime);
    }
    expect([...seen].sort()).toEqual(["icon", "scroll", "titled"]);
  } finally { daemon?.stop(); herdr.stop(); }
});
