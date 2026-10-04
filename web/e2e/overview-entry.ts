// Content scenarios enter the shared Overview page through its current scope.
// Modal lifetime, focus and terminal preservation belong to overview-modal.spec.
import { expect, type Page } from "@playwright/test";

export async function openProjectOverview(page: Page, label: string) {
  await page.locator("[data-project-row]", { hasText: new RegExp(`^${label}`) }).click();
  await expect(page.locator("[data-workspace-screen]")).toBeVisible();
  await page.locator("[data-go-main]").click();
  await page.getByRole("tab", { name: label, exact: true }).click();
  await expect(page.locator("[data-overview-screen]")).toBeVisible();
}
