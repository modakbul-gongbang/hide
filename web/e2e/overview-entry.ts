// Content scenarios enter the shared Overview page through its current scope.
// Modal lifetime, focus and terminal preservation belong to overview-modal.spec.
import { expect, type Page } from "@playwright/test";

export async function openProjectOverview(page: Page, label: string) {
  await page.locator("[data-project-row]", { hasText: new RegExp(`^${label}`) }).click();
  await openCurrentProjectOverview(page, label);
}

/** #350 opens a Workspace first; #349 exposes its scope through shared Overview. */
export async function openCurrentProjectOverview(page: Page, label: string) {
  await expect(page.locator("[data-workspace-screen]")).toBeVisible();
  await expect(page.locator("[data-overview-screen]")).toHaveCount(0);
  await expect(page.locator("[data-project-overview]")).toHaveCount(0);
  await page.locator("[data-go-main]").click();
  await expect(page.locator("[data-overview-page]")).toBeVisible();
  await page.getByRole("tab", { name: label, exact: true }).click();
  await expect(page.locator("[data-overview-screen]")).toBeVisible();
  await expect(page.locator("[data-sidebar-overview]")).toHaveAttribute("aria-current", "page");
}
