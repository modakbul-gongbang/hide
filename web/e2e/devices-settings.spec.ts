// Settings > Devices on an isolated pinned Herdr and hided (PRD settings-cleanup
// B50, B54, B55, B56, B66): a device row is one line with Test and a ⋯ menu,
// Add device is one button above the list, Connection details holds what a
// healthy row no longer shows, and Revoke and Remove ask before they act. The
// device is an alias no SSH config knows, so it is registered and unreachable;
// a failed kit part and a healthy kit are the component test's
// (`web/src/settings/DevicesTab.test.tsx`), because a reachable device needs
// the desktop suite's isolated sshd (`desktop/e2e/device-kit.spec.ts`).

import { expect, test, type Page } from "@playwright/test";
import { startHerdr } from "./herdr-fixture";
import { startHided, type Daemon } from "./hided-fixture";
import { countSent, enterWorkspace, screenshot } from "./wire";

test.describe.configure({ timeout: 120_000 });

const ALIAS = "unreachable-e2e";

async function openDevices(page: Page, daemon: Daemon) {
  await page.goto(`${daemon.origin}/#token=${daemon.token}`);
  await enterWorkspace(page, "fixture");
  await page.locator("[data-open-settings]").click();
  await page.locator('[data-settings-tab="devices"]').click();
  return page.locator('[data-settings="true"]');
}

test("a device row is one line with Test and a menu, Add device is one button, and Remove asks first", async ({ page }) => {
  await page.setViewportSize({ width: 900, height: 800 });
  const herdr = await startHerdr();
  let daemon: Daemon | null = null;
  try {
    daemon = await startHided(herdr, "devices-settings");
    const sent = countSent(page);
    const sheet = await openDevices(page, daemon);

    // B50: one Add device button above the list; the form is not on the page until it is pressed.
    await expect(sheet.locator("[data-add-device]")).toHaveCount(0);
    await expect(sheet.locator("[data-device-add-open]")).toBeVisible();

    // B55: This Mac's menu has Connection details and nothing that removes it or revokes a helper.
    const local = sheet.locator('[data-device-row="local"]');
    await expect(local.locator('[data-device-subtitle="local"]')).toBeVisible();
    await local.locator('[data-device-menu="local"]').click();
    const localMenu = page.locator('[data-device-menu-content="local"]');
    await expect(localMenu.getByRole("menuitem")).toHaveText(["Connection details…"]);
    await localMenu.locator('[data-device-details="local"]').click();
    const details = page.locator('[data-device-details-dialog="local"]');
    await expect(details).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(details).toHaveCount(0);

    // B50: Add device opens the dialog, and the device is listed once it is registered; the dialog closes itself.
    await sheet.locator("[data-device-add-open]").click();
    await expect(page.locator("[data-add-device-dialog]")).toBeVisible();
    await page.locator("[data-device-label]").fill("연구실 빌드 서버");
    await page.locator("[data-device-alias]").fill(ALIAS);
    await page.locator("[data-add-device]").click();
    await expect(page.locator("[data-add-device-dialog]")).toHaveCount(0);

    // B54: the device is one row: name, alias, and its connection; Test and the menu are its only controls.
    const row = sheet.locator(`[data-device-row="${ALIAS}"]`);
    await expect(row).toBeVisible({ timeout: 20_000 });
    await expect(row.locator(`[data-device-subtitle="${ALIAS}"]`)).toHaveText(ALIAS);
    await expect(row.locator(`[data-device-state="${ALIAS}:unavailable"]`)).toBeVisible({ timeout: 30_000 });
    await expect(row.locator(`[data-device-test="${ALIAS}"]`)).toBeVisible();
    // B56: no per-device Agents line, no Coordination retirement row, no kit part list.
    await expect(row).not.toContainText(/Agents:|Coordination|hide command/);
    await expect(row.locator("[data-kit-part]")).toHaveCount(0);
    await screenshot(page, "devices-row");

    // B55, B66: the menu opens from the keyboard, lists the actions in order, and Escape closes it back to its button.
    const trigger = row.locator(`[data-device-menu="${ALIAS}"]`);
    await trigger.focus();
    await page.keyboard.press("Enter");
    const menu = page.locator(`[data-device-menu-content="${ALIAS}"]`);
    await expect(menu).toBeVisible();
    const items = await menu.getByRole("menuitem").allTextContents();
    expect(items.slice(0, 2)).toEqual(["Select", "Connection details…"]);
    expect(items.at(-1)).toBe("Remove…");
    await screenshot(page, "devices-menu");
    await page.keyboard.press("Escape");
    await expect(menu).toHaveCount(0);
    await expect(trigger).toBeFocused();

    // B55: Select is one event, and the selected device is marked on its row.
    await trigger.click();
    await page.locator(`[data-device-select="${ALIAS}"]`).click();
    await expect.poll(() => sent.get("focus_device") ?? 0).toBe(1);
    await expect(row.getByText("selected", { exact: true })).toBeVisible();

    // B55: Remove asks before it acts, Keep leaves the device, and Remove takes it away.
    await trigger.click();
    await page.locator(`[data-device-remove="${ALIAS}"]`).click();
    const confirm = page.locator(`[data-device-remove-confirm="${ALIAS}"]`);
    await expect(confirm).toBeVisible();
    await confirm.getByRole("button", { name: "Keep device" }).click();
    await expect(confirm).toHaveCount(0);
    await expect(row).toBeVisible();
    expect(sent.get("remove_device") ?? 0).toBe(0);
    await trigger.click();
    await page.locator(`[data-device-remove="${ALIAS}"]`).click();
    await page.locator("[data-device-remove-go]").click();
    await expect(row).toHaveCount(0, { timeout: 20_000 });
    expect(sent.get("remove_device")).toBe(1);
  } finally {
    daemon?.stop();
    herdr.stop();
  }
});
