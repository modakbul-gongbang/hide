// The device rail on an isolated pinned Herdr and hided (quick
// device-rail-badges B1-B6 and quick device-rail-slack, replacing PRD
// home-device-rail B1-B13): the rail is the sidebar's full-height left column,
// shown with This Mac alone, and has no Inbox or footer device button, its
// `+` opens Settings > Devices > Add device, and each device's sidebar is its
// name over Projects | Agents with the Home row in Projects. A right-click
// hides the rail, the name on the top line becomes the device menu, and the
// choice survives a reload. Registering one device that cannot be reached (an
// alias no SSH config knows) adds a dimmed monogram tile with a cross and no
// mark, named by its hint, and, selected, the sidebar reduced to its name, `연결 안 됨` and `다시 연결`.
// Removing it leaves This Mac's rail. The Add project dialog's Host list entry
// needs the desktop host's folder picker, so it is proved in desktop/e2e, not
// in a browser tab.

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

    // B1: the rail is shown with This Mac alone, and there is no Inbox tile and no footer device button.
    const rail = page.locator("[data-device-rail]");
    await expect(rail).toBeVisible();
    await expect(rail.locator("[data-rail-tile]")).toHaveCount(1);
    await expect(rail.locator('[data-rail-tile="local"]')).toHaveAttribute("aria-pressed", "true");
    await expect(rail.locator('[data-rail-tile="inbox"]')).toHaveCount(0);
    await expect(page.locator("[data-footer-device]")).toHaveCount(0);
    // B3: the device's sidebar is its name over Projects | Agents, with the Home row in Projects and no Overview row.
    await expect(page.locator("[data-sidebar-title-name]")).toHaveText("This Mac");
    await expect(page.locator("[data-sidebar-mode]")).toHaveText(["Projects", "Agents"]);
    await expect(page.locator("[data-overview-destination]")).toHaveCount(0);
    await expect(page.locator("[data-project-list] [data-home-destination]")).toContainText("Home");
    await page.locator('[data-sidebar-mode="agents"]').click();
    await expect(page.locator("[data-agent-list], [data-agents-empty]").first()).toBeVisible();
    await expect(page.locator("[data-project-list]")).toHaveCount(0);
    await page.locator('[data-sidebar-mode="projects"]').click();

    // B6: a right-click on the rail offers 레일 숨기기; hidden, the name is the device menu with 기기 추가… and 레일 표시.
    await rail.click({ button: "right", position: { x: 10, y: 400 } });
    const railMenu = page.locator('[data-device-rail-menu][role="menu"]');
    await expect(railMenu).toContainText("레일 숨기기");
    await railMenu.locator('[data-menu-item="hide"]').click();
    await expect(rail).toHaveCount(0);
    await expect(page.locator("[data-sidebar-device-menu]")).toContainText("This Mac");
    // The choice is the core's, so it survives a reload.
    await page.reload();
    await expect(page.locator(CENTER).first()).toBeVisible({ timeout: 20_000 });
    await expect(page.locator("[data-device-rail]")).toHaveCount(0);
    await screenshot(page, "device-rail-hidden");
    await page.locator("[data-sidebar-device-menu]").click();
    const deviceMenu = page.locator("[data-device-menu]");
    await expect(deviceMenu.locator('[data-device-menu-item="local"]')).toBeVisible();
    await screenshot(page, "device-rail-hidden-menu");
    // B1: 기기 추가… opens Settings > Devices > Add device.
    await deviceMenu.locator("[data-device-menu-add]").click();
    await openAddDeviceForm(page);
    await page.keyboard.press("Escape");
    await expect(page.locator('[data-settings="true"]')).toHaveCount(0);
    await page.locator("[data-sidebar-device-menu]").click();
    await page.locator("[data-device-menu-show-rail]").click();
    await expect(rail).toBeVisible();

    // B1: `+` sits directly under the last device tile and opens the same form.
    const addBox = (await rail.locator("[data-rail-add]").boundingBox())!;
    const lastBox = (await rail.locator('[data-rail-tile="local"]').boundingBox())!;
    expect(addBox.y).toBeGreaterThan(lastBox.y + lastBox.height - 1);
    expect(addBox.y - (lastBox.y + lastBox.height)).toBeLessThan(24);
    await rail.locator("[data-rail-add]").click();
    await openAddDeviceForm(page);

    // Registering a device, even an unreachable one, adds its tile; the center stays where it was.
    const centerBefore = await page.locator("[data-main-screen]").count();
    await page.locator("[data-device-label]").fill("연구실 빌드 서버 자동화 장비");
    await page.locator("[data-device-alias]").fill(ALIAS);
    await page.locator("[data-add-device]").click();
    await page.keyboard.press("Escape");
    await expect(page.locator('[data-settings="true"]')).toHaveCount(0);
    await expect(rail.locator("[data-rail-tile]")).toHaveCount(2, { timeout: 20_000 });
    await expect(page.locator("[data-sidebar-mode]")).toHaveText(["Projects", "Agents"]);
    const ids = await rail.locator("[data-rail-tile]").evaluateAll((tiles) => tiles.map((tile) => tile.getAttribute("data-rail-tile")));
    expect(ids).toEqual(["local", ALIAS]);
    // The rail is its own fixed column beside the content column: the stored width stays the content's, and the rail adds to it.
    // It runs the sidebar's full height, so the header line and the tab strip sit right of it, not over it.
    const railBox = (await rail.boundingBox())!;
    const contentBox = (await page.locator("[data-sidebar-content]").boundingBox())!;
    const sidebarBox = (await sidebar.boundingBox())!;
    expect(Math.round(railBox.width)).toBe(48);
    expect(Math.round(contentBox.width)).toBe(292);
    expect(Math.round(sidebarBox.width)).toBe(48 + 292);
    expect(Math.round(railBox.x)).toBe(Math.round(sidebarBox.x));
    expect(Math.round(railBox.y)).toBe(Math.round(sidebarBox.y));
    expect(Math.round(railBox.height)).toBe(Math.round(sidebarBox.height));
    expect(Math.round((await page.locator("[data-sidebar-title]").boundingBox())!.x)).toBe(Math.round(railBox.x + railBox.width));
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
    await expect(page.locator("[data-sidebar-mode]")).toHaveCount(0);
    await expect(page.locator("[data-sidebar-title-name]")).toContainText("연구실");
    await screenshot(page, "device-rail-not-connected");
    const retried = sent.get("retry_connect") ?? 0;
    await disconnected.locator("[data-device-reconnect]").click();
    await expect.poll(() => (sent.get("retry_connect") ?? 0) - retried).toBe(1);
    await expect(disconnected).toBeVisible();

    // The Korean name pushes none of the rail, the header line and the content column sideways (B46).
    // The nav itself is not measured: its drag edge straddles its right side on purpose.
    for (const part of ["[data-device-rail]", "[data-sidebar-title]", "[data-sidebar-content]"]) {
      const overflow = await sidebar.locator(part).evaluate((element) => element.scrollWidth - element.clientWidth);
      expect(overflow, part).toBeLessThanOrEqual(0);
    }
    // The tile draws the name's monogram and no name; the name is the tile's hint.
    await expect(tile.locator("[data-rail-glyph]")).toHaveText("연빌");
    await tile.hover();
    await expect(page.getByRole("tooltip")).toContainText("연구실 빌드 서버 자동화 장비");

    // B13: the rail's + opens the same Add device form.
    await rail.locator("[data-rail-add]").click();
    await openAddDeviceForm(page);

    // B10: removing the only remote device leaves This Mac's rail, with This Mac in front.
    await page.locator(`[data-device-remove="${ALIAS}"]`).click();
    await page.locator("[data-device-remove-go]").click();
    await expect(rail.locator("[data-rail-tile]")).toHaveCount(1, { timeout: 20_000 });
    await page.keyboard.press("Escape");
    await expect(rail.locator('[data-rail-tile="local"]')).toHaveAttribute("aria-pressed", "true");
    await expect(page.locator("[data-sidebar-mode]")).toHaveText(["Projects", "Agents"]);
    await expect(page.locator("[data-home-destination]")).toBeVisible();
    await expect(page.locator(CENTER).first()).toBeVisible();
  } finally {
    daemon?.stop();
    herdr.stop();
  }
});
