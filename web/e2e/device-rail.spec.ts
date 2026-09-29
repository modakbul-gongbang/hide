// The device rail on an isolated pinned Herdr and hided (PRD home-device-rail
// B1-B13, B41): with no remote device there is no rail, the Home row heads the
// sidebar above the Projects | Agents tabs (no Overview row), and the footer's
// laptop button offers `기기 추가…`. Registering one device that cannot be
// reached (an alias no SSH config knows) shows the rail with This Mac selected
// and the center where it was, the new tile dimmed with a cross and no badge,
// and, selected, the sidebar reduced to its name, `연결 안 됨` and `다시 연결`.
// Removing it brings the footer button back. Every Add device entry opens the
// one form. The Add project dialog's Host list entry needs the desktop host's
// folder picker, so it is proved in desktop/e2e, not in a browser tab.

import { expect, test, type Page } from "@playwright/test";
import { startHerdr } from "./herdr-fixture";
import { startHided, type Daemon } from "./hided-fixture";
import { countSent, screenshot } from "./wire";

test.describe.configure({ timeout: 180_000 });

const ALIAS = "unreachable-e2e";
const CENTER = "[data-main-screen], [data-workspace-screen]";

async function openAddDeviceForm(page: Page): Promise<void> {
  await expect(page.locator('[data-settings="true"]')).toBeVisible();
  await expect(page.locator('[data-settings-tab="devices"]')).toHaveAttribute("data-state", "active");
  await expect(page.locator("[data-add-device]")).toBeVisible();
}

test("the rail follows the registered devices; a device that cannot be reached is dimmed and offers 다시 연결", async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  const herdr = await startHerdr();
  let daemon: Daemon | null = null;
  try {
    daemon = await startHided(herdr, "device-rail");
    const sent = countSent(page);
    await page.goto(`${daemon.origin}/#token=${daemon.token}`);
    await expect(page.locator(CENTER).first()).toBeVisible({ timeout: 20_000 });
    const sidebar = page.locator("nav[data-sidebar]");

    // B11: no remote device, no rail. The Home row heads the sidebar in place of an Overview row, above the tabs.
    await expect(page.locator("[data-device-rail]")).toHaveCount(0);
    await expect(page.locator("[data-overview-destination]")).toHaveCount(0);
    await expect(page.locator("[data-home-destination]")).toContainText("Home");
    await expect(page.locator("[data-sidebar-mode]")).toHaveText(["Projects", "Agents"]);

    // B11, B13: the footer's laptop button offers This Mac and 기기 추가…, which opens Settings › Devices › Add device.
    await page.locator("[data-footer-device]").click();
    const menu = page.locator("[data-footer-device-menu]");
    await expect(menu).toContainText("This Mac · 이 기기");
    await menu.locator("[data-footer-device-add]").click();
    await openAddDeviceForm(page);

    // B12: registering the first device, even an unreachable one, shows the rail and takes the footer button away.
    const centerBefore = await page.locator("[data-main-screen]").count();
    await page.locator("[data-device-label]").fill("연구실 빌드 서버 자동화 장비");
    await page.locator("[data-device-alias]").fill(ALIAS);
    await page.locator("[data-add-device-without-helper]").click();
    await page.keyboard.press("Escape");
    await expect(page.locator('[data-settings="true"]')).toHaveCount(0);
    const rail = page.locator("[data-device-rail]");
    await expect(rail).toBeVisible({ timeout: 20_000 });
    await expect(page.locator("[data-footer-device]")).toHaveCount(0);
    await expect(page.locator("[data-sidebar-mode]")).toHaveCount(0);
    await expect(rail.locator("[data-rail-tile]")).toHaveCount(3);
    const ids = await rail.locator("[data-rail-tile]").evaluateAll((tiles) => tiles.map((tile) => tile.getAttribute("data-rail-tile")));
    expect(ids).toEqual(["inbox", "local", ALIAS]);
    // The rail is its own fixed column beside the content column: the stored width stays the content's, and the rail adds to it.
    const railBox = (await rail.boundingBox())!;
    const contentBox = (await page.locator("[data-sidebar-content]").boundingBox())!;
    expect(Math.round(railBox.width)).toBe(52);
    expect(Math.round(contentBox.width)).toBe(292);
    expect(Math.round((await sidebar.boundingBox())!.width)).toBe(52 + 292);
    // This Mac is the selection, and the center is where it was.
    await expect(rail.locator('[data-rail-tile="local"]')).toHaveAttribute("aria-pressed", "true");
    expect(await page.locator("[data-main-screen]").count()).toBe(centerBefore);
    await expect(page.locator("[data-sidebar-title-name]")).toHaveText("This Mac");

    // B3, B8: the unreachable device is dimmed with a cross, has no badge, and its name says so.
    const tile = rail.locator(`[data-rail-tile="${ALIAS}"]`);
    await expect(tile).toHaveAttribute("data-rail-connected", "false", { timeout: 30_000 });
    await expect(tile.locator("[data-rail-off]")).toBeVisible();
    await expect(tile.locator("[data-rail-badge]")).toHaveCount(0);
    await expect(tile).toHaveAccessibleName(/연결 안 됨/);
    for (const theme of ["dark", "light"] as const) {
      await page.evaluate((next) => {
        document.documentElement.classList.toggle("dark", next === "dark");
        document.documentElement.classList.toggle("light", next === "light");
      }, theme);
      await page.waitForTimeout(400);
      await screenshot(page, `device-rail-this-mac-${theme}`);
    }

    // B41: a tile takes Tab focus and answers Enter.
    await tile.focus();
    await expect(tile).toBeFocused();
    await page.keyboard.press("Enter");
    await expect(tile).toHaveAttribute("aria-pressed", "true");
    await expect(rail.locator('[data-rail-tile="local"]')).toHaveAttribute("aria-pressed", "false");

    // B8, B9: selected, the sidebar is the name, 연결 안 됨 and 다시 연결, with no tree.
    const disconnected = page.locator("[data-device-disconnected]");
    await expect(disconnected).toBeVisible();
    await expect(disconnected).toContainText("연결 안 됨");
    await expect(disconnected.locator("[data-device-disconnected-name]")).toHaveText("연구실 빌드 서버 자동화 장비");
    await expect(page.locator("[data-project-list]")).toHaveCount(0);
    await expect(page.locator("[data-home-destination]")).toHaveCount(0);
    await expect(page.locator("[data-sidebar-title-name]")).toContainText("연구실");
    await screenshot(page, "device-rail-not-connected");
    const retried = sent.get("retry_connect") ?? 0;
    await disconnected.locator("[data-device-reconnect]").click();
    await expect.poll(() => (sent.get("retry_connect") ?? 0) - retried).toBe(1);
    await expect(disconnected).toBeVisible();

    // The Korean name truncates inside the rail, the header line and the content column without pushing any sideways (B46).
    // The nav itself is not measured: its drag edge straddles its right side on purpose.
    for (const part of ["[data-device-rail]", "[data-sidebar-title]", "[data-sidebar-content]"]) {
      const overflow = await sidebar.locator(part).evaluate((element) => element.scrollWidth - element.clientWidth);
      expect(overflow, part).toBeLessThanOrEqual(0);
    }
    expect(await tile.locator("[data-rail-label]").evaluate((label) => label.scrollWidth > label.clientWidth)).toBe(true);

    // B4: the Inbox is a page state; the center does not change.
    await rail.locator('[data-rail-tile="inbox"]').click();
    await expect(rail.locator('[data-rail-tile="inbox"]')).toHaveAttribute("aria-pressed", "true");
    await expect(page.locator("[data-sidebar-title-name]")).toHaveText("Inbox");
    await expect(page.locator("[data-agent-list], [data-agents-empty]").first()).toBeVisible();
    await expect(page.locator("[data-sidebar-new-workspace]")).toHaveCount(0);
    expect(await page.locator("[data-main-screen]").count()).toBe(centerBefore);

    // B13: the rail's + opens the same Add device form.
    await rail.locator("[data-rail-add]").click();
    await openAddDeviceForm(page);

    // B10, B12: removing the only device takes the rail away, the footer button returns, and This Mac is in front.
    await page.locator(`[data-device-remove="${ALIAS}"]`).click();
    await page.locator("[data-device-remove-go]").click();
    await expect(page.locator("[data-device-rail]")).toHaveCount(0, { timeout: 20_000 });
    await page.keyboard.press("Escape");
    await expect(page.locator("[data-footer-device]")).toBeVisible();
    await expect(page.locator("[data-sidebar-mode]")).toHaveText(["Projects", "Agents"]);
    await expect(page.locator("[data-home-destination]")).toBeVisible();
    await expect(page.locator(CENTER).first()).toBeVisible();
  } finally {
    daemon?.stop();
    herdr.stop();
  }
});
